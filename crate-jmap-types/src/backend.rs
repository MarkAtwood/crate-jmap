//! Backend marker traits shared across all JMAP server crates.
//!
//! These traits are defined here in `jmap-types` so that implementations can
//! go in the type crates (`jmap-mail-types`, `jmap-chat-types`) without
//! violating the orphan rule.

// ---------------------------------------------------------------------------
// Marker traits
// ---------------------------------------------------------------------------

/// Marker trait for all JMAP object types.
///
/// Implement this in the *types* crate for each first-class JMAP object (e.g.
/// `Mailbox`, `Email`, `Chat`). The [`TYPE_NAME`](JmapObject::TYPE_NAME) constant
/// is used in error messages and capability checks in the server crates.
pub trait JmapObject:
    serde::Serialize + for<'de> serde::Deserialize<'de> + Send + Sync + 'static
{
    /// The JMAP type name string (e.g. `"Email"`, `"Mailbox"`, `"Chat"`).
    const TYPE_NAME: &'static str;
    /// The property selector enum for this type (server-side only, no serde).
    type Property: Send + Sync + 'static;
}

/// Marker for object types that support `get` and `changes` operations.
pub trait GetObject: JmapObject {}

/// Marker for object types that support `set` (create/update/destroy) operations.
pub trait SetObject: JmapObject {
    /// The patch type for update operations.
    ///
    /// Typically [`serde_json::Value`] holding an RFC 8620 §5.3 PatchObject,
    /// whose keys are JSON Pointer paths (RFC 6901) with an implicit leading
    /// `/`. This is not an RFC 7396 JSON Merge Patch. A typed struct is also
    /// allowed if it serializes to that PatchObject shape.
    type Patch: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;

    /// Wire names of the properties the spec marks "server-set": "Only the
    /// server can set the value for this property. The client MUST NOT send
    /// this property when creating a new object of this type." (RFC 8620
    /// §1.1).
    ///
    /// A `/set` create that carries one, or an update patch that changes
    /// one, is `invalidProperties` (RFC 8620 §5.3).
    const SERVER_SET: &'static [&'static str];

    /// Wire names of every property that is not [`SERVER_SET`]: the ones a
    /// client may send in a `/set` create.
    ///
    /// Each property of the type is in exactly one of [`SERVER_SET`] and
    /// `CLIENT_SETTABLE`; [`check_property_classification`] tests that.
    ///
    /// [`SERVER_SET`]: SetObject::SERVER_SET
    const CLIENT_SETTABLE: &'static [&'static str];

    /// The [`CLIENT_SETTABLE`] properties the spec marks "immutable": "The
    /// value MUST NOT change after the object is created." (RFC 8620 §1.1).
    ///
    /// A client may set one in a `/set` create; an update patch that changes
    /// one is `invalidProperties`.
    ///
    /// [`CLIENT_SETTABLE`]: SetObject::CLIENT_SETTABLE
    const IMMUTABLE: &'static [&'static str];
}

/// Check the property classification of `T` against `obj`, a value with
/// every field populated.
///
/// Every top-level property `obj` serializes MUST be in exactly one of
/// [`SetObject::SERVER_SET`] and [`SetObject::CLIENT_SETTABLE`], and every
/// [`SetObject::IMMUTABLE`] name MUST be in `CLIENT_SETTABLE`.
///
/// Types crates call this from a test that builds `obj` with a struct
/// literal and no `..Default::default()`. A field added to the struct then
/// fails to compile until the test sets it, and fails this check until it is
/// classified.
///
/// # Errors
///
/// Returns every problem found, one per line, or a single line if `obj`
/// does not serialize to a JSON object.
pub fn check_property_classification<T: SetObject>(obj: &T) -> Result<(), String> {
    let value = serde_json::to_value(obj).map_err(|e| format!("serialize: {e}"))?;
    let serde_json::Value::Object(map) = value else {
        return Err(format!("{} does not serialize to an object", T::TYPE_NAME));
    };
    let mut problems = Vec::new();
    for key in map.keys() {
        let k = key.as_str();
        match (T::SERVER_SET.contains(&k), T::CLIENT_SETTABLE.contains(&k)) {
            (false, false) => problems.push(format!("{}.{k} is unclassified", T::TYPE_NAME)),
            (true, true) => problems.push(format!("{}.{k} is classified twice", T::TYPE_NAME)),
            _ => {}
        }
    }
    for name in T::IMMUTABLE {
        if !T::CLIENT_SETTABLE.contains(name) {
            problems.push(format!(
                "{}.{name} is IMMUTABLE but not CLIENT_SETTABLE",
                T::TYPE_NAME
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// Marker for object types that support `query` and `queryChanges` operations.
pub trait QueryObject: JmapObject {
    /// The filter condition type.
    ///
    /// Must implement both `Serialize` and `DeserializeOwned` so that backends
    /// can inspect or forward the filter.
    type Filter: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;
    /// The comparator type for sort operations.
    type Comparator: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Probe {
        id: String,
        name: String,
        email: String,
    }

    impl JmapObject for Probe {
        const TYPE_NAME: &'static str = "Probe";
        type Property = ();
    }

    impl SetObject for Probe {
        type Patch = serde_json::Value;
        const SERVER_SET: &'static [&'static str] = &["id"];
        const CLIENT_SETTABLE: &'static [&'static str] = &["name", "email"];
        const IMMUTABLE: &'static [&'static str] = &["email"];
    }

    /// The same shape with `email` left out of both lists, `name` in both,
    /// and an IMMUTABLE name that is server-set.
    #[derive(serde::Serialize, serde::Deserialize)]
    struct BadProbe {
        id: String,
        name: String,
        email: String,
    }

    impl JmapObject for BadProbe {
        const TYPE_NAME: &'static str = "BadProbe";
        type Property = ();
    }

    impl SetObject for BadProbe {
        type Patch = serde_json::Value;
        const SERVER_SET: &'static [&'static str] = &["id", "name"];
        const CLIENT_SETTABLE: &'static [&'static str] = &["name"];
        const IMMUTABLE: &'static [&'static str] = &["id"];
    }

    fn probe<T: SetObject>(make: fn(String, String, String) -> T) -> Result<(), String> {
        check_property_classification(&make("i".into(), "n".into(), "e".into()))
    }

    #[test]
    fn classification_accepts_a_partition() {
        assert_eq!(probe(|id, name, email| Probe { id, name, email }), Ok(()));
    }

    #[test]
    fn classification_reports_every_problem() {
        let err = probe(|id, name, email| BadProbe { id, name, email }).unwrap_err();
        let mut lines: Vec<&str> = err.lines().collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            [
                "BadProbe.email is unclassified",
                "BadProbe.id is IMMUTABLE but not CLIENT_SETTABLE",
                "BadProbe.name is classified twice",
            ]
        );
    }
}
