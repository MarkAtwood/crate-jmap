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
    /// The `identity` property (draft-atwood-jmap-chat-00 §4.8).
    Identity,
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

/// draft-atwood-jmap-chat-00 §Chat Object Fields (lines 580-664): `id` and
/// `createdAt` are "immutable, server-set"; `closedAt`, `lastMessageAt`,
/// `unreadCount`, `activeCallId` and `activeCallAccountId` are
/// "server-set"; `kind` and `contactId` are "immutable".
///
/// §Chat/set never accepts `members` (changed by the `addMembers`,
/// `removeMembers` and `updateMemberRoles` patch keys) or the channel
/// properties `spaceId`, `categoryId`, `position`, `topic`,
/// `slowModeSeconds` and `permissionOverrides` ("Channel Chats are created
/// exclusively via the `addChannels` patch key in `Space/set`"), so they
/// are server-set here.
///
/// draft-ietf-jmap-metadata-04 §3.2: clients write `metadata` and
/// `privateMetadata` through `Foo/set` (draft-atwood-jmap-chat-00 lets
/// servers list `Chat` in the capability's `dataTypes`).
impl SetObject for crate::Chat {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "createdAt",
        "closedAt",
        "lastMessageAt",
        "unreadCount",
        "activeCallId",
        "activeCallAccountId",
        "members",
        "spaceId",
        "categoryId",
        "position",
        "topic",
        "slowModeSeconds",
        "permissionOverrides",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "kind",
        "contactId",
        "name",
        "description",
        "avatarBlobId",
        "pinnedMessageIds",
        "muted",
        "muteUntil",
        "receiveTypingIndicators",
        "receiptSharing",
        "messageExpirySeconds",
        "metadata",
        "privateMetadata",
    ];
    const IMMUTABLE: &'static [&'static str] = &["kind", "contactId"];
}

impl QueryObject for crate::Chat {
    type Filter = crate::query::ChatFilter;
    type Comparator = crate::query::ChatComparator;
}

impl JmapObject for crate::Message {
    const TYPE_NAME: &'static str = "Message";
    type Property = MessageProperty;
}

impl GetObject for crate::Message {}

/// draft-atwood-jmap-chat-00 §Message Object Fields (lines 683-777): `id`,
/// `senderMsgId`, `senderId` and `receivedAt` are "immutable,
/// server-set"; `replyCount`, `unreadReplyCount`, `deliveryState`,
/// `deliveryReceipts`, `deliveredAt`, `editedAt` and `editHistory` are
/// "server-set"; `chatId`, `topicId`, `subject`, `senderExpiresAt`,
/// `burnOnRead` and `expiresAfterReadSeconds` are "immutable".
///
/// §Message/set "Creating a Message" accepts `attachments`, `actions`,
/// `replyTo`, `threadRootId` and `sentAt`, and no update section changes
/// them, so they are immutable here.
///
/// draft-ietf-jmap-metadata-04 §3.2: clients write `metadata` and
/// `privateMetadata` through `Foo/set`.
impl SetObject for crate::Message {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "senderMsgId",
        "senderId",
        "receivedAt",
        "replyCount",
        "unreadReplyCount",
        "deliveryState",
        "deliveryReceipts",
        "deliveredAt",
        "editedAt",
        "editHistory",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "chatId",
        "body",
        "bodyType",
        "attachments",
        "mentions",
        "broadcastMentions",
        "actions",
        "reactions",
        "replyTo",
        "threadRootId",
        "topicId",
        "subject",
        "sentAt",
        "senderExpiresAt",
        "burnOnRead",
        "expiresAfterReadSeconds",
        "readAt",
        "readDisposition",
        "deletedAt",
        "deletedForAll",
        "metadata",
        "privateMetadata",
    ];
    const IMMUTABLE: &'static [&'static str] = &[
        "chatId",
        "attachments",
        "actions",
        "replyTo",
        "threadRootId",
        "topicId",
        "subject",
        "sentAt",
        "senderExpiresAt",
        "burnOnRead",
        "expiresAfterReadSeconds",
    ];
}

impl QueryObject for crate::Message {
    type Filter = crate::query::MessageFilter;
    type Comparator = crate::query::MessageComparator;
}

