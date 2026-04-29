//! JMAP Chat extension data types (draft-atwood-jmap-chat).
//!
//! Provides serde-annotated structs and enums for the Chat, Message, Space,
//! ChatContact, ReadPosition, PresenceStatus, and ephemeral WebSocket event
//! types defined by the JMAP Chat extension drafts.
//!
//! Types only — no method handlers, no async, no network I/O.

#![forbid(unsafe_code)]

pub mod chat;
pub mod clearable;
pub mod contact;
pub mod emoji;
pub mod ephemeral;
pub mod message;
pub mod position;
pub mod presence;
pub mod push;
pub mod space;

pub use chat::{ChannelPermission, Chat, ChatMember};
pub use clearable::Clearable;
pub use contact::{ChatContact, Endpoint};
pub use emoji::CustomEmoji;
pub use ephemeral::{
    ChatPresenceEvent, ChatStreamDisable, ChatStreamEnable, ChatTypingEvent, EphemeralMessage,
};
pub use message::{
    Attachment, DeliveryReceipt, Mention, Message, MessageAction, MessageRevision, Reaction,
};
pub use position::ReadPosition;
pub use presence::PresenceStatus;
pub use push::{ChatMessageEntry, ChatMessagePush, ChatPushConfig};
pub use space::{Category, Space, SpaceBan, SpaceInvite, SpaceMember, SpaceRole};
