//! draft-ietf-jmap-metadata-05 §1.2.1 / §2.1 — validation helpers.
//!
//! Provides:
//!
//! - [`value_depth`] / [`exceeds_max_depth`] — depth validation for
//!   namespace values per §2.1.
//! - [`is_namespace_supported`] — check whether a namespace identifier
//!   is supported by a given [`DataTypeMetadataInfo`] capability.

use jmap_metadata_types::{
    DataTypeMetadataInfo, is_registered_namespace, is_valid_namespace, is_vendor_namespace,
};
use serde_json::{Map, Value};

use crate::changes::{METADATA_PROPERTY, PRIVATE_METADATA_PROPERTY};

/// Compute the depth of a metadata namespace value per
/// draft-ietf-jmap-metadata-05 §2.1: "a scalar (string, number, boolean
/// or null) or a Binary value (Section 2.1.1) has depth 0; an array has
/// the maximum depth of its elements (0 if empty); an object has depth 1
/// plus the maximum depth of its member values (1 if empty)."
///
/// Arrays do not add depth themselves, but objects inside them do:
/// `{"x": [{"y": 1}]}` has depth 2.
///
/// A Binary value (§2.1.1, an object whose `@type` member is
/// `"Binary"`) therefore has depth 0, like a scalar. A Binary is an
/// object "within a namespace value", so `value` itself is always
/// measured as a plain object even if it has `@type: "Binary"`; only its
/// descendants can be Binary values.
///
/// §2.1 also requires every namespace value to be a JSON object. This
/// function does not check that; the caller rejects a non-object value
/// (see [`serde_json::Value::is_object`]) before measuring depth.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use jmap_metadata_server::value_depth;
///
/// assert_eq!(value_depth(&json!("hello")), 0);
/// assert_eq!(value_depth(&json!({})), 1);
/// assert_eq!(value_depth(&json!({"a": 1})), 1);
/// assert_eq!(value_depth(&json!({"key": {"nested": "val"}})), 2);
/// assert_eq!(value_depth(&json!({"x": [{"y": 1}]})), 2);
/// assert_eq!(value_depth(&json!({"t": {"@type": "Binary", "base64": "AA=="}})), 1);
/// ```
pub fn value_depth(value: &Value) -> u64 {
    let nested = |child: &Value| {
        if is_binary(child) {
            0
        } else {
            value_depth(child)
        }
    };
    match value {
        Value::Object(map) => 1 + map.values().map(nested).max().unwrap_or(0),
        Value::Array(arr) => arr.iter().map(nested).max().unwrap_or(0),
        _ => 0,
    }
}

/// Returns `true` if `value` is a Binary value: draft-ietf-jmap-metadata-05
/// §2.1.1, "Any object within a namespace value that has an @type member
/// with the value "Binary" is a Binary value."
pub(crate) fn is_binary(value: &Value) -> bool {
    value.get("@type").and_then(Value::as_str) == Some("Binary")
}

/// Returns `true` if `binary` is a well-formed Binary value
/// (draft-ietf-jmap-metadata-05 §2.1.1): exactly the members `@type` and
/// `base64`, the latter "encoded in base64 with padding (Section 4 of
/// [RFC4648])".
///
/// ponytail: pad bits are not checked (`"AB=="` is accepted although its
/// unused bits are not zero); -05 does not say, and the question is open
/// on the list.
fn is_well_formed_binary(binary: &Map<String, Value>) -> bool {
    let Some(Value::String(b64)) = binary.get("base64") else {
        return false;
    };
    let body = b64.trim_end_matches('=');
    binary.len() == 2
        && b64.len() % 4 == 0
        && b64.len() - body.len() <= 2
        && body
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
}

/// Returns `true` if a Binary value anywhere below `value` (not `value`
/// itself) is malformed.
fn contains_malformed_binary(value: &Value) -> bool {
    let malformed = |child: &Value| match child {
        Value::Object(map) if is_binary(child) => !is_well_formed_binary(map),
        _ => contains_malformed_binary(child),
    };
    match value {
        Value::Object(map) => map.values().any(malformed),
        Value::Array(arr) => arr.iter().any(malformed),
        _ => false,
    }
}

