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

#![forbid(unsafe_code)]

pub mod email;
pub mod identity;
pub mod mailbox;
pub mod snippet;
pub mod submission;
pub mod thread;

pub use email::{
    Email, EmailAddress, EmailAddressGroup, EmailBodyPart, EmailBodyValue, EmailHeader,
};
pub use identity::Identity;
pub use mailbox::{Mailbox, MailboxRights, MailboxRole};
pub use snippet::SearchSnippet;
pub use submission::{
    Address, Delivered, DeliveryStatus, Displayed, EmailSubmission, Envelope, UndoStatus,
};
pub use thread::Thread;
