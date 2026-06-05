//! jmap-metadata-client — JMAP Object Metadata extension client helpers.
//!
//! Implements the client-side helpers for
//! [draft-ietf-jmap-metadata-02](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/).
//!
//! ## Architecture change from -01 to -02
//!
//! The -02 revision eliminates the standalone `Metadata` object type.
//! Metadata becomes `metadata` and `privateMetadata` properties on
//! each opted-in JMAP data type. Clients read and write metadata
//! through the existing `Foo/get`, `Foo/set`, `Foo/changes`, and
//! `Foo/query` methods — there are no separate `Metadata/*` methods.
//!
//! This crate no longer provides a `SessionClient` or method bindings.
//! Instead, it provides **pure helper functions** that consumers use
//! alongside their existing extension-client crate calls:
//!
//! - **Property constants** — [`METADATA_PROPERTY`], [`PRIVATE_METADATA_PROPERTY`]
//!   for inclusion in `/get` `properties` lists.
//! - **Filter-condition builders** — [`metadata_exists_condition`],
//!   [`metadata_text_contains_condition`], [`metadata_text_equals_condition`],
//!   and their `private_metadata_*` counterparts, producing JSON values
//!   suitable for merging into per-type `FilterCondition` objects.
//! - **`/changes` helpers** — [`IGNORE_METADATA_ONLY_CHANGES`] arg name,
//!   [`is_metadata_property`], [`is_metadata_only_change`] for classifying
//!   `updatedProperties` in `/changes` responses.
//!
//! All functions are stateless and operate on string/JSON values.
//!
//! # Migration from -01
//!
//! The -01 crate exposed `JmapMetadataExt`, `SessionClient`,
//! `metadata_get`, `metadata_set`, `metadata_changes`, `metadata_query`,
//! `metadata_query_changes`, five `*Params` structs, and `MetadataChangesParams`.
//! All of those are removed in -02. Extension-client crates that previously
//! depended on this crate for standalone `Metadata/*` methods should instead
//! use the helper functions from this crate alongside their own typed method
//! calls (e.g. `email_get` with metadata properties, `email_query` with
//! metadata filter conditions).
//!
//! # Example
//!
//! ```rust
//! use jmap_metadata_client::{
//!     METADATA_PROPERTY,
//!     PRIVATE_METADATA_PROPERTY,
//!     metadata_exists_condition,
//!     metadata_text_contains_condition,
//! };
//!
//! // Include metadata properties in a /get properties list
//! let properties = vec!["id", "subject", METADATA_PROPERTY, PRIVATE_METADATA_PROPERTY];
//! assert!(properties.contains(&"metadata"));
//!
//! // Build filter conditions for a /query request
//! let exists = metadata_exists_condition("photography");
//! assert_eq!(exists, serde_json::json!({ "metadataExists": "photography" }));
//!
//! let text_match = metadata_text_contains_condition("photography/caption", "sunset");
//! assert_eq!(text_match, serde_json::json!({
//!     "metadataTextContains": { "path": "photography/caption", "value": "sunset" }
//! }));
//! ```

#![forbid(unsafe_code)]

// ---------------------------------------------------------------------------
// Re-exports from jmap-metadata-types
// ---------------------------------------------------------------------------

/// Capability URI for `urn:ietf:params:jmap:metadata`.
pub use jmap_metadata_types::JMAP_METADATA_URI;

/// Per-data-type metadata capability advertisement (§1.2.1).
pub use jmap_metadata_types::DataTypeMetadataInfo;

/// Value shape for text-matching filter conditions (§3.5).
pub use jmap_metadata_types::MetadataTextMatch;

/// Namespace identifier validation helpers.
pub use jmap_metadata_types::{is_registered_namespace, is_valid_namespace, is_vendor_namespace};

// ---------------------------------------------------------------------------
// Property name constants (§2)
// ---------------------------------------------------------------------------

/// Wire name of the shared metadata property on opted-in data types (§2).
///
/// Include in the `properties` list of a `/get` request to fetch the
/// per-namespace shared metadata map.
pub const METADATA_PROPERTY: &str = "metadata";

