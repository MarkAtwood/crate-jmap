//! Restrictions on a Chat with a `roomId` (draft-atwood-jmap-chat-mimi-00
//! §Chat.roomId and §message/mls), for a `jmap-chat-server` `ChatBackend`
//! to return from its per-Chat restriction hooks.
//!
//! Each function takes the stored Chat's extension properties
//! (`Chat::extra`) and permits everything when they carry no `roomId`. An
//! unbound Chat "keeps its `roomId`", so it stays restricted.

use jmap_chat_mimi_types::chat_room_id;
use serde_json::{Map, Value};

use crate::{SetError, SetErrorType};

/// The property a patch key changes: the key up to its first `/`.
fn property(patch_key: &str) -> &str {
    patch_key.split('/').next().unwrap_or(patch_key)
}

/// For `ChatBackend::may_update_chat`.
///
/// §Chat.roomId: "The server MUST reject, with `forbidden`, a `Chat/set`
/// update on such a Chat that sets `name`, `description`, or
/// `avatarBlobId` or that uses the `addMembers`, `removeMembers`, or
/// `updateMemberRoles` patch key".
// SetError is the type every `/set` refusal takes.
#[allow(clippy::result_large_err)]
pub fn may_update_room_chat(
    chat_extra: &Map<String, Value>,
    patch_keys: &[&str],
) -> Result<(), SetError> {
    let restricted = patch_keys.iter().any(|k| {
        matches!(
            property(k),
            "name"
                | "description"
                | "avatarBlobId"
                | "addMembers"
                | "removeMembers"
                | "updateMemberRoles"
        )
    });
    if chat_room_id(chat_extra).is_some() && restricted {
        return Err(SetError::new(SetErrorType::Forbidden).with_description(
            "the participants and metadata of a room-bound Chat change only through Room/update",
        ));
    }
    Ok(())
}

/// For `ChatBackend::may_create_message`.
///
/// §message/mls: "The server MUST reject a `Message/set` create in a Chat
/// with a `roomId` with a `forbidden` SetError."
#[allow(clippy::result_large_err)]
pub fn may_create_room_message(chat_extra: &Map<String, Value>) -> Result<(), SetError> {
    if chat_room_id(chat_extra).is_some() {
        return Err(SetError::new(SetErrorType::Forbidden)
            .with_description("messages in a room-bound Chat are sent with Room/send"));
    }
    Ok(())
}

/// For `ChatBackend::may_update_message`.
///
/// §message/mls: "The server MUST reject a `Message/set` update that
/// changes `body`, `bodyType`, or `reactions` of a Message in a Chat with
/// a `roomId` with an `invalidProperties` SetError naming the changed
/// properties." A patch key below one of them, such as
/// `reactions/<senderReactionId>`, changes that property.
#[allow(clippy::result_large_err)]
pub fn may_update_room_message(
    chat_extra: &Map<String, Value>,
    patch_keys: &[&str],
) -> Result<(), SetError> {
    if chat_room_id(chat_extra).is_none() {
        return Ok(());
    }
    let changed: Vec<&str> = ["body", "bodyType", "reactions"]
        .into_iter()
        .filter(|p| patch_keys.iter().any(|k| property(k) == *p))
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    Err(SetError::new(SetErrorType::InvalidProperties)
        .with_properties(changed)
        .with_description("MIMI edits, reactions and retractions are new messages"))
}
