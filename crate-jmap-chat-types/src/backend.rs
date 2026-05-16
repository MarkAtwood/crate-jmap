//! Property selector enums and [`jmap_types::JmapObject`] impls for JMAP Chat types.
//!
//! These are defined here so that `jmap-chat-server` can use them without
//! violating the orphan rule (`JmapObject` is foreign but the chat types are
//! local to this crate).

use jmap_types::{GetObject, JmapObject, PatchObject, QueryObject, SetObject};

// ---------------------------------------------------------------------------
// Property selector enums (server-side; no serde required)
// ---------------------------------------------------------------------------

/// Property selector for [`crate::Chat`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.10).
    Id,
    /// The `kind` property (draft-atwood-jmap-chat-00 §4.10).
    Kind,
    /// The `contactId` property (draft-atwood-jmap-chat-00 §4.10).
    ContactId,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.10).
    Name,
    /// The `description` property (draft-atwood-jmap-chat-00 §4.10).
    Description,
    /// The `avatarBlobId` property (draft-atwood-jmap-chat-00 §4.10).
    AvatarBlobId,
    /// The `members` property (draft-atwood-jmap-chat-00 §4.10).
    Members,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.10).
    SpaceId,
    /// The `categoryId` property (draft-atwood-jmap-chat-00 §4.10).
    CategoryId,
    /// The `position` property (draft-atwood-jmap-chat-00 §4.10).
    Position,
    /// The `topic` property (draft-atwood-jmap-chat-00 §4.10).
    Topic,
    /// The `slowModeSeconds` property (draft-atwood-jmap-chat-00 §4.10).
    SlowModeSeconds,
    /// The `permissionOverrides` property (draft-atwood-jmap-chat-00 §4.10).
    PermissionOverrides,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.10).
    CreatedAt,
    /// The `unreadCount` property (draft-atwood-jmap-chat-00 §4.10).
    UnreadCount,
    /// The `pinnedMessageIds` property (draft-atwood-jmap-chat-00 §4.10).
    PinnedMessageIds,
    /// The `muted` property (draft-atwood-jmap-chat-00 §4.10).
    Muted,
    /// The `receiveTypingIndicators` property (draft-atwood-jmap-chat-00 §4.10).
    ReceiveTypingIndicators,
    /// The `lastMessageAt` property (draft-atwood-jmap-chat-00 §4.10).
    LastMessageAt,
    /// The `muteUntil` property (draft-atwood-jmap-chat-00 §4.10).
    MuteUntil,
    /// The `receiptSharing` property (draft-atwood-jmap-chat-00 §4.10).
    ReceiptSharing,
    /// The `messageExpirySeconds` property (draft-atwood-jmap-chat-00 §4.10).
    MessageExpirySeconds,
}