/// Returns `true` if `value` exceeds `max_depth` per
/// draft-ietf-jmap-metadata-05 §2.1.
///
/// Servers MUST reject writes that would exceed `maxDepth` with an
/// `invalidProperties` SetError. `max_depth` is `u64` to match
/// [`DataTypeMetadataInfo::max_depth`], so a `Some(d)` passes straight
/// through without a narrowing cast. A non-object value has depth 0 and
/// never exceeds a `maxDepth` (which §1.2.1 requires to be at least 1),
/// so the caller must reject it separately, as with [`value_depth`].
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use jmap_metadata_server::exceeds_max_depth;
///
/// let flat = json!({"key": "val"});
/// assert!(!exceeds_max_depth(&flat, 1));
/// assert!(!exceeds_max_depth(&flat, 2));
///
/// let nested = json!({"a": {"b": "c"}});
/// assert!(exceeds_max_depth(&nested, 1));
/// assert!(!exceeds_max_depth(&nested, 2));
/// ```
pub fn exceeds_max_depth(value: &Value, max_depth: u64) -> bool {
    value_depth(value) > max_depth
}

/// Returns `true` if `namespace` is supported by the given
/// [`DataTypeMetadataInfo`] capability (draft-ietf-jmap-metadata-05
/// §1.2.1).
///
/// A namespace is supported if:
/// - It is a registered name (no dot) listed in `info.namespaces`, **or**
/// - It is a vendor domain name (contains a dot) and
///   `info.supports_vendor_namespaces` is `true`.
///
/// Returns `false` for invalid namespace identifiers (empty, containing
/// spaces, etc.).
///
/// # Examples
///
/// ```
/// use jmap_metadata_types::DataTypeMetadataInfo;
/// use jmap_metadata_server::is_namespace_supported;
///
/// let info: DataTypeMetadataInfo = serde_json::from_str(r#"{
///     "namespaces": ["photography"],
///     "supportsVendorNamespaces": false,
///     "supportsPrivate": false,
///     "maxDepth": null
/// }"#).unwrap();
///
/// assert!(is_namespace_supported("photography", &info));
/// assert!(!is_namespace_supported("workflow", &info));
/// assert!(!is_namespace_supported("acme.example.com", &info));
/// ```
pub fn is_namespace_supported(namespace: &str, info: &DataTypeMetadataInfo) -> bool {
    if is_registered_namespace(namespace) {
        info.namespaces.iter().any(|n| n == namespace)
    } else if is_vendor_namespace(namespace) {
        info.supports_vendor_namespaces
    } else {
        false
    }
}

/// Which kind of `Foo/set` entry a write comes from.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetKind {
    /// A `create` entry: a complete object.
    Create,
    /// An `update` entry: a PatchObject (RFC 8620 §5.3).
    Update,
}

/// A metadata write that `Foo/set` rejects with an `invalidProperties`
/// SetError (draft-ietf-jmap-metadata-05 §2.1, §3.2 rules 2 to 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidMetadataWrite {
    property: String,
    description: String,
}

impl InvalidMetadataWrite {
    fn new(property: &str, description: impl Into<String>) -> Self {
        Self {
            property: property.to_owned(),
            description: description.into(),
        }
    }

    /// The request key to list in the SetError's `properties`, such as
    /// `metadata` or `privateMetadata/acme.example.com`.
    pub fn property(&self) -> &str {
        &self.property
    }

    /// A human-readable reason for the SetError's `description`.
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl std::fmt::Display for InvalidMetadataWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid property {:?}: {}",
            self.property, self.description
        )
    }
}

impl std::error::Error for InvalidMetadataWrite {}

/// A `/set` key that writes `metadata` or `privateMetadata`, split into
/// the property name and the rest of the path after it.
fn metadata_target(key: &str) -> Option<(&str, Option<&str>)> {
    let (root, rest) = key
        .split_once('/')
        .map_or((key, None), |(r, p)| (r, Some(p)));
    (root == METADATA_PROPERTY || root == PRIVATE_METADATA_PROPERTY).then_some((root, rest))
}

