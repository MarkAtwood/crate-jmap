//! Property selector enums and [`jmap_types::JmapObject`] impls for RFC 8621 types.
//!
//! These are defined here so that `jmap-mail-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the mail types are
//! local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Mailbox`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MailboxProperty {
    /// The `id` property (RFC 8621 §2).
    Id,
    /// The `name` property (RFC 8621 §2).
    Name,
    /// The `parentId` property (RFC 8621 §2).
    ParentId,
    /// The `role` property (RFC 8621 §2).
    Role,
    /// The `sortOrder` property (RFC 8621 §2).
    SortOrder,
    /// The `totalEmails` property (RFC 8621 §2).
    TotalEmails,
    /// The `unreadEmails` property (RFC 8621 §2).
    UnreadEmails,
    /// The `totalThreads` property (RFC 8621 §2).
    TotalThreads,
    /// The `unreadThreads` property (RFC 8621 §2).
    UnreadThreads,
    /// The `myRights` property (RFC 8621 §2).
    MyRights,
    /// The `isSubscribed` property (RFC 8621 §2).
    IsSubscribed,
}

/// Property selector for [`crate::Thread`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThreadProperty {
    /// The `id` property (RFC 8621 §3).
    Id,
    /// The `emailIds` property (RFC 8621 §3).
    EmailIds,
}

/// Property selector for [`crate::Email`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmailProperty {
    /// The `id` property (RFC 8621 §4).
    Id,
    /// The `blobId` property (RFC 8621 §4).
    BlobId,
    /// The `threadId` property (RFC 8621 §4).
    ThreadId,
    /// The `mailboxIds` property (RFC 8621 §4).
    MailboxIds,
    /// The `keywords` property (RFC 8621 §4).
    Keywords,
    /// The `size` property (RFC 8621 §4).
    Size,
    /// The `receivedAt` property (RFC 8621 §4).
    ReceivedAt,
    /// The `messageId` property (RFC 8621 §4).
    MessageId,
    /// The `inReplyTo` property (RFC 8621 §4).
    InReplyTo,
    /// The `references` property (RFC 8621 §4).
    References,
    /// The `subject` property (RFC 8621 §4).
    Subject,
    /// The `from` property (RFC 8621 §4).
    From,
    /// The `to` property (RFC 8621 §4).
    To,
    /// The `cc` property (RFC 8621 §4).
    Cc,
    /// The `bcc` property (RFC 8621 §4).
    Bcc,
    /// The `replyTo` property (RFC 8621 §4).
    ReplyTo,
    /// The `sender` property (RFC 8621 §4).
    Sender,
    /// The `sentAt` property (RFC 8621 §4).
    SentAt,
    /// The `hasAttachment` property (RFC 8621 §4).
    HasAttachment,
    /// The `preview` property (RFC 8621 §4).
    Preview,
    /// The `bodyStructure` property (RFC 8621 §4).
    BodyStructure,
    /// The `textBody` property (RFC 8621 §4).
    TextBody,
    /// The `htmlBody` property (RFC 8621 §4).
    HtmlBody,
    /// The `attachments` property (RFC 8621 §4).
    Attachments,
    /// The `bodyValues` property (RFC 8621 §4).
    BodyValues,
    /// The `headers` property (RFC 8621 §4).
    Headers,
}

/// Property selector for [`crate::Identity`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdentityProperty {
    /// The `id` property (RFC 8621 §6).
    Id,
    /// The `name` property (RFC 8621 §6).
    Name,
    /// The `email` property (RFC 8621 §6).
    Email,
    /// The `replyTo` property (RFC 8621 §6).
    ReplyTo,
    /// The `bcc` property (RFC 8621 §6).
    Bcc,
    /// The `textSignature` property (RFC 8621 §6).
    TextSignature,
    /// The `htmlSignature` property (RFC 8621 §6).
    HtmlSignature,
    /// The `mayDelete` property (RFC 8621 §6).
    MayDelete,
}