impl JmapObject for crate::Space {
    const TYPE_NAME: &'static str = "Space";
    type Property = SpaceProperty;
}

impl GetObject for crate::Space {}

/// draft-atwood-jmap-chat-00 §Space (lines 868-904): `id` and `createdAt`
/// are "immutable, server-set"; `memberCount` and `closedAt` are
/// "server-set".
///
/// §Space/set changes `roles`, `members`, `categories` and
/// `uncategorizedChannelIds` only through its semantic patch keys
/// (`addRoles`, `addMembers`, ...), never as properties, so they are
/// server-set here.
///
/// draft-ietf-jmap-metadata-04 §3.2: clients write `metadata` and
/// `privateMetadata` through `Foo/set`.
impl SetObject for crate::Space {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "createdAt",
        "memberCount",
        "closedAt",
        "roles",
        "members",
        "categories",
        "uncategorizedChannelIds",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "name",
        "description",
        "iconBlobId",
        "isPublic",
        "isPubliclyPreviewable",
        "metadata",
        "privateMetadata",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

impl QueryObject for crate::Space {
    type Filter = crate::query::SpaceFilter;
    type Comparator = crate::query::SpaceComparator;
}

impl JmapObject for crate::ChatContact {
    const TYPE_NAME: &'static str = "ChatContact";
    type Property = ChatContactProperty;
}

impl GetObject for crate::ChatContact {}

/// draft-atwood-jmap-chat-00 §ChatContact (lines 506-539): `id` is
/// "immutable, server-set"; `login`, `firstSeenAt`, `lastSeenAt`,
/// `presence`, `lastActiveAt`, `statusText` and `statusEmoji` are
/// "server-set"; `identity` is "immutable". draft-atwood-jmap-chat-did-00
/// §Optional did field: `did` "is server-set". §ChatContact/set: "`create`
/// accepts only `identity`" and "`update` supports: `blocked`,
/// `displayName`", so `endpoints` is server-set here.
impl SetObject for crate::ChatContact {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &[
        "id",
        "login",
        "firstSeenAt",
        "lastSeenAt",
        "presence",
        "lastActiveAt",
        "statusText",
        "statusEmoji",
        "did",
        "endpoints",
    ];
    const CLIENT_SETTABLE: &'static [&'static str] = &["identity", "displayName", "blocked"];
    const IMMUTABLE: &'static [&'static str] = &["identity"];
}

impl QueryObject for crate::ChatContact {
    type Filter = crate::query::ChatContactFilter;
    type Comparator = crate::query::ChatContactComparator;
}

impl JmapObject for crate::ReadPosition {
    const TYPE_NAME: &'static str = "ReadPosition";
    type Property = ReadPositionProperty;
}

impl GetObject for crate::ReadPosition {}

/// draft-atwood-jmap-chat-00 §ReadPosition (lines 995-1004): `id` is
/// "immutable, server-set", `lastReadAt` is "server-set", `chatId` is
/// "immutable".
impl SetObject for crate::ReadPosition {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "lastReadAt"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["chatId", "lastReadMessageId"];
    const IMMUTABLE: &'static [&'static str] = &["chatId"];
}

impl JmapObject for crate::CustomEmoji {
    const TYPE_NAME: &'static str = "CustomEmoji";
    type Property = CustomEmojiProperty;
}

impl GetObject for crate::CustomEmoji {}

/// draft-atwood-jmap-chat-00 §CustomEmoji (lines 917-932): `id`,
/// `createdBy` and `createdAt` are "immutable, server-set".
/// §CustomEmoji/set: "`create` accepts: `name` ..., `blobId` ..., `spaceId`"
/// and "`update` supports: `name`, `blobId`", so `spaceId` is immutable.
impl SetObject for crate::CustomEmoji {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "createdBy", "createdAt"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["name", "blobId", "spaceId"];
    const IMMUTABLE: &'static [&'static str] = &["spaceId"];
}

impl QueryObject for crate::CustomEmoji {
    type Filter = crate::query::CustomEmojiFilter;
    type Comparator = crate::query::CustomEmojiComparator;
}

