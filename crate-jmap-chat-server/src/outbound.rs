//! Outbound federation events reported to
//! [`ChatBackend::on_outbound_event`](crate::ChatBackend::on_outbound_event).
//!
//! draft-atwood-jmap-chat-00 requires the server to notify other
//! participants' servers when the owner retracts a Message, edits one, or
//! changes a group Chat. The wire methods (`Peer/retract`,
//! `Peer/deliver` `edit`, `Peer/groupUpdate`) are defined by
//! draft-atwood-jmap-chat-federation-00 and implemented outside this
//! crate. These events carry local ids (ChatContact ids, Message ids);
//! the federation layer maps them to identity strings and sends them.
//!
//! An event carries only what the stored records cannot tell the backend
//! afterwards: which members a change added, removed or re-roled, and who
//! must be told. Everything else (`body`, `name`, the full member list)
//! is read from the stored Message or Chat.

use jmap_chat_types::ChatMember;
use jmap_types::Id;

/// One outbound event the `Message/set` or `Chat/set` handler reports.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum OutboundEvent {
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
