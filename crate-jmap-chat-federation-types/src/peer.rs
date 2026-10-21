//! draft-atwood-jmap-chat-federation-00 §Server-to-Server Methods — request
//! and response arguments of the `Peer/*` methods.
//!
//! REQUIRED methods: `Peer/deliver`, `Peer/receipt`, `Peer/typing`,
//! `Peer/retract`, `Peer/groupUpdate`. OPTIONAL methods:
//! `Peer/subscribePresence`, `Peer/unsubscribePresence`, `Peer/presence`.
//!
//! Every field that names a user carries the user's identity string, not a
//! ChatContact `id` (draft §User References in Peer Methods). Those fields
//! are `String` here, and [`PeerMention`], [`PeerChatMember`] and
//! [`PeerRoleUpdate`] stand in for the chat types whose `id` is a local
//! [`Id`].
//!
//! Deserializing checks only the wire shape. The cross-field rules are
//! the receiving server's job: exactly one `Peer/deliver` payload, the
//! `Peer/groupUpdate` fields each `action` requires, and the caller
//! identity matching the peer identity.

use jmap_chat_types::{
    Attachment, BroadcastMention, ChatKind, ChatMemberRole, Clearable, MessageAction, Presence,
    ReadDisposition,
};
use jmap_types::{Id, UTCDate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

/// Deserialize a present field into `Some(Clearable<T>)`: JSON `null` is
/// `Clear`, any other value is `Set`. Pair with `#[serde(default)]` so an
/// absent field is `None`.
fn some_clearable<'de, D, T>(d: D) -> Result<Option<Clearable<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Clearable::deserialize(d).map(Some)
}

/// A user's `@mention` in a federated message (draft §Peer/deliver
/// `mentions`): the chat Mention with `id` carrying the user's identity
/// string.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerMention {
    /// The mentioned user's identity string.
    pub id: String,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerMention {
    /// A mention of the user whose identity string is `id`.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            extra: serde_json::Map::new(),
        }
    }
}

/// An attachment of a federated message (draft §Peer/deliver
/// `attachments`): the chat [`Attachment`] fields plus `fetchUrl`.
///
/// `fetch_url` is peer-supplied. Restrict the fetch per draft §Blob Fetch
/// and SSRF before using it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerAttachment {
    /// The chat Attachment fields, including its `extra`.
    #[serde(flatten)]
    pub attachment: Attachment,
    /// The URL from which the receiver fetches the blob.
    pub fetch_url: String,
}

impl PeerAttachment {
    /// `attachment`, fetchable from `fetch_url`.
    pub fn new(attachment: Attachment, fetch_url: impl Into<String>) -> Self {
        Self {
            attachment,
            fetch_url: fetch_url.into(),
        }
    }
}

