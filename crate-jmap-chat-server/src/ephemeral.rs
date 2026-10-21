//! Ephemeral-stream subscription state (draft-atwood-jmap-chat-wss-00
//! §ChatStreamEnable).
//!
//! A WebSocket transport keeps one [`EphemeralSubscription`] per
//! connection, starting from [`EphemeralSubscription::default`] (nothing
//! subscribed). Each `ChatStreamEnable` frame goes through
//! [`EphemeralSubscription::replace_with`], which validates the frame
//! against the server's [`CompanionStreams`], normalizes it, and replaces
//! the whole subscription. A `ChatStreamDisable` frame resets it to the
//! default.
//!
//! Before sending a `ChatTypingEvent` to an owner's connections, the
//! transport asks [`should_deliver_typing`] whether that owner may receive
//! it. Before sending a `ChatPresenceEvent`, it asks [`project_presence`]
//! for the frame that owner may see, if any.
//!
//! # Rate limits are the deployment's job
//!
//! This crate does not rate-limit ephemeral events, and neither predicate
//! counts or times anything. The draft gives the outbound limits as SHOULD
//! rules, and the transport or deployment applies them:
//!
//! - Typing: "no more than one `ChatTypingEvent` per sender per chat per 3
//!   seconds SHOULD be delivered; events above this rate MAY be silently
//!   dropped" (draft-atwood-jmap-chat-wss-00 §ChatTypingEvent).
//! - Presence: "no more than one `ChatPresenceEvent` per contact per 30
//!   seconds SHOULD be delivered; events above this rate MAY be silently
//!   dropped" (draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent).

use jmap_chat_types::{
    Chat, ChatKind, ChatPresenceEvent, ChatStreamEnable, ChatTypingEvent, Presence,
};
use jmap_types::{Id, JmapError};

use crate::push::owner_is_member;
use crate::ChatBackend;

/// Which ids a subscribed data type covers.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdScope {
    /// Every id: the frame sent `null` or left the field out.
    All,
    /// Only the listed ids, in the order the frame sent them.
    Only(Vec<Id>),
}

impl From<Option<&Vec<Id>>> for IdScope {
    /// `None` (absent or JSON `null`) maps to [`IdScope::All`]: "An absent
    /// `chatIds` or `contactIds` is equivalent to `null`"
    /// (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable).
    fn from(ids: Option<&Vec<Id>>) -> Self {
        ids.map_or(Self::All, |ids| Self::Only(ids.clone()))
    }
}

/// The companion WebSocket capabilities the server advertises, which
/// decide whether the companion `dataTypes` values `"vtc"` and `"scene"`
/// are recognized in a `ChatStreamEnable` frame.
///
/// The default advertises neither. Per draft-atwood-jmap-chat-wss-00
/// §Enabling Ephemeral Push (L175), `"vtc"` is "recognized only when the
/// server advertises `urn:ietf:params:jmap:vtc:websocket`", and `"scene"`
/// "only when the server advertises `urn:ietf:params:jmap:scene:websocket`".
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompanionStreams {
    /// The server advertises `urn:ietf:params:jmap:vtc:websocket`.
    pub vtc: bool,
    /// The server advertises `urn:ietf:params:jmap:scene:websocket`.
    pub scene: bool,
}

/// The ephemeral subscription of one WebSocket connection, normalized
/// from a `ChatStreamEnable` frame.
///
/// The default value subscribes to nothing, which is the state before the
/// first `ChatStreamEnable` and after a `ChatStreamDisable`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EphemeralSubscription {
    typing: Option<IdScope>,
    presence: Option<IdScope>,
    vtc: bool,
    scene: bool,
}

