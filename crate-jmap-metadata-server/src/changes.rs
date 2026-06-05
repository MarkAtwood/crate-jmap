//! draft-ietf-jmap-metadata-02 §3.3 — `/changes` extension helpers.
//!
//! Provides building blocks for extension-server crates that integrate
//! the metadata `/changes` extensions:
//!
//! - **`ignoreMetadataOnlyChanges`** (request arg): when `true`, the
//!   server omits ids whose only changes are to `metadata` and/or
//!   `privateMetadata`. Use [`is_metadata_only_change`] to test whether
//!   a set of changed property names qualifies.
//!
//! - **`updatedProperties`** (response field): if every id in the
//!   `updated` array had only metadata-property changes, the response
//!   MUST list those property names. Use [`is_metadata_property`] and
//!   [`is_metadata_only_change`] to drive this logic.
//!
//! Extension-server backends track which properties changed per update.
//! These helpers classify the property names — they do not inspect object
//! values themselves.

/// Wire name of the shared metadata property added by
/// draft-ietf-jmap-metadata-02.
pub const METADATA_PROPERTY: &str = "metadata";

/// Wire name of the per-user private metadata property added by
/// draft-ietf-jmap-metadata-02.
pub const PRIVATE_METADATA_PROPERTY: &str = "privateMetadata";

/// Returns `true` if `property` is a metadata extension property
/// (`"metadata"` or `"privateMetadata"`).
///
/// # Examples
///
/// ```
/// use jmap_metadata_server::is_metadata_property;
///
/// assert!(is_metadata_property("metadata"));
/// assert!(is_metadata_property("privateMetadata"));
/// assert!(!is_metadata_property("subject"));
/// ```
pub fn is_metadata_property(property: &str) -> bool {
    property == METADATA_PROPERTY || property == PRIVATE_METADATA_PROPERTY
}

/// Returns `true` if **all** properties in `changed_properties` are
/// metadata extension properties (draft-ietf-jmap-metadata-02 §3.3).
///
/// Returns `false` if the slice is empty — an empty change set is not a
/// "metadata-only change".
///
/// # Usage
///
/// When processing `/changes` with `ignoreMetadataOnlyChanges: true`,
/// call this for each id in the `updated` array. If it returns `true`,
/// omit that id from the response.
///
/// # Examples
///
/// ```
/// use jmap_metadata_server::is_metadata_only_change;
///
/// assert!(is_metadata_only_change(&["metadata"]));
/// assert!(is_metadata_only_change(&["metadata", "privateMetadata"]));
/// assert!(!is_metadata_only_change(&["metadata", "subject"]));
/// assert!(!is_metadata_only_change(&[]));
/// ```
pub fn is_metadata_only_change(changed_properties: &[&str]) -> bool {
    !changed_properties.is_empty() && changed_properties.iter().all(|p| is_metadata_property(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_metadata_property ---------------------------------------------

    #[test]
    fn metadata_property_shared() {
        assert!(is_metadata_property("metadata"));
    }

    #[test]
    fn metadata_property_private() {
        assert!(is_metadata_property("privateMetadata"));
    }

    #[test]
    fn metadata_property_rejects_other() {
        assert!(!is_metadata_property("subject"));
        assert!(!is_metadata_property("receivedAt"));
        assert!(!is_metadata_property(""));
        assert!(!is_metadata_property("Metadata")); // case-sensitive
    }

    // -- is_metadata_only_change ------------------------------------------

    #[test]
    fn metadata_only_shared() {
        assert!(is_metadata_only_change(&["metadata"]));
    }

    #[test]
    fn metadata_only_private() {
        assert!(is_metadata_only_change(&["privateMetadata"]));
    }

    #[test]
    fn metadata_only_both() {
        assert!(is_metadata_only_change(&["metadata", "privateMetadata"]));
    }

    #[test]
    fn metadata_only_mixed() {
        assert!(!is_metadata_only_change(&["metadata", "subject"]));
    }

    #[test]
    fn metadata_only_none() {
        assert!(!is_metadata_only_change(&["subject"]));
    }

    #[test]
    fn metadata_only_empty() {
        assert!(!is_metadata_only_change(&[]));
    }
}
