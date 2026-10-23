//! draft-ietf-jmap-metadata-04 §1.2.1 / §2.1 — validation helpers.
//!
//! Provides:
//!
//! - [`value_depth`] / [`exceeds_max_depth`] — depth validation for
//!   namespace values per §2.1.
//! - [`is_namespace_supported`] — check whether a namespace identifier
//!   is supported by a given [`DataTypeMetadataInfo`] capability.

use jmap_metadata_types::{DataTypeMetadataInfo, is_registered_namespace, is_vendor_namespace};
use serde_json::Value;

/// Compute the depth of a metadata namespace value per
/// draft-ietf-jmap-metadata-04 §2.1: "a scalar (string, number, boolean
/// or null) has depth 0; an array has the maximum depth of its elements
/// (0 if empty); an object has depth 1 plus the maximum depth of its
/// member values (1 if empty)."
///
/// Arrays do not add depth themselves, but objects inside them do:
/// `{"x": [{"y": 1}]}` has depth 2.
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
/// ```
pub fn value_depth(value: &Value) -> u64 {
    match value {
        Value::Object(map) => 1 + map.values().map(value_depth).max().unwrap_or(0),
        Value::Array(arr) => arr.iter().map(value_depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Returns `true` if `value` exceeds `max_depth` per
/// draft-ietf-jmap-metadata-04 §2.1.
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
/// [`DataTypeMetadataInfo`] capability (draft-ietf-jmap-metadata-04
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- value_depth ------------------------------------------------------
    //
    // Oracle: draft-ietf-jmap-metadata-04 §2.1 — "a scalar (string,
    // number, boolean or null) has depth 0; an array has the maximum
    // depth of its elements (0 if empty); an object has depth 1 plus the
    // maximum depth of its member values (1 if empty)."

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
        // Oracle: -04 §2.1 — "{"a": 1} has depth 1".
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
        // A scalar has depth 0 under -04 §2.1, below the §1.2.1 minimum
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
        // -04 §1.2.1: "maxDepth: UnsignedInt|null"; RFC 8620 §1.3 lets an
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

    // draft-ietf-jmap-metadata-04 §2.1: "Servers MUST reject ... a domain
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
}
