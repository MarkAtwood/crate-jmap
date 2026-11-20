//! RFC 8621 JMAP for Mail data types.
//!
//! Provides [`Email`], [`Mailbox`], [`Thread`], [`Identity`], [`EmailSubmission`],
//! and [`SearchSnippet`] — the core object types defined by
//! [RFC 8621](https://www.rfc-editor.org/rfc/rfc8621).
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` (shared wire primitives) and `jmap-mail-server`
//! (method handlers).
//!
//! All types implement [`serde::Serialize`] and [`serde::Deserialize`] with the
//! camelCase field names required by the JMAP wire format.
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types ([`Email`],
//! [`EmailAddress`], [`EmailAddressGroup`], [`EmailHeader`], [`EmailBodyPart`],
//! [`EmailBodyValue`], [`SearchSnippet`], [`Identity`], [`EmailSubmission`] and
//! its [`Envelope`] / [`Address`], [`VacationResponse`], and, behind their
//! features, `Mdn`, `SieveScript` and `EmailPush`). `tracing` or `log` macros
//! that interpolate one of these values with `?` (debug) or `%` (display) emit
//! its full contents to the configured sink: names and email addresses,
//! subjects, header values, body text, SMTP envelope addresses, signatures, and
//! auto-reply text. That data is typically regulated as personal data under
//! GDPR, CCPA, and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks sender, recipients, subject, headers, and body text.
//! tracing::debug!(email = ?email, "fetched email");
//!
//! // GOOD — emits only the opaque id.
//! tracing::debug!(email_id = ?email.id, "fetched email");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.
//!
//! # Example
//!
//! ```rust
//! use jmap_mail_types::Mailbox;
//!
//! let json = r#"{
//!     "id": "mb1",
//!     "name": "Inbox",
//!     "role": "inbox",
//!     "sortOrder": 10,
//!     "totalEmails": 42,
//!     "unreadEmails": 3,
//!     "totalThreads": 20,
//!     "unreadThreads": 2,
//!     "myRights": {
//!         "mayReadItems": true,
//!         "mayAddItems": true,
//!         "mayRemoveItems": true,
//!         "maySetSeen": true,
//!         "maySetKeywords": true,
//!         "mayCreateChild": true,
//!         "mayRename": true,
//!         "mayDelete": false,
//!         "maySubmit": false
//!     },
//!     "isSubscribed": true
//! }"#;
//!
//! let mailbox: Mailbox = serde_json::from_str(json).unwrap();
//! assert_eq!(mailbox.name, "Inbox");
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod backend;
mod capability;
mod email;
mod identity;
mod keyword;
mod mailbox;
mod query;
mod snippet;
mod submission;
mod thread;
mod vacation;

#[cfg(feature = "emailpush")]
mod emailpush;
#[cfg(feature = "emailpush")]
pub use emailpush::{
    EMAIL_PUSH_PROPERTY, EmailPush, EmailPushAccountCapability, EmailPushCapability,
    EmailPushConfig, JMAP_EMAILPUSH_URI, email_push, set_email_push,
};

#[cfg(feature = "mdn")]
mod mdn;
#[cfg(feature = "mdn")]
pub use mdn::{
    ActionMode, Disposition, DispositionType, JMAP_MDN_URI, Mdn, MdnParseRequest, MdnParseResponse,
    MdnSendRequest, MdnSendResponse, SendingMode,
};

#[cfg(feature = "sieve")]
mod sieve;
#[cfg(feature = "sieve")]
pub use sieve::{JMAP_SIEVE_SCRIPTS_URI, SieveAccountCapability, SieveCapability, SieveScript};

#[cfg(feature = "sieve")]
pub use backend::SieveScriptProperty;
pub use backend::{
    EmailProperty, EmailSubmissionProperty, IdentityProperty, MailboxProperty,
    SearchSnippetProperty, ThreadProperty, VacationResponseProperty,
};
pub use capability::{
    JMAP_MAIL_SHARE_URI, JMAP_MAIL_URI, JMAP_SUBMISSION_URI, JMAP_VACATIONRESPONSE_URI,
    MailAccountCapability, MailCapability, SubmissionAccountCapability, SubmissionCapability,
    VacationResponseCapability,
};
pub use email::{
    Email, EmailAddress, EmailAddressGroup, EmailBodyPart, EmailBodyValue, EmailHeader,
};
pub use identity::Identity;
pub use keyword::{
    ANSWERED, DRAFT, FLAGGED, FORWARDED, JUNK, Keyword, KeywordError, NOT_JUNK, PHISHING, SEEN,
};
pub use mailbox::{Mailbox, MailboxFilterCondition, MailboxRights, MailboxRole};
pub use query::{
    ComparatorProperty, EmailComparator, EmailFilter, EmailFilterCondition,
    EmailSubmissionComparator, EmailSubmissionComparatorProperty, EmailSubmissionFilter, Filter,
    FilterOperator, MailboxComparator, MailboxComparatorProperty, MailboxFilter, Operator,
    SieveScriptComparator, SieveScriptComparatorProperty,
};
pub use snippet::{SearchSnippet, SearchSnippetGetResponse};
pub use submission::{
    Address, Delivered, DeliveryStatus, Displayed, EmailSubmission, EmailSubmissionFilterCondition,
    Envelope, UndoStatus,
};
pub use thread::Thread;
pub use vacation::VacationResponse;
