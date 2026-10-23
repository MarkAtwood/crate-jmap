//! Chat binding for the JMAP VTC method handlers: the rules of
//! draft-atwood-jmap-vtc-00 and draft-atwood-jmap-chat-00 that apply when
//! a server offers both `urn:ietf:params:jmap:vtc` and
//! `urn:ietf:params:jmap:chat`.
//!
//! `jmap-vtc-server` and `jmap-chat-server` do not depend on each other.
//! A deployment that offers both implements
//! [`jmap_vtc_server::VtcBackend`] and [`jmap_chat_server::ChatBackend`]
//! and calls these functions from its trait methods:
//!
//! | Backend method | Call |
//! |---|---|
//! | `VtcBackend::create_call`, before the insert | [`check_call_create`] |
//! | `VtcBackend::create_call`, `update_object::<VtcCall>` and `update_object_if::<VtcCall>`, after the write | [`sync_active_call_id`] |
//! | `VtcBackend::is_user_blocked` | [`is_blocked_by_contact`] |
//! | `JmapBackend::get_objects`, `query_objects` and `query_changes`, per VTCCall (`VtcBackend` §Per-call access) | [`may_access_call`] |
//! | `ChatBackend::on_chat_destroyed` | [`clear_channel_references`] |
//! | `ChatBackend::on_space_destroyed` | [`clear_space_references`] |
//! | `ChatBackend::on_space_ban_created` | [`contact_identity`], then [`eject_banned_user`] |
//! | WebSocket transport, before sending a VTC event | [`should_deliver_vtc`] |
//!
//! Each function takes only the backend it reads or writes, so the two
//! backends may have different caller contexts and error types.

#![forbid(unsafe_code)]

use std::collections::HashSet;

use jmap_chat_server::ChatBackend;
use jmap_chat_server::resolve_channel_permissions;
use jmap_chat_types::{Chat, ChatContact, ChatKind, ChatPermission, Space};
use jmap_server::{
    BackendSetError, GetObject, JmapBackend, SetError, SetErrorType, now_utc_string,
};
use jmap_types::{Id, PatchObject};
use jmap_vtc_server::{VtcBackend, VtcEphemeralSubscription, after_leave};
use jmap_vtc_types::{
    CallRole, CallState, ParticipantEventKind, VtcCall, VtcEvent, VtcParticipant,
    VtcParticipantEvent, VtcStreamEnable,
};
use serde_json::{Value, json};

