//! RFC 8621 §2 Mailbox object and its component types.
//!
//! Provides [`Mailbox`], [`MailboxRole`], and [`MailboxRights`].
//! Mailboxes are the folder containers for [`crate::Email`] objects.

use jmap_metadata_types::MetadataTextMatch;
use jmap_types::{Id, impl_string_enum};
use serde::{Deserialize, Serialize};

/// The role of a Mailbox, identifying its common purpose (RFC 8621 §2).
///
/// Values correspond to IMAP Mailbox Name Attributes (RFC 8457), converted to
/// lowercase.  An account is not required to have Mailboxes with any particular
/// role, and at most one Mailbox per account may hold each role.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MailboxRole {
    /// The primary inbox for new incoming messages.
    Inbox,
    /// Deleted messages.
    Trash,
    /// Sent messages.
    Sent,
    /// Unsent draft messages.
    Drafts,
    /// Messages identified as likely spam.
    Junk,
    /// Archived messages.
    Archive,
    /// Messages flagged for follow-up.
    Flagged,
    /// Messages considered important.
    Important,
    /// Virtual mailbox containing all messages.
    All,
    /// Any role string not recognized by this implementation.
    ///
    /// RFC 8621 §2: "An unrecognized role SHOULD be treated as if no role were set."
    ///
    /// The inner string retains the original value received from the server, so
    /// this variant round-trips correctly.  When sending a `Mailbox/set` request
    /// for a mailbox whose role came from the server, it is safe to echo the role
    /// back — or omit it by setting `role` to `None`.
    Other(String),
}

impl_string_enum!(MailboxRole, "a JMAP Mailbox role string",
    "inbox"     => Inbox,
    "trash"     => Trash,
    "sent"      => Sent,
    "drafts"    => Drafts,
    "junk"      => Junk,
    "archive"   => Archive,
    "flagged"   => Flagged,
    "important" => Important,
    "all"       => All,
);

