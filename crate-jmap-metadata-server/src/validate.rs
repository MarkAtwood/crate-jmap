//! draft-ietf-jmap-metadata-02 §1.2.1 / §2.1 — validation helpers.
//!
//! Provides:
//!
//! - [`value_depth`] / [`exceeds_max_depth`] — depth validation for
//!   namespace values per §2.1.
//! - [`is_namespace_supported`] — check whether a namespace identifier
//!   is supported by a given [`DataTypeMetadataInfo`] capability.

use jmap_metadata_types::{is_registered_namespace, is_vendor_namespace, DataTypeMetadataInfo};
use serde_json::Value;

/// Count the object-nesting depth within a value.
///
/// - Scalars: 0 (no nesting)
/// - Arrays: transparent — max nesting of any element (0 if empty)
/// - Objects: 1 + max nesting of any value (1 if empty)
fn object_nesting(value: &Value) -> u32 {
    match value {
        Value::Object(map) => 1 + map.values().map(object_nesting).max().unwrap_or(0),
        Value::Array(arr) => arr.iter().map(object_nesting).max().unwrap_or(0),
        _ => 0,
    }
}

/// Compute the nesting depth of a metadata namespace value per
/// draft-ietf-jmap-metadata-02 §2.1.
///
/// Depth is the longest path of nested *objects* from the value to any
/// descendant:
///
/// - A scalar, empty array, or empty object has depth 1.
/// - A flat object (values are all scalars or arrays) has depth 1.
/// - An object containing nested objects has depth 2+.
/// - Arrays do not contribute to depth themselves, but objects inside
///   arrays do. `{"x": [{"y": 1}]}` has depth 2.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use jmap_metadata_server::value_depth;
///
/// assert_eq!(value_depth(&json!("hello")), 1);
/// assert_eq!(value_depth(&json!({})), 1);
/// assert_eq!(value_depth(&json!({"key": "val"})), 1);
/// assert_eq!(value_depth(&json!({"key": {"nested": "val"}})), 2);
/// assert_eq!(value_depth(&json!({"x": [{"y": 1}]})), 2);
/// ```
pub fn value_depth(value: &Value) -> u32 {
    object_nesting(value).max(1)
}

/// Returns `true` if `value` exceeds `max_depth` per §2.1.
///
/// Servers MUST reject patches that would produce a structure exceeding
/// `maxDepth` with an `invalidProperties` SetError.
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
pub fn exceeds_max_depth(value: &Value, max_depth: u32) -> bool {
    value_depth(value) > max_depth
}

/// Returns `true` if `namespace` is supported by the given
/// [`DataTypeMetadataInfo`] capability (draft-ietf-jmap-metadata-02
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

    #[test]
    fn depth_scalar_string() {
        assert_eq!(value_depth(&json!("hello")), 1);
    }

    #[test]
    fn depth_scalar_number() {
        assert_eq!(value_depth(&json!(42)), 1);
    }

    #[test]
    fn depth_scalar_bool() {
        assert_eq!(value_depth(&json!(true)), 1);
    }

    #[test]
    fn depth_scalar_null() {
        assert_eq!(value_depth(&json!(null)), 1);
    }

    #[test]
    fn depth_empty_array() {
        assert_eq!(value_depth(&json!([])), 1);
    }

    #[test]
    fn depth_empty_object() {
        assert_eq!(value_depth(&json!({})), 1);
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

    #[test]
    fn supported_spec_example_section_6_1() {
        // Oracle: §6.1 — Email type with namespaces: ["photography"],
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
