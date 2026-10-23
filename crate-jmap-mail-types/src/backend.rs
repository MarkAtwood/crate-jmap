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
#[non_exhaustive]
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
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThreadProperty {
    /// The `id` property (RFC 8621 §3).
    Id,
    /// The `emailIds` property (RFC 8621 §3).
    EmailIds,
}

/// Property selector for [`crate::Email`] `/get` and `/set`.
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
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

/// RFC 8621 §2 (rfc8621.txt:487, 554, 567, 572, 577, 623): `id` is
/// "(immutable; server-set)"; the four counts and `myRights` are
/// "(server-set)".
impl SetObject for crate::Mailbox {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "totalEmails",
        "unreadEmails",
        "totalThreads",
        "unreadThreads",
        "myRights",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] =
        &["name", "parentId", "role", "sortOrder", "isSubscribed"];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::Mailbox {
    type Filter = crate::MailboxFilter;
    type Comparator = crate::MailboxComparator;
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

/// RFC 8621 §4.1 (rfc8621.txt:1316-2135): `id`, `blobId`, `threadId`,
/// `size`, `hasAttachment` and `preview` are "(immutable; server-set)";
/// `mailboxIds` and `keywords` carry no attribute; every other property is
/// "(immutable)". The `header:` forms (§4.1.3) are not struct fields.
impl SetObject for crate::Email {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "blobId",
        "threadId",
        "size",
        "hasAttachment",
        "preview",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "mailboxIds",
        "keywords",
        "receivedAt",
        "messageId",
        "inReplyTo",
        "references",
        "sender",
        "from",
        "to",
        "cc",
        "bcc",
        "replyTo",
        "subject",
        "sentAt",
        "headers",
        "bodyStructure",
        "bodyValues",
        "textBody",
        "htmlBody",
        "attachments",
    ];
    const IMMUTABLE: &'static [&'static str] = &[
        "receivedAt",
        "messageId",
        "inReplyTo",
        "references",
        "sender",
        "from",
        "to",
        "cc",
        "bcc",
        "replyTo",
        "subject",
        "sentAt",
        "headers",
        "bodyStructure",
        "bodyValues",
        "textBody",
        "htmlBody",
        "attachments",
    ];
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

/// RFC 8621 §6 (rfc8621.txt:3988, 3997, 4039): `id` is "(immutable;
/// server-set)", `email` is "(immutable)", `mayDelete` is "(server-set)".
impl SetObject for crate::Identity {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "mayDelete"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "name",
        "email",
        "replyTo",
        "bcc",
        "textSignature",
        "htmlSignature",
    ];
    const IMMUTABLE: &'static [&'static str] = &["email"];
}

impl JmapObject for crate::EmailSubmission {
    const TYPE_NAME: &'static str = "EmailSubmission";
    type Property = EmailSubmissionProperty;
}

impl GetObject for crate::EmailSubmission {}

/// RFC 8621 §7 (rfc8621.txt:4134-4417): `id`, `threadId` and `sendAt` are
/// "(immutable; server-set)"; `deliveryStatus`, `dsnBlobIds` and
/// `mdnBlobIds` are "(server-set)"; `identityId`, `emailId` and `envelope`
/// are "(immutable)"; `undoStatus` carries no attribute.
impl SetObject for crate::EmailSubmission {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "threadId",
        "sendAt",
        "deliveryStatus",
        "dsnBlobIds",
        "mdnBlobIds",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] =
        &["identityId", "emailId", "envelope", "undoStatus"];
    const IMMUTABLE: &'static [&'static str] = &["identityId", "emailId", "envelope"];
}

impl QueryObject for crate::EmailSubmission {
    type Filter = crate::EmailSubmissionFilter;
    type Comparator = crate::EmailSubmissionComparator;
}

impl JmapObject for crate::VacationResponse {
    const TYPE_NAME: &'static str = "VacationResponse";
    type Property = VacationResponseProperty;
}

impl GetObject for crate::VacationResponse {}

/// RFC 8621 §8 (rfc8621.txt:4801): `id` is "(immutable; server-set)"; the
/// other properties carry no attribute.
impl SetObject for crate::VacationResponse {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "isEnabled",
        "fromDate",
        "toDate",
        "subject",
        "textBody",
        "htmlBody",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
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

/// RFC 9661 §2 (rfc9661.txt:261, 284): `id` is "(immutable; server-set)",
/// `isActive` is "(server-set; default: false)".
#[cfg(feature = "sieve")]
impl SetObject for crate::sieve::SieveScript {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "isActive"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["name", "blobId"];
    const IMMUTABLE: &'static [&'static str] = &[];
}

