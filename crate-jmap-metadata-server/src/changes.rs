//! draft-ietf-jmap-metadata-04 §3.3 — `/changes` extension helpers.
//!
//! Provides building blocks for extension-server crates that integrate
//! the metadata `/changes` extensions:
//!
//! - **`ignoreMetadataOnlyChanges`** (request arg): when `true`, the
//!   server excludes from `updated` any id whose only changes, from the
//!   viewer's perspective, are to `metadata` and/or `privateMetadata`.
//!   Use [`is_metadata_only_change`] to test whether a set of changed
//!   property names qualifies. An id the server "cannot determine" to
//!   be metadata-only "MUST" stay in `updated`, and "Excluded ids do not
//!   count toward maxChanges". When the request's `using` lacks
//!   `urn:ietf:params:jmap:metadata`, -04 §3 says `Foo/changes` "MUST
//!   behave as if ignoreMetadataOnlyChanges were true".
//!
//! - **`updatedProperties`** (response field): for a request whose
//!   `using` includes the capability, it "is always present, and is
//!   null when updated is empty". If every id in `updated` changed only
//!   in metadata properties it lists those property names, otherwise it
//!   is `null`. For `Mailbox/changes`, when every id changed only in its
//!   RFC 8621 §2.2 counts and/or metadata properties, it is "the union
//!   of the count property names and the metadata property names that
//!   may have changed". With `ignoreMetadataOnlyChanges` true it "MUST
//!   NOT list metadata or privateMetadata": it may list the Mailbox
//!   counts, and "for other types it MUST be null". Use
//!   [`is_metadata_property`] and [`is_metadata_only_change`] to drive
//!   this logic.
//!
//! Extension-server backends track which properties changed per update.
//! These helpers classify the property names — they do not inspect object
//! values themselves.

/// Wire name of the shared metadata property added by
/// draft-ietf-jmap-metadata-04.
pub const METADATA_PROPERTY: &str = "metadata";

/// Wire name of the per-user private metadata property added by
/// draft-ietf-jmap-metadata-04.
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
/// metadata extension properties (draft-ietf-jmap-metadata-04 §3.3).
///
/// Returns `false` if the slice is empty — an empty change set is not a
/// "metadata-only change".
///
/// # Usage
///
/// When processing `/changes` with `ignoreMetadataOnlyChanges: true`
/// (explicit, or implied because `using` lacks the metadata
/// capability), call this for each candidate id. If it returns `true`,
/// omit that id from `updated` and do not count it toward
/// `maxChanges`. Pass only property sets the backend knows to be
/// complete: an id whose changes are unknown must stay in `updated`.
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
