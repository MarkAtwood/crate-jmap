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
//! | `websocket` feature: per connection, after building both stream handlers | `link_scene_stream` |

#![cfg_attr(docsrs, feature(doc_cfg))]
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
/// `SceneStreamEnable` without `observe` covers a subset of that, so the
/// union the server "MUST deliver" is the `"scene"` path when it is on and
/// `subscription` otherwise. An observer subscription (jmap-chat-spec
/// 93f2258) also reaches listed regions where the user has no avatar, and
/// the `"scene"` path "has no observer form", so with the path on an event
/// it refuses is still offered to an observing `subscription`.
/// [`jmap_scene_server::should_deliver`] decides each way,
/// with the same delivery-time access, visibility and blocked-sender
/// checks. The answer is one `bool` per event and connection, so an event
/// that matches both paths is still sent once (draft-atwood-jmap-scene-wss-00,
/// jmap-chat-spec 77c3474 L917: "SHOULD NOT deliver duplicate events",
/// followed here by choice).
///
/// `hosting_account_id` must be the event's `accountId`, its region's
/// hosting account; an event naming another account is not delivered.
/// `recipient_account_id` is the recipient's own account, whose Chat
/// capability the blocked-sender check consults; `caller` is the
/// recipient, whose `ChatContact` records it consults, as
/// [`jmap_scene_server::should_deliver`] takes them.
///
/// # Errors
///
/// Returns the backend's error from [`jmap_scene_server::should_deliver`].
///
/// # Panics
///
/// Does not panic: the `expect` on the `"scene"` path's catch-all
/// subscription holds because
/// [`SceneEphemeralSubscription::replace_with`] fails only on an
/// `eventTypes` array with no recognized value, and the default
/// [`SceneStreamEnable`] has `eventTypes` `null`.
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
    if jmap_scene_server::should_deliver(
        backend,
        caller,
        hosting_account_id,
        recipient_account_id,
        &every,
        event,
    )
    .await?
    {
        return Ok(true);
    }
    // An observer subscription reaches listed regions without an avatar,
    // which the `"scene"` path ("It has no observer form") does not.
    if !subscription
        .filter()
        .is_some_and(jmap_scene_server::SceneStreamFilter::observes)
    {
        return Ok(false);
    }
    jmap_scene_server::should_deliver(
        backend,
        caller,
        hosting_account_id,
        recipient_account_id,
        subscription,
        event,
    )
    .await
}

/// Connect a connection's `ChatStreamEnable` handler to its
/// `SceneStreamEnable` handler, so that `"scene"` in `dataTypes` turns on
/// the Scene events through `scene` (draft-atwood-jmap-scene-wss-00
/// §Interoperability with JMAP Chat WebSocket).
///
/// Returns `chat` with `"scene"` recognized (its
/// [`CompanionStreams`](jmap_chat_server::CompanionStreams) `scene` set:
/// a server that registers `scene` advertises
/// `urn:ietf:params:jmap:scene:websocket`) and an observer that sets
/// `scene`'s
/// [`SceneChatAlias`](jmap_scene_server::websocket::SceneChatAlias) from
/// every new Chat subscription. Register both handlers on the connection.
///
/// `scene` then decides each Scene event once over both paths, as
/// [`should_deliver_scene`] does: the union is delivered and "if an event
/// matches both subscriptions, it is delivered exactly once"
/// (§Subscription Union). `SceneStreamDisable` leaves the `"scene"` path
/// on, and `ChatStreamDisable` turns it off and leaves the
/// `SceneStreamEnable` subscription in place (§Subscription
/// Independence). A failed `SceneStreamEnable` leaves the `"scene"` path
/// alone, and a failed `ChatStreamEnable` the `SceneStreamEnable`
/// subscription ("An error in one mechanism ... MUST NOT affect the
/// other's active subscription", §Coexistence Rules).
///
/// ```
/// use std::sync::Arc;
/// use jmap_chat_server::ChatBackend;
/// use jmap_chat_server::websocket::{ChatEventHub, ChatStreamHandler};
/// use jmap_chat_scene_server::link_scene_stream;
/// use jmap_scene_server::SceneBackend;
/// use jmap_scene_server::websocket::{SceneEventHub, SceneStreamHandler};
/// use jmap_types::Id;
/// use jmap_websocket::server::{Connection, DispatcherHandler};
///
/// fn wire<C: ChatBackend<CallerCtx = ()>, S: SceneBackend<CallerCtx = ()>>(
///     (chat, chat_hub): (Arc<C>, &ChatEventHub),
///     (scene, scene_hub): (Arc<S>, &SceneEventHub),
///     connection: Connection<DispatcherHandler<()>>,
/// ) -> Connection<DispatcherHandler<()>> {
///     let account = Id::from("acct-alice");
///     let scene = SceneStreamHandler::new(scene, (), account.clone(), scene_hub);
///     let chat = link_scene_stream(ChatStreamHandler::new(chat, (), [account], chat_hub), &scene);
///     connection.with_extension(chat).with_extension(scene)
/// }
/// ```
#[cfg(feature = "websocket")]
pub fn link_scene_stream<C: jmap_chat_server::ChatBackend, B: SceneBackend>(
    chat: jmap_chat_server::websocket::ChatStreamHandler<C>,
    scene: &jmap_scene_server::websocket::SceneStreamHandler<B>,
) -> jmap_chat_server::websocket::ChatStreamHandler<C> {
    let mut companions = chat.companions();
    companions.scene = true;
    let alias = scene.chat_alias();
    chat.with_companions(companions)
        .on_subscription_change(move |subscription| alias.set(subscription.scene()))
}
