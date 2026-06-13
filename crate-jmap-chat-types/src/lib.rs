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
//! # Constructing types
//!
//! Types in this crate fall into two categories:
//!
//! ## Top-level objects with `new()` constructors
//!
//! [`Chat`], [`Message`], [`ChatContact`], [`Space`], [`SpaceInvite`],
//! [`SpaceBan`], [`ReadPosition`], [`PresenceStatus`], [`CustomEmoji`],
//! and [`ChatMessagePush`] provide `new()` constructors that accept
//! required fields and default optional fields to `None`/empty.
//! Set optional fields after construction:
//!
//! ```rust
//! use jmap_chat_types::Message;
//! use jmap_types::{Id, UTCDate};
//! use jmap_chat_types::{SenderId, DeliveryState};
//!
//! let mut msg = Message::new(
//!     Id::from("msg-1"), Id::from("msg-1"),
//!     SenderId::Owner, Id::from("chat-1"),
//!     "hello", "text/plain",
//!     UTCDate::from("2026-01-01T00:00:00Z"),
//!     UTCDate::from("2026-01-01T00:00:01Z"),
//!     DeliveryState::Pending,
//! );
//! msg.reply_to = Some(Id::from("msg-0"));
//! ```
//!
//! ## Sub-objects: construct via serde
//!
//! [`Attachment`], [`Mention`], [`BroadcastMention`], [`Reaction`],
//! [`MessageRevision`], [`DeliveryReceipt`], [`MessageAction`],
//! [`Endpoint`], [`ChatMember`], [`ChannelPermission`], [`SpaceRole`],
//! [`SpaceMember`], and [`Category`] are `#[non_exhaustive]` without
//! `new()` constructors. **This is intentional.** Construct them via
//! [`serde_json::from_value`]:
//!
//! ```rust
//! use jmap_chat_types::Attachment;
//! use serde_json::json;
//!
//! let attachment: Attachment = serde_json::from_value(json!({
//!     "blobId": "blob-abc",
//!     "filename": "photo.png",
//!     "contentType": "image/png",
//!     "size": 102400,
//!     "sha256": "a".repeat(64),
//! })).expect("valid attachment fields");
//!
//! assert_eq!(attachment.filename, "photo.png");
//! ```
//!
//! This pattern ensures forward compatibility: when new fields are
//! added to the spec, the serde constructor picks up their defaults
//! automatically. A `new()` constructor would require a breaking
//! signature change for every new field.
//!
//! Downstream crates that construct these types frequently should
//! define thin helper functions:
//!
//! ```rust
//! use jmap_chat_types::Mention;
//! use serde_json::json;
//!
//! fn make_mention(id: &str, offset: u64, length: u64) -> Mention {
//!     serde_json::from_value(json!({
//!         "id": id, "offset": offset, "length": length,
//!     })).expect("valid mention fields")
//! }
//! ```
//!
//! # Deserialization
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

pub mod backend;
pub mod capability;
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
pub mod space_set;
pub mod vocabulary;

pub use backend::{
    ChatContactProperty, ChatProperty, CustomEmojiProperty, MessageProperty,
    PresenceStatusProperty, ReadPositionProperty, SpaceBanProperty, SpaceInviteProperty,
    SpaceProperty,
};
pub use capability::{ChatCapability, ChatPushCapability, JMAP_CHAT_PUSH_URI, JMAP_CHAT_URI};
pub use chat::{ChannelPermission, Chat, ChatKind, ChatMember};
pub use clearable::Clearable;
pub use contact::{ChatContact, Endpoint};
pub use emoji::CustomEmoji;
pub use ephemeral::{
    ChatPresenceEvent, ChatStreamDisable, ChatStreamEnable, ChatTypingEvent, EphemeralMessage,
};
pub use message::{
    Attachment, BodyType, BroadcastMention, DeliveryReceipt, DeliveryState, Mention, Message,
    MessageAction, MessageRevision, Reaction, ReadDisposition, SenderId,
    BROADCAST_MENTION_SCOPES,
};
pub use position::ReadPosition;
pub use presence::{Presence, PresenceStatus};
pub use push::{ChatMessageEntry, ChatMessagePush, ChatPushConfig, UrgencyLevel};
pub use space::{Category, Space, SpaceBan, SpaceInvite, SpaceMember, SpaceRole};
pub use space_set::{
    CategoryPatch, ChannelCreate, ChannelPatch, MemberCreate, MemberPatch, RolePatch,
    SpaceMetadataPatch, SpacePatchOp,
};