/// Wire name of the per-user private metadata property on opted-in data
/// types (§2).
///
/// Include in the `properties` list of a `/get` request to fetch the
/// per-namespace per-user metadata map. Only available when the account's
/// [`DataTypeMetadataInfo::supports_private`] is `true`.
pub const PRIVATE_METADATA_PROPERTY: &str = "privateMetadata";

// ---------------------------------------------------------------------------
// /changes helpers (§2.4, §2.5)
// ---------------------------------------------------------------------------

/// Wire name of the `ignoreMetadataOnlyChanges` argument for `/changes`
/// requests (§2.4).
///
/// When set to `true` in a `/changes` request, objects whose only changes
/// are to `metadata` and/or `privateMetadata` properties are excluded from
/// the `updated` array in the response.
pub const IGNORE_METADATA_ONLY_CHANGES: &str = "ignoreMetadataOnlyChanges";

/// Returns `true` if `property` is one of the metadata-extension properties
/// (`metadata` or `privateMetadata`).
///
/// Useful for classifying the `updatedProperties` array in a `/changes`
/// response (§2.5) to determine whether a change is metadata-only.
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::is_metadata_property;
///
/// assert!(is_metadata_property("metadata"));
/// assert!(is_metadata_property("privateMetadata"));
/// assert!(!is_metadata_property("subject"));
/// assert!(!is_metadata_property("id"));
/// ```
pub fn is_metadata_property(property: &str) -> bool {
    property == METADATA_PROPERTY || property == PRIVATE_METADATA_PROPERTY
}

/// Returns `true` if ALL properties in `updated_properties` are metadata
/// properties (§2.5).
///
/// A `/changes` response with `updatedProperties` present indicates that
/// only those properties changed. If every property in the list is a
/// metadata property, the change is metadata-only and may be skipped by
/// clients that don't need metadata state updates.
///
/// Returns `false` for an empty slice (no properties ≠ metadata-only).
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::is_metadata_only_change;
///
/// assert!(is_metadata_only_change(&["metadata".to_string()]));
/// assert!(is_metadata_only_change(&["metadata".to_string(), "privateMetadata".to_string()]));
/// assert!(!is_metadata_only_change(&["metadata".to_string(), "subject".to_string()]));
/// assert!(!is_metadata_only_change(&[]));
/// ```
pub fn is_metadata_only_change(updated_properties: &[String]) -> bool {
    !updated_properties.is_empty() && updated_properties.iter().all(|p| is_metadata_property(p))
}

// ---------------------------------------------------------------------------
// Filter-condition builders (§3.5)
// ---------------------------------------------------------------------------

/// Build a `metadataExists` filter condition (§3.5).
///
/// Returns a JSON object `{ "metadataExists": "<path>" }` suitable for
/// merging into a per-type `FilterCondition` object in a `/query` request.
///
/// `path` is a namespace identifier or `<namespace>/<key>` path with
/// RFC 6901 escaping where applicable.
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::metadata_exists_condition;
/// use serde_json::json;
///
/// assert_eq!(
///     metadata_exists_condition("photography"),
///     json!({ "metadataExists": "photography" })
/// );
/// assert_eq!(
///     metadata_exists_condition("acme.example.com/memo"),
///     json!({ "metadataExists": "acme.example.com/memo" })
/// );
/// ```
pub fn metadata_exists_condition(path: &str) -> serde_json::Value {
    serde_json::json!({ "metadataExists": path })
}

/// Build a `privateMetadataExists` filter condition (§3.5).
///
/// Same as [`metadata_exists_condition`] but targets `privateMetadata`.
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::private_metadata_exists_condition;
/// use serde_json::json;
///
/// assert_eq!(
///     private_metadata_exists_condition("photography"),
///     json!({ "privateMetadataExists": "photography" })
/// );
/// ```
pub fn private_metadata_exists_condition(path: &str) -> serde_json::Value {
    serde_json::json!({ "privateMetadataExists": path })
}

