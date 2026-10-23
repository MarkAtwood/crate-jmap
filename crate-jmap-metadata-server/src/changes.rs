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

use std::collections::{BTreeSet, HashMap};

use jmap_metadata_types::JMAP_METADATA_URI;
use serde_json::{Map, Value};

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

/// Names of the top-level properties whose values differ between two
/// stored representations of one object.
///
/// A property present on one side only counts as changed. Backends that
/// keep each object as a JSON map call this in `Foo/set` update to
/// record what an update changed, then feed the result to
/// [`ChangedProperties::record`].
///
/// # Examples
///
/// ```
/// use jmap_metadata_server::changed_properties;
/// use serde_json::json;
///
/// let before = json!({"name": "a", "metadata": {}});
/// let after = json!({"name": "a", "metadata": {"x.example": {}}, "role": "inbox"});
/// let changed = changed_properties(before.as_object().unwrap(), after.as_object().unwrap());
/// assert_eq!(changed.into_iter().collect::<Vec<_>>(), ["metadata", "role"]);
/// ```
pub fn changed_properties(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|k| before.get(*k) != after.get(*k))
        .cloned()
        .collect()
}

/// Whether a `Foo/changes` request asks to hide metadata-only changes
/// (draft-ietf-jmap-metadata-04 §3.3 `ignoreMetadataOnlyChanges`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataOnlyChanges {
    /// Report metadata-only changes in `updated` (the argument's default).
    #[default]
    Report,
    /// Exclude metadata-only changes from `updated`.
    Ignore,
}

impl MetadataOnlyChanges {
    /// Mode for a request with `ignoreMetadataOnlyChanges` set to
    /// `argument` and the capability URIs `using`.
    ///
    /// -04 §3: for a request whose `using` lacks
    /// `urn:ietf:params:jmap:metadata`, "Foo/changes MUST behave as if
    /// ignoreMetadataOnlyChanges were true".
    ///
    /// # Examples
    ///
    /// ```
    /// use jmap_metadata_server::MetadataOnlyChanges;
    ///
    /// let with = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:metadata"];
    /// assert_eq!(MetadataOnlyChanges::from_request(None, &with), MetadataOnlyChanges::Report);
    /// assert_eq!(MetadataOnlyChanges::from_request(Some(true), &with), MetadataOnlyChanges::Ignore);
    /// assert_eq!(
    ///     MetadataOnlyChanges::from_request(None, &["urn:ietf:params:jmap:core"]),
    ///     MetadataOnlyChanges::Ignore
    /// );
    /// ```
    pub fn from_request<S: AsRef<str>>(argument: Option<bool>, using: &[S]) -> Self {
        let uses_metadata = using.iter().any(|u| u.as_ref() == JMAP_METADATA_URI);
        if argument == Some(true) || !uses_metadata {
            Self::Ignore
        } else {
            Self::Report
        }
    }
}

/// Per-id changed-property accumulator over one `Foo/changes` window
/// (draft-ietf-jmap-metadata-04 §3.3).
///
/// A backend calls [`record`](Self::record) for each update in the
/// window, oldest first, then asks [`is_metadata_only`](Self::is_metadata_only)
/// to decide which ids `ignoreMetadataOnlyChanges` drops from `updated`
/// and [`updated_properties`](Self::updated_properties) for the response
/// field. An id with an update of unknown extent stays unknown, so it
/// is never classed as metadata-only (§3.3: "If the server cannot
/// determine that an id's changes are metadata-only, it MUST include
/// the id").
///
/// The accumulator does not filter by viewer: a backend with
/// per-viewer `privateMetadata` (§2.2) records only the changes the
/// viewer can see.
///
/// # Examples
///
/// ```
/// use std::collections::BTreeSet;
/// use jmap_metadata_server::{ChangedProperties, MetadataOnlyChanges};
///
/// let props = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<BTreeSet<_>>();
/// let mut window = ChangedProperties::new();
/// window.record("e1", Some(&props(&["metadata"])));
/// window.record("e1", Some(&props(&["privateMetadata"])));
/// window.record("e2", Some(&props(&["subject"])));
///
/// assert!(window.is_metadata_only("e1"));
/// assert!(!window.is_metadata_only("e2"));
/// assert_eq!(
///     window.updated_properties(&["e1"], MetadataOnlyChanges::Report, &[]),
///     Some(vec!["metadata".to_owned(), "privateMetadata".to_owned()])
/// );
/// assert_eq!(window.updated_properties(&["e1", "e2"], MetadataOnlyChanges::Report, &[]), None);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedProperties {
    by_id: HashMap<String, Option<BTreeSet<String>>>,
}

