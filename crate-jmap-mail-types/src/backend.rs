//! Property selector enums and [`jmap_types::JmapObject`] impls for RFC 8621 types.
//!
//! These are defined here so that `jmap-mail-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the mail types are
//! local to this crate).

use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Mailbox`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MailboxProperty {
    Id,
    Name,
    ParentId,
    Role,
    SortOrder,
    TotalEmails,
    UnreadEmails,
    TotalThreads,
    UnreadThreads,
    MyRights,
    IsSubscribed,
}

/// Property selector for [`crate::Thread`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThreadProperty {
    Id,
    EmailIds,
}

/// Property selector for [`crate::Email`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmailProperty {
    Id,
    BlobId,
    ThreadId,
    MailboxIds,
    Keywords,
    Size,
    ReceivedAt,
    MessageId,
    InReplyTo,
    References,
    Subject,
    From,
    To,
    Cc,
    Bcc,
    ReplyTo,
    Sender,
    SentAt,
    HasAttachment,
    Preview,
    BodyStructure,
    TextBody,
    HtmlBody,
    Attachments,
    BodyValues,
    Headers,
}

/// Property selector for [`crate::Identity`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdentityProperty {
    Id,
    Name,
    Email,
    ReplyTo,
    Bcc,
    TextSignature,
    HtmlSignature,
    MayDelete,
}

/// Property selector for [`crate::EmailSubmission`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmailSubmissionProperty {
    Id,
    IdentityId,
    EmailId,
    ThreadId,
    Envelope,
    SendAt,
    UndoStatus,
    DeliveryStatus,
    DsnBlobIds,
    MdnBlobIds,
}

/// Property selector for [`crate::VacationResponse`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VacationResponseProperty {
    Id,
    IsEnabled,
    FromDate,
    ToDate,
    Subject,
    TextBody,
    HtmlBody,
}

/// Property selector for [`crate::SearchSnippet`] `/get`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SearchSnippetProperty {
    EmailId,
    Subject,
    Preview,
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
    type Patch = serde_json::Value;
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
    type Patch = serde_json::Value;
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
    type Patch = serde_json::Value;
}

impl JmapObject for crate::EmailSubmission {
    const TYPE_NAME: &'static str = "EmailSubmission";
    type Property = EmailSubmissionProperty;
}

impl GetObject for crate::EmailSubmission {}

impl SetObject for crate::EmailSubmission {
    type Patch = serde_json::Value;
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
    type Patch = serde_json::Value;
}

impl JmapObject for crate::SearchSnippet {
    const TYPE_NAME: &'static str = "SearchSnippet";
    type Property = SearchSnippetProperty;
}