/// Check the Chat rules of draft-atwood-jmap-vtc-00 §Creating a Ring Call
/// and §Creating a Room Call for a `VTCCall/set` create.
///
/// - `chatId` must name a Chat in `account_id` that
///   [`ChatBackend::may_view_chat`] allows, and the owner must be a member
///   of that Chat: "When `chatId` does not resolve to a Chat object, the
///   server MUST return `invalidProperties` naming `chatId`. The server
///   SHOULD return the same error when the caller is not a member of that
///   Chat, so that the response does not reveal that the Chat exists."
///   Both cases return the same SetError. A direct Chat always includes
///   the owner. A group Chat must list the owner (by accountId) in
///   `members`. For a channel, the owner must be a member of its Space.
/// - A call with a `spaceId` or `channelId` needs the
///   [`ChatPermission::StartCall`] permission (draft-atwood-jmap-chat-00
///   §SpaceRole: "`"start_call"` — initiate a voice or video call in a
///   channel"): "the server SHOULD return `forbidden` if the caller lacks
///   the `"start_call"` permission in the bound Space or channel." The
///   Space is `spaceId`, or else the
///   channel's `spaceId`. The permissions are resolved as in
///   draft-atwood-jmap-chat-00 §Space Permission Resolution, with the
///   implicit `@everyone` role's permissions from
///   [`ChatBackend::everyone_permissions`]. A Space that does not resolve, or
///   does not list the owner as a member, grants nothing.
///
/// `caller` and `account_id` are the `VTCCall/set` caller and account.
/// The owner is named by `account_id` in Chat and Space member lists
/// (draft-atwood-jmap-chat-00 §Terminology: "a client-facing reference
/// to the owner carries the accountId of the owner's account").
///
/// Returns `Ok(Err(set_error))` when the create must be refused. Return
/// that SetError from `create_call`.
///
/// # Errors
///
/// Returns the Chat backend's error if reading the Chat, the channel,
/// the Space or its `@everyone` permissions fails.
pub async fn check_call_create<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
) -> Result<Result<(), SetError>, C::Error> {
    if let Some(chat_id) = bound(&call.chat_id) {
        let visible = match get_one::<Chat, _>(chat, caller, account_id, chat_id).await? {
            Some(c) if chat.may_view_chat(caller, account_id, chat_id).await? => {
                is_member(chat, caller, account_id, account_id, &c).await?
            }
            _ => false,
        };
        if !visible {
            return Ok(Err(SetError::new(SetErrorType::InvalidProperties)
                .with_properties(["chatId"])
                .with_description(
                    "chatId does not name a Chat the caller has access to",
                )));
        }
    }

    let space_id = bound(&call.space_id);
    let channel_id = bound(&call.channel_id);
    if space_id.is_none() && channel_id.is_none() {
        return Ok(Ok(()));
    }
    let channel = match channel_id {
        Some(id) => get_one::<Chat, _>(chat, caller, account_id, id).await?,
        None => None,
    };
    let space_id = space_id.or_else(|| channel.as_ref().and_then(|c| c.space_id.as_ref()));
    let space = match space_id {
        Some(id) => get_one::<Space, _>(chat, caller, account_id, id).await?,
        None => None,
    };
    let permissions = match space {
        Some(s) => {
            let everyone = chat.everyone_permissions(caller, account_id, &s).await?;
            member_permissions(&s, account_id, channel.as_ref(), &everyone)
        }
        None => HashSet::new(),
    };
    Ok(if permissions.contains(&ChatPermission::StartCall) {
        Ok(())
    } else {
        Err(forbidden(
            "the caller lacks the start_call permission in the bound Space or channel",
        ))
    })
}