impl EphemeralSubscription {
    /// Validates and normalizes a `ChatStreamEnable` frame.
    ///
    /// `"typing"` and `"presence"` in `dataTypes` always take effect.
    /// `"vtc"` and `"scene"` take effect only when `companions` says the
    /// server advertises their capability; otherwise they are unrecognized
    /// (draft-atwood-jmap-chat-wss-00 §Enabling Ephemeral Push, L196: the
    /// value "is treated as unrecognized and silently ignored"). Other
    /// values are ignored. `chatIds` applies only when `"typing"` is
    /// present and `contactIds` only when `"presence"` is present.
    ///
    /// # Errors
    ///
    /// Returns an `invalidArguments` [`JmapError`] when `dataTypes` holds
    /// no recognized value: "none of `"typing"`, `"presence"`, or a
    /// companion value whose capability the server advertises"
    /// (draft-atwood-jmap-chat-wss-00 §Enabling Ephemeral Push, L194). The
    /// transport sends it to the client as an RFC 8887 §4.3.4
    /// `RequestError` frame whose `type` is the full URN
    /// `"urn:ietf:params:jmap:error:invalidArguments"`; the returned
    /// [`JmapError`] carries the short method-level name.
    pub fn from_enable(
        enable: &ChatStreamEnable,
        companions: CompanionStreams,
    ) -> Result<Self, JmapError> {
        let has = |tag: &str| enable.data_types.iter().any(|t| t == tag);
        let subscription = Self {
            typing: has("typing").then(|| IdScope::from(enable.chat_ids.as_ref())),
            presence: has("presence").then(|| IdScope::from(enable.contact_ids.as_ref())),
            vtc: companions.vtc && has("vtc"),
            scene: companions.scene && has("scene"),
        };
        if subscription == Self::default() {
            return Err(JmapError::invalid_arguments(
                "dataTypes must contain \"typing\", \"presence\" or an advertised companion value",
            ));
        }
        Ok(subscription)
    }

    /// Replaces this subscription with the one in `enable`.
    ///
    /// The new frame replaces the old subscription in full; nothing is
    /// merged from it (draft-atwood-jmap-chat-wss-00 §ChatStreamEnable).
    ///
    /// # Errors
    ///
    /// Returns the same error as [`Self::from_enable`]. On error `self` is
    /// unchanged, as the draft requires.
    pub fn replace_with(
        &mut self,
        enable: &ChatStreamEnable,
        companions: CompanionStreams,
    ) -> Result<(), JmapError> {
        *self = Self::from_enable(enable, companions)?;
        Ok(())
    }

    /// The Chats covered for typing events, or `None` when typing is not
    /// subscribed. [`IdScope::All`] means every Chat of which the owner is
    /// a current member.
    pub fn typing(&self) -> Option<&IdScope> {
        self.typing.as_ref()
    }

    /// The ChatContacts covered for presence events, or `None` when
    /// presence is not subscribed. [`IdScope::All`] means every known
    /// ChatContact.
    pub fn presence(&self) -> Option<&IdScope> {
        self.presence.as_ref()
    }

    /// Whether `"vtc"` is subscribed: it was in `dataTypes` and the server
    /// advertises `urn:ietf:params:jmap:vtc:websocket`. This is the
    /// `chat_vtc` flag that `jmap-chat-vtc-server` takes.
    pub fn vtc(&self) -> bool {
        self.vtc
    }

    /// Whether `"scene"` is subscribed: it was in `dataTypes` and the
    /// server advertises `urn:ietf:params:jmap:scene:websocket`. This is
    /// the `chat_scene` flag that `jmap-chat-scene-server` takes.
    pub fn scene(&self) -> bool {
        self.scene
    }
}