/// Property selector for [`crate::EmailSubmission`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmailSubmissionProperty {
    /// The `id` property (RFC 8621 §7).
    Id,
    /// The `identityId` property (RFC 8621 §7).
    IdentityId,
    /// The `emailId` property (RFC 8621 §7).
    EmailId,
    /// The `threadId` property (RFC 8621 §7).
    ThreadId,
    /// The `envelope` property (RFC 8621 §7).
    Envelope,
    /// The `sendAt` property (RFC 8621 §7).
    SendAt,
    /// The `undoStatus` property (RFC 8621 §7).
    UndoStatus,
    /// The `deliveryStatus` property (RFC 8621 §7).
    DeliveryStatus,
    /// The `dsnBlobIds` property (RFC 8621 §7).
    DsnBlobIds,
    /// The `mdnBlobIds` property (RFC 8621 §7).
    MdnBlobIds,
}

/// Property selector for [`crate::VacationResponse`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VacationResponseProperty {
    /// The `id` property (RFC 8621 §8).
    Id,
    /// The `isEnabled` property (RFC 8621 §8).
    IsEnabled,
    /// The `fromDate` property (RFC 8621 §8).
    FromDate,
    /// The `toDate` property (RFC 8621 §8).
    ToDate,
    /// The `subject` property (RFC 8621 §8).
    Subject,
    /// The `textBody` property (RFC 8621 §8).
    TextBody,
    /// The `htmlBody` property (RFC 8621 §8).
    HtmlBody,
}

/// Property selector for [`crate::SearchSnippet`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SearchSnippetProperty {
    /// The `emailId` property (RFC 8621 §5).
    EmailId,
    /// The `subject` property (RFC 8621 §5).
    Subject,
    /// The `preview` property (RFC 8621 §5).
    Preview,
}

/// Property selector for [`crate::SieveScript`] `/get` and `/set`.
#[cfg(feature = "sieve")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SieveScriptProperty {
    /// The `id` property (RFC 9661).
    Id,
    /// The `name` property (RFC 9661).
    Name,
    /// The `blobId` property (RFC 9661).
    BlobId,
    /// The `isActive` property (RFC 9661).
    IsActive,
}

// ---------------------------------------------------------------------------
// JmapObject impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::Mailbox {
    const TYPE_NAME: &'static str = "Mailbox";
    type Property = MailboxProperty;
}

impl GetObject for crate::Mailbox {}

impl SetObject for crate::Mailbox {
    type Patch = PatchObject;
}

impl QueryObject for crate::Mailbox {
    type Filter = crate::MailboxFilterCondition;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::Thread {
    const TYPE_NAME: &'static str = "Thread";
    type Property = ThreadProperty;
}

impl GetObject for crate::Thread {}

impl JmapObject for crate::Email {
    const TYPE_NAME: &'static str = "Email";
    type Property = EmailProperty;
}

impl GetObject for crate::Email {}

impl SetObject for crate::Email {
    type Patch = PatchObject;
}

impl QueryObject for crate::Email {
    type Filter = crate::EmailFilter;
    type Comparator = crate::EmailComparator;
}

impl JmapObject for crate::Identity {
    const TYPE_NAME: &'static str = "Identity";
    type Property = IdentityProperty;
}

impl GetObject for crate::Identity {}

impl SetObject for crate::Identity {
    type Patch = PatchObject;
}

impl JmapObject for crate::EmailSubmission {
    const TYPE_NAME: &'static str = "EmailSubmission";
    type Property = EmailSubmissionProperty;
}

impl GetObject for crate::EmailSubmission {}

impl SetObject for crate::EmailSubmission {
    type Patch = PatchObject;
}

impl QueryObject for crate::EmailSubmission {
    type Filter = crate::EmailSubmissionFilter;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::VacationResponse {
    const TYPE_NAME: &'static str = "VacationResponse";
    type Property = VacationResponseProperty;
}

impl GetObject for crate::VacationResponse {}

impl SetObject for crate::VacationResponse {
    type Patch = PatchObject;
}

impl JmapObject for crate::SearchSnippet {
    const TYPE_NAME: &'static str = "SearchSnippet";
    type Property = SearchSnippetProperty;
}

#[cfg(feature = "sieve")]
impl JmapObject for crate::sieve::SieveScript {
    const TYPE_NAME: &'static str = "SieveScript";
    type Property = SieveScriptProperty;
}

#[cfg(feature = "sieve")]
impl GetObject for crate::sieve::SieveScript {}

#[cfg(feature = "sieve")]
impl SetObject for crate::sieve::SieveScript {
    type Patch = PatchObject;
}