impl MailboxRole {
    /// Return the RFC 8621 wire-format string for this role.
    ///
    /// Because this method is defined inside the crate that owns `MailboxRole`,
    /// the match is exhaustive even though the enum is `#[non_exhaustive]`.
    /// Adding a new variant without updating this method is a compile error.
    pub fn to_wire_str(&self) -> &str {
        match self {
            Self::Inbox => "inbox",
            Self::Trash => "trash",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Junk => "junk",
            Self::Archive => "archive",
            Self::Flagged => "flagged",
            Self::Important => "important",
            Self::All => "all",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// Access control rights the authenticated user holds for a Mailbox (RFC 8621 §2).
///
/// Backwards compatible with IMAP ACLs (RFC 4314).
///
/// `Default` produces all-false (no access), which is the most restrictive valid value
/// and a safe starting point when constructing rights in tests or server code.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxRights {
    /// User may use this Mailbox in Email/query filters and read its emails.
    pub may_read_items: bool,
    /// User may add mail to this Mailbox.
    pub may_add_items: bool,
    /// User may remove mail from this Mailbox.
    pub may_remove_items: bool,
    /// User may add or remove the `$seen` keyword on emails in this Mailbox.
    pub may_set_seen: bool,
    /// User may add or remove keywords other than `$seen` on emails.
    pub may_set_keywords: bool,
    /// User may create a child Mailbox under this one.
    pub may_create_child: bool,
    /// User may rename this Mailbox or move it under another parent.
    pub may_rename: bool,
    /// User may delete this Mailbox.
    pub may_delete: bool,
    /// Messages may be submitted directly to this Mailbox.
    pub may_submit: bool,
    /// User may modify the shared metadata of this Mailbox and of the Emails
    /// in it (draft-ietf-jmap-metadata-04 §4.1; IMAP ACL "n" right, RFC 5257).
    /// `None` when the server omits it: the right is present only when the
    /// metadata capability lists this type or a type it contains (§4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub may_set_metadata: Option<bool>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A JMAP Mailbox object (RFC 8621 §2).
///
/// Mailboxes are the containers for Email objects.  Each Email must belong to
/// at least one Mailbox.  Mailboxes form an acyclic forest via `parent_id`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    /// Server-assigned immutable identifier.
    pub id: Id,
    /// User-visible name for this Mailbox.
    pub name: String,
    /// Id of the parent Mailbox, or `None` if top-level; `None`
    /// serializes as `null`.
    pub parent_id: Option<Id>,
    /// Well-known role identifying the Mailbox's common purpose; `None`
    /// serializes as `null`.
    pub role: Option<MailboxRole>,
    /// Client UI sort position; lower values sort first among siblings.
    /// Defaults to `0`.
    #[serde(default)]
    pub sort_order: u32,
    /// Total number of Emails in this Mailbox (server-set).
    pub total_emails: u32,
    /// Number of Emails without `$seen` or `$draft` (server-set).
    pub unread_emails: u32,
    /// Number of Threads with at least one Email in this Mailbox (server-set).
    pub total_threads: u32,
    /// Number of unread Threads in this Mailbox (server-set).
    pub unread_threads: u32,
    /// ACL rights the authenticated user has on this Mailbox (server-set).
    pub my_rights: MailboxRights,
    /// Whether the user has subscribed to this Mailbox.
    pub is_subscribed: bool,
    /// Shared metadata, keyed by namespace identifier
    /// (draft-ietf-jmap-metadata-04 §2). Each value is a JSON object (§2.1).
    /// `None` when absent, which -04 §2 treats as `{}`; in an update a
    /// `null` value "is equivalent to {}" (-04 §3.2) and also decodes as
    /// `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    /// The authenticated user's own metadata, keyed by namespace
    /// identifier (draft-ietf-jmap-metadata-04 §2). Same shape and `None`
    /// handling as `metadata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_metadata: Option<serde_json::Map<String, serde_json::Value>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Mailbox {
    /// Construct a [`Mailbox`] from its required fields.
    ///
    /// `parent_id` and `role` default to `None`.
    // Nine arguments because Mailbox has nine required RFC 8621 properties; all
    // are needed for construction since #[non_exhaustive] prevents struct
    // literals outside this crate.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        name: impl Into<String>,
        sort_order: u32,
        total_emails: u32,
        unread_emails: u32,
        total_threads: u32,
        unread_threads: u32,
        my_rights: MailboxRights,
        is_subscribed: bool,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            sort_order,
            total_emails,
            unread_emails,
            total_threads,
            unread_threads,
            my_rights,
            is_subscribed,
            parent_id: None,
            role: None,
            metadata: None,
            private_metadata: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Deserialize a filter property that may be absent, `null` or a value into
/// `Option<Option<T>>`: absent is `None` (via `#[serde(default)]`), `null`
/// is `Some(None)` and a value is `Some(Some(v))`. Serde's own
/// `Option<T>` impl collapses `null` into absent, which would lose the
/// RFC 8621 §2.3 meaning of `null`. Paired with
/// `skip_serializing_if = "Option::is_none"`, the three states round-trip.
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}

/// Filter condition for `Mailbox/query` (RFC 8621 §2.3).
///
/// All fields are optional; a condition with no fields set matches every Mailbox.
///
/// ## Nullable properties
///
/// `parentId` (`Id|null`) and `role` (`String|null`) each have three
/// distinct states, held as `Option<Option<T>>`:
/// - **absent** (`None`) — do not filter on the property.
/// - **`null`** (`Some(None)`) — only mailboxes whose property is `null`:
///   top-level mailboxes for `parentId`, role-less ones for `role`.
/// - **a value** (`Some(Some(v))`) — only mailboxes whose property equals `v`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailboxFilterCondition {
    /// The `parentId` the Mailbox must have; see the type-level docs for
    /// the three states.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_id: Option<Option<Id>>,

    /// Mailbox name must contain this string (case-sensitive substring match).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// "The Mailbox role must match the given value exactly"; see the
    /// type-level docs for the three states.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub role: Option<Option<MailboxRole>>,

    /// If `true`, only mailboxes with a non-null role are returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_any_role: Option<bool>,

    /// If `true`, only subscribed mailboxes are returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subscribed: Option<bool>,

    /// draft-ietf-jmap-metadata-04 §3.6 `metadataExists`: a value must be
    /// present at this path in the object's `metadata`. The path is a
    /// namespace identifier, optionally followed by an RFC 6901 JSON
    /// Pointer into the namespace value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_exists: Option<String>,

    /// -04 §3.6 `privateMetadataExists`: as `metadata_exists`, against the
    /// authenticated user's `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_exists: Option<String>,

    /// -04 §3.6 `metadataTextContains`: substring match under the
    /// `i;unicode-casemap` collation on the `metadata` string at a path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_text_contains: Option<MetadataTextMatch>,

    /// -04 §3.6 `privateMetadataTextContains`: as
    /// `metadata_text_contains`, against `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_text_contains: Option<MetadataTextMatch>,

    /// -04 §3.6 `metadataTextEquals`: exact code-point match on the
    /// `metadata` string at a path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_text_equals: Option<MetadataTextMatch>,

    /// -04 §3.6 `privateMetadataTextEquals`: as `metadata_text_equals`,
    /// against `privateMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_metadata_text_equals: Option<MetadataTextMatch>,
}

impl MailboxFilterCondition {
    /// `true` if any of the six draft-ietf-jmap-metadata-04 §3.6
    /// `metadata*` / `privateMetadata*` fields is set.
    pub fn has_metadata_condition(&self) -> bool {
        self.metadata_exists.is_some()
            || self.private_metadata_exists.is_some()
            || self.metadata_text_contains.is_some()
            || self.private_metadata_text_contains.is_some()
            || self.metadata_text_equals.is_some()
            || self.private_metadata_text_equals.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────

    /// `MailboxRights.extra` captures vendor fields and preserves them.
    #[test]
    fn mailbox_rights_preserves_vendor_extras() {
        let raw = json!({
            "mayReadItems": true,
            "mayAddItems": true,
            "mayRemoveItems": false,
            "maySetSeen": true,
            "maySetKeywords": false,
            "mayCreateChild": false,
            "mayRename": false,
            "mayDelete": false,
            "maySubmit": false,
            "acmeCorpMayPin": true
        });
        let rights: MailboxRights = serde_json::from_value(raw).unwrap();
        assert_eq!(
            rights.extra.get("acmeCorpMayPin").and_then(|v| v.as_bool()),
            Some(true)
        );
        let back = serde_json::to_value(&rights).unwrap();
        assert_eq!(back["acmeCorpMayPin"], true);
    }

    /// `Mailbox.extra` captures vendor fields and preserves them.
    #[test]
    fn mailbox_preserves_vendor_extras() {
        let raw = json!({
            "id": "m1",
            "name": "Inbox",
            "sortOrder": 0,
            "totalEmails": 0,
            "unreadEmails": 0,
            "totalThreads": 0,
            "unreadThreads": 0,
            "myRights": {
                "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
                "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
                "mayRename": true, "mayDelete": false, "maySubmit": true
            },
            "isSubscribed": true,
            "acmeCorpColor": "#ff0000"
        });
        let mbox: Mailbox = serde_json::from_value(raw).unwrap();
        assert_eq!(
            mbox.extra.get("acmeCorpColor").and_then(|v| v.as_str()),
            Some("#ff0000")
        );
        let back = serde_json::to_value(&mbox).unwrap();
        assert_eq!(back["acmeCorpColor"], "#ff0000");
    }

    // ── parentId three-way semantics (RFC 8621 §2.3) ────────────────────
    //
    // The Mailbox/query filter distinguishes three states for `parentId`:
    // absent (no filter), explicit JSON null (top-level only), explicit string
    // (specific parent). The default `Option<T>` Deserialize impl collapses
    // `null` to `None`, so `MailboxFilterCondition.parent_id` uses a custom
    // deserializer to preserve `Some(None)`. These tests lock that
    // behavior.

    /// Oracle: absent `parentId` deserializes as `None` and serializes back to
    /// an object without the field.
    #[test]
    fn mailbox_filter_parent_id_absent_round_trips() {
        let cond: MailboxFilterCondition = serde_json::from_value(json!({})).unwrap();
        assert!(cond.parent_id.is_none(), "absent must deserialize as None");
        let back = serde_json::to_value(&cond).unwrap();
        assert!(
            back.get("parentId").is_none(),
            "absent parentId must not appear in serialized output"
        );
    }

    /// Oracle: explicit JSON `null` for `parentId` deserializes as
    /// `Some(None)` (NOT `None`) and serializes back to `null`.
    /// This is the wire-level signal for "top-level mailboxes only".
    #[test]
    fn mailbox_filter_parent_id_null_round_trips() {
        let cond: MailboxFilterCondition =
            serde_json::from_value(json!({"parentId": null})).unwrap();
        assert_eq!(
            cond.parent_id,
            Some(None),
            "explicit null must deserialize as Some(None)"
        );
        let back = serde_json::to_value(&cond).unwrap();
        assert_eq!(
            back["parentId"],
            serde_json::Value::Null,
            "Some(None) must serialize back as null"
        );
    }

    /// Oracle: explicit string `parentId` deserializes as
    /// `Some(Some(id))` and serializes back to the same string.
    #[test]
    fn mailbox_filter_parent_id_string_round_trips() {
        let cond: MailboxFilterCondition =
            serde_json::from_value(json!({"parentId": "mbox-42"})).unwrap();
        assert_eq!(cond.parent_id, Some(Some(Id::from("mbox-42"))));
        let back = serde_json::to_value(&cond).unwrap();
        assert_eq!(back["parentId"], "mbox-42");
    }

    /// RFC 8621 §2.3 admits only JSON `null` and a JSON string for
    /// `parentId`. Numbers, booleans, arrays, and nested objects MUST
    /// be rejected at deserialize time.
    ///
    /// Independent oracle: the spec text itself (`parentId: Id|null`).
    #[test]
    fn mailbox_filter_parent_id_rejects_non_spec_shapes() {
        for (bad, label) in [
            (json!({"parentId": 42}), "number"),
            (json!({"parentId": true}), "boolean"),
            (json!({"parentId": ["m1"]}), "array"),
            (json!({"parentId": {"id": "m1"}}), "object"),
        ] {
            assert!(
                serde_json::from_value::<MailboxFilterCondition>(bad).is_err(),
                "{label} must be rejected"
            );
        }
    }
    /// Oracle: RFC 8621 §2.3 types the filter `role` as `String|null`
    /// ("The Mailbox role must match the given value exactly"), so an
    /// explicit `null` (role-less mailboxes) is distinct from absent and
    /// round-trips as `null`.
    #[test]
    fn mailbox_filter_role_null_round_trips() {
        let cond: MailboxFilterCondition = serde_json::from_value(json!({"role": null})).unwrap();
        assert_eq!(cond.role, Some(None));
        assert_eq!(serde_json::to_value(&cond).unwrap(), json!({"role": null}));
        let absent: MailboxFilterCondition = serde_json::from_value(json!({})).unwrap();
        assert_eq!(absent.role, None);
        assert_eq!(serde_json::to_value(&absent).unwrap(), json!({}));
    }

    /// Oracle: RFC 8621 §2 role registry value `"trash"`; a filter `role`
    /// string deserializes into the typed [`MailboxRole`] and serializes
    /// back to the same string, and a non-string is rejected
    /// (`String|null`, RFC 8621 §2.3).
    #[test]
    fn mailbox_filter_role_value_is_typed() {
        let cond: MailboxFilterCondition =
            serde_json::from_value(json!({"role": "trash"})).unwrap();
        assert_eq!(cond.role, Some(Some(MailboxRole::Trash)));
        assert_eq!(
            serde_json::to_value(&cond).unwrap(),
            json!({"role": "trash"})
        );
        assert!(serde_json::from_value::<MailboxFilterCondition>(json!({"role": 7})).is_err());
    }

    /// Oracle: -04 §3.6 extends the Mailbox/query FilterCondition with
    /// `metadataTextEquals`; hand-written leaf inside a `NOT` operator.
    /// A misspelt name is still rejected.
    #[test]
    fn mailbox_filter_metadata_condition_in_tree() {
        let wire = json!({
            "operator": "NOT",
            "conditions": [
                { "metadataTextEquals": { "path": "ns/tag", "value": "x" } }
            ]
        });
        let filter: crate::MailboxFilter = serde_json::from_value(wire.clone()).unwrap();
        let jmap_types::Filter::Operator(op) = &filter else {
            panic!("expected an operator, got {filter:?}");
        };
        let jmap_types::Filter::Condition(leaf) = &op.conditions[0] else {
            panic!("expected a condition");
        };
        assert_eq!(
            leaf.metadata_text_equals,
            Some(MetadataTextMatch::new("ns/tag", "x"))
        );
        assert!(leaf.has_metadata_condition());
        assert!(!MailboxFilterCondition::default().has_metadata_condition());
        assert_eq!(serde_json::to_value(&filter).unwrap(), wire);
        assert!(
            serde_json::from_value::<MailboxFilterCondition>(json!({"metadataEquals": {}}))
                .is_err()
        );
    }

    /// Oracle: -04 §2 adds `metadata` and `privateMetadata` (objects keyed
    /// by namespace) to each listed type; hand-written Mailbox JSON. The
    /// values land in the typed fields, not `extra`. -04 §3.2: in an update
    /// "null is equivalent to {}", so `null` decodes as absent; a
    /// non-object value is rejected.
    #[test]
    fn mailbox_metadata_properties() {
        let mut raw = json!({
            "id": "m1",
            "name": "Inbox",
            "parentId": null,
            "role": null,
            "totalEmails": 0,
            "unreadEmails": 0,
            "totalThreads": 0,
            "unreadThreads": 0,
            "myRights": {
                "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
                "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
                "mayRename": true, "mayDelete": true, "maySubmit": true
            },
            "isSubscribed": true,
            "metadata": {"acme.example.com": {"color": "red"}},
            "privateMetadata": {"acme.example.com": {"memo": "mine"}}
        });
        let mb: Mailbox = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            mb.metadata
                .as_ref()
                .map(|m| &m["acme.example.com"]["color"]),
            Some(&json!("red"))
        );
        assert_eq!(
            mb.private_metadata
                .as_ref()
                .map(|m| &m["acme.example.com"]["memo"]),
            Some(&json!("mine"))
        );
        assert!(mb.extra.is_empty());
        assert_eq!(
            serde_json::to_value(&mb).unwrap()["metadata"],
            raw["metadata"]
        );

        raw["metadata"] = json!(null);
        let mb: Mailbox = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(mb.metadata, None);
        assert!(serde_json::to_value(&mb).unwrap().get("metadata").is_none());

        raw["metadata"] = json!("red");
        assert!(serde_json::from_value::<Mailbox>(raw).is_err());
    }
}