/// `false` if the Chat rules of draft-atwood-jmap-vtc-00 deny `viewer`
/// access to `call`, a VTCCall in `account_id`, its hosting account.
///
/// `VtcBackend` §Per-call access makes `get_objects`, `query_objects` and
/// `query_changes` treat a call the caller cannot access, and its
/// VTCParticipant, VTCRecording and VTCLivestream records, as absent.
/// Call this per VTCCall from those methods and hide the call when it
/// returns `false`. §Joining a Room or Scheduled Call: "When
/// `urn:ietf:params:jmap:chat` is present, callers without access include
/// a caller who lacks `"view"` permission on the channel of a Space-bound
/// call, a caller who is not a member of the Chat bound by `chatId`, and
/// a caller whom the call initiator has blocked (`ChatContact.blocked` is
/// `true` on the initiator's contact record for the caller)."
///
/// `viewer` names the caller in the Chat data of `account_id`:
/// `account_id` for the owner, otherwise the caller's ChatContact id
/// (draft-atwood-jmap-chat-00 §Terminology: "a client-facing reference
/// to the owner carries the accountId of the owner's account"). Access is
/// denied when any of these holds:
///
/// - `viewer` is not the owner and its ChatContact in `account_id` has
///   `blocked` set. This treats the hosting account's contact list as the
///   initiator's, as [`check_call_create`] treats the owner as the
///   creator. It applies to every call, bound or not.
/// - The call has a `chatId` that names no Chat in `account_id`, that
///   [`ChatBackend::may_view_chat`] hides, or whose members do not include
///   `viewer`: the owner or `contactId` of a direct Chat, a `members`
///   entry of a group Chat, a member of the Space of a channel.
/// - The call has a `spaceId` or `channelId` and `viewer` is not a member
///   of the bound Space (`spaceId`, or else the channel's `spaceId`),
///   which then grants nothing: §Chat-Bound Calls "A call is visible to
///   members of the bound Chat or Space channel." A `channelId` that names
///   no Chat denies too.
/// - The call has a `channelId` and `viewer` lacks
///   [`ChatPermission::View`] there, resolved as in [`check_call_create`]
///   (draft-atwood-jmap-chat-00 §Space Permission Resolution, with the
///   `@everyone` floor from [`ChatBackend::everyone_permissions`]).
///
/// `true` only means the Chat rules do not deny access. A call with no
/// binding still needs the backend's own rule, draft-atwood-jmap-vtc-00
/// §Standalone Calls: participants and the initiator may access it, and
/// "Deployment-defined administrative roles MAY have broader access".
/// Nothing else there admits a non-participant, so a deployment that lets
/// other users join a standalone room call grants them that access
/// itself.
///
/// # Errors
///
/// Returns the Chat backend's error if reading the ChatContact, the Chat,
/// the channel, the Space or its `@everyone` permissions fails.
pub async fn may_access_call<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    viewer: &Id,
    call: &VtcCall,
) -> Result<bool, C::Error> {
    if viewer != account_id
        && get_one::<ChatContact, _>(chat, caller, account_id, viewer)
            .await?
            .is_some_and(|c| c.blocked)
    {
        return Ok(false);
    }
    if let Some(chat_id) = bound(&call.chat_id) {
        let member = match get_one::<Chat, _>(chat, caller, account_id, chat_id).await? {
            Some(c) if chat.may_view_chat(caller, account_id, chat_id).await? => {
                is_member(chat, caller, account_id, viewer, &c).await?
            }
            _ => false,
        };
        if !member {
            return Ok(false);
        }
    }
    let space_id = bound(&call.space_id);
    let channel = match bound(&call.channel_id) {
        Some(id) => match get_one::<Chat, _>(chat, caller, account_id, id).await? {
            Some(c) => Some(c),
            None => return Ok(false),
        },
        None if space_id.is_none() => return Ok(true),
        None => None,
    };
    let Some(space_id) = space_id.or_else(|| channel.as_ref().and_then(|c| c.space_id.as_ref()))
    else {
        return Ok(false);
    };
    let Some(space) = get_one::<Space, _>(chat, caller, account_id, space_id).await? else {
        return Ok(false);
    };
    if !space.members.iter().any(|m| &m.id == viewer) {
        return Ok(false);
    }
    let Some(channel) = channel else {
        return Ok(true);
    };
    let everyone = chat
        .everyone_permissions(caller, account_id, &space)
        .await?;
    Ok(
        member_permissions(&space, viewer, Some(&channel), &everyone)
            .contains(&ChatPermission::View),
    )
}