/// Property selector for [`crate::Message`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.11).
    Id,
    /// The `senderMsgId` property (draft-atwood-jmap-chat-00 §4.11).
    SenderMsgId,
    /// The `senderId` property (draft-atwood-jmap-chat-00 §4.11).
    SenderId,
    /// The `chatId` property (draft-atwood-jmap-chat-00 §4.11).
    ChatId,
    /// The `body` property (draft-atwood-jmap-chat-00 §4.11).
    Body,
    /// The `bodyType` property (draft-atwood-jmap-chat-00 §4.11).
    BodyType,
    /// The `attachments` property (draft-atwood-jmap-chat-00 §4.11).
    Attachments,
    /// The `mentions` property (draft-atwood-jmap-chat-00 §4.11).
    Mentions,
    /// The `actions` property (draft-atwood-jmap-chat-00 §4.11).
    Actions,
    /// The `reactions` property (draft-atwood-jmap-chat-00 §4.11).
    Reactions,
    /// The `sentAt` property (draft-atwood-jmap-chat-00 §4.11).
    SentAt,
    /// The `receivedAt` property (draft-atwood-jmap-chat-00 §4.11).
    ReceivedAt,
    /// The `deliveryState` property (draft-atwood-jmap-chat-00 §4.11).
    DeliveryState,
    /// The `replyTo` property (draft-atwood-jmap-chat-00 §4.11).
    ReplyTo,
    /// The `threadRootId` property (draft-atwood-jmap-chat-00 §4.11).
    ThreadRootId,
    /// The `replyCount` property (draft-atwood-jmap-chat-00 §4.11).
    ReplyCount,
    /// The `unreadReplyCount` property (draft-atwood-jmap-chat-00 §4.11).
    UnreadReplyCount,
    /// The `senderExpiresAt` property (draft-atwood-jmap-chat-00 §4.11).
    SenderExpiresAt,
    /// The `burnOnRead` property (draft-atwood-jmap-chat-00 §4.11).
    BurnOnRead,
    /// The `deliveryReceipts` property (draft-atwood-jmap-chat-00 §4.11).
    DeliveryReceipts,
    /// The `deliveredAt` property (draft-atwood-jmap-chat-00 §4.11).
    DeliveredAt,
    /// The `readAt` property (draft-atwood-jmap-chat-00 §4.11).
    ReadAt,
    /// The `readDisposition` property (draft-atwood-jmap-chat-00 §4.11).
    ReadDisposition,
    /// The `editedAt` property (draft-atwood-jmap-chat-00 §4.11).
    EditedAt,
    /// The `editHistory` property (draft-atwood-jmap-chat-00 §4.11).
    EditHistory,
    /// The `deletedAt` property (draft-atwood-jmap-chat-00 §4.11).
    DeletedAt,
    /// The `deletedForAll` property (draft-atwood-jmap-chat-00 §4.11).
    DeletedForAll,
}

/// Property selector for [`crate::Space`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpaceProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.16).
    Id,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.16).
    Name,
    /// The `description` property (draft-atwood-jmap-chat-00 §4.16).
    Description,
    /// The `iconBlobId` property (draft-atwood-jmap-chat-00 §4.16).
    IconBlobId,
    /// The `roles` property (draft-atwood-jmap-chat-00 §4.16).
    Roles,
    /// The `members` property (draft-atwood-jmap-chat-00 §4.16).
    Members,
    /// The `categories` property (draft-atwood-jmap-chat-00 §4.16).
    Categories,
    /// The `uncategorizedChannelIds` property (draft-atwood-jmap-chat-00 §4.16).
    UncategorizedChannelIds,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.16).
    CreatedAt,
    /// The `isPublic` property (draft-atwood-jmap-chat-00 §4.16).
    IsPublic,
    /// The `isPubliclyPreviewable` property (draft-atwood-jmap-chat-00 §4.16).
    IsPubliclyPreviewable,
    /// The `memberCount` property (draft-atwood-jmap-chat-00 §4.16).
    MemberCount,
}

/// Property selector for [`crate::ChatContact`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatContactProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.8).
    Id,
    /// The `login` property (draft-atwood-jmap-chat-00 §4.8).
    Login,
    /// The `firstSeenAt` property (draft-atwood-jmap-chat-00 §4.8).
    FirstSeenAt,
    /// The `lastSeenAt` property (draft-atwood-jmap-chat-00 §4.8).
    LastSeenAt,
    /// The `blocked` property (draft-atwood-jmap-chat-00 §4.8).
    Blocked,
    /// The `displayName` property (draft-atwood-jmap-chat-00 §4.8).
    DisplayName,
    /// The `presence` property (draft-atwood-jmap-chat-00 §4.8).
    Presence,
    /// The `lastActiveAt` property (draft-atwood-jmap-chat-00 §4.8).
    LastActiveAt,
    /// The `statusText` property (draft-atwood-jmap-chat-00 §4.8).
    StatusText,
    /// The `statusEmoji` property (draft-atwood-jmap-chat-00 §4.8).
    StatusEmoji,
    /// The `endpoints` property (draft-atwood-jmap-chat-00 §4.8).
    Endpoints,
}

