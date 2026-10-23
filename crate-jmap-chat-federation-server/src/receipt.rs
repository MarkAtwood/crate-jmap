//! `Peer/receipt` (draft-atwood-jmap-chat-federation-00 §Peer/receipt):
//! a recipient's server reports that a Message the owner sent was stored
//! or read.

use jmap_chat_federation_types::{PeerAck, PeerReceiptArgs};
use jmap_chat_types::{DeliveryState, Message, PresenceStatus, ReadDisposition};
use jmap_server::{HandlerOutput, serialize_value, server_fail_from_backend};
use jmap_types::{JmapError, PatchObject};
use serde_json::Map;

use crate::FederationBackend;
use crate::deliver::{
    Identities, Who, authenticate, check_account, get_chat, is_participant, set_failure,
};

/// Handle a `Peer/receipt` method call.
///
/// - `readerUserId` must be the peer identity: "the receiver MUST reject
///   the request with `forbidden` if they differ".
/// - The Message is the owner's Message whose `senderMsgId` is the
///   request's; with none, `invalidArguments`. The reader must be a
///   participant of its Chat (the direct Chat's contact, a group member,
///   or a member of the channel's Space), else `forbidden`. An identity
///   with no ChatContact is no participant, so no placeholder is created.
/// - The receipt is stored in `deliveryReceipts` under the reader's
///   ChatContact id ("the receiver updates the per-recipient delivery
///   receipt in `deliveryReceipts` under that user's local ChatContact
///   `id`"), for every Chat kind: a direct Chat has no other place for the
///   `readDisposition` the draft requires to be stored. Fields the request
///   carries replace the stored ones. `readAt` without `readDisposition`
///   stores `"displayed"`; `readDisposition` without `readAt` is ignored.
/// - `deliveryState` rises to `"delivered"` with `deliveredAt` and to
///   `"received"` with `readAt`, and never falls (§Delivery State
///   Transitions: "the best state any recipient has reached"). The
///   top-level `deliveredAt` is set when absent.
/// - When the owner's effective `receiptSharing` (`Chat.receiptSharing`,
///   else `PresenceStatus.receiptSharing`, default `true`) is `false`,
///   only `deliveredAt` is recorded (§Peer/receipt "Receiver Behavior").
///   The response is the same.
/// - A receipt that changes nothing writes nothing.
///
/// # Errors
///
/// `invalidArguments`, `forbidden` and `serverFail`; `accountNotFound`
/// for an unknown `accountId`.
pub async fn handle_peer_receipt<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: serde_json::Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerReceiptArgs = serde_json::from_value(args)
        .map_err(|e| JmapError::invalid_arguments(format!("Peer/receipt arguments: {e}")))?;
    authenticate(backend, caller, &args.reader_user_id)?;
    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;

    let identities = Identities::load(backend, caller, account_id).await?;
    let Who::Contact(reader) = identities.lookup(&args.reader_user_id) else {
        return Err(JmapError::forbidden());
    };
    let (messages, _) = backend
        .get_objects::<Message>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let msg = messages
        .into_iter()
        .find(|m| m.sender_msg_id == args.sender_msg_id && &m.sender_id == account_id)
        .ok_or_else(|| {
            JmapError::invalid_arguments("no message of the owner with this senderMsgId")
        })?;
    let chat = get_chat(backend, caller, account_id, &msg.chat_id)
        .await?
        .ok_or_else(JmapError::forbidden)?;
    if !is_participant(backend, caller, account_id, &chat, &reader.id).await? {
        return Err(JmapError::forbidden());
    }
    let shares = match chat.receipt_sharing {
        Some(shares) => shares,
        None => backend
            .get_objects::<PresenceStatus>(caller, account_id, None, None)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
            .0
            .first()
            .is_none_or(|p| p.receipt_sharing),
    };

    let mut receipts = msg.delivery_receipts.clone().unwrap_or_default();
    let receipt = receipts.entry(reader.id.to_string()).or_default();
    let before = receipt.clone();
    let mut state = msg.delivery_state.clone();
    if let Some(delivered_at) = &args.delivered_at {
        receipt.delivered_at = Some(delivered_at.clone());
        state = raise(state, DeliveryState::Delivered);
    }
    if shares {
        if let Some(device) = &args.device_delivered_at {
            receipt.device_delivered_at = Some(device.clone());
        }
        if let Some(read_at) = &args.read_at {
            receipt.read_at = Some(read_at.clone());
            receipt.read_disposition = Some(
                args.read_disposition
                    .clone()
                    .unwrap_or(ReadDisposition::Displayed),
            );
            state = raise(state, DeliveryState::Received);
        }
    }

    let mut patch = Map::new();
    if *receipt != before {
        patch.insert("deliveryReceipts".to_owned(), serialize_value(&receipts)?);
    }
    if state != msg.delivery_state {
        patch.insert("deliveryState".to_owned(), serialize_value(&state)?);
    }
    if let (None, Some(delivered_at)) = (&msg.delivered_at, &args.delivered_at) {
        patch.insert("deliveredAt".to_owned(), serialize_value(delivered_at)?);
    }
    if !patch.is_empty() {
        backend
            .update_object::<Message>(caller, account_id, &msg.id, PatchObject::from_map(patch))
            .await
            .map_err(set_failure)?;
    }
    Ok(HandlerOutput::new(serialize_value(PeerAck::new(
        account_id.clone(),
    ))?))
}

/// `current` raised to `to` in the order `"pending"` < `"delivered"` <
/// `"received"` (§Delivery State Transitions). A receipt shows the
/// recipient was reached, so `"failed"` ranks with `"pending"`. A value
/// this crate does not know is kept.
fn raise(current: DeliveryState, to: DeliveryState) -> DeliveryState {
    let rank = |s: &DeliveryState| match s {
        DeliveryState::Pending | DeliveryState::Failed => Some(0),
        DeliveryState::Delivered => Some(1),
        DeliveryState::Received => Some(2),
        _ => None,
    };
    match (rank(&current), rank(&to)) {
        (Some(c), Some(t)) if t > c => to,
        _ => current,
    }
}
