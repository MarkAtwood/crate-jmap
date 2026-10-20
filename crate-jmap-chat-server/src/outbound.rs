//! Outbound federation events reported to
//! [`ChatBackend::on_outbound_event`](crate::ChatBackend::on_outbound_event).
//!
//! draft-atwood-jmap-chat-00 requires the server to notify other
//! participants' servers when the owner sends a Message, retracts or edits
//! one, adds or removes a reaction, reads a received Message, types, or
//! changes a group Chat or their presence. The wire methods (`Peer/deliver`
//! `message`, `edit` and `reactionUpdate`, `Peer/retract`,
//! `Peer/groupUpdate`, `Peer/receipt`, `Peer/typing`, `Peer/presence`) are
//! defined by
//! draft-atwood-jmap-chat-federation-00 and implemented outside this
//! crate. These events carry local ids (ChatContact ids, Message ids);
//! the federation layer maps them to identity strings and sends them.
//!
//! An event carries only what the stored records cannot tell the backend
//! afterwards: which members a change added, removed or re-roled, and who
//! must be told. Everything else (`body`, `name`, the full member list)
//! is read from the stored Message or Chat.

use jmap_chat_types::{ChatMember, Reaction, ReadDisposition};
use jmap_types::{Id, UTCDate};

/// One outbound event the `Message/set`, `Chat/set`, `Chat/typing` or
/// `PresenceStatus/set` handler reports.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum OutboundEvent {
    /// The owner created a Message. draft-atwood-jmap-chat-00
    /// §Message/set: the server sets the server-assigned fields "then
    /// enqueues the message for outbound delivery" (`Peer/deliver`
    /// `message`). Reported after the Message is written; the payload
    /// fields are read from the stored Message.
    NewMessage {
        /// The Chat that holds the Message (`Peer/deliver` `chatId`).
        chat_id: Id,
        /// The Message's local id.
        message_id: Id,
        /// The Message's `senderMsgId` (`message.senderMsgId`).
        sender_msg_id: Id,
    },
    /// The owner set `deletedForAll: true` on a Message they sent.
    /// draft-atwood-jmap-chat-federation-00 §Peer/retract "Sender
    /// Behavior": "the server MUST send `Peer/retract` to the server of
    /// every other participant in the Chat before marking the local
    /// record". The handler reports it before the update is written, so
    /// the stored Message still holds its content.
    Retract {
        /// The Chat that holds the Message (`Peer/retract` `chatId`).
        chat_id: Id,
        /// The Message's local id.
        message_id: Id,
        /// The Message's `senderMsgId` (`Peer/retract` `senderMsgId`).
        sender_msg_id: Id,
    },
    /// The owner edited a Message's `body`, `bodyType`, `mentions` or
    /// `broadcastMentions`. draft-atwood-jmap-chat-00 §Editing a Message:
    /// "Send `Peer/deliver` carrying an `edit` payload to all recipients".
    /// Reported after the edit is written; the payload fields are read
    /// from the stored Message.
    Edit {
        /// The Chat that holds the Message (`Peer/deliver` `chatId`).
        chat_id: Id,
        /// The Message's local id.
        message_id: Id,
        /// The Message's `senderMsgId` (`edit.senderMsgId`).
        sender_msg_id: Id,
    },
    /// The owner added or removed one of their reactions on a Message.
    /// draft-atwood-jmap-chat-00 §Adding and Removing Reactions: the
    /// server "MUST propagate the reaction to all recipients via
    /// `Peer/deliver` `reactionUpdate` payload, carrying the
    /// `senderReactionId` as the map key", and a removal "using the same
    /// `senderReactionId`". Reported after the update is written, one
    /// event per `reactions/<senderReactionId>` patch key.
    ReactionUpdate {
        /// The Chat that holds the Message (`Peer/deliver` `chatId`).
        chat_id: Id,
        /// The Message's local id.
        message_id: Id,
        /// The Message's `senderMsgId` (`reactionUpdate.senderMsgId`).
        sender_msg_id: Id,
        /// The `reactions` map key (`reactionUpdate.senderReactionId`).
        sender_reaction_id: String,
        /// `reactionUpdate.action`.
        action: ReactionAction,
    },
    /// A group Chat was created or changed (`Peer/groupUpdate`), reported
    /// after the change is written.
    GroupUpdate {
        /// The group Chat (`Peer/groupUpdate` `chatId`).
        chat_id: Id,
        /// What changed (`Peer/groupUpdate` `action`).
        action: GroupUpdateAction,
        /// ChatContact ids of the members to notify, per
        /// draft-atwood-jmap-chat-00 §Chat/set (see each
        /// [`GroupUpdateAction`] variant). Never contains the accountId.
        recipients: Vec<Id>,
    },
    /// The owner set a non-null `readAt` on a Message another participant
    /// sent. draft-atwood-jmap-chat-federation-00 §Peer/receipt: `readAt`
    /// "SHOULD be present when the recipient's owner has read the
    /// message". Reported after the update is written, and only when the
    /// owner's effective receipt-sharing preference is `true`: "If the
    /// effective preference is `false`, the server MUST NOT invoke
    /// `Peer/receipt`" (§Peer/receipt "Sender Behavior"). The read fields
    /// are carried here because a `burnOnRead` Message is gone once read.
    Receipt {
        /// The Chat that holds the Message.
        chat_id: Id,
        /// The Message's local id. Under `burnOnRead` the Message has been
        /// hard-deleted by the time the event is reported.
        message_id: Id,
        /// The Message's `senderMsgId` (`Peer/receipt` `senderMsgId`).
        sender_msg_id: Id,
        /// `Peer/receipt` `readAt`.
        read_at: UTCDate,
        /// `Peer/receipt` `readDisposition`, as stored: `"displayed"` when
        /// the patch set `readAt` without it.
        read_disposition: Option<ReadDisposition>,
    },
    /// The owner called `Chat/typing`. draft-atwood-jmap-chat-00
    /// §Chat/typing: the server SHOULD deliver a `typing` push event to
    /// each local participant other than the owner and, "For federated
    /// Chats, the server SHOULD invoke `Peer/typing`" on each remote
    /// participant's home server. Reported after the request is validated;
    /// the event is never stored. The per-recipient checks belong to the
    /// receiving side (see [`should_deliver_typing`](crate::should_deliver_typing)).
    Typing {
        /// The Chat (`Peer/typing` `chatId`).
        chat_id: Id,
        /// `Peer/typing` `typing`.
        typing: bool,
    },
    /// The owner changed `presence`, `statusText` or `statusEmoji`.
    /// draft-atwood-jmap-chat-federation-00 §Peer/subscribePresence: the
    /// server "MAY push subsequent `Peer/presence` calls to the subscriber
    /// whenever the owner's presence, `statusText`, or `statusEmoji`
    /// changes". Reported after a `PresenceStatus/set` update that names
    /// any of the three is written; the payload is read from the stored
    /// PresenceStatus, and the subscribers are the backend's.
    Presence,
}