/// Set or clear `Chat.activeCallId` and `Chat.activeCallAccountId` for `call`, the VTCCall as stored
/// after a create or update.
///
/// draft-atwood-jmap-chat-00 §Chat `activeCallId`: "When a VTCCall with a
/// `chatId` matching this Chat's `id` transitions to state `"active"`,
/// the server MUST set this field to that VTCCall's `id` ... When that
/// VTCCall transitions to state `"ended"`, the server MUST clear this
/// field to `null`".
///
/// §Chat `activeCallAccountId`: "The server sets and clears this field
/// together with `activeCallId`, in the same state change: it is
/// non-null exactly when `activeCallId` is non-null."
///
/// For an `"active"` call the Chat's `activeCallId` becomes the call's
/// `id` and its `activeCallAccountId` becomes `account_id`, the account
/// hosting the call (the Chat is looked up in the same account, as in
/// [`check_call_create`]). For an `"ended"` call both become `null` if
/// `activeCallId` named this call. No other state changes them, and
/// unchanged values are not written again, so this is safe to call after
/// every VTCCall create and update. A `chatId` that names no Chat is
/// ignored.
///
/// The write goes through [`ChatBackend::update_object`], which advances
/// the Chat state, so the Chat `StateChange` follows. Call this in the
/// same atomic step as the VTCCall write and before the VTCCall
/// `StateChange` is delivered: the update MUST be "applied atomically
/// with it from the client's perspective".
///
/// # Errors
///
/// Returns the Chat backend's error from reading or updating the Chat.
// Same error type as `ChatBackend::update_object`, which it propagates.
pub async fn sync_active_call_id<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    call: &VtcCall,
) -> Result<(), BackendSetError<C::Error>> {
    let Some(chat_id) = bound(&call.chat_id) else {
        return Ok(());
    };
    let target = match call.state {
        CallState::Active => (Some(&call.id), Some(account_id)),
        CallState::Ended => (None, None),
        _ => return Ok(()),
    };
    let Some(stored) = get_one::<Chat, _>(chat, caller, account_id, chat_id)
        .await
        .map_err(BackendSetError::Other)?
    else {
        return Ok(());
    };
    let current = (
        stored.active_call_id.as_ref(),
        stored.active_call_account_id.as_ref(),
    );
    if current == target || (target.0.is_none() && current.0 != Some(&call.id)) {
        return Ok(());
    }
    let mut p = PatchObject::new();
    let map = p.as_map_mut();
    map.insert("activeCallId".to_owned(), json!(target.0));
    map.insert("activeCallAccountId".to_owned(), json!(target.1));
    chat.update_object::<Chat>(caller, account_id, chat_id, p)
        .await
        .map(|_| ())
}

/// Clear `spaceId` on every VTCCall that references the destroyed Space
/// `space_id`.
///
/// draft-atwood-jmap-vtc-00 §VTCCall: "When a Space or channel Chat
/// referenced by `spaceId` or `channelId` is destroyed, the server MUST
/// clear the corresponding field to `null` on all VTCCalls that
/// reference it." Each change goes through
/// [`VtcBackend::update_object`], which advances the VTCCall state ("MUST
/// emit a `StateChange` for the VTCCall type").
///
/// # Errors
///
/// Returns the VTC backend's error from reading or updating a call. Calls
/// cleared before the error stay cleared.
// Same error type as `VtcBackend::update_object`, which it propagates.
pub async fn clear_space_references<V: VtcBackend>(
    vtc: &V,
    caller: &V::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Result<(), BackendSetError<V::Error>> {
    clear_references(vtc, caller, account_id, "spaceId", |c| {
        bound(&c.space_id) == Some(space_id)
    })
    .await
}

/// Clear `channelId` on every VTCCall that references the destroyed
/// channel Chat `chat_id`, as [`clear_space_references`] does for
/// `spaceId`.
///
/// # Errors
///
/// As for [`clear_space_references`].
// Same error type as `VtcBackend::update_object`, which it propagates.
pub async fn clear_channel_references<V: VtcBackend>(
    vtc: &V,
    caller: &V::CallerCtx,
    account_id: &Id,
    chat_id: &Id,
) -> Result<(), BackendSetError<V::Error>> {
    clear_references(vtc, caller, account_id, "channelId", |c| {
        bound(&c.channel_id) == Some(chat_id)
    })
    .await
}

/// The identity string of the user that the Chat reference `contact_id`
/// names, as VTC uses it in `userId` and `initiatorId`.
///
/// draft-atwood-jmap-chat-00 §Terminology: a user's identity is
/// "recorded in the `identity` property of the user's ChatContact", and
/// "The owner has no ChatContact: a client-facing reference to the owner
/// carries the accountId". So `account_id` maps to
/// [`ChatBackend::owner_user_id`] and any other id to that ChatContact's
/// `identity`. `None` when the ChatContact does not exist or the owner's
/// identity is unknown.
///
/// A SpaceBan's `userId` is such a reference ("The ChatContact.id of the
/// banned user").
///
/// # Errors
///
/// Returns the Chat backend's error from reading the ChatContact or the
/// owner's identity.
pub async fn contact_identity<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    contact_id: &Id,
) -> Result<Option<String>, C::Error> {
    if contact_id == account_id {
        return chat.owner_user_id(caller, account_id).await;
    }
    Ok(
        get_one::<ChatContact, _>(chat, caller, account_id, contact_id)
            .await?
            .map(|c| c.identity),
    )
}