/// Check the metadata keys of one `Foo/set` entry against §3.2 rules 2
/// and 3, before the backend applies the write.
///
/// `entry` is the create object or the update PatchObject; keys for
/// other properties are ignored. `stored` is the object as stored
/// before an update, used for the §2.1.2 migration case, and [`None`]
/// for a create. The checks, in the order -05 §3.2 requires:
///
/// 2. A `privateMetadata` key when `info.supports_private` is false.
/// 3. A namespace that is not a valid identifier (§2.1), or one that
///    is not supported on the data type, except a removal
///    (`"metadata/<namespace>": null`) or a complete replacement that
///    carries the stored value unchanged (§2.1.2).
///
/// It also rejects a complete `metadata` or `privateMetadata` value that
/// is not an object, and `null` on create (§3.2: "A value of null for
/// either property MUST be rejected"; in an update "null is equivalent
/// to {}"). The caller still owns rule 1 (`notFound`, then `forbidden`
/// without `maySetMetadata`), patch errors (`invalidPatch`: those the
/// request alone determines before rule 1, those that depend on the
/// stored value after this check and before rule 4), and, after applying
/// the write, rule 4 through [`check_metadata_depth`].
///
/// # Errors
///
/// Returns the first [`InvalidMetadataWrite`] found.
///
/// # Examples
///
/// ```
/// use jmap_metadata_server::{SetKind, check_metadata_write};
/// use jmap_metadata_types::DataTypeMetadataInfo;
/// use serde_json::json;
///
/// let info: DataTypeMetadataInfo = serde_json::from_value(json!({
///     "namespaces": ["photography"], "supportsVendorNamespaces": false,
///     "supportsPrivate": false, "maxDepth": 3
/// })).unwrap();
/// let ok = json!({"metadata/photography/album": "sunset"});
/// assert!(check_metadata_write(SetKind::Update, ok.as_object().unwrap(), None, &info).is_ok());
///
/// let bad = json!({"metadata/acme.example.com": {"memo": "x"}});
/// let err = check_metadata_write(SetKind::Update, bad.as_object().unwrap(), None, &info).unwrap_err();
/// assert_eq!(err.property(), "metadata/acme.example.com");
/// ```
pub fn check_metadata_write(
    kind: SetKind,
    entry: &Map<String, Value>,
    stored: Option<&Map<String, Value>>,
    info: &DataTypeMetadataInfo,
) -> Result<(), InvalidMetadataWrite> {
    let targets: Vec<(&str, &str, Option<&str>, &Value)> = entry
        .iter()
        .filter_map(|(key, value)| {
            metadata_target(key).map(|(root, rest)| (key.as_str(), root, rest, value))
        })
        .collect();

    if !info.supports_private
        && let Some((key, ..)) = targets
            .iter()
            .find(|(_, root, ..)| *root == PRIVATE_METADATA_PROPERTY)
    {
        return Err(InvalidMetadataWrite::new(
            key,
            "privateMetadata is not supported on this data type",
        ));
    }

    for (key, root, rest, value) in targets {
        let stored_namespace = |ns: &str| stored?.get(root)?.as_object()?.get(ns);
        match rest {
            None => match value {
                Value::Null if kind == SetKind::Create => {
                    return Err(InvalidMetadataWrite::new(
                        key,
                        "null is not allowed on create; use {}",
                    ));
                }
                Value::Null => {}
                Value::Object(namespaces) => {
                    for (ns, ns_value) in namespaces {
                        if !is_valid_namespace(ns) {
                            return Err(InvalidMetadataWrite::new(
                                key,
                                format!("{ns:?} is not a valid namespace identifier"),
                            ));
                        }
                        if !is_namespace_supported(ns, info)
                            && stored_namespace(ns) != Some(ns_value)
                        {
                            return Err(InvalidMetadataWrite::new(
                                key,
                                format!("namespace {ns:?} is not supported on this data type"),
                            ));
                        }
                    }
                }
                _ => return Err(InvalidMetadataWrite::new(key, "value must be an object")),
            },
            Some(path) => {
                let ns = path.split('/').next().unwrap_or(path);
                if !is_valid_namespace(ns) {
                    return Err(InvalidMetadataWrite::new(
                        key,
                        format!("{ns:?} is not a valid namespace identifier"),
                    ));
                }
                let removes_namespace = path == ns && value.is_null();
                if !is_namespace_supported(ns, info) && !removes_namespace {
                    return Err(InvalidMetadataWrite::new(
                        key,
                        format!("namespace {ns:?} is not supported on this data type"),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Check §3.2 rules 4 and 5 and the §2.1 object rule for every namespace a
/// `Foo/set` entry wrote, after the backend has applied the write.
///
/// `entry` is the same create object or PatchObject passed to
/// [`check_metadata_write`], and `written` is the object as it would be
/// stored. For each supported namespace a metadata key of `entry`
/// targets that is still present in `written`, the value must be a JSON
/// object (§2.1: "The value associated with each namespace key MUST be
/// a JSON object") whose depth does not exceed `info.max_depth` (§2.1:
/// "servers MUST reject writes that would exceed it with an
/// "invalidProperties" SetError"). Then, as §3.2 rule 5 of
/// draft-ietf-jmap-metadata-05, no namespace value may contain a
/// malformed Binary value (§2.1.1: "A Binary value that has any other
/// member, or whose base64 member is not valid base64 with padding, is
/// malformed"). Rule 4 is checked for every namespace before rule 5, as
/// §3.2 requires ("the first matching rule determines the SetError").
/// Unsupported namespaces are skipped: [`check_metadata_write`] has
/// already limited them to removal or the stored value.
///
/// # Errors
///
/// Returns the first [`InvalidMetadataWrite`] found.
///
/// # Examples
///
/// ```
/// use jmap_metadata_server::check_metadata_depth;
/// use jmap_metadata_types::DataTypeMetadataInfo;
/// use serde_json::json;
///
/// let info: DataTypeMetadataInfo = serde_json::from_value(json!({
///     "namespaces": ["photography"], "supportsVendorNamespaces": false,
///     "supportsPrivate": false, "maxDepth": 1
/// })).unwrap();
/// let patch = json!({"metadata/photography/camera": {"model": "X"}});
/// let written = json!({"metadata": {"photography": {"camera": {"model": "X"}}}});
/// let err = check_metadata_depth(patch.as_object().unwrap(), written.as_object().unwrap(), &info)
///     .unwrap_err();
/// assert_eq!(err.property(), "metadata/photography/camera");
/// ```
pub fn check_metadata_depth(
    entry: &Map<String, Value>,
    written: &Map<String, Value>,
    info: &DataTypeMetadataInfo,
) -> Result<(), InvalidMetadataWrite> {
    let mut values = Vec::new();
    for (key, value) in entry {
        let Some((root, rest)) = metadata_target(key) else {
            continue;
        };
        let touched: Vec<&str> = match rest {
            None => value
                .as_object()
                .map_or_else(Vec::new, |m| m.keys().map(String::as_str).collect()),
            Some(path) => vec![path.split('/').next().unwrap_or(path)],
        };
        let Some(stored) = written.get(root).and_then(Value::as_object) else {
            continue;
        };
        for ns in touched
            .into_iter()
            .filter(|ns| is_namespace_supported(ns, info))
        {
            let Some(ns_value) = stored.get(ns) else {
                continue;
            };
            if !ns_value.is_object() {
                return Err(InvalidMetadataWrite::new(
                    key,
                    format!("namespace {ns:?} value must be an object"),
                ));
            }
            if let Some(max) = info.max_depth
                && exceeds_max_depth(ns_value, max)
            {
                return Err(InvalidMetadataWrite::new(
                    key,
                    format!("namespace {ns:?} value exceeds maxDepth {max}"),
                ));
            }
            values.push((key, ns, ns_value));
        }
    }
    match values
        .into_iter()
        .find(|(.., ns_value)| contains_malformed_binary(ns_value))
    {
        Some((key, ns, _)) => Err(InvalidMetadataWrite::new(
            key,
            format!("namespace {ns:?} value contains a malformed Binary value"),
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- value_depth ------------------------------------------------------
    //
    // Oracle: draft-ietf-jmap-metadata-05 §2.1 — "a scalar (string,
    // number, boolean or null) or a Binary value (Section 2.1.1) has depth
    // 0; an array has the maximum depth of its elements (0 if empty); an
    // object has depth 1 plus the maximum depth of its member values (1 if
    // empty)."

    #[test]
    fn depth_scalar_string() {
        assert_eq!(value_depth(&json!("hello")), 0);
    }

    #[test]
    fn depth_scalar_number() {
        assert_eq!(value_depth(&json!(42)), 0);
    }

    #[test]
    fn depth_scalar_bool() {
        assert_eq!(value_depth(&json!(true)), 0);
    }

    #[test]
    fn depth_scalar_null() {
        assert_eq!(value_depth(&json!(null)), 0);
    }

    #[test]
    fn depth_empty_array() {
        assert_eq!(value_depth(&json!([])), 0);
    }

    #[test]
    fn depth_empty_object() {
        assert_eq!(value_depth(&json!({})), 1);
    }

    #[test]
    fn depth_spec_example_flat() {
        // Oracle: -05 §2.1 — "{"a": 1} has depth 1".
        assert_eq!(value_depth(&json!({"a": 1})), 1);
    }

    #[test]
    fn depth_array_of_scalars() {
        assert_eq!(value_depth(&json!([1, "a", null])), 0);
    }

    #[test]
    fn depth_array_of_objects() {
        assert_eq!(value_depth(&json!([{}, {"a": {}}])), 2);
    }

    #[test]
    fn depth_flat_object() {
        assert_eq!(value_depth(&json!({"key": "val", "n": 1})), 1);
    }

    #[test]
    fn depth_one_level_nesting() {
        assert_eq!(value_depth(&json!({"key": {"nested": "val"}})), 2);
    }

    #[test]
    fn depth_two_levels_nesting() {
        assert_eq!(value_depth(&json!({"a": {"b": {"c": "d"}}})), 3);
    }

    #[test]
    fn depth_array_with_objects_spec_example() {
        // Oracle: §2.1 — {"x": [{"y": 1}]} has depth 2.
        assert_eq!(value_depth(&json!({"x": [{"y": 1}]})), 2);
    }

    #[test]
    fn depth_array_with_scalars_only() {
        // Arrays of scalars don't add nesting.
        assert_eq!(value_depth(&json!({"tags": ["a", "b"]})), 1);
    }

    #[test]
    fn depth_nested_array_with_nested_objects() {
        // {"a": [[{"b": {"c": 1}}]]} — inner object is depth 2, outer object adds 1 → 3.
        assert_eq!(value_depth(&json!({"a": [[{"b": {"c": 1}}]]})), 3);
    }

    #[test]
    fn depth_mixed_values() {
        // Depth is the maximum across all branches.
        let v = json!({
            "flat": "val",
            "nested": {"deep": {"deeper": 1}},
            "arr": [{"shallow": 2}]
        });
        assert_eq!(value_depth(&v), 3); // nested branch
    }

    // -- exceeds_max_depth ------------------------------------------------

    #[test]
    fn exceeds_scalar_never_at_max_1() {
        // A scalar has depth 0 under -05 §2.1, below the §1.2.1 minimum
        // maxDepth of 1; object-ness is the caller's check.
        assert!(!exceeds_max_depth(&json!("hello"), 1));
    }

    #[test]
    fn exceeds_flat_at_max_1() {
        assert!(!exceeds_max_depth(&json!({"key": "val"}), 1));
    }

    #[test]
    fn exceeds_nested_at_max_1() {
        assert!(exceeds_max_depth(&json!({"a": {"b": "c"}}), 1));
    }

    #[test]
    fn exceeds_nested_at_max_2() {
        assert!(!exceeds_max_depth(&json!({"a": {"b": "c"}}), 2));
    }

    #[test]
    fn exceeds_three_levels_at_max_2() {
        assert!(exceeds_max_depth(&json!({"a": {"b": {"c": "d"}}}), 2));
    }

    #[test]
    fn exceeds_max_above_u32_range() {
        // -05 §1.2.1: "maxDepth: UnsignedInt|null"; RFC 8620 §1.3 lets an
        // UnsignedInt reach 2^53-1. A maxDepth of 2^32 permits depth 2.
        assert!(!exceeds_max_depth(&json!({"a": {"b": "c"}}), 4_294_967_296));
        assert!(!exceeds_max_depth(&json!({"a": 1}), 9_007_199_254_740_991));
    }

    // -- is_namespace_supported -------------------------------------------

    fn info_with_registered(namespaces: &[&str], vendor: bool) -> DataTypeMetadataInfo {
        serde_json::from_value(json!({
            "namespaces": namespaces,
            "supportsVendorNamespaces": vendor,
            "supportsPrivate": false,
            "maxDepth": null
        }))
        .unwrap()
    }

    #[test]
    fn supported_registered_listed() {
        let info = info_with_registered(&["photography"], false);
        assert!(is_namespace_supported("photography", &info));
    }

    #[test]
    fn supported_registered_not_listed() {
        let info = info_with_registered(&["photography"], false);
        assert!(!is_namespace_supported("workflow", &info));
    }

    #[test]
    fn supported_vendor_allowed() {
        let info = info_with_registered(&[], true);
        assert!(is_namespace_supported("acme.example.com", &info));
    }

    #[test]
    fn supported_vendor_not_allowed() {
        let info = info_with_registered(&[], false);
        assert!(!is_namespace_supported("acme.example.com", &info));
    }

    #[test]
    fn supported_invalid_namespace() {
        let info = info_with_registered(&[], true);
        assert!(!is_namespace_supported("", &info));
        assert!(!is_namespace_supported("has space", &info));
    }

    // draft-ietf-jmap-metadata-05 §2.1: "Servers MUST reject ... a domain
    // name containing uppercase ASCII letters"; registered names are
    // "lowercase US-ASCII letters, digits, hyphens, and underscores".
    #[test]
    fn supported_rejects_uppercase() {
        let info = info_with_registered(&["photography"], true);
        assert!(!is_namespace_supported("Photography", &info));
        assert!(!is_namespace_supported("Acme.example.com", &info));
    }

    #[test]
    fn supported_spec_example_section_6_1() {
        // Oracle: §6.1 — the FileNode entry: namespaces: ["photography"],
        // supportsVendorNamespaces: false.
        let info: DataTypeMetadataInfo = serde_json::from_str(
            r#"{
                "namespaces": ["photography"],
                "supportsVendorNamespaces": false,
                "supportsPrivate": false,
                "maxDepth": 3
            }"#,
        )
        .unwrap();
        assert!(is_namespace_supported("photography", &info));
        assert!(!is_namespace_supported("workflow", &info));
        assert!(!is_namespace_supported("acme.example.com", &info));
    }

    // -- check_metadata_write / check_metadata_depth ----------------------

    /// The -05 §6.1 FileNode entry: namespaces ["photography"], no vendor
    /// namespaces, no private metadata, maxDepth 3.
    fn filenode_info() -> DataTypeMetadataInfo {
        serde_json::from_value(json!({
            "namespaces": ["photography"],
            "supportsVendorNamespaces": false,
            "supportsPrivate": false,
            "maxDepth": 3
        }))
        .unwrap()
    }

    fn vendor_info(max_depth: u64) -> DataTypeMetadataInfo {
        serde_json::from_value(json!({
            "namespaces": [],
            "supportsVendorNamespaces": true,
            "supportsPrivate": true,
            "maxDepth": max_depth
        }))
        .unwrap()
    }

    fn write(
        kind: SetKind,
        entry: Value,
        stored: Option<Value>,
        info: &DataTypeMetadataInfo,
    ) -> Result<(), InvalidMetadataWrite> {
        check_metadata_write(
            kind,
            entry.as_object().unwrap(),
            stored.as_ref().and_then(Value::as_object),
            info,
        )
    }

    #[test]
    fn write_spec_6_3_patches_accepted() {
        // Oracle: -05 §6.3 — "metadata/acme.example.com/color": "green"
        // and the same path set to null.
        let info = vendor_info(3);
        assert!(
            write(
                SetKind::Update,
                json!({"metadata/acme.example.com/color": "green"}),
                None,
                &info
            )
            .is_ok()
        );
        assert!(
            write(
                SetKind::Update,
                json!({"metadata/acme.example.com/color": null}),
                None,
                &info
            )
            .is_ok()
        );
    }

    #[test]
    fn write_ignores_other_properties() {
        let info = filenode_info();
        assert!(
            write(
                SetKind::Update,
                json!({"name": "x", "metadataX": 1}),
                None,
                &info
            )
            .is_ok()
        );
    }

    #[test]
    fn write_private_rejected_before_namespace() {
        // Oracle: -05 §3.2 rule 2 — "If the path begins with
        // privateMetadata and supportsPrivate is false for this data type,
        // reject with "invalidProperties"", evaluated before rule 3.
        let info = filenode_info();
        let err = write(
            SetKind::Update,
            json!({"metadata/acme.example.com": {}, "privateMetadata/photography": {}}),
            None,
            &info,
        )
        .unwrap_err();
        assert_eq!(err.property(), "privateMetadata/photography");
    }

    #[test]
    fn write_unsupported_namespace_rejected() {
        // Oracle: -05 §3.2 rule 3 — "If the targeted namespace is not
        // supported on this data type, reject with "invalidProperties"".
        let info = filenode_info();
        for entry in [
            json!({"metadata/workflow": {"step": 1}}),
            json!({"metadata/acme.example.com/k": 1}),
            json!({"metadata": {"workflow": {}}}),
        ] {
            assert!(
                write(SetKind::Update, entry.clone(), None, &info).is_err(),
                "{entry}"
            );
        }
    }

    #[test]
    fn write_invalid_namespace_identifier_rejected() {
        // Oracle: -05 §2.1 — "Servers MUST reject, with an
        // "invalidProperties" SetError, a /set operation that uses a key
        // that is not a valid namespace identifier, including a domain
        // name containing uppercase ASCII letters."
        let info = vendor_info(3);
        for entry in [
            json!({"metadata/Acme.example.com": {}}),
            json!({"metadata": {"Acme.example.com": {}}}),
            json!({"metadata/": {}}),
        ] {
            assert!(
                write(SetKind::Update, entry.clone(), None, &info).is_err(),
                "{entry}"
            );
        }
    }

    #[test]
    fn write_unsupported_namespace_migration_case() {
        // Oracle: -05 §2.1.2 — other than "a patch that removes the
        // namespace entirely ("metadata/<namespace>": null ...)", any /set
        // targeting it is rejected; "A complete replacement ... MAY include
        // such a namespace only with a value equal to the stored one ...
        // regardless of member order".
        let info = filenode_info();
        let stored = json!({"metadata": {"workflow": {"a": 1, "b": 2}, "photography": {}}});
        assert!(
            write(
                SetKind::Update,
                json!({"metadata/workflow": null}),
                Some(stored.clone()),
                &info
            )
            .is_ok()
        );
        assert!(
            write(
                SetKind::Update,
                json!({"metadata/workflow/a": null}),
                Some(stored.clone()),
                &info
            )
            .is_err()
        );
        assert!(
            write(
                SetKind::Update,
                json!({"metadata": {"workflow": {"b": 2, "a": 1}, "photography": {"x": 1}}}),
                Some(stored.clone()),
                &info
            )
            .is_ok()
        );
        assert!(
            write(
                SetKind::Update,
                json!({"metadata": {"workflow": {"a": 1}}}),
                Some(stored.clone()),
                &info
            )
            .is_err()
        );
        // Omitting it from a complete replacement removes it, which is allowed.
        assert!(
            write(
                SetKind::Update,
                json!({"metadata": {}}),
                Some(stored),
                &info
            )
            .is_ok()
        );
    }

    #[test]
    fn write_null_property_on_create_rejected_update_accepted() {
        // Oracle: -05 §3.2 — "A value of null for either property MUST be
        // rejected with an "invalidProperties" SetError ... In an update,
        // null is equivalent to {}."
        let info = vendor_info(3);
        let err = write(SetKind::Create, json!({"metadata": null}), None, &info).unwrap_err();
        assert_eq!(err.property(), "metadata");
        assert!(write(SetKind::Update, json!({"metadata": null}), None, &info).is_ok());
        assert!(write(SetKind::Create, json!({"metadata": {}}), None, &info).is_ok());
        assert!(write(SetKind::Create, json!({"metadata": []}), None, &info).is_err());
    }

    fn depth(
        entry: Value,
        written: Value,
        info: &DataTypeMetadataInfo,
    ) -> Result<(), InvalidMetadataWrite> {
        check_metadata_depth(
            entry.as_object().unwrap(),
            written.as_object().unwrap(),
            info,
        )
    }

    #[test]
    fn depth_rejects_written_value_over_max() {
        // Oracle: -05 §2.1 — "{"x": [{"y": 1}]} has depth 2" and writes
        // that "would exceed" maxDepth are rejected with
        // "invalidProperties".
        let info = vendor_info(1);
        let err = depth(
            json!({"metadata/x.example/x": [{"y": 1}]}),
            json!({"metadata": {"x.example": {"x": [{"y": 1}]}}}),
            &info,
        )
        .unwrap_err();
        assert_eq!(err.property(), "metadata/x.example/x");
        assert!(
            depth(
                json!({"metadata": {"x.example": {"a": 1}}}),
                json!({"metadata": {"x.example": {"a": 1}}}),
                &info
            )
            .is_ok()
        );
    }

    #[test]
    fn depth_rejects_non_object_namespace_value() {
        // Oracle: -05 §2.1 — "The value associated with each namespace key
        // MUST be a JSON object".
        let info = vendor_info(3);
        assert!(
            depth(
                json!({"privateMetadata/x.example": "s"}),
                json!({"privateMetadata": {"x.example": "s"}}),
                &info
            )
            .is_err()
        );
        assert!(
            depth(
                json!({"metadata": {"x.example": 1}}),
                json!({"metadata": {"x.example": 1}}),
                &info
            )
            .is_err()
        );
    }

    #[test]
    fn depth_skips_removed_and_untouched_namespaces() {
        // A removed namespace has no value to measure; an untouched one
        // was not written (-05 §2.1: "A patch that targets one namespace
        // MUST NOT remove or alter any other namespace").
        let info = vendor_info(1);
        let written = json!({"metadata": {"y.example": {"deep": {"deeper": 1}}}});
        assert!(depth(json!({"metadata/x.example": null}), written.clone(), &info).is_ok());
        assert!(depth(json!({"name": "n"}), written, &info).is_ok());
    }

    // -- Binary values (draft-ietf-jmap-metadata-05 §2.1.1) ---------------

    /// The -05 §2.1.1 example: "a vendor namespace holding a 32-byte
    /// value".
    fn spec_binary_namespace() -> Value {
        json!({
            "label": "draft copy",
            "thumbprint": {
                "@type": "Binary",
                "base64": "VEVYVHR0eHQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            }
        })
    }

    #[test]
    fn depth_binary_counts_as_scalar() {
        // Oracle: -05 §2.1 — "a scalar (string, number, boolean or null)
        // or a Binary value (Section 2.1.1) has depth 0"; the example
        // namespace value is therefore an object of depth-0 members.
        assert_eq!(value_depth(&spec_binary_namespace()), 1);
        assert_eq!(
            value_depth(&json!({"l": [{"@type": "Binary", "base64": ""}]})),
            1
        );
    }

    #[test]
    fn root_level_binary_type_does_not_bypass_max_depth() {
        // Oracle: -05 §2.1.1 — a Binary is "Any object within a namespace
        // value", so the namespace value itself is measured as an object:
        // by the §2.1 depth rule this one has depth 4. §3.2 rule 4: "If a
        // namespace value would exceed maxDepth, reject with
        // "invalidProperties"" (maxDepth here is 1).
        let info = vendor_info(1);
        let ns = json!({"@type": "Binary", "x": {"a": {"b": {"c": 1}}}});
        assert_eq!(value_depth(&ns), 4);
        let err = depth(
            json!({"metadata/x.example": ns}),
            json!({"metadata": {"x.example": ns}}),
            &info,
        )
        .expect_err("deep root-level @type:Binary value must exceed maxDepth");
        assert_eq!(err.property(), "metadata/x.example");
    }

    #[test]
    fn binary_spec_example_accepted_at_max_depth_1() {
        // Oracle: -05 §2.1.1 example, written whole and by patch.
        let info = vendor_info(1);
        let ns = spec_binary_namespace();
        assert!(
            depth(
                json!({"metadata": {"acme.example.com": ns}}),
                json!({"metadata": {"acme.example.com": ns}}),
                &info
            )
            .is_ok()
        );
        assert!(
            depth(
                json!({"metadata/acme.example.com/thumbprint": ns["thumbprint"]}),
                json!({"metadata": {"acme.example.com": ns}}),
                &info
            )
            .is_ok()
        );
    }

    #[test]
    fn malformed_binary_rejected() {
        // Oracle: -05 §2.1.1 — "A Binary value that has any other member,
        // or whose base64 member is not valid base64 with padding, is
        // malformed"; §3.2 rule 5 — "reject with "invalidProperties"".
        // RFC 4648 §4: "AA" is unpadded, "A===" has three pad characters,
        // "A*==" uses a character outside the alphabet, "AA=A" pads
        // mid-string, and "AA-_" is the base64url alphabet of §5.
        let info = vendor_info(3);
        let malformed = [
            json!({"@type": "Binary", "base64": "AA==", "extra": 1}),
            json!({"@type": "Binary"}),
            json!({"@type": "Binary", "base64": 7}),
            json!({"@type": "Binary", "base64": "AA"}),
            json!({"@type": "Binary", "base64": "A==="}),
            json!({"@type": "Binary", "base64": "A*=="}),
            json!({"@type": "Binary", "base64": "AA=A"}),
            json!({"@type": "Binary", "base64": "AA-_"}),
        ];
        for bin in malformed {
            for ns_value in [json!({"b": bin}), json!({"l": [{"deep": bin}]})] {
                let err = depth(
                    json!({"privateMetadata/x.example": ns_value}),
                    json!({"privateMetadata": {"x.example": ns_value}}),
                    &info,
                )
                .expect_err("malformed Binary must be rejected");
                assert_eq!(err.property(), "privateMetadata/x.example", "{ns_value}");
            }
        }
    }

    #[test]
    fn well_formed_binary_variants_accepted() {
        // Oracle: RFC 4648 §10 test vectors: BASE64("") = "",
        // BASE64("f") = "Zg==", BASE64("foo") = "Zm9v", BASE64("fooba") = "Zm9vYmE=";
        // "+/+/" uses the two non-alphanumeric characters of the §4 alphabet.
        let info = vendor_info(3);
        for b64 in ["", "Zg==", "Zm9v", "Zm9vYmE=", "+/+/"] {
            let ns_value = json!({"b": {"@type": "Binary", "base64": b64}});
            assert!(
                depth(
                    json!({"metadata": {"x.example": ns_value}}),
                    json!({"metadata": {"x.example": ns_value}}),
                    &info
                )
                .is_ok(),
                "{b64:?}"
            );
        }
    }

    #[test]
    fn depth_rule_precedes_malformed_binary_rule() {
        // Oracle: -05 §3.2 — rules are evaluated "in the order listed; the
        // first matching rule determines the SetError": rule 4 (maxDepth)
        // before rule 5 (malformed Binary), across namespaces.
        let info = vendor_info(1);
        let bad_bin = json!({"b": {"@type": "Binary", "base64": "AA"}});
        let too_deep = json!({"a": {"b": 1}});
        let err = depth(
            json!({"metadata/a.example": bad_bin, "metadata/b.example": too_deep}),
            json!({"metadata": {"a.example": bad_bin, "b.example": too_deep}}),
            &info,
        )
        .unwrap_err();
        assert_eq!(err.property(), "metadata/b.example");
    }
}
