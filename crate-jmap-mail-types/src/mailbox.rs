//! RFC 8621 §2 Mailbox object and its component types.
//!
//! Provides [`Mailbox`], [`MailboxRole`], and [`MailboxRights`].
//! Mailboxes are the folder containers for [`crate::Email`] objects.

use jmap_types::{impl_string_enum, Id};
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
    /// Id of the parent Mailbox, or `None` if top-level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Id>,
    /// Well-known role identifying the Mailbox's common purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<MailboxRole>,
    /// Client UI sort position; lower values sort first among siblings.
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
            extra: serde_json::Map::new(),
        }
    }
}

/// Deserialize `parent_id` so that an explicit JSON `null` is preserved as
/// `Some(Value::Null)` instead of being collapsed to `None`.
///
/// `#[serde(default)]` on the field handles the absent case (produces `None`
/// without calling this function). When the field is present, serde calls
/// this function with the value, which always wraps the result in `Some(...)`.
/// This is the only way to distinguish `{}` from `{"parentId": null}` for a
/// field of type `Option<T>` — serde's default `Option<T>` Deserialize impl
/// treats both as `None`.
fn deserialize_parent_id_three_way<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    serde_json::Value::deserialize(deserializer).map(Some)
}

/// Filter condition for `Mailbox/query` (RFC 8621 §2.3).
///
/// All fields are optional; a condition with no fields set matches every Mailbox.
///
/// ## `parentId` semantics
///
/// The `parentId` field has three distinct states that must be preserved:
/// - **absent** (`None`) — do not filter by parent; return mailboxes at any level.
/// - **`null`** (`Some(serde_json::Value::Null)`) — return only top-level mailboxes
///   (those with no parent).
/// - **`"<id>"`** (`Some(serde_json::Value::String(...))`) — return only mailboxes
///   whose `parentId` equals the given `Id`.
///
/// The combination of `#[serde(default, deserialize_with = ...)]` preserves
/// this three-way distinction. Without the custom deserializer, serde's
/// default `Option<T>` Deserialize impl would collapse a JSON `null` to
/// `None`, making `null` and absent indistinguishable.
#[non_exhaustive]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxFilterCondition {
    /// See type-level docs for three-way semantics.
    #[serde(
        default,
        deserialize_with = "deserialize_parent_id_three_way",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_id: Option<serde_json::Value>,

    /// Mailbox name must contain this string (case-sensitive substring match).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Mailbox role must equal this string (e.g. `"inbox"`, `"trash"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// If `true`, only mailboxes with a non-null role are returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_any_role: Option<bool>,

    /// If `true`, only subscribed mailboxes are returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_subscribed: Option<bool>,
}

/// The wire-format field names accepted by [`MailboxFilterCondition`]
/// (RFC 8621 §2.3). Server crates use this to pre-validate
/// `Mailbox/query.filter` against unknown keys per RFC 8620 §5.5
/// (return `unsupportedFilter`) before deserialising into the typed
/// struct. The drift-check unit test in this module asserts that the
/// list matches every field actually serialised by the struct, so a
/// new field on `MailboxFilterCondition` cannot quietly diverge from
/// this list — `cargo test -p jmap-mail-types` will fail.
pub const MAILBOX_FILTER_CONDITION_KEYS: &[&str] = &[
    "parentId",
    "name",
    "role",
    "hasAnyRole",
    "isSubscribed",
];

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
    // deserializer to preserve `Some(Value::Null)`. These tests lock that
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
    /// `Some(Value::Null)` (NOT `None`) and serializes back to `null`.
    /// This is the wire-level signal for "top-level mailboxes only".
    #[test]
    fn mailbox_filter_parent_id_null_round_trips() {
        let cond: MailboxFilterCondition =
            serde_json::from_value(json!({"parentId": null})).unwrap();
        assert!(
            matches!(cond.parent_id, Some(serde_json::Value::Null)),
            "explicit null must deserialize as Some(Value::Null), got {:?}",
            cond.parent_id
        );
        let back = serde_json::to_value(&cond).unwrap();
        assert_eq!(
            back["parentId"],
            serde_json::Value::Null,
            "Some(Value::Null) must serialize back as null"
        );
    }

    /// Oracle: explicit string `parentId` deserializes as
    /// `Some(Value::String(...))` and serializes back to the same string.
    #[test]
    fn mailbox_filter_parent_id_string_round_trips() {
        let cond: MailboxFilterCondition =
            serde_json::from_value(json!({"parentId": "mbox-42"})).unwrap();
        match cond.parent_id {
            Some(serde_json::Value::String(ref s)) => assert_eq!(s, "mbox-42"),
            other => panic!("expected Some(String), got {other:?}"),
        }
        let back = serde_json::to_value(&cond).unwrap();
        assert_eq!(back["parentId"], "mbox-42");
    }

    /// Oracle: [`MAILBOX_FILTER_CONDITION_KEYS`] is a single source of truth
    /// for the wire-format keys of [`MailboxFilterCondition`]. Mirror the
    /// const against the actual serialised field names, derived by
    /// fully-populating the struct and asking serde for the JSON keys. A
    /// future contributor who adds a new field but forgets to update the
    /// const trips this test (`cargo test -p jmap-mail-types`), avoiding
    /// the silent server-side drift documented in `bd:JMAP-j7pa.3`.
    ///
    /// Independent oracle: the JSON object produced by `serde_json::to_value`
    /// over a hand-built struct value. The struct is owned by this crate;
    /// the const is owned by this crate; the test compares one against the
    /// other at compile-equivalent time. No JMAP-server code is involved.
    #[test]
    fn mailbox_filter_condition_keys_matches_struct_fields() {
        use std::collections::BTreeSet;

        // Every field gets a non-None value so every key is serialised.
        let mut cond = MailboxFilterCondition {
            parent_id: Some(serde_json::Value::String("any-id".into())),
            name: Some("inbox".into()),
            role: Some("inbox".into()),
            has_any_role: Some(true),
            is_subscribed: Some(true),
        };
        // Touch the `cond` binding via serialisation; the let-binding is
        // intentionally mutable to force this test to be updated alongside
        // any future field addition that lands without an obvious default.
        let _ = &mut cond;

        let value = serde_json::to_value(&cond).expect("serialisation must succeed");
        let serialised_keys: BTreeSet<&str> = value
            .as_object()
            .expect("filter condition must serialise as an object")
            .keys()
            .map(String::as_str)
            .collect();

        let declared_keys: BTreeSet<&str> =
            MAILBOX_FILTER_CONDITION_KEYS.iter().copied().collect();

        assert_eq!(
            declared_keys, serialised_keys,
            "MAILBOX_FILTER_CONDITION_KEYS must match every serialised field of MailboxFilterCondition; \
             declared={declared_keys:?}, serialised={serialised_keys:?}. \
             If you added a field to MailboxFilterCondition, add its camelCase wire name to the const."
        );
    }
}