/// The `message` payload of `Peer/deliver`: a new message.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerMessage {
    /// Sender-assigned ULID; the idempotency key.
    pub sender_msg_id: Id,
    /// Message content.
    pub body: String,
    /// MIME type of `body`.
    pub body_type: String,
    /// Sender's claimed composition time.
    pub sent_at: UTCDate,
    /// Attachments, each with its `fetchUrl`.
    pub attachments: Vec<PeerAttachment>,
    /// Per-user mentions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Vec<PeerMention>>,
    /// Broadcast-scope mentions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcast_mentions: Option<Vec<BroadcastMention>>,
    /// Out-of-band action invitations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<MessageAction>>,
    /// The sending server's `senderMsgId` of the replied-to message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Id>,
    /// The sending server's `senderMsgId` of the thread root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root_id: Option<Id>,
    /// Topic identifier, carried unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    /// Topic name, carried unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Causal hint: the latest `senderMsgId` the sender had stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_sender_msg_id: Option<Id>,
    /// Hard-deletion deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_expires_at: Option<UTCDate>,
    /// Hard-delete immediately after `readAt` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burn_on_read: Option<bool>,
    /// Hard-delete this many seconds after `readAt` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_read_seconds: Option<u64>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerMessage {
    /// A new message from its required fields.
    pub fn new(
        sender_msg_id: Id,
        body: impl Into<String>,
        body_type: impl Into<String>,
        sent_at: UTCDate,
        attachments: Vec<PeerAttachment>,
    ) -> Self {
        Self {
            sender_msg_id,
            body: body.into(),
            body_type: body_type.into(),
            sent_at,
            attachments,
            mentions: None,
            broadcast_mentions: None,
            actions: None,
            reply_to: None,
            thread_root_id: None,
            topic_id: None,
            subject: None,
            last_seen_sender_msg_id: None,
            sender_expires_at: None,
            burn_on_read: None,
            expires_after_read_seconds: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// The `edit` payload of `Peer/deliver`: an edit to an existing message.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerEdit {
    /// `senderMsgId` of the message to edit.
    pub sender_msg_id: Id,
    /// New body content.
    pub body: String,
    /// MIME type of the new body.
    pub body_type: String,
    /// Claimed edit time.
    pub edited_at: UTCDate,
    /// Per-user mentions for the new body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Vec<PeerMention>>,
    /// Broadcast-scope mentions for the new body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcast_mentions: Option<Vec<BroadcastMention>>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerEdit {
    /// An edit from its required fields.
    pub fn new(
        sender_msg_id: Id,
        body: impl Into<String>,
        body_type: impl Into<String>,
        edited_at: UTCDate,
    ) -> Self {
        Self {
            sender_msg_id,
            body: body.into(),
            body_type: body_type.into(),
            edited_at,
            mentions: None,
            broadcast_mentions: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// The `action` of a [`PeerReactionUpdate`].
///
/// A control value: an unrecognised string fails to deserialize.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReactionAction {
    /// Add the reaction.
    Add,
    /// Remove the reaction.
    Remove,
}

/// The `reactionUpdate` payload of `Peer/deliver`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerReactionUpdate {
    /// `senderMsgId` of the target message.
    pub sender_msg_id: Id,
    /// Sender-assigned ULID of this reaction; the `reactions` map key.
    pub sender_reaction_id: Id,
    /// Non-empty string identifying the reaction.
    pub emoji: String,
    /// Space-scoped custom emoji id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_emoji_id: Option<Id>,
    /// Add or remove.
    pub action: ReactionAction,
    /// Time of the reaction event.
    pub sent_at: UTCDate,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerReactionUpdate {
    /// A reaction update from its required fields.
    pub fn new(
        sender_msg_id: Id,
        sender_reaction_id: Id,
        emoji: impl Into<String>,
        action: ReactionAction,
        sent_at: UTCDate,
    ) -> Self {
        Self {
            sender_msg_id,
            sender_reaction_id,
            emoji: emoji.into(),
            custom_emoji_id: None,
            action,
            sent_at,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/deliver` request arguments.
///
/// The draft requires exactly one of `message`, `edit` and
/// `reactionUpdate`. The constructors build exactly one; deserializing
/// does not check it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDeliverArgs {
    /// Account ID on the receiving server.
    pub account_id: Id,
    /// The Chat ID.
    pub chat_id: Id,
    /// The sender's identity string.
    pub sender_user_id: String,
    /// Selects the chatId verification procedure.
    pub chat_kind: ChatKind,
    /// A new message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<PeerMessage>,
    /// An edit to an existing message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<PeerEdit>,
    /// A reaction change on an existing message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction_update: Option<PeerReactionUpdate>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerDeliverArgs {
    fn base(account_id: Id, chat_id: Id, sender_user_id: String, chat_kind: ChatKind) -> Self {
        Self {
            account_id,
            chat_id,
            sender_user_id,
            chat_kind,
            message: None,
            edit: None,
            reaction_update: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Deliver a new message.
    pub fn new_message(
        account_id: Id,
        chat_id: Id,
        sender_user_id: impl Into<String>,
        chat_kind: ChatKind,
        message: PeerMessage,
    ) -> Self {
        Self {
            message: Some(message),
            ..Self::base(account_id, chat_id, sender_user_id.into(), chat_kind)
        }
    }

    /// Deliver an edit.
    pub fn new_edit(
        account_id: Id,
        chat_id: Id,
        sender_user_id: impl Into<String>,
        chat_kind: ChatKind,
        edit: PeerEdit,
    ) -> Self {
        Self {
            edit: Some(edit),
            ..Self::base(account_id, chat_id, sender_user_id.into(), chat_kind)
        }
    }

    /// Deliver a reaction update.
    pub fn new_reaction_update(
        account_id: Id,
        chat_id: Id,
        sender_user_id: impl Into<String>,
        chat_kind: ChatKind,
        reaction_update: PeerReactionUpdate,
    ) -> Self {
        Self {
            reaction_update: Some(reaction_update),
            ..Self::base(account_id, chat_id, sender_user_id.into(), chat_kind)
        }
    }
}

/// `Peer/deliver` response.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDeliverResponse {
    /// The account ID from the request.
    pub account_id: Id,
    /// The stored message's local id (new message), or the affected
    /// message's local id (edit or reaction update).
    pub received_msg_id: Id,
    /// When the receiver stored the message or applied the change.
    pub received_at: UTCDate,
    /// The canonical chatId, present only after a direct-chat
    /// simultaneous-initiation race was resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_chat_id: Option<Id>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerDeliverResponse {
    /// A response from its required fields.
    pub fn new(account_id: Id, received_msg_id: Id, received_at: UTCDate) -> Self {
        Self {
            account_id,
            received_msg_id,
            received_at,
            canonical_chat_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/receipt` request arguments.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerReceiptArgs {
    /// Account ID on the receiving server.
    pub account_id: Id,
    /// The sender-assigned ULID of the acknowledged message.
    pub sender_msg_id: Id,
    /// When the recipient's mailbox stored the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<UTCDate>,
    /// When the recipient's device received the notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_delivered_at: Option<UTCDate>,
    /// When the recipient read the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<UTCDate>,
    /// Why the message was acknowledged. A receiver stores `"displayed"`
    /// when `readAt` is present and this is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_disposition: Option<ReadDisposition>,
    /// The acknowledging user's identity string.
    pub reader_user_id: String,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerReceiptArgs {
    /// A receipt carrying no timestamps yet.
    pub fn new(account_id: Id, sender_msg_id: Id, reader_user_id: impl Into<String>) -> Self {
        Self {
            account_id,
            sender_msg_id,
            delivered_at: None,
            device_delivered_at: None,
            read_at: None,
            read_disposition: None,
            reader_user_id: reader_user_id.into(),
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/typing` request arguments.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerTypingArgs {
    /// Account ID on the receiving server.
    pub account_id: Id,
    /// The Chat in which typing is occurring.
    pub chat_id: Id,
    /// The typing user's identity string.
    pub sender_user_id: String,
    /// `true` while typing, `false` once typing stopped.
    pub typing: bool,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerTypingArgs {
    /// A typing notification.
    pub fn new(
        account_id: Id,
        chat_id: Id,
        sender_user_id: impl Into<String>,
        typing: bool,
    ) -> Self {
        Self {
            account_id,
            chat_id,
            sender_user_id: sender_user_id.into(),
            typing,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/retract` request arguments.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerRetractArgs {
    /// Account ID on the receiving server.
    pub account_id: Id,
    /// The Chat containing the message.
    pub chat_id: Id,
    /// The retracting user's identity string.
    pub sender_user_id: String,
    /// The sender-assigned ULID of the message to retract.
    pub sender_msg_id: Id,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerRetractArgs {
    /// A retraction of `sender_msg_id` in `chat_id`.
    pub fn new(
        account_id: Id,
        chat_id: Id,
        sender_user_id: impl Into<String>,
        sender_msg_id: Id,
    ) -> Self {
        Self {
            account_id,
            chat_id,
            sender_user_id: sender_user_id.into(),
            sender_msg_id,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/retract` response.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerRetractResponse {
    /// The account ID from the request.
    pub account_id: Id,
    /// When the retraction was applied.
    pub retracted_at: UTCDate,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerRetractResponse {
    /// A response for a retraction applied at `retracted_at`.
    pub fn new(account_id: Id, retracted_at: UTCDate) -> Self {
        Self {
            account_id,
            retracted_at,
            extra: serde_json::Map::new(),
        }
    }
}

/// The `action` of `Peer/groupUpdate`.
///
/// A control value: an unrecognised string fails to deserialize.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupUpdateAction {
    /// Initial group creation; requires `members`.
    Create,
    /// Members were added; requires `addedMembers`.
    AddMembers,
    /// Members were removed; requires `removedMemberIds`.
    RemoveMembers,
    /// Members' roles changed; requires `updatedRoles`.
    UpdateRoles,
    /// `name`, `description` or `avatarBlobId` changed.
    UpdateMetadata,
    /// The sender left the group.
    Leave,
    /// The sender, the last admin, left and closed the group.
    Close,
}

/// A group member in `Peer/groupUpdate` `members` and `addedMembers`: the
/// chat ChatMember with `id` and `invitedBy` carrying identity strings.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerChatMember {
    /// The member's identity string.
    pub id: String,
    /// The member's role.
    pub role: ChatMemberRole,
    /// When the member joined.
    pub joined_at: UTCDate,
    /// The inviting user's identity string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invited_by: Option<String>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerChatMember {
    /// A member from its required fields.
    pub fn new(id: impl Into<String>, role: ChatMemberRole, joined_at: UTCDate) -> Self {
        Self {
            id: id.into(),
            role,
            joined_at,
            invited_by: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// An `updatedRoles` entry of `Peer/groupUpdate`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerRoleUpdate {
    /// The member's identity string.
    pub id: String,
    /// The new role: `"admin"` or `"member"`.
    pub role: ChatMemberRole,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerRoleUpdate {
    /// Give the member whose identity string is `id` the role `role`.
    pub fn new(id: impl Into<String>, role: ChatMemberRole) -> Self {
        Self {
            id: id.into(),
            role,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/groupUpdate` request arguments.
///
/// Which optional fields are required depends on `action` (see
/// [`GroupUpdateAction`]); deserializing does not check it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerGroupUpdateArgs {
    /// Account ID on the receiving server.
    pub account_id: Id,
    /// The group chat ULID.
    pub chat_id: Id,
    /// The changing user's identity string.
    pub sender_user_id: String,
    /// What changed.
    pub action: GroupUpdateAction,
    /// Full membership list; required for `"create"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<PeerChatMember>>,
    /// Newly added members; required for `"addMembers"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_members: Option<Vec<PeerChatMember>>,
    /// Identity strings of removed members; required for
    /// `"removeMembers"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_member_ids: Option<Vec<String>>,
    /// Role changes; required for `"updateRoles"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_roles: Option<Vec<PeerRoleUpdate>>,
    /// Group display name (`"create"` or `"updateMetadata"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Group description (`"create"` or `"updateMetadata"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Group avatar blob ID (`"create"` or `"updateMetadata"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_blob_id: Option<Id>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerGroupUpdateArgs {
    /// A group update with no optional fields set.
    pub fn new(
        account_id: Id,
        chat_id: Id,
        sender_user_id: impl Into<String>,
        action: GroupUpdateAction,
    ) -> Self {
        Self {
            account_id,
            chat_id,
            sender_user_id: sender_user_id.into(),
            action,
            members: None,
            added_members: None,
            removed_member_ids: None,
            updated_roles: None,
            name: None,
            description: None,
            avatar_blob_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/subscribePresence` request arguments.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerSubscribePresenceArgs {
    /// Account ID whose owner's presence is subscribed to.
    pub account_id: Id,
    /// The calling server's `ownerUserId`.
    pub subscriber_id: String,
    /// Requested subscription lifetime in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u64>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerSubscribePresenceArgs {
    /// A subscription request with the server's default TTL.
    pub fn new(account_id: Id, subscriber_id: impl Into<String>) -> Self {
        Self {
            account_id,
            subscriber_id: subscriber_id.into(),
            ttl: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/subscribePresence` response.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerSubscribePresenceResponse {
    /// The account ID from the request.
    pub account_id: Id,
    /// The owner's current presence state.
    pub presence: Presence,
    /// The owner's `lastActiveAt`, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<UTCDate>,
    /// Current custom status text, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    /// Current status emoji, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_emoji: Option<String>,
    /// Granted subscription lifetime in seconds.
    pub ttl: u64,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerSubscribePresenceResponse {
    /// A response from its required fields.
    pub fn new(account_id: Id, presence: Presence, ttl: u64) -> Self {
        Self {
            account_id,
            presence,
            last_active_at: None,
            status_text: None,
            status_emoji: None,
            ttl,
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/unsubscribePresence` request arguments.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerUnsubscribePresenceArgs {
    /// Account ID on the remote server.
    pub account_id: Id,
    /// The calling server's `ownerUserId`.
    pub subscriber_id: String,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerUnsubscribePresenceArgs {
    /// Cancel `subscriber_id`'s subscription.
    pub fn new(account_id: Id, subscriber_id: impl Into<String>) -> Self {
        Self {
            account_id,
            subscriber_id: subscriber_id.into(),
            extra: serde_json::Map::new(),
        }
    }
}

/// `Peer/presence` request arguments.
///
/// A sender reports an `"invisible"` owner as `"offline"`; the draft
/// never sends `"invisible"` to a peer.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerPresenceArgs {
    /// Account ID on the receiving server.
    pub account_id: Id,
    /// The pushing server's `ownerUserId`.
    pub identity: String,
    /// The remote owner's presence state.
    pub presence: Presence,
    /// Updated `lastActiveAt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<UTCDate>,
    /// `None` leaves the status text unchanged; `Clear` (`null`) clears
    /// it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub status_text: Option<Clearable<String>>,
    /// `None` leaves the status emoji unchanged; `Clear` (`null`) clears
    /// it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub status_emoji: Option<Clearable<String>>,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerPresenceArgs {
    /// A presence update that leaves the status fields unchanged.
    pub fn new(account_id: Id, identity: impl Into<String>, presence: Presence) -> Self {
        Self {
            account_id,
            identity: identity.into(),
            presence,
            last_active_at: None,
            status_text: None,
            status_emoji: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// The response of `Peer/receipt`, `Peer/typing`, `Peer/groupUpdate`,
/// `Peer/unsubscribePresence` and `Peer/presence`: the request's
/// `accountId` alone.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerAck {
    /// The account ID from the request.
    pub account_id: Id,
    /// Catch-all for extension fields; see workspace AGENTS.md.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PeerAck {
    /// An acknowledgement for `account_id`.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            extra: serde_json::Map::new(),
        }
    }
}

/// Whether `uri` is a JMAP object URI, which a receiving server removes
/// from peer data.
///
/// Spec: draft-atwood-jmap-chat-federation-00 §User References in Peer
/// Methods: "A receiving server removes every MessageAction and every
/// Endpoint whose `uri` is a JMAP object URI (a URI with the scheme
/// `jmap`) from data it receives from a peer". Only the scheme is tested,
/// so a malformed `jmap:` URI is removed too; the scheme name is
/// case-insensitive (draft-atwood-jmap-object-uri-00 §Equivalence).
///
/// # Example
///
/// ```
/// use jmap_chat_federation_types::is_jmap_object_uri;
/// assert!(is_jmap_object_uri("jmap:task:A1:T9"));
/// assert!(!is_jmap_object_uri("https://tracker.example/T-1"));
/// ```
pub fn is_jmap_object_uri(uri: &str) -> bool {
    uri.get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("jmap:"))
}
