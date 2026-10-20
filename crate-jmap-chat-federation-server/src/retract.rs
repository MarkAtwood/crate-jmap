//! `Peer/retract` (draft-atwood-jmap-chat-federation-00 §Peer/retract):
//! the original sender tombstones one of their Messages.

use jmap_chat_federation_types::{PeerRetractArgs, PeerRetractResponse};
use jmap_chat_server::find_message_by_sender_msg_id;
use jmap_chat_types::Message;
use jmap_server::{now_utc_string, serialize_value, server_fail_from_backend, HandlerOutput};
use jmap_types::{JmapError, PatchObject, UTCDate};
use serde_json::{json, Map, Value};

use crate::deliver::{authenticate, check_account, set_failure, Identities, Who};
use crate::FederationBackend;

/// Handle a `Peer/retract` method call.
///
/// - `senderUserId` must be the peer identity, else `forbidden`.
/// - The Message is looked up by `senderMsgId` within `chatId`; with
///   none, `invalidArguments`. Its `senderId` must be the ChatContact
///   `senderUserId` resolves to, else `forbidden` ("If the sender does not
///   match, the server MUST reject the request"). An identity with no
///   ChatContact sent nothing here, so no placeholder is created.
/// - On success `body` becomes `""`, `attachments` `[]`, `editHistory` is
///   removed, `deletedAt` is now and `deletedForAll` is `true`; the record
///   stays as a tombstone. `retractedAt` is `deletedAt`.
/// - A Message already retracted for all is not written again: a retry
///   answers with its stored `deletedAt`.
///
/// # Errors
///
/// `invalidArguments`, `forbidden` and `serverFail`; `accountNotFound`
/// for an unknown `accountId`.
pub async fn handle_peer_retract<B: FederationBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let args: PeerRetractArgs = serde_json::from_value(args)
        .map_err(|e| JmapError::invalid_arguments(format!("Peer/retract arguments: {e}")))?;
    authenticate(backend, caller, &args.sender_user_id)?;
    let account_id = &args.account_id;
    check_account(backend, caller, account_id).await?;

    let identities = Identities::load(backend, caller, account_id).await?;
    let msg = find_message_by_sender_msg_id(
        backend,
        caller,
        account_id,
        &args.chat_id,
        &args.sender_msg_id,
    )
    .await
    .map_err(|e| server_fail_from_backend(&e))?
    .ok_or_else(|| JmapError::invalid_arguments("no message with this senderMsgId in the chat"))?;
    let is_sender = matches!(
        identities.lookup(&args.sender_user_id),
        Who::Contact(c) if c.id == msg.sender_id
    );
    if !is_sender {
        return Err(JmapError::forbidden());
    }

    let respond = |retracted_at: UTCDate| -> Result<HandlerOutput, JmapError> {
        Ok(HandlerOutput::new(serialize_value(
            PeerRetractResponse::new(account_id.clone(), retracted_at),
        )?))
    };
    if msg.deleted_for_all == Some(true) {
        if let Some(deleted_at) = msg.deleted_at {
            return respond(deleted_at);
        }
    }
    let now = now_utc_string();
    let mut patch = Map::new();
    patch.insert("body".to_owned(), json!(""));
    patch.insert("attachments".to_owned(), json!([]));
    patch.insert("editHistory".to_owned(), Value::Null);
    patch.insert("deletedAt".to_owned(), json!(now));
    patch.insert("deletedForAll".to_owned(), json!(true));
    backend
        .update_object::<Message>(caller, account_id, &msg.id, PatchObject::from_map(patch))
        .await
        .map_err(set_failure)?;
    respond(now)
}