/// Remove the banned user `user_id` (an identity string, from
/// [`contact_identity`]) from every call bound to the Space `space_id`.
///
/// draft-atwood-jmap-vtc-00 §Chat-Bound Calls: "When a user is banned from a
/// Space (via SpaceBan creation) and has an active VTCParticipant record
/// in a call bound to that Space (via `spaceId`), the server SHOULD set
/// `leftAt` on that VTCParticipant record and emit a
/// `VTCParticipantEvent` with event `"left"`. The ban takes effect
/// immediately on active calls."
///
/// The calls are those in `account_id` whose `spaceId` is `space_id` and
/// that have not ended. A participant is active when its `userId` is
/// `user_id`, `joinedAt` is set and `leftAt` is `null`, as in
/// §VTCParticipant/query `isActive`. A ring target that has not answered
/// is not active and is left as it is. Each `leftAt` write goes through
/// [`VtcBackend::update_object`], which advances the VTCParticipant
/// state.
///
/// After the `leftAt` writes in a call, [`jmap_vtc_server::after_leave`]
/// applies the side effects of §Leaving a Call, as a `VTCParticipant/set`
/// leave does: a ring or scheduled call left with no active participant
/// ends, and a call whose last moderator was ejected hands the role to
/// the earliest-joined authenticated participant.
///
/// Returns one `"left"` event per participant written, for the caller to
/// pass to [`VtcBackend::deliver_event`].
///
/// # Errors
///
/// Returns the VTC backend's error from reading the calls or the
/// participants, from updating a participant, or from
/// [`jmap_vtc_server::after_leave`]. Writes made before the error stay
/// made, and their events are lost; the VTCCall and VTCParticipant
/// `StateChange`s still report them.
// Same error type as `VtcBackend::update_object`, which it propagates.
pub async fn eject_banned_user<V: VtcBackend>(
    vtc: &V,
    caller: &V::CallerCtx,
    account_id: &Id,
    space_id: &Id,
    user_id: &str,
) -> Result<Vec<VtcParticipantEvent>, BackendSetError<V::Error>> {
    // ponytail: reads every VTCCall and VTCParticipant, and `after_leave`
    // reads every VTCParticipant again per ejected call; O(calls +
    // ejected calls * participants) per ban. Upgrade: VTCCall/query on
    // `spaceId` and VTCParticipant/query on `callId`.
    let (calls, _) = vtc
        .get_objects::<VtcCall>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    let calls: Vec<&VtcCall> = calls
        .iter()
        .filter(|c| c.state != CallState::Ended && bound(&c.space_id) == Some(space_id))
        .collect();
    if calls.is_empty() {
        return Ok(Vec::new());
    }
    let (participants, _) = vtc
        .get_objects::<VtcParticipant>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    let mut events = Vec::new();
    for call in calls {
        let mut left_role = None;
        for p in participants.iter().filter(|p| {
            p.call_id == call.id
                && p.user_id.as_deref() == Some(user_id)
                && p.joined_at.is_some()
                && p.left_at.is_none()
        }) {
            vtc.update_object::<VtcParticipant>(
                caller,
                account_id,
                &p.id,
                patch("leftAt", json!(now_utc_string())),
            )
            .await?;
            if left_role != Some(CallRole::Moderator) {
                left_role = Some(p.role.clone());
            }
            events.push(
                serde_json::from_value(json!({
                    "accountId": account_id,
                    "callId": p.call_id,
                    "participantId": p.id,
                    "event": ParticipantEventKind::Left,
                    "displayName": p.display_name,
                    "role": p.role,
                }))
                .expect("a VTCParticipantEvent built from a stored participant is valid"),
            );
        }
        if let Some(role) = left_role {
            after_leave(vtc, caller, account_id, call, &role).await?;
        }
    }
    Ok(events)
}

