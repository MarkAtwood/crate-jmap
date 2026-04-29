use std::fmt;

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// The role of a Mailbox, identifying its common purpose (RFC 8621 §2).
///
/// Values correspond to IMAP Mailbox Name Attributes (RFC 8457), converted to
/// lowercase.  An account is not required to have Mailboxes with any particular
/// role, and at most one Mailbox per account may hold each role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    /// **Round-trip warning**: this variant serializes as `"other"`, not as the original
    /// string received from the server.  Do not echo a `MailboxRole::Other` back to the
    /// server in a `Mailbox/set` request — omit the `role` field instead (treat as `None`).
    #[serde(other)]
    Other,
}

impl fmt::Display for MailboxRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MailboxRole::Inbox => "inbox",
            MailboxRole::Trash => "trash",
            MailboxRole::Sent => "sent",
            MailboxRole::Drafts => "drafts",
            MailboxRole::Junk => "junk",
            MailboxRole::Archive => "archive",
            MailboxRole::Flagged => "flagged",
            MailboxRole::Important => "important",
            MailboxRole::All => "all",
            MailboxRole::Other => "other",
        })
    }
}

/// Access control rights the authenticated user holds for a Mailbox (RFC 8621 §2).
///
/// Backwards compatible with IMAP ACLs (RFC 4314).
///
/// `Default` produces all-false (no access), which is the most restrictive valid value
/// and a safe starting point when constructing rights in tests or server code.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
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
