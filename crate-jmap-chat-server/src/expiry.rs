//! Time-driven expiry for one account (draft-atwood-jmap-chat-00).
//!
//! The kit runs no scheduler. A deployment calls [`sweep_expired`] from
//! its own timer, passing the current time, and the sweep applies every
//! deadline that has passed:
//!
//! - §Message `senderExpiresAt`: hard-delete "at or before this time".
//! - §Chat `messageExpirySeconds`: "messages in this chat older than this
//!   many seconds are deleted by this mailbox", age measured from
//!   `receivedAt` ("Authoritative for ordering and expiry calculations").
//! - §Message `expiresAfterReadSeconds`: hard-delete "this many seconds
//!   after setting `readAt`".
//! - §Message `burnOnRead`: a read message is hard-deleted "immediately
//!   after setting `readAt`". `Message/set` already does this atomically
//!   through [`ChatBackend::read_and_burn_message`]; the sweep deletes any
//!   read `burnOnRead` message that reached the store another way.
//!
//!   Both read-based rules apply "only to the receiving mailbox; the
//!   sender's own copy is not affected", so the sweep skips them for a
//!   Message whose `senderId` is the account itself.
//! - §Chat `muteUntil`: "Servers SHOULD clear `muted` and `muteUntil`
//!   automatically when the time passes."
//! - §PresenceStatus `expiresAt`: "the server SHOULD clear `statusText`
//!   and `statusEmoji` (setting them to null) at this time, and SHOULD
//!   reset `presence` to `"online"`."
//!
//! Messages are hard-deleted through [`ChatBackend::expire_message`],
//! after which each affected Chat's derived §Chat `unreadCount` is
//! recorded as changed, as `Message/set` destroy does. Chat and
//! PresenceStatus resets go through
//! [`ChatBackend::update_object`], so each advances its type's state.

use std::time::{SystemTime, UNIX_EPOCH};

use jmap_chat_types::{Chat, Message, PresenceStatus};
use jmap_types::{Id, PatchObject, UTCDate};
use serde_json::{json, Map, Value};

use crate::chat::record_unread_count_changes;
use crate::{BackendSetError, ChatBackend};

/// Apply every expiry deadline in `account_id` that has passed at `now`.
///
/// See the [module documentation](self) for the rules applied. Each step
/// is idempotent: a re-run at the same `now` finds nothing left to do,
/// and changes no state.
///
/// A stored date that does not parse as a UTCDate never counts as passed.
///
/// ponytail: loads every Message, Chat and PresenceStatus of the account
/// on each run, so a sweep is O(records). A backend with many messages
/// should index the deadlines itself and call
/// [`ChatBackend::expire_message`] directly.
///
/// # Errors
///
/// Returns the first backend error. A `SetError` comes from a reset
/// patch that [`ChatBackend::update_object`] rejected. Work done before
/// the error stays applied, so the next run continues from there.
// Same error type as `ChatBackend::update_object`, which it propagates.
#[allow(clippy::result_large_err)]
pub async fn sweep_expired<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    now: SystemTime,
) -> Result<(), BackendSetError<B::Error>> {
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let passed = |t: &UTCDate, offset: u64| {
        t.to_epoch_seconds().is_ok_and(|t| {
            i64::try_from(offset).is_ok_and(|o| t.checked_add(o).is_some_and(|d| now >= d))
        })
    };

    let (chats, _) = backend
        .get_objects::<Chat>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    let (messages, _) = backend
        .get_objects::<Message>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;

    let mut expired_chat_ids = Vec::new();
    let mut expire_result = Ok(());
    for msg in &messages {
        let policy = chats
            .iter()
            .find(|c| c.id == msg.chat_id)
            .and_then(|c| c.message_expiry_seconds)
            .filter(|&s| s > 0);
        let read_deadline = msg.sender_id != *account_id
            && msg.read_at.as_ref().is_some_and(|read| {
                msg.burn_on_read == Some(true)
                    || msg
                        .expires_after_read_seconds
                        .is_some_and(|s| passed(read, s))
            });
        // "older than this many seconds": strictly more than `s` elapsed.
        let expired = msg.sender_expires_at.as_ref().is_some_and(|t| passed(t, 0))
            || policy.is_some_and(|s| s < u64::MAX && passed(&msg.received_at, s + 1))
            || read_deadline;
        if expired {
            if let Err(e) = backend.expire_message(caller, account_id, &msg.id).await {
                expire_result = Err(BackendSetError::Other(e));
                break;
            }
            expired_chat_ids.push(msg.chat_id.clone());
        }
    }
    // §Chat `unreadCount` dropped for these Chats; RFC 8620 §5.1 requires
    // the Chat state to advance. Recorded before any error returns: a
    // later run no longer sees the deleted Messages.
    record_unread_count_changes(backend, caller, account_id, expired_chat_ids).await;
    expire_result?;

    for chat in &chats {
        if chat.mute_until.as_ref().is_some_and(|t| passed(t, 0)) {
            let patch = patch([("muted", json!(false)), ("muteUntil", Value::Null)]);
            backend
                .update_object::<Chat>(caller, account_id, &chat.id, patch)
                .await?;
        }
    }

    let (presences, _) = backend
        .get_objects::<PresenceStatus>(caller, account_id, None, None)
        .await
        .map_err(BackendSetError::Other)?;
    for status in &presences {
        if status.expires_at.as_ref().is_some_and(|t| passed(t, 0)) {
            // `expiresAt` is cleared too, so a later status set without a
            // new `expiresAt` is not wiped by the stale deadline.
            let patch = patch([
                ("statusText", Value::Null),
                ("statusEmoji", Value::Null),
                ("presence", json!("online")),
                ("expiresAt", Value::Null),
            ]);
            backend
                .update_object::<PresenceStatus>(caller, account_id, &status.id, patch)
                .await?;
        }
    }
    Ok(())
}

fn patch<const N: usize>(entries: [(&str, Value); N]) -> PatchObject {
    PatchObject::from(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<Map<_, _>>(),
    )
}