/// `true` if `user_id` (an identity string) is blocked on the contact list
/// of `account_id`: a ChatContact with that `identity` has `blocked` set.
///
/// draft-atwood-jmap-vtc-00 §Blocked-Sender Suppression: "the server SHOULD
/// check whether the initiator corresponds to a contact whose `blocked`
/// field is `true` on the target participant's contact list (when
/// {{JMAP-CHAT}} is present) ... If the initiator is blocked, the server
/// SHOULD silently drop the ring notification." Return it from
/// [`VtcBackend::is_user_blocked`], passing the caller's own account as
/// `account_id`: the hook takes only the caller, the recipient
/// [`jmap_vtc_server::should_deliver`] asks it for.
///
/// # Errors
///
/// Returns the Chat backend's error from reading the contacts.
pub async fn is_blocked_by_contact<C: ChatBackend>(
    chat: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    user_id: &str,
) -> Result<bool, C::Error> {
    // ponytail: reads every ChatContact; O(contacts) per check. Upgrade: a
    // ChatContact/query with the `identity` filter.
    let (contacts, _) = chat
        .get_objects::<ChatContact>(caller, account_id, None, None)
        .await?;
    Ok(contacts.iter().any(|c| c.blocked && c.identity == user_id))
}

/// Whether VTC ephemeral `event` may be delivered on a connection, over
/// both subscription paths of draft-atwood-jmap-vtc-wss-00
/// §Interoperability with JMAP Chat WebSocket.
///
/// `chat_vtc` is [`jmap_chat_server::EphemeralSubscription::vtc`] of the
/// connection's current `ChatStreamEnable` subscription: `true` only when
/// `"vtc"` was in `dataTypes` and the server advertises
/// `urn:ietf:params:jmap:vtc:websocket`, as passed in
/// [`jmap_chat_server::CompanionStreams`]. Without that capability,
/// `"vtc"` "is treated as unrecognized and silently ignored"
/// (draft-atwood-jmap-chat-wss-00 §Enabling Ephemeral Push, L196). A
/// failed [`jmap_chat_server::EphemeralSubscription::replace_with`] leaves
/// the subscription, and so the flag, unchanged, and `ChatStreamDisable`
/// clears it. `subscription` is the connection's `VTCStreamEnable`
/// subscription, which only `VTCStreamDisable` resets (§Subscription
/// Independence).
///
/// The `"vtc"` path covers "all VTC event types for all calls in which the
/// user participates", which is a `VTCStreamEnable` with `callIds` and
/// `eventTypes` both `null`. Every `VTCStreamEnable` covers a subset of
/// that, so the union the server "MUST deliver" is the `"vtc"` path when it
/// is on and `subscription` otherwise. [`jmap_vtc_server::should_deliver`]
/// decides either way, with the same delivery-time authorization. The
/// answer is one `bool` per event and connection, so an event that matches
/// both paths is still sent once ("MUST NOT deliver duplicate events").
///
/// `hosting_account_id` is the event's `accountId` and
/// `recipient_account_id` the recipient's own account, as
/// [`jmap_vtc_server::should_deliver`] takes them.
///
/// # Errors
///
/// Returns the backend's error from [`jmap_vtc_server::should_deliver`].
pub async fn should_deliver_vtc<V: VtcBackend>(
    vtc: &V,
    caller: &V::CallerCtx,
    hosting_account_id: &Id,
    recipient_account_id: &Id,
    chat_vtc: bool,
    subscription: &VtcEphemeralSubscription,
    event: &VtcEvent,
) -> Result<bool, V::Error> {
    if !chat_vtc {
        return jmap_vtc_server::should_deliver(
            vtc,
            caller,
            hosting_account_id,
            recipient_account_id,
            subscription,
            event,
        )
        .await;
    }
    let mut every = VtcEphemeralSubscription::default();
    every
        .replace_with(&VtcStreamEnable::default())
        .expect("a VTCStreamEnable with null eventTypes is always valid");
    jmap_vtc_server::should_deliver(
        vtc,
        caller,
        hosting_account_id,
        recipient_account_id,
        &every,
        event,
    )
    .await
}