#[cfg(test)]
mod tests {
    //! Each test builds a fully populated object with a struct literal, so a
    //! new field is a compile error here until it is set, then a
    //! classification failure until the `SetObject` lists name it.

    use jmap_types::{check_property_classification, Date, Id, UTCDate};
    use serde_json::{json, Map, Value};
    use std::collections::BTreeMap;

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    fn addrs() -> Option<Vec<crate::EmailAddress>> {
        Some(de(json!([{"name": "A", "email": "a@example.com"}])))
    }

    fn part() -> crate::EmailBodyPart {
        de(json!({"partId": "1", "type": "text/plain"}))
    }

    #[test]
    fn mailbox_properties_are_classified() {
        let obj = crate::Mailbox {
            id: Id::from("m1"),
            name: "Inbox".into(),
            parent_id: Some(Id::from("m0")),
            role: Some(de(json!("inbox"))),
            sort_order: 1,
            total_emails: 1,
            unread_emails: 1,
            total_threads: 1,
            unread_threads: 1,
            my_rights: crate::MailboxRights::default(),
            is_subscribed: true,
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn email_properties_are_classified() {
        let obj = crate::Email {
            id: Id::from("e1"),
            blob_id: Id::from("b1"),
            thread_id: Id::from("t1"),
            mailbox_ids: BTreeMap::from([(Id::from("m1"), true)]),
            keywords: de(json!({"$seen": true})),
            size: 1,
            received_at: UTCDate::from("2026-01-01T00:00:00Z"),
            message_id: Some(vec!["a@example.com".into()]),
            in_reply_to: Some(vec!["b@example.com".into()]),
            references: Some(vec!["b@example.com".into()]),
            sender: addrs(),
            from: addrs(),
            to: addrs(),
            cc: addrs(),
            bcc: addrs(),
            reply_to: addrs(),
            subject: Some("s".into()),
            sent_at: Some(Date::from("2026-01-01T00:00:00Z")),
            headers: de(json!([{"name": "Subject", "value": " s"}])),
            body_values: de(json!({"1": {"value": "v"}})),
            text_body: vec![part()],
            html_body: vec![part()],
            attachments: vec![part()],
            body_structure: Some(part()),
            has_attachment: true,
            preview: Some("p".into()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn identity_properties_are_classified() {
        let obj = crate::Identity {
            id: Id::from("i1"),
            name: "A".into(),
            email: "a@example.com".into(),
            reply_to: addrs(),
            bcc: addrs(),
            text_signature: "t".into(),
            html_signature: "h".into(),
            may_delete: true,
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn email_submission_properties_are_classified() {
        let obj = crate::EmailSubmission {
            id: Id::from("s1"),
            identity_id: Id::from("i1"),
            email_id: Id::from("e1"),
            thread_id: Id::from("t1"),
            envelope: Some(de(json!({
                "mailFrom": {"email": "a@example.com"},
                "rcptTo": [{"email": "b@example.com"}],
            }))),
            send_at: UTCDate::from("2026-01-01T00:00:00Z"),
            undo_status: de(json!("pending")),
            delivery_status: Some(de(json!({"b@example.com": {
                "smtpReply": "250 OK",
                "delivered": "queued",
                "displayed": "unknown",
            }}))),
            dsn_blob_ids: vec![Id::from("d1")],
            mdn_blob_ids: vec![Id::from("n1")],
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn vacation_response_properties_are_classified() {
        let obj = crate::VacationResponse {
            id: Id::from("singleton"),
            is_enabled: true,
            from_date: Some(UTCDate::from("2026-01-01T00:00:00Z")),
            to_date: Some(UTCDate::from("2026-02-01T00:00:00Z")),
            subject: Some("s".into()),
            text_body: Some("t".into()),
            html_body: Some("h".into()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[cfg(feature = "sieve")]
    #[test]
    fn sieve_script_properties_are_classified() {
        let obj = crate::SieveScript {
            id: Id::from("s1"),
            name: Some("n".into()),
            blob_id: Id::from("b1"),
            is_active: true,
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }
}
