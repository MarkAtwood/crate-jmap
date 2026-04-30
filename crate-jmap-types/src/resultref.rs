//! RFC 8620 §7 ResultReference — back-references between method calls in a batch.
//!
//! Provides [`ResultReference`] and [`Argument<T>`].

use serde::{Deserialize, Serialize};

/// Sealed marker for types that are safe to use with [`Argument<T>`].
///
/// This trait is intentionally sealed: it is defined in a private module and
/// can only be implemented inside this crate. The current sealed set is:
/// `String`, `Vec<String>`, `Id`, `Vec<Id>`, `u32`, `u64`, `bool`.
///
/// **External contributors**: you cannot implement `Sealed` outside this crate.
/// To add a new type to the sealed set, open a PR to `crate-jmap-types` and
/// add the impl here. Before doing so, read the `# Invariant` section on
/// [`Argument`] — the new type must not deserialize from a JSON object.
mod sealed {
    pub trait Sealed {}

    impl Sealed for String {}
    impl Sealed for Vec<String> {}
    impl Sealed for crate::Id {}
    impl Sealed for Vec<crate::Id> {}
    impl Sealed for u32 {}
    impl Sealed for u64 {}
    impl Sealed for bool {}
}

/// A reference to the result of a previous invocation in the same JMAP request batch.
/// Used in method arguments with a "#" prefix on the JSON key (RFC 8620 §9).
///
/// Example JSON:
/// ```json
/// {"resultOf": "0", "name": "ChatContact/get", "path": "/list/0/id"}
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResultReference {
    /// The call-id of the prior method invocation being referenced.
    #[serde(rename = "resultOf")]
    pub result_of: String,
    /// The method name of that prior invocation (e.g. "ChatContact/get").
    pub name: String,
    /// JSON Pointer (RFC 6901) into the result, e.g. "/list/0/id".
    pub path: String,
}

impl ResultReference {
    /// Construct a `ResultReference` from its three required fields.
    ///
    /// `result_of`: call-id of the prior method invocation
    /// `name`: method name of that invocation (e.g. `"Foo/get"`)
    /// `path`: RFC 6901 JSON Pointer into the result (e.g. `"/list/*/id"`)
    pub fn new(
        result_of: impl Into<String>,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            result_of: result_of.into(),
            name: name.into(),
            path: path.into(),
        }
    }
}

/// A JMAP method argument that can be either a direct value or a ResultReference.
///
/// In JMAP JSON, a ResultReference is indicated by a "#" prefix on the key:
///   `"ids": [...]`  →  Argument::Value([...])
///   `"#ids": {...}` →  Argument::Ref(ResultReference { ... })
///
/// The resolver in kith-jmap evaluates Ref variants before method dispatch.
///
/// # Deserialization note
/// Uses `#[serde(untagged)]` which tries to deserialize as T first.
/// If T and ResultReference share field names, T is preferred.
/// Callers must handle the `#` key prefix before deserializing.
///
/// # Invariant
///
/// `T` must not deserialize from a JSON object with keys `resultOf`, `name`,
/// and `path`. If `T` accepts arbitrary JSON objects, `#[serde(untagged)]`
/// will match `T` before trying `Ref`, silently swallowing any
/// `ResultReference` payload. The sealed set is chosen specifically to exclude
/// struct types for this reason. Never implement `Sealed` for a type that
/// deserializes from a JSON object.
///
/// # Type safety
/// `T` is constrained to `sealed::Sealed` types. `serde_json::Value` does NOT
/// implement `Sealed` — `Argument<Value>` therefore fails to compile. This
/// prevents a silent bug: `Value` accepts any JSON, so with `T = Value` the
/// `Ref` variant can never be reached via serde; every ResultReference JSON
/// would deserialize as `Argument::Value(json_object)` and the reference would
/// be silently ignored.
///
/// ```compile_fail
/// use jmap_types::resultref::Argument;
/// // serde_json::Value does not implement Sealed — this must not compile.
/// let _: Argument<serde_json::Value> = Argument::Value(serde_json::Value::Null);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument<T: sealed::Sealed> {
    Value(T),
    Ref(ResultReference),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_reference_round_trip() {
        // Independent oracle: RFC 8620 §9 example
        let rr = ResultReference {
            result_of: "0".into(),
            name: "ChatContact/get".into(),
            path: "/list/0/id".into(),
        };
        let json_str = serde_json::to_string(&rr).unwrap();
        let rr2: ResultReference = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rr, rr2);
    }

    #[test]
    fn result_reference_field_names() {
        let rr = ResultReference {
            result_of: "req-1".into(),
            name: "Chat/get".into(),
            path: "/id".into(),
        };
        let json_str = serde_json::to_string(&rr).unwrap();
        assert!(
            json_str.contains("\"resultOf\""),
            "must use camelCase resultOf"
        );
        assert!(json_str.contains("\"name\""));
        assert!(json_str.contains("\"path\""));
        assert!(
            !json_str.contains("\"result_of\""),
            "must not use snake_case"
        );
    }

    #[test]
    fn argument_value_serializes_as_inner_type() {
        let arg: Argument<u32> = Argument::Value(42);
        let json_str = serde_json::to_string(&arg).unwrap();
        assert_eq!(json_str, "42");
    }

    #[test]
    fn argument_ref_serializes_as_result_reference() {
        let rr = ResultReference {
            result_of: "0".into(),
            name: "ChatContact/get".into(),
            path: "/list/0/id".into(),
        };
        let arg: Argument<Vec<String>> = Argument::Ref(rr);
        let json_str = serde_json::to_string(&arg).unwrap();
        assert!(json_str.contains("\"resultOf\""));
        assert!(json_str.contains("\"ChatContact/get\""));
    }

    #[test]
    fn argument_value_vec_string_deserializes() {
        let json_str = r#"["alice","bob"]"#;
        let arg: Argument<Vec<String>> = serde_json::from_str(json_str).unwrap();
        match arg {
            Argument::Value(v) => assert_eq!(v, vec!["alice", "bob"]),
            Argument::Ref(_) => panic!("expected Value variant"),
        }
    }

    // Oracle: tests/fixtures/rfc8620-result-reference.json (RFC 8620 §9)
    #[test]
    fn result_reference_deserializes_from_fixture() {
        let raw = include_str!("../tests/fixtures/rfc8620-result-reference.json");
        let rr: ResultReference = serde_json::from_str(raw).expect("deserialize ResultReference");
        assert_eq!(rr.result_of, "t0");
        assert_eq!(rr.name, "Foo/changes");
        assert_eq!(rr.path, "/created");
    }

    // Oracle: #[serde(untagged)] — ResultReference JSON object deserializes as Argument::Ref.
    #[test]
    fn argument_ref_deserializes() {
        let j = r#"{"resultOf":"0","name":"X/get","path":"/ids"}"#;
        let arg: Argument<Vec<String>> = serde_json::from_str(j).expect("deser");
        match arg {
            Argument::Ref(rr) => {
                assert_eq!(rr.result_of, "0");
                assert_eq!(rr.name, "X/get");
            }
            Argument::Value(_) => panic!("expected Ref"),
        }
    }
}