impl ChangedProperties {
    /// An empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one update of `id` that changed the properties `changed`,
    /// or [`None`] when the backend does not know what changed.
    pub fn record(&mut self, id: &str, changed: Option<&BTreeSet<String>>) {
        let entry = self
            .by_id
            .entry(id.to_owned())
            .or_insert_with(|| Some(BTreeSet::new()));
        match (entry.as_mut(), changed) {
            (Some(acc), Some(changed)) => acc.extend(changed.iter().cloned()),
            _ => *entry = None,
        }
    }

    /// Returns `true` if every recorded update of `id` changed only
    /// `metadata` and/or `privateMetadata`.
    ///
    /// Returns `false` for an id that was never recorded, whose changes
    /// are unknown, or whose recorded change set is empty.
    pub fn is_metadata_only(&self, id: &str) -> bool {
        matches!(
            self.by_id.get(id),
            Some(Some(set)) if !set.is_empty() && set.iter().all(|p| is_metadata_property(p))
        )
    }

    /// The `updatedProperties` value for a response whose `updated`
    /// array is `updated` (draft-ietf-jmap-metadata-04 §3.3).
    ///
    /// `count_properties` lists the Mailbox count property names
    /// (RFC 8621 §2.2) for `Mailbox/changes` and is empty for every other
    /// type. The result is the sorted union of the changed property names
    /// when every id changed only in metadata properties or
    /// `count_properties`, and [`None`] (wire `null`) otherwise, including
    /// when `updated` is empty or any id is unknown. With
    /// [`MetadataOnlyChanges::Ignore`] the metadata property names are
    /// left out (§3.3: it "MUST NOT list metadata or privateMetadata"),
    /// which makes the result [`None`] for any type without count
    /// properties.
    pub fn updated_properties<S: AsRef<str>>(
        &self,
        updated: &[S],
        mode: MetadataOnlyChanges,
        count_properties: &[&str],
    ) -> Option<Vec<String>> {
        let mut union = BTreeSet::new();
        for id in updated {
            for property in self.by_id.get(id.as_ref())?.as_ref()? {
                if !is_metadata_property(property) && !count_properties.contains(&property.as_str())
                {
                    return None;
                }
                union.insert(property.as_str());
            }
        }
        if mode == MetadataOnlyChanges::Ignore {
            union.retain(|p| !is_metadata_property(p));
        }
        (!union.is_empty()).then(|| union.into_iter().map(str::to_owned).collect())
    }
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

    // -- changed_properties -----------------------------------------------

