//! `Peer/typing` (draft-atwood-jmap-chat-federation-00 §Peer/typing): a
//! remote participant is or is not typing in a shared Chat.

use jmap_chat_federation_types::{PeerAck, PeerTypingArgs};
use jmap_chat_server::should_deliver_typing;
use jmap_chat_types::ChatTypingEvent;
use jmap_server::{HandlerOutput, serialize_value, server_fail_from_backend};
use jmap_types::JmapError;
use serde_json::json;

use crate::FederationBackend;
use crate::deliver::{Identities, Who, authenticate, get_chat, is_participant, route};

/// Handle a `Peer/typing` method call.
///
/// - `senderUserId` must be a peer identity of the request, else
///   `forbidden`. The call is processed in the mailbox `recipientUserId`
///   names, else `forbidden`; the response carries the request's
///   `accountId`, and the forwarded event the mailbox's.
/// - The sender must be a participant of the Chat (the direct Chat's
///   contact, a group member, or a member of the channel's Space), else
///   `forbidden` ("If it is not a member, the server SHOULD return a
///   `forbidden` method error"). An unknown Chat or an identity with no
///   ChatContact is not a membership, and creates nothing.
/// - A sender whose ChatContact is `blocked`, and every event that
///   [`should_deliver_typing`] refuses (`receiveTypingIndicators: false`
///   on a direct or group Chat, a sender the backend reports blocked), is
///   dropped silently: success, nothing forwarded.
/// - Otherwise the event goes to [`FederationBackend::forward_typing`]
///   with the sender's ChatContact id as `senderId`. Nothing is stored.
///
/// # Errors
///
/// `invalidArguments`, `forbidden` and `serverFail`; `accountNotFound`
/// for an `accountId` that is not the peer's account.
pub async fn handle_peer_typing<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: serde_json::Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerTypingArgs = serde_json::from_value(args)
        .map_err(|e| JmapError::invalid_arguments(format!("Peer/typing arguments: {e}")))?;
    let peer = authenticate(backend, caller, &args.sender_user_id).await?;
    let mailbox = &route(
        backend,
        caller,
        &peer,
        &args.account_id,
        &args.recipient_user_id,
    )
    .await?;

    let identities = Identities::load(backend, caller, mailbox).await?;
    let Who::Contact(sender) = identities.lookup(&args.sender_user_id) else {
        return Err(JmapError::forbidden());
    };
    let chat = get_chat(backend, caller, mailbox, &args.chat_id)
        .await?
        .ok_or_else(JmapError::forbidden)?;
    if !is_participant(backend, caller, mailbox, &chat, &sender.id).await? {
        return Err(JmapError::forbidden());
    }
    let ack = HandlerOutput::new(serialize_value(PeerAck::new(args.account_id.clone()))?);
    if sender.blocked {
        return Ok(ack);
    }
    let event: ChatTypingEvent = serde_json::from_value(json!({
        "accountId": mailbox,
        "chatId": chat.id,
        "senderId": sender.id,
        "typing": args.typing,
    }))
    .map_err(|e| JmapError::server_fail(format!("typing event: {e}")))?;
    if should_deliver_typing(backend, caller, mailbox, &event)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        backend.forward_typing(caller, mailbox, &event).await;
    }
    Ok(ack)
}