/// Build a `metadataTextContains` filter condition (§3.5).
///
/// Returns a JSON object suitable for merging into a per-type
/// `FilterCondition`. Matches objects where the string value at `path`
/// in the `metadata` map contains `value` as a substring.
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::metadata_text_contains_condition;
/// use serde_json::json;
///
/// assert_eq!(
///     metadata_text_contains_condition("photography/caption", "sunset"),
///     json!({ "metadataTextContains": { "path": "photography/caption", "value": "sunset" } })
/// );
/// ```
pub fn metadata_text_contains_condition(path: &str, value: &str) -> serde_json::Value {
    serde_json::json!({
        "metadataTextContains": {
            "path": path,
            "value": value,
        }
    })
}

/// Build a `privateMetadataTextContains` filter condition (§3.5).
///
/// Same as [`metadata_text_contains_condition`] but targets `privateMetadata`.
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::private_metadata_text_contains_condition;
/// use serde_json::json;
///
/// assert_eq!(
///     private_metadata_text_contains_condition("workflow/note", "urgent"),
///     json!({ "privateMetadataTextContains": { "path": "workflow/note", "value": "urgent" } })
/// );
/// ```
pub fn private_metadata_text_contains_condition(path: &str, value: &str) -> serde_json::Value {
    serde_json::json!({
        "privateMetadataTextContains": {
            "path": path,
            "value": value,
        }
    })
}

/// Build a `metadataTextEquals` filter condition (§3.5).
///
/// Returns a JSON object suitable for merging into a per-type
/// `FilterCondition`. Matches objects where the string value at `path`
/// in the `metadata` map is exactly equal to `value`.
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::metadata_text_equals_condition;
/// use serde_json::json;
///
/// assert_eq!(
///     metadata_text_equals_condition("photography/rating", "5"),
///     json!({ "metadataTextEquals": { "path": "photography/rating", "value": "5" } })
/// );
/// ```
pub fn metadata_text_equals_condition(path: &str, value: &str) -> serde_json::Value {
    serde_json::json!({
        "metadataTextEquals": {
            "path": path,
            "value": value,
        }
    })
}