    fn obj(v: serde_json::Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn props(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn changed_properties_lists_differing_top_level_keys() {
        let before = obj(
            serde_json::json!({"name": "Inbox", "metadata": {"x.example": {"a": 1}}, "gone": true}),
        );
        let after = obj(
            serde_json::json!({"name": "Inbox", "metadata": {"x.example": {"a": 2}}, "new": 1}),
        );
        assert_eq!(
            changed_properties(&before, &after),
            props(&["gone", "metadata", "new"])
        );
        assert!(changed_properties(&before, &before).is_empty());
    }

    // -- MetadataOnlyChanges ----------------------------------------------

    #[test]
    fn mode_without_capability_ignores() {
        // Oracle: -04 §3 — for requests without the capability
        // "Foo/changes MUST behave as if ignoreMetadataOnlyChanges were
        // true".
        let core = ["urn:ietf:params:jmap:core"];
        assert_eq!(
            MetadataOnlyChanges::from_request(None, &core),
            MetadataOnlyChanges::Ignore
        );
        assert_eq!(
            MetadataOnlyChanges::from_request(Some(false), &core),
            MetadataOnlyChanges::Ignore
        );
    }

    #[test]
    fn mode_with_capability_defaults_to_report() {
        // Oracle: -04 §3.3 — "ignoreMetadataOnlyChanges: Boolean
        // (default: false)".
        let using = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:metadata"];
        assert_eq!(
            MetadataOnlyChanges::from_request(None, &using),
            MetadataOnlyChanges::Report
        );
        assert_eq!(
            MetadataOnlyChanges::from_request(Some(false), &using),
            MetadataOnlyChanges::Report
        );
        assert_eq!(
            MetadataOnlyChanges::from_request(Some(true), &using),
            MetadataOnlyChanges::Ignore
        );
    }

    // -- ChangedProperties ------------------------------------------------

    #[test]
    fn spec_6_8_metadata_only_window() {
        // Oracle: -04 §6.8 — two Emails whose shared metadata changed give
        // "updated": ["EM1", "EM2"], "updatedProperties": ["metadata"].
        let mut window = ChangedProperties::new();
        window.record("EM1", Some(&props(&["metadata"])));
        window.record("EM2", Some(&props(&["metadata"])));
        window.record("EM1", Some(&props(&["metadata"])));
        assert_eq!(
            window.updated_properties(&["EM1", "EM2"], MetadataOnlyChanges::Report, &[]),
            Some(vec!["metadata".to_owned()])
        );
        assert!(window.is_metadata_only("EM1"));
        assert!(window.is_metadata_only("EM2"));
    }

    #[test]
    fn union_across_window_breaks_metadata_only() {
        // Oracle: -04 §3.3 — exclude an id "whose only changes since
        // sinceState ... are confined to metadata and/or privateMetadata".
        let mut window = ChangedProperties::new();
        window.record("EM1", Some(&props(&["metadata"])));
        window.record("EM1", Some(&props(&["keywords"])));
        assert!(!window.is_metadata_only("EM1"));
        assert_eq!(
            window.updated_properties(&["EM1"], MetadataOnlyChanges::Report, &[]),
            None
        );
    }

    #[test]
    fn unknown_changes_are_never_metadata_only() {
        // Oracle: -04 §3.3 — "If the server cannot determine that an id's
        // changes are metadata-only, it MUST include the id" and
        // updatedProperties "MUST be null" if "the server cannot determine
        // the answer for any id".
        let mut window = ChangedProperties::new();
        window.record("EM1", Some(&props(&["metadata"])));
        window.record("EM1", None);
        window.record("EM1", Some(&props(&["metadata"])));
        assert!(!window.is_metadata_only("EM1"));
        assert!(!window.is_metadata_only("never-recorded"));
        assert_eq!(
            window.updated_properties(&["EM1"], MetadataOnlyChanges::Report, &[]),
            None
        );
        assert_eq!(
            window.updated_properties(&["never-recorded"], MetadataOnlyChanges::Report, &[]),
            None
        );
    }

    #[test]
    fn updated_properties_null_when_updated_empty() {
        // Oracle: -04 §3.3 — "It is always present, and is null when
        // updated is empty."
        let window = ChangedProperties::new();
        assert_eq!(
            window.updated_properties::<&str>(&[], MetadataOnlyChanges::Report, &[]),
            None
        );
    }

    #[test]
    fn ignore_mode_never_lists_metadata() {
        // Oracle: -04 §3.3 — when ignoreMetadataOnlyChanges is true,
        // updatedProperties "MUST NOT list metadata or privateMetadata.
        // For Mailbox/changes it may list the count properties ...; for
        // other types it MUST be null."
        let counts = [
            "totalEmails",
            "unreadEmails",
            "totalThreads",
            "unreadThreads",
        ];
        let mut window = ChangedProperties::new();
        window.record("MB1", Some(&props(&["unreadEmails", "metadata"])));
        assert_eq!(
            window.updated_properties(&["MB1"], MetadataOnlyChanges::Ignore, &counts),
            Some(vec!["unreadEmails".to_owned()])
        );
        window.record("EM1", Some(&props(&["metadata", "privateMetadata"])));
        assert_eq!(
            window.updated_properties(&["EM1"], MetadataOnlyChanges::Ignore, &[]),
            None
        );
    }

    #[test]
    fn mailbox_union_of_counts_and_metadata() {
        // Oracle: -04 §3.3 — "For Mailbox/changes, when every id in
        // updated changed only in its counts ... and/or in metadata or
        // privateMetadata, the value is the union of the count property
        // names and the metadata property names that may have changed."
        let counts = [
            "totalEmails",
            "unreadEmails",
            "totalThreads",
            "unreadThreads",
        ];
        let mut window = ChangedProperties::new();
        window.record("MB1", Some(&props(&["totalEmails"])));
        window.record("MB2", Some(&props(&["metadata"])));
        assert_eq!(
            window.updated_properties(&["MB1", "MB2"], MetadataOnlyChanges::Report, &counts),
            Some(vec!["metadata".to_owned(), "totalEmails".to_owned()])
        );
        window.record("MB3", Some(&props(&["name"])));
        assert_eq!(
            window.updated_properties(&["MB1", "MB3"], MetadataOnlyChanges::Report, &counts),
            None
        );
    }
}