/// Whether `event` may be delivered to the owner of `account_id`.
///
/// `account_id` is the recipient. `event.chat_id` names the recipient's
/// Chat record, and `event.sender_id` is the typing user as the recipient
/// sees it: a ChatContact id on the recipient's contact list. The
/// transport calls this once per recipient, at delivery time, and sends
/// the event to none of the recipient's connections when it returns
/// `false`. The sender is never told. The connection's `chatIds` scope
/// ([`EphemeralSubscription::typing`]) is a separate filter, checked by
/// the transport on its own.
///
/// Delivery is allowed only when all of these hold:
///
/// - `event.account_id` is `account_id` (draft-atwood-jmap-chat-wss-00
///   §ChatTypingEvent: `accountId` is "The id of the account that
///   contains the Chat").
/// - The sender is not the recipient (draft-atwood-jmap-chat-00
///   §Chat/typing: delivery goes to "each local participant in the Chat
///   other than the requesting account").
/// - The Chat exists in the recipient's account and the recipient is
///   still a member of it (draft-atwood-jmap-chat-wss-00
///   §ChatTypingEvent: "Servers MUST verify at delivery time that the
///   owner remains a current member of the identified Chat").
/// - For a direct or group Chat, its `receiveTypingIndicators` is `true`.
///   Other kinds ignore it.
/// - The sender's ChatContact is not blocked on the recipient's contact
///   list, per [`ChatBackend::is_contact_blocked`].
///
/// # Errors
///
/// Returns the backend's error if reading the Chat, its Space or the
/// sender's blocked status fails.
pub async fn should_deliver_typing<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    event: &ChatTypingEvent,
) -> Result<bool, B::Error> {
    if event.account_id != *account_id || event.sender_id == *account_id {
        return Ok(false);
    }
    let (chats, _) = backend
        .get_objects::<Chat>(
            caller,
            account_id,
            Some(std::slice::from_ref(&event.chat_id)),
            None,
        )
        .await?;
    let Some(chat) = chats.into_iter().next() else {
        return Ok(false);
    };
    if !owner_is_member(backend, caller, account_id, &chat).await? {
        return Ok(false);
    }
    if matches!(chat.kind, ChatKind::Direct | ChatKind::Group) && !chat.receive_typing_indicators {
        return Ok(false);
    }
    Ok(!backend
        .is_contact_blocked(caller, account_id, &event.sender_id)
        .await?)
}

/// The `ChatPresenceEvent` the owner of `account_id` may receive for
/// `event`, or `None` if it must be dropped.
///
/// `account_id` is the recipient and `event.contact_id` is the contact
/// whose presence changed, as a ChatContact id on the recipient's contact
/// list. The transport calls this once per recipient, at delivery time,
/// and sends the returned frame, or nothing, to that owner's connections.
/// The contact is never told. The connection's `contactIds` scope
/// ([`EphemeralSubscription::presence`]) is a separate filter, checked by
/// the transport on its own.
///
/// - If `event.account_id` is not `account_id`, the result is `None`
///   (same section: `accountId` is "The id of the account that contains
///   the ChatContact").
/// - If the contact is blocked on the recipient's contact list, per
///   [`ChatBackend::is_contact_blocked`], the result is `None`
///   (draft-atwood-jmap-chat-wss-00 §ChatPresenceEvent: "Servers MUST NOT
///   deliver `ChatPresenceEvent` for a contact whose `blocked` field is
///   `true` on the owner's ChatContact record for that contact").
/// - Otherwise the result is `event` with an `"invisible"` presence
///   replaced by `"offline"` (same section: "The `"invisible"` value MUST
///   NOT appear in events delivered to other users"). Every other field
///   is unchanged.
///
/// # Errors
///
/// Returns the backend's error if reading the contact's blocked status
/// fails.
pub async fn project_presence<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    event: &ChatPresenceEvent,
) -> Result<Option<ChatPresenceEvent>, B::Error> {
    if event.account_id != *account_id {
        return Ok(None);
    }
    if backend
        .is_contact_blocked(caller, account_id, &event.contact_id)
        .await?
    {
        return Ok(None);
    }
    let mut projected = event.clone();
    if projected.presence == Presence::Invisible {
        projected.presence = Presence::Offline;
    }
    Ok(Some(projected))
}
