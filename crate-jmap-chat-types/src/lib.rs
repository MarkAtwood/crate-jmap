//! JMAP Chat extension data types (draft-atwood-jmap-chat).
//!
//! Provides [`Chat`], [`Message`], [`Space`], [`ChatContact`], [`ReadPosition`],
//! [`PresenceStatus`], [`CustomEmoji`], and ephemeral WebSocket event and push
//! notification types defined by the JMAP Chat extension drafts.
//!
//! [`Clearable<T>`] handles the JSON null-vs-absent distinction for patch fields.
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` (shared wire primitives) and `jmap-chat-server`
//! (method handlers).
//!
//! All types implement [`serde::Serialize`] and [`serde::Deserialize`] with the
//! camelCase field names required by the JMAP wire format.
//!
//! # Example
//!
//! ```rust
//! use jmap_chat_types::{Chat, ChatKind};
//!
//! let json = r#"{
//!     "id": "c1",
//!     "kind": "direct",
//!     "contactId": "u1",
//!     "createdAt": "2026-01-01T00:00:00Z",
//!     "unreadCount": 0,
//!     "pinnedMessageIds": [],
//!     "muted": false,
//!     "receiveTypingIndicators": true
//! }"#;
//!
//! let chat: Chat = serde_json::from_str(json).unwrap();
//! assert_eq!(chat.kind, ChatKind::Direct);
//! ```

#![forbid(unsafe_code)]

#[macro_use]
mod string_enum;

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

pub use chat::{ChannelPermission, Chat, ChatKind, ChatMember};
pub use clearable::Clearable;
pub use contact::{ChatContact, Endpoint};
pub use emoji::CustomEmoji;
pub use ephemeral::{
    ChatPresenceEvent, ChatStreamDisable, ChatStreamEnable, ChatTypingEvent, EphemeralMessage,
};
pub use message::{
    Attachment, DeliveryReceipt, DeliveryState, Mention, Message, MessageAction, MessageRevision,
    Reaction, SenderId,
};
pub use position::ReadPosition;
pub use presence::{Presence, PresenceStatus};
pub use push::{ChatMessageEntry, ChatMessagePush, ChatPushConfig};
pub use space::{Category, Space, SpaceBan, SpaceInvite, SpaceMember, SpaceRole};