/// Property selector for [`crate::ReadPosition`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReadPositionProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.20).
    Id,
    /// The `chatId` property (draft-atwood-jmap-chat-00 §4.20).
    ChatId,
    /// The `lastReadMessageId` property (draft-atwood-jmap-chat-00 §4.20).
    LastReadMessageId,
    /// The `lastReadAt` property (draft-atwood-jmap-chat-00 §4.20).
    LastReadAt,
}

/// Property selector for [`crate::CustomEmoji`] `/get`, `/set`, and `/query`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CustomEmojiProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.17).
    Id,
    /// The `name` property (draft-atwood-jmap-chat-00 §4.17).
    Name,
    /// The `blobId` property (draft-atwood-jmap-chat-00 §4.17).
    BlobId,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.17).
    SpaceId,
    /// The `createdBy` property (draft-atwood-jmap-chat-00 §4.17).
    CreatedBy,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.17).
    CreatedAt,
}

/// Property selector for [`crate::SpaceInvite`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpaceInviteProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.18).
    Id,
    /// The `code` property (draft-atwood-jmap-chat-00 §4.18).
    Code,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.18).
    SpaceId,
    /// The `defaultChannelId` property (draft-atwood-jmap-chat-00 §4.18).
    DefaultChannelId,
    /// The `createdBy` property (draft-atwood-jmap-chat-00 §4.18).
    CreatedBy,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.18).
    ExpiresAt,
    /// The `maxUses` property (draft-atwood-jmap-chat-00 §4.18).
    MaxUses,
    /// The `uses` property (draft-atwood-jmap-chat-00 §4.18).
    Uses,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.18).
    CreatedAt,
}

/// Property selector for [`crate::SpaceBan`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpaceBanProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.19).
    Id,
    /// The `spaceId` property (draft-atwood-jmap-chat-00 §4.19).
    SpaceId,
    /// The `userId` property (draft-atwood-jmap-chat-00 §4.19).
    UserId,
    /// The `bannedBy` property (draft-atwood-jmap-chat-00 §4.19).
    BannedBy,
    /// The `reason` property (draft-atwood-jmap-chat-00 §4.19).
    Reason,
    /// The `createdAt` property (draft-atwood-jmap-chat-00 §4.19).
    CreatedAt,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.19).
    ExpiresAt,
}

/// Property selector for [`crate::PresenceStatus`] `/get` and `/set`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PresenceStatusProperty {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.21).
    Id,
    /// The `presence` property (draft-atwood-jmap-chat-00 §4.21).
    Presence,
    /// The `statusText` property (draft-atwood-jmap-chat-00 §4.21).
    StatusText,
    /// The `statusEmoji` property (draft-atwood-jmap-chat-00 §4.21).
    StatusEmoji,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.21).
    ExpiresAt,
    /// The `receiptSharing` property (draft-atwood-jmap-chat-00 §4.21).
    ReceiptSharing,
    /// The `updatedAt` property (draft-atwood-jmap-chat-00 §4.21).
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
    type Patch = PatchObject;
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
    type Patch = PatchObject;
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
    type Patch = PatchObject;
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
    type Patch = PatchObject;
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
    type Patch = PatchObject;
}

impl JmapObject for crate::CustomEmoji {
    const TYPE_NAME: &'static str = "CustomEmoji";
    type Property = CustomEmojiProperty;
}

impl GetObject for crate::CustomEmoji {}

impl SetObject for crate::CustomEmoji {
    type Patch = PatchObject;
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
    type Patch = PatchObject;
}

impl JmapObject for crate::SpaceBan {
    const TYPE_NAME: &'static str = "SpaceBan";
    type Property = SpaceBanProperty;
}

impl GetObject for crate::SpaceBan {}

impl SetObject for crate::SpaceBan {
    type Patch = PatchObject;
}

impl JmapObject for crate::PresenceStatus {
    const TYPE_NAME: &'static str = "PresenceStatus";
    type Property = PresenceStatusProperty;
}

impl GetObject for crate::PresenceStatus {}

impl SetObject for crate::PresenceStatus {
    type Patch = PatchObject;
}