/// The Id in a `T|null` binding property, if it is set.
fn bound(field: &Option<Option<Id>>) -> Option<&Id> {
    field.as_ref().and_then(Option::as_ref)
}

/// A `forbidden` SetError with `description`.
fn forbidden(description: &str) -> SetError {
    SetError::new(SetErrorType::Forbidden).with_description(description)
}

/// A one-property PatchObject setting `key` to `value`.
fn patch(key: &str, value: Value) -> PatchObject {
    let mut p = PatchObject::new();
    p.as_map_mut().insert(key.to_owned(), value);
    p
}

/// Object `id` of type `O`, or `None` if `backend` does not return it.
async fn get_one<O: GetObject + Send + Sync, B: JmapBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<Option<O>, B::Error> {
    let (found, _) = backend
        .get_objects::<O>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await?;
    Ok(found.into_iter().next())
}

/// `true` if `member` (`account_id` for the owner, else a ChatContact id)
/// is a member of `chat`, a Chat in `account_id`. A direct Chat's members
/// are the owner and its `contactId`.
async fn is_member<C: ChatBackend>(
    backend: &C,
    caller: &C::CallerCtx,
    account_id: &Id,
    member: &Id,
    chat: &Chat,
) -> Result<bool, C::Error> {
    Ok(match &chat.kind {
        ChatKind::Direct => member == account_id || chat.contact_id.as_ref() == Some(member),
        ChatKind::Group => chat.members.iter().flatten().any(|m| &m.id == member),
        ChatKind::Channel => match &chat.space_id {
            Some(space_id) => get_one::<Space, _>(backend, caller, account_id, space_id)
                .await?
                .is_some_and(|s| s.members.iter().any(|m| &m.id == member)),
            None => false,
        },
        _ => false,
    })
}

/// The effective permissions of `member` (`account_id` for the owner,
/// else a ChatContact id) in `space`, or in `channel` when one is given
/// (draft-atwood-jmap-chat-00 §Space Permission Resolution). Empty when
/// `member` is not a member of `space`.
fn member_permissions(
    space: &Space,
    member: &Id,
    channel: Option<&Chat>,
    everyone: &HashSet<ChatPermission>,
) -> HashSet<ChatPermission> {
    let Some(entry) = space.members.iter().find(|m| &m.id == member) else {
        return HashSet::new();
    };
    let role_ids: HashSet<&str> = entry.role_ids.iter().map(AsRef::as_ref).collect();
    let space_permissions: HashSet<ChatPermission> = space
        .roles
        .iter()
        .filter(|r| role_ids.contains(r.id.as_ref()))
        .flat_map(|r| r.permissions.iter().cloned())
        .chain(everyone.iter().cloned())
        .collect();
    match channel {
        Some(c) => resolve_channel_permissions(
            space_permissions,
            space.id.as_ref(),
            member.as_ref(),
            &role_ids,
            c.permission_overrides.as_deref().unwrap_or_default(),
        ),
        None => space_permissions,
    }
}

/// Set `property` to `null` on every VTCCall for which `references` holds.
// Same error type as `VtcBackend::update_object`, which it propagates.
async fn clear_references<V: VtcBackend>(
    vtc: &V,
    caller: &V::CallerCtx,
    account_id: &Id,
    property: &str,
    references: impl Fn(&VtcCall) -> bool,
) -> Result<(), BackendSetError<V::Error>> {
    // ponytail: reads every VTCCall; O(calls) per destroy. Upgrade: a
    // VTCCall/query on the `spaceId` filter.
    let (calls, _) = vtc
        .get_objects::<VtcCall>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    for call in calls.iter().filter(|c| references(c)) {
        vtc.update_object::<VtcCall>(caller, account_id, &call.id, patch(property, Value::Null))
            .await?;
    }
    Ok(())
}
