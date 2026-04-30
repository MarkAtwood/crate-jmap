//! RFC 8621 §2 Mailbox object and its component types.
//!
//! Provides [`Mailbox`], [`MailboxRole`], and [`MailboxRights`].
//! Mailboxes are the folder containers for [`crate::Email`] objects.

use jmap_types::Id;
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
        }
    }
}
