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