/// Build a `privateMetadataTextEquals` filter condition (§3.5).
///
/// Same as [`metadata_text_equals_condition`] but targets `privateMetadata`.
///
/// # Examples
///
/// ```
/// use jmap_metadata_client::private_metadata_text_equals_condition;
/// use serde_json::json;
///
/// assert_eq!(
///     private_metadata_text_equals_condition("workflow/status", "approved"),
///     json!({ "privateMetadataTextEquals": { "path": "workflow/status", "value": "approved" } })
/// );
/// ```
pub fn private_metadata_text_equals_condition(path: &str, value: &str) -> serde_json::Value {
    serde_json::json!({
        "privateMetadataTextEquals": {
            "path": path,
            "value": value,
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- Property constants ------------------------------------------------

    #[test]
    fn property_constants_match_spec_wire_names() {
        assert_eq!(METADATA_PROPERTY, "metadata");
        assert_eq!(PRIVATE_METADATA_PROPERTY, "privateMetadata");
    }

    // -- is_metadata_property ----------------------------------------------

    #[test]
    fn is_metadata_property_metadata() {
        assert!(is_metadata_property("metadata"));
    }

    #[test]
    fn is_metadata_property_private() {
        assert!(is_metadata_property("privateMetadata"));
    }

    #[test]
    fn is_metadata_property_non_metadata() {
        assert!(!is_metadata_property("subject"));
        assert!(!is_metadata_property("id"));
        assert!(!is_metadata_property(""));
    }

    // -- is_metadata_only_change -------------------------------------------

    #[test]
    fn metadata_only_change_single() {
        assert!(is_metadata_only_change(&["metadata".into()]));
    }

    #[test]
    fn metadata_only_change_both() {
        assert!(is_metadata_only_change(&[
            "metadata".into(),
            "privateMetadata".into()
        ]));
    }

    #[test]
    fn metadata_only_change_mixed() {
        assert!(!is_metadata_only_change(&[
            "metadata".into(),
            "subject".into()
        ]));
    }

    #[test]
    fn metadata_only_change_empty() {
        assert!(!is_metadata_only_change(&[]));
    }

    #[test]
    fn metadata_only_change_non_metadata() {
        assert!(!is_metadata_only_change(&["subject".into()]));
    }

    // -- Filter-condition builders -----------------------------------------

    #[test]
    fn metadata_exists_condition_namespace_only() {
        assert_eq!(
            metadata_exists_condition("photography"),
            json!({ "metadataExists": "photography" })
        );
    }

    #[test]
    fn metadata_exists_condition_with_key() {
        assert_eq!(
            metadata_exists_condition("acme.example.com/memo"),
            json!({ "metadataExists": "acme.example.com/memo" })
        );
    }

    #[test]
    fn private_metadata_exists_condition_spec() {
        assert_eq!(
            private_metadata_exists_condition("photography"),
            json!({ "privateMetadataExists": "photography" })
        );
    }

    #[test]
    fn metadata_text_contains_wire_shape() {
        let v = metadata_text_contains_condition("photography/caption", "sunset");
        assert_eq!(v["metadataTextContains"]["path"], "photography/caption");
        assert_eq!(v["metadataTextContains"]["value"], "sunset");
    }

    #[test]
    fn private_metadata_text_contains_wire_shape() {
        let v = private_metadata_text_contains_condition("workflow/note", "urgent");
        assert_eq!(v["privateMetadataTextContains"]["path"], "workflow/note");
        assert_eq!(v["privateMetadataTextContains"]["value"], "urgent");
    }

    #[test]
    fn metadata_text_equals_wire_shape() {
        let v = metadata_text_equals_condition("photography/rating", "5");
        assert_eq!(v["metadataTextEquals"]["path"], "photography/rating");
        assert_eq!(v["metadataTextEquals"]["value"], "5");
    }

    #[test]
    fn private_metadata_text_equals_wire_shape() {
        let v = private_metadata_text_equals_condition("workflow/status", "approved");
        assert_eq!(v["privateMetadataTextEquals"]["path"], "workflow/status");
        assert_eq!(v["privateMetadataTextEquals"]["value"], "approved");
    }

    // -- MetadataTextMatch round-trip via re-export -------------------------

    #[test]
    fn metadata_text_match_re_export_round_trip() {
        let m: MetadataTextMatch = serde_json::from_value(json!({
            "path": "acme.example.com/memo",
            "value": "follow up"
        }))
        .unwrap();
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v, json!({ "path": "acme.example.com/memo", "value": "follow up" }));
        let back: MetadataTextMatch = serde_json::from_value(v).unwrap();
        assert_eq!(back, m);
    }

    // -- DataTypeMetadataInfo re-export ------------------------------------

    #[test]
    fn data_type_metadata_info_re_export_deserializes() {
        let info: DataTypeMetadataInfo = serde_json::from_str(
            r#"{
                "namespaces": ["photography"],
                "supportsVendorNamespaces": false,
                "supportsPrivate": true,
                "maxDepth": 3
            }"#,
        )
        .unwrap();
        assert_eq!(info.namespaces, vec!["photography"]);
        assert!(!info.supports_vendor_namespaces);
        assert!(info.supports_private);
        assert_eq!(info.max_depth, Some(3));
    }

    // -- JMAP_METADATA_URI re-export ---------------------------------------

    #[test]
    fn jmap_metadata_uri_value() {
        assert_eq!(JMAP_METADATA_URI, "urn:ietf:params:jmap:metadata");
    }

    // -- IGNORE_METADATA_ONLY_CHANGES constant -----------------------------

    #[test]
    fn ignore_metadata_only_changes_wire_name() {
        assert_eq!(IGNORE_METADATA_ONLY_CHANGES, "ignoreMetadataOnlyChanges");
    }
}
