//! Chat binding for the JMAP Scene method handlers: the rules of
//! draft-atwood-jmap-scene-wss-00 that apply when a server offers both
//! `urn:ietf:params:jmap:chat:websocket` and
//! `urn:ietf:params:jmap:scene:websocket`.
//!
//! `jmap-scene-server` and `jmap-chat-server` do not depend on each other.
//! A deployment that offers both calls these functions:
//!
//! | Call site | Call |
//! |---|---|
//! | WebSocket transport, before sending a Scene event | [`should_deliver_scene`] |

#![forbid(unsafe_code)]

use jmap_scene_server::{SceneBackend, SceneEphemeralSubscription};
use jmap_scene_types::{SceneEvent, SceneStreamEnable};
use jmap_types::Id;

/// Whether Scene ephemeral `event` may be delivered on a connection, over
/// both subscription paths of draft-atwood-jmap-scene-wss-00
/// §Interoperability with JMAP Chat WebSocket.
///
/// `chat_scene` is `jmap_chat_server::EphemeralSubscription::scene` of the
/// connection's current `ChatStreamEnable` subscription: `true` only when
/// `"scene"` was in `dataTypes` and the server advertises
/// `urn:ietf:params:jmap:scene:websocket`, as passed in
/// `jmap_chat_server::CompanionStreams`. Without that capability, a
/// companion `dataTypes` value "is treated as unrecognized and silently
/// ignored" (draft-atwood-jmap-chat-wss-00 §Enabling Ephemeral Push,
/// L196). A failed `replace_with` leaves the subscription, and so the
/// flag, unchanged, and `ChatStreamDisable` clears it.
/// `subscription` is the connection's `SceneStreamEnable` subscription,
/// which only `SceneStreamDisable` resets (§Subscription Independence).
///
/// The `"scene"` path covers "all Scene event types for all regions in
/// which the user has an active avatar", which is a `SceneStreamEnable`
/// with `regionIds` and `eventTypes` both `null`. Every
/// `SceneStreamEnable` covers a subset of that, so the union the server
/// "MUST deliver" is the `"scene"` path when it is on and `subscription`
/// otherwise. [`jmap_scene_server::should_deliver`] decides either way,
/// with the same delivery-time access, visibility and blocked-sender
/// checks. The answer is one `bool` per event and connection, so an event
/// that matches both paths is still sent once ("MUST NOT deliver duplicate
/// events").
///
/// `hosting_account_id` must be the event's `accountId`, its region's
/// hosting account; an event naming another account is not delivered.
/// `recipient_account_id` is the recipient's own account, whose Chat
/// capability and ChatContact records the blocked-sender check consults,
/// as [`jmap_scene_server::should_deliver`] takes them.
///
/// # Errors
///
/// Returns the backend's error from [`jmap_scene_server::should_deliver`].
pub async fn should_deliver_scene<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    hosting_account_id: &Id,
    recipient_account_id: &Id,
    chat_scene: bool,
    subscription: &SceneEphemeralSubscription,
    event: &SceneEvent,
) -> Result<bool, B::Error> {
    if !chat_scene {
        return jmap_scene_server::should_deliver(
            backend,
            caller,
            hosting_account_id,
            recipient_account_id,
            subscription,
            event,
        )
        .await;
    }
    let mut every = SceneEphemeralSubscription::default();
    every
        .replace_with(&SceneStreamEnable::default())
        .expect("a SceneStreamEnable with null eventTypes is always valid");
    jmap_scene_server::should_deliver(
        backend,
        caller,
        hosting_account_id,
        recipient_account_id,
        &every,
        event,
    )
    .await
}
