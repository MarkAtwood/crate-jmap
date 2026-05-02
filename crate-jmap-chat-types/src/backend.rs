//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP Chat types.
//!
//! These are defined here so that `jmap-chat-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the chat types are
//! local to this crate).

use jmap_types::{GetObject, JmapObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Chat`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatProperty {
    Id,
    Kind,
    ContactId,
    Name,
    Description,
    AvatarBlobId,
    Members,
    SpaceId,
    CategoryId,
    Position,
    Topic,
    SlowModeSeconds,
    PermissionOverrides,
    CreatedAt,
    UnreadCount,
    PinnedMessageIds,
    Muted,
    ReceiveTypingIndicators,
    LastMessageAt,
    MuteUntil,
    ReceiptSharing,
    MessageExpirySeconds,
}

/// Property selector for [`crate::Message`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageProperty {
    Id,
    SenderMsgId,
    SenderId,
    ChatId,
    Body,
    BodyType,
    Attachments,
    Mentions,
    Actions,
    Reactions,
    SentAt,
    ReceivedAt,
    DeliveryState,
    ReplyTo,
    ThreadRootId,
    ReplyCount,
    UnreadReplyCount,
    SenderExpiresAt,
    BurnOnRead,
    DeliveryReceipts,
    DeliveredAt,
    ReadAt,
    ReadDisposition,
    EditedAt,
    EditHistory,
    DeletedAt,
    DeletedForAll,
}

/// Property selector for [`crate::Space`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpaceProperty {
    Id,
    Name,
    Description,
    IconBlobId,
    Roles,
    Members,
    Categories,
    UncategorizedChannelIds,
    CreatedAt,
    IsPublic,
    IsPubliclyPreviewable,
    MemberCount,
}

/// Property selector for [`crate::ChatContact`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatContactProperty {
    Id,
    Login,
    FirstSeenAt,
    LastSeenAt,
    Blocked,
    DisplayName,
    Presence,
    LastActiveAt,
    StatusText,
    StatusEmoji,
    Endpoints,
}

/// Property selector for [`crate::ReadPosition`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReadPositionProperty {
    Id,
    ChatId,
    LastReadMessageId,
    LastReadAt,
}

/// Property selector for [`crate::CustomEmoji`] `/get`, `/set`, and `/query`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CustomEmojiProperty {
    Id,
    Name,
    BlobId,
    SpaceId,
    CreatedBy,
    CreatedAt,
}

/// Property selector for [`crate::SpaceInvite`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpaceInviteProperty {
    Id,
    Code,
    SpaceId,
    DefaultChannelId,
    CreatedBy,
    ExpiresAt,
    MaxUses,
    Uses,
    CreatedAt,
}

/// Property selector for [`crate::SpaceBan`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpaceBanProperty {
    Id,
    SpaceId,
    UserId,
    BannedBy,
    Reason,
    CreatedAt,
    ExpiresAt,
}

/// Property selector for [`crate::PresenceStatus`] `/get` and `/set`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PresenceStatusProperty {
    Id,
    Presence,
    StatusText,
    StatusEmoji,
    ExpiresAt,
    ReceiptSharing,
    UpdatedAt,
}

// ---------------------------------------------------------------------------
// JmapObject impls
// ---------------------------------------------------------------------------

impl JmapObject for crate::Chat {
    const TYPE_NAME: &'static str = "Chat";
    type Property = ChatProperty;
}

impl GetObject for crate::Chat {}

impl SetObject for crate::Chat {
    type Patch = serde_json::Value;
}

impl QueryObject for crate::Chat {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::Message {
    const TYPE_NAME: &'static str = "Message";
    type Property = MessageProperty;
}

impl GetObject for crate::Message {}

impl SetObject for crate::Message {
    type Patch = serde_json::Value;
}

impl QueryObject for crate::Message {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::Space {
    const TYPE_NAME: &'static str = "Space";
    type Property = SpaceProperty;
}

impl GetObject for crate::Space {}

impl SetObject for crate::Space {
    type Patch = serde_json::Value;
}

impl QueryObject for crate::Space {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::ChatContact {
    const TYPE_NAME: &'static str = "ChatContact";
    type Property = ChatContactProperty;
}

impl GetObject for crate::ChatContact {}

impl SetObject for crate::ChatContact {
    type Patch = serde_json::Value;
}

impl QueryObject for crate::ChatContact {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::ReadPosition {
    const TYPE_NAME: &'static str = "ReadPosition";
    type Property = ReadPositionProperty;
}

impl GetObject for crate::ReadPosition {}

impl SetObject for crate::ReadPosition {
    type Patch = serde_json::Value;
}

impl JmapObject for crate::CustomEmoji {
    const TYPE_NAME: &'static str = "CustomEmoji";
    type Property = CustomEmojiProperty;
}

impl GetObject for crate::CustomEmoji {}

impl SetObject for crate::CustomEmoji {
    type Patch = serde_json::Value;
}

impl QueryObject for crate::CustomEmoji {
    type Filter = serde_json::Value;
    type Comparator = serde_json::Value;
}

impl JmapObject for crate::SpaceInvite {
    const TYPE_NAME: &'static str = "SpaceInvite";
    type Property = SpaceInviteProperty;
}

impl GetObject for crate::SpaceInvite {}

impl SetObject for crate::SpaceInvite {
    type Patch = serde_json::Value;
}

impl JmapObject for crate::SpaceBan {
    const TYPE_NAME: &'static str = "SpaceBan";
    type Property = SpaceBanProperty;
}

impl GetObject for crate::SpaceBan {}

impl SetObject for crate::SpaceBan {
    type Patch = serde_json::Value;
}

impl JmapObject for crate::PresenceStatus {
    const TYPE_NAME: &'static str = "PresenceStatus";
    type Property = PresenceStatusProperty;
}

impl GetObject for crate::PresenceStatus {}

impl SetObject for crate::PresenceStatus {
    type Patch = serde_json::Value;
}