/// The `action` of a [`OutboundEvent::ReactionUpdate`]
/// (draft-atwood-jmap-chat-federation-00 §Peer/deliver `reactionUpdate`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ReactionAction {
    /// `"add"`. The reaction (`emoji`, `customEmojiId`, `sentAt`) is read
    /// from the stored Message's `reactions` map under the
    /// `senderReactionId`. Also reported when the owner replaces one of
    /// their reactions under the same key.
    Add,
    /// `"remove"`, with the reaction as it was stored before the removal,
    /// since the stored Message no longer holds it. Reported only when the
    /// key named a stored reaction.
    Remove {
        /// The removed reaction.
        removed: Reaction,
    },
}

/// The `action` of a [`OutboundEvent::GroupUpdate`]
/// (draft-atwood-jmap-chat-federation-00 §Peer/groupUpdate).
///
/// One `Chat/set` update can report several actions. They come in this
/// order: `UpdateMetadata`, `AddMembers`, `RemoveMembers`, `UpdateRoles`,
/// then `Leave` or `Close`, so the owner's departure is last.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum GroupUpdateAction {
    /// `"create"`. draft-atwood-jmap-chat-00 §Creating a Chat: the server
    /// "MUST send `Peer/groupUpdate` to each initial member". Recipients:
    /// the initial members.
    Create,
    /// `"addMembers"`, with the added members as stored. §Chat/set
    /// `addMembers`: "the server MUST send `Peer/groupUpdate` to all
    /// current members". Recipients: every member after the change.
    AddMembers {
        /// `addedMembers`.
        added: Vec<ChatMember>,
    },
    /// `"removeMembers"`. §Chat/set `removeMembers`: "The server MUST
    /// send `Peer/groupUpdate` to all remaining members and to the
    /// removed members." Recipients: both.
    RemoveMembers {
        /// `removedMemberIds`.
        removed_ids: Vec<Id>,
    },
    /// `"updateRoles"`, with the members whose role changed, as stored.
    /// §Chat/set `updateMemberRoles`: "The server MUST send
    /// `Peer/groupUpdate` to all members." Recipients: every member.
    UpdateRoles {
        /// `updatedRoles` (each member's `id` and new `role`).
        updated: Vec<ChatMember>,
    },
    /// `"updateMetadata"`: `name`, `description` or `avatarBlobId`
    /// changed. Recipients: every member.
    UpdateMetadata,
    /// `"leave"`: the owner left. §Chat/set `removeMembers`: the server
    /// "MUST notify the other members with a `Peer/groupUpdate` of action
    /// `"leave"`". Recipients: the remaining members.
    Leave,
    /// `"close"`: the owner, the last admin, left while members remain,
    /// which closed the group. The server "MUST notify the other members
    /// with a `Peer/groupUpdate` of action `"close"` in place of
    /// `"leave"`". Recipients: the remaining members.
    Close,
}