impl JmapObject for crate::SpaceInvite {
    const TYPE_NAME: &'static str = "SpaceInvite";
    type Property = SpaceInviteProperty;
}

impl GetObject for crate::SpaceInvite {}

/// draft-atwood-jmap-chat-00 §SpaceInvite (lines 939-963): `id`, `code`,
/// `createdBy` and `createdAt` are "immutable, server-set"; `uses` is
/// "server-set"; `spaceId` is "immutable".
impl SetObject for crate::SpaceInvite {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "code", "createdBy", "uses", "createdAt"];
    const CLIENT_SETTABLE: &'static [&'static str] =
        &["spaceId", "defaultChannelId", "expiresAt", "maxUses"];
    const IMMUTABLE: &'static [&'static str] = &["spaceId"];
}

impl JmapObject for crate::SpaceBan {
    const TYPE_NAME: &'static str = "SpaceBan";
    type Property = SpaceBanProperty;
}

impl GetObject for crate::SpaceBan {}

/// draft-atwood-jmap-chat-00 §SpaceBan (lines 970-988): `id`, `bannedBy`
/// and `createdAt` are "immutable, server-set"; `spaceId` and `userId` are
/// "immutable".
impl SetObject for crate::SpaceBan {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "bannedBy", "createdAt"];
    const CLIENT_SETTABLE: &'static [&'static str] = &["spaceId", "userId", "reason", "expiresAt"];
    const IMMUTABLE: &'static [&'static str] = &["spaceId", "userId"];
}

impl JmapObject for crate::PresenceStatus {
    const TYPE_NAME: &'static str = "PresenceStatus";
    type Property = PresenceStatusProperty;
}

impl GetObject for crate::PresenceStatus {}

/// draft-atwood-jmap-chat-00 §PresenceStatus (lines 1013-1031): `id` is
/// "immutable, server-set", `updatedAt` is "server-set".
impl SetObject for crate::PresenceStatus {
    type Patch = PatchObject;
    const SERVER_SET: &'static [&'static str] = &["id", "updatedAt"];
    const CLIENT_SETTABLE: &'static [&'static str] = &[
        "presence",
        "statusText",
        "statusEmoji",
        "expiresAt",
        "receiptSharing",
    ];
    const IMMUTABLE: &'static [&'static str] = &[];
}

#[cfg(test)]
mod tests {
    //! Each test builds a fully populated object with a struct literal, so a
    //! new field is a compile error here until it is set, then a
    //! classification failure until the `SetObject` lists name it.

    use jmap_types::{Id, UTCDate, check_property_classification};
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;

    fn de<T: serde::de::DeserializeOwned>(v: Value) -> T {
        serde_json::from_value(v).expect("hand-written fixture deserializes")
    }

    fn date() -> UTCDate {
        UTCDate::from("2026-01-01T00:00:00Z")
    }

