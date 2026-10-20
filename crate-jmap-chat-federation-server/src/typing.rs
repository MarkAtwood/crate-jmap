//! `Peer/typing` (draft-atwood-jmap-chat-federation-00 §Peer/typing): a
//! remote participant is or is not typing in a shared Chat.

use jmap_chat_federation_types::{PeerAck, PeerTypingArgs};
use jmap_chat_server::should_deliver_typing;
use jmap_chat_types::ChatTypingEvent;
use jmap_server::{serialize_value, server_fail_from_backend, HandlerOutput};
use jmap_types::JmapError;
use serde_json::json;

use crate::deliver::{authenticate, check_account, get_chat, is_participant, Identities, Who};
use crate::FederationBackend;

/// Handle a `Peer/typing` method call.
///
/// - `senderUserId` must be the peer identity, else `forbidden`.
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
/// for an unknown `accountId`.
pub async fn handle_peer_typing<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: serde_json::Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerTypingArgs = serde_json::from_value(args)
        .map_err(|e| JmapError::invalid_arguments(format!("Peer/typing arguments: {e}")))?;
    authenticate(backend, caller, &args.sender_user_id)?;
    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;

    let identities = Identities::load(backend, caller, account_id).await?;
    let Who::Contact(sender) = identities.lookup(&args.sender_user_id) else {
        return Err(JmapError::forbidden());
    };
    let chat = get_chat(backend, caller, account_id, &args.chat_id)
        .await?
        .ok_or_else(JmapError::forbidden)?;
    if !is_participant(backend, caller, account_id, &chat, &sender.id).await? {
        return Err(JmapError::forbidden());
    }
    let ack = HandlerOutput::new(serialize_value(PeerAck::new(account_id.clone()))?);
    if sender.blocked {
        return Ok(ack);
    }
    let event: ChatTypingEvent = serde_json::from_value(json!({
        "accountId": account_id,
        "chatId": chat.id,
        "senderId": sender.id,
        "typing": args.typing,
    }))
    .map_err(|e| JmapError::server_fail(format!("typing event: {e}")))?;
    if should_deliver_typing(backend, caller, account_id, &event)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        backend.forward_typing(caller, account_id, &event).await;
    }
    Ok(ack)
}