    #[test]
    fn chat_properties_are_classified() {
        let obj = crate::Chat {
            id: Id::from("c1"),
            kind: de(json!("group")),
            created_at: date(),
            unread_count: 1,
            pinned_message_ids: vec![Id::from("m1")],
            muted: true,
            receive_typing_indicators: true,
            contact_id: Some(Id::from("k1")),
            name: Some("n".into()),
            description: Some("d".into()),
            avatar_blob_id: Some(Id::from("b1")),
            members: Some(vec![]),
            space_id: Some(Id::from("s1")),
            category_id: Some(Id::from("g1")),
            position: Some(1),
            topic: Some("t".into()),
            slow_mode_seconds: Some(1),
            permission_overrides: Some(vec![]),
            last_message_at: Some(date()),
            mute_until: Some(date()),
            receipt_sharing: Some(true),
            message_expiry_seconds: Some(1),
            closed_at: Some(date()),
            active_call_id: Some(Id::from("v1")),
            active_call_account_id: Some(Id::from("a1")),
            metadata: Some(de(json!({"acme.example.com": {"memo": "m"}}))),
            private_metadata: Some(de(json!({"acme.example.com": {"memo": "p"}}))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn message_properties_are_classified() {
        let obj = crate::Message {
            id: Id::from("m1"),
            sender_msg_id: Id::from("x1"),
            sender_id: Id::from("a1"),
            chat_id: Id::from("c1"),
            body: "b".into(),
            body_type: de(json!("text/plain")),
            attachments: vec![],
            mentions: vec![],
            broadcast_mentions: vec![],
            actions: vec![],
            reactions: BTreeMap::new(),
            sent_at: date(),
            received_at: date(),
            delivery_state: de(json!("delivered")),
            reply_to: Some(Id::from("m0")),
            thread_root_id: Some(Id::from("m0")),
            reply_count: Some(1),
            unread_reply_count: Some(1),
            topic_id: Some("t".into()),
            subject: Some("s".into()),
            sender_expires_at: Some(date()),
            burn_on_read: Some(true),
            expires_after_read_seconds: Some(1),
            delivery_receipts: Some(BTreeMap::new()),
            delivered_at: Some(date()),
            read_at: Some(date()),
            read_disposition: Some(de(json!("displayed"))),
            edited_at: Some(date()),
            edit_history: Some(vec![]),
            deleted_at: Some(date()),
            deleted_for_all: Some(true),
            metadata: Some(de(json!({"acme.example.com": {"memo": "m"}}))),
            private_metadata: Some(de(json!({"acme.example.com": {"memo": "p"}}))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn space_properties_are_classified() {
        let obj = crate::Space {
            id: Id::from("s1"),
            name: "n".into(),
            description: Some("d".into()),
            icon_blob_id: Some(Id::from("b1")),
            roles: vec![],
            members: vec![],
            categories: vec![],
            uncategorized_channel_ids: vec![],
            created_at: date(),
            is_public: true,
            is_publicly_previewable: true,
            member_count: 1,
            closed_at: Some(date()),
            metadata: Some(de(json!({"acme.example.com": {"memo": "m"}}))),
            private_metadata: Some(de(json!({"acme.example.com": {"memo": "p"}}))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn chat_contact_properties_are_classified() {
        let obj = crate::ChatContact {
            id: Id::from("k1"),
            identity: "u@example.com".into(),
            login: "u".into(),
            first_seen_at: date(),
            last_seen_at: date(),
            blocked: true,
            display_name: Some("U".into()),
            presence: Some(de(json!("online"))),
            last_active_at: Some(date()),
            status_text: Some("t".into()),
            status_emoji: Some("e".into()),
            endpoints: Some(vec![]),
            did: Some(de(json!("did:web:example.com"))),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn read_position_properties_are_classified() {
        let obj = crate::ReadPosition {
            id: Id::from("r1"),
            chat_id: Id::from("c1"),
            last_read_message_id: Some(Id::from("m1")),
            last_read_at: Some(date()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn custom_emoji_properties_are_classified() {
        let obj = crate::CustomEmoji {
            id: Id::from("e1"),
            name: "n".into(),
            blob_id: Id::from("b1"),
            created_by: Id::from("a1"),
            created_at: date(),
            space_id: Some(Id::from("s1")),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn space_invite_properties_are_classified() {
        let obj = crate::SpaceInvite {
            id: Id::from("i1"),
            code: "c".into(),
            space_id: Id::from("s1"),
            created_by: Id::from("a1"),
            uses: 1,
            created_at: date(),
            default_channel_id: Some(Id::from("c1")),
            expires_at: Some(date()),
            max_uses: Some(1),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn space_ban_properties_are_classified() {
        let obj = crate::SpaceBan {
            id: Id::from("b1"),
            space_id: Id::from("s1"),
            user_id: Id::from("u1"),
            banned_by: Id::from("a1"),
            created_at: date(),
            reason: Some("r".into()),
            expires_at: Some(date()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }

    #[test]
    fn presence_status_properties_are_classified() {
        let obj = crate::PresenceStatus {
            id: Id::from("p1"),
            presence: de(json!("online")),
            receipt_sharing: true,
            updated_at: date(),
            status_text: Some("t".into()),
            status_emoji: Some("e".into()),
            expires_at: Some(date()),
            extra: Map::new(),
        };
        assert_eq!(check_property_classification(&obj), Ok(()));
    }
}
