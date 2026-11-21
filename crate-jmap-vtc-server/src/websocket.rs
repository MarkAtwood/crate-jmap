//! `VTCStreamEnable` / `VTCStreamDisable` and the VTC ephemeral events
//! they turn on, as a [`jmap_websocket`] extension
//! (draft-atwood-jmap-vtc-wss-00).
//!
//! - The deployment keeps one [`VtcEventHub`] and publishes every VTC
//!   event to it: per-call events with [`VtcEventHub::publish`], the
//!   per-user `VTCRingEvent` and `VTCCallEndEvent` with
//!   [`VtcEventHub::publish_to_user`], once per user they concern. Every
//!   event carries `accountId`, "the id of the call's hosting account"
//!   (§Event Types).
//! - For each WebSocket connection it builds a [`VtcStreamHandler`] with
//!   the connection's caller and the caller's own account, and registers
//!   it with
//!   [`Connection::with_extension`](jmap_websocket::server::Connection::with_extension)
//!   next to advertising `urn:ietf:params:jmap:vtc:websocket`.
//!
//! For each published event the handler asks [`should_deliver`], at
//! delivery time, with the event's `accountId` as the hosting account,
//! applies the draft's rate limits ([`VtcStreamLimits`]), and queues the
//! frame with [`ExtensionSender::try_send`], which drops it if the client
//! is too slow.
//!
//! `ChatStreamEnable` with `"vtc"` in `dataTypes` is a second path to the
//! same events (§Interoperability with JMAP Chat WebSocket). The Chat
//! handler switches it through the [`VtcChatAlias`] of this handler
//! (`jmap-chat-vtc-server` wires the two), so both paths are decided
//! here, once per event: the union is delivered without duplicates.
//!
//! Credential expiry is the deployment's: "a server that does not close
//! the connection SHOULD NOT deliver further VTC ephemeral events on that
//! connection" (§Credential Expiry). The handler does not know when
//! credentials expire; close the connection then.
//!
//! ```
//! use std::sync::Arc;
//! use jmap_types::Id;
//! use jmap_vtc_server::VtcBackend;
//! use jmap_vtc_server::websocket::{VtcEventHub, VtcStreamHandler};
//! use jmap_websocket::server::{Connection, DispatcherHandler};
//!
//! /// Called once per accepted connection; `hub` is the server's.
//! fn wire<B: VtcBackend<CallerCtx = ()>>(
//!     backend: Arc<B>,
//!     hub: &VtcEventHub,
//!     connection: Connection<DispatcherHandler<()>>,
//! ) -> Connection<DispatcherHandler<()>> {
//!     let vtc = VtcStreamHandler::new(backend, (), Id::from("acct-alice"), hub);
//!     connection.with_extension(vtc)
//! }
//! ```

mod limit;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use jmap_types::{Id, JmapError};
use jmap_vtc_types::{VtcEvent, VtcStreamEnable};
use jmap_websocket::server::{ExtensionHandler, ExtensionMessage, ExtensionSender};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::VtcBackend;
use crate::ephemeral::{VtcEphemeralSubscription, should_deliver};
use limit::{Limiter, Window, sleep_until};

/// The `@type` of the message that enables ephemeral VTC events.
const ENABLE: &str = "VTCStreamEnable";
/// The `@type` of the message that disables them.
const DISABLE: &str = "VTCStreamDisable";

/// One event published to a [`VtcEventHub`], with the user it is for
/// when it is addressed.
#[derive(Debug, Clone)]
struct Published {
    user: Option<String>,
    event: VtcEvent,
}

/// The server-wide source of VTC ephemeral events: the deployment
/// publishes each event here, and every connection's
/// [`VtcStreamHandler`] decides whether its client receives it.
///
/// The hub keeps the last `capacity` events for connections that fall
/// behind; a connection that falls further behind skips the events it
/// missed. Clones share the hub.
#[derive(Clone)]
pub struct VtcEventHub {
    tx: broadcast::Sender<Published>,
}

impl fmt::Debug for VtcEventHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VtcEventHub")
            .field("connections", &self.tx.receiver_count())
            .finish()
    }
}

impl VtcEventHub {
    /// A hub that buffers `capacity` events per slow connection.
    ///
    /// # Panics
    ///
    /// If `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Offer a per-call event to every connection: it reaches the current
    /// participants of its call that the subscriptions cover
    /// ([`should_deliver`]).
    ///
    /// The per-user `VTCRingEvent` and `VTCCallEndEvent` are "delivered
    /// based on the authenticated user's identity" (§Event Types); the
    /// hub cannot tell whom they are for, so published here they reach
    /// no one. Use [`publish_to_user`](Self::publish_to_user).
    pub fn publish(&self, event: VtcEvent) {
        let _ = self.tx.send(Published { user: None, event });
    }

    /// Offer `event` to the connections of the user `user_id` (as
    /// [`VtcBackend::caller_user_id`] returns it for the event's hosting
    /// account) only: a ring to each callee, a call end to each user it
    /// ends the ring or call for.
    pub fn publish_to_user(&self, user_id: impl Into<String>, event: VtcEvent) {
        let _ = self.tx.send(Published {
            user: Some(user_id.into()),
            event,
        });
    }
}

/// The outbound rate limits a [`VtcStreamHandler`] applies, per
/// connection. The default is the draft's (§Rate Limiting):
///
/// - `VTCMediaStateEvent`: "no more than one event per participant per
///   call per second. Events above this rate MAY be silently dropped; the
///   most recent state SHOULD be delivered when the rate window reopens."
///   The latest one over the limit is held and sent when the window
///   reopens, after the delivery checks run again.
/// - `VTCActiveSpeakerEvent`: "no more than two events per call per
///   second. Events above this rate MAY be silently dropped." Dropped.
///
/// `VTCRingEvent`'s limits are the ring rate limits of the VTC core
/// draft, applied where rings are created, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VtcStreamLimits {
    media_state: Option<Duration>,
    active_speaker: Option<Duration>,
}

impl Default for VtcStreamLimits {
    fn default() -> Self {
        Self {
            media_state: Some(Duration::from_secs(1)),
            active_speaker: Some(Duration::from_secs(1)),
        }
    }
}

impl VtcStreamLimits {
    /// The media-state window: at most one event per participant per call
    /// in it, the latest one over the limit sent when it reopens; `None`
    /// for no limit.
    pub fn media_state_window(mut self, window: Option<Duration>) -> Self {
        self.media_state = window;
        self
    }

    /// The active-speaker window: at most two events per call in it;
    /// `None` for no limit.
    pub fn active_speaker_window(mut self, window: Option<Duration>) -> Self {
        self.active_speaker = window;
        self
    }

    /// The media-state window.
    pub fn media_state_window_value(&self) -> Option<Duration> {
        self.media_state
    }

    /// The active-speaker window.
    pub fn active_speaker_window_value(&self) -> Option<Duration> {
        self.active_speaker
    }
}

/// The `"vtc"` switch of one connection's `ChatStreamEnable`
/// subscription, shared with that connection's [`VtcStreamHandler`].
///
/// While on, the handler delivers every VTC event the user may receive,
/// as a `VTCStreamEnable` with `callIds` and `eventTypes` both `null`
/// would, whatever the connection's own `VTCStreamEnable` subscription
/// is: "Subscribes to all VTC event types: per-user events for the user,
/// and per-call events for all calls in which the user is a current
/// participant" (draft-atwood-jmap-vtc-wss-00 §Interoperability with JMAP
/// Chat WebSocket). That covers every `VTCStreamEnable` subscription, so
/// it is the union the server "MUST deliver", and each event is decided
/// once: "if an event matches both subscriptions, it is delivered exactly
/// once" (§Subscription Union). `VTCStreamDisable` does not touch it, and
/// turning it off leaves the `VTCStreamEnable` subscription in place
/// (§Subscription Independence).
///
/// Set it to `jmap_chat_server::EphemeralSubscription::vtc` after every
/// change of the Chat subscription; `jmap-chat-vtc-server` does.
#[derive(Debug, Clone, Default)]
pub struct VtcChatAlias {
    on: Arc<AtomicBool>,
}

impl VtcChatAlias {
    /// Switch the `"vtc"` path on or off.
    pub fn set(&self, on: bool) {
        self.on.store(on, Ordering::SeqCst);
    }

    /// Whether the `"vtc"` path is on.
    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }
}

/// What the forwarding task needs.
struct Shared<B: VtcBackend> {
    backend: Arc<B>,
    caller: B::CallerCtx,
    recipient_account_id: Id,
    subscription: Mutex<VtcEphemeralSubscription>,
    alias: VtcChatAlias,
    /// The subscription of the `"vtc"` path: everything.
    every: VtcEphemeralSubscription,
}

impl<B: VtcBackend> Shared<B> {
    fn subscription(&self) -> VtcEphemeralSubscription {
        self.subscription
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether this connection receives `published` now. A backend error
    /// drops it.
    async fn admits(&self, published: &Published) -> bool {
        let event = &published.event;
        let Some(hosting) = hosting_account(event) else {
            return false;
        };
        let per_user = matches!(event, VtcEvent::Ring(_) | VtcEvent::CallEnd(_));
        match &published.user {
            Some(user) => match self.backend.caller_user_id(&self.caller, hosting).await {
                Ok(me) if me == *user => {}
                _ => return false,
            },
            None if per_user => return false,
            None => {}
        }
        let own;
        let subscription = if self.alias.is_on() {
            &self.every
        } else {
            own = self.subscription();
            &own
        };
        should_deliver(
            &*self.backend,
            &self.caller,
            hosting,
            &self.recipient_account_id,
            subscription,
            event,
        )
        .await
        .unwrap_or(false)
    }
}

/// The event's `accountId`, its call's hosting account; `None` for an
/// unrecognized `@type`.
fn hosting_account(event: &VtcEvent) -> Option<&Id> {
    Some(match event {
        VtcEvent::Ring(e) => &e.account_id,
        VtcEvent::CallEnd(e) => &e.account_id,
        VtcEvent::GatewaySignal(e) => &e.account_id,
        VtcEvent::Participant(e) => &e.account_id,
        VtcEvent::MediaState(e) => &e.account_id,
        VtcEvent::ActiveSpeaker(e) => &e.account_id,
        VtcEvent::UnmuteRequest(e) => &e.account_id,
        VtcEvent::RecordingState(e) => &e.account_id,
        _ => return None,
    })
}

/// The `VTCStreamEnable` / `VTCStreamDisable` handler of one WebSocket
/// connection; see the [module docs](self).
///
/// - `VTCStreamEnable` replaces the subscription through
///   [`VtcEphemeralSubscription::replace_with`]. Its error becomes the
///   `RequestError` (`invalidArguments`, with the message's `id` as
///   `requestId`) and leaves the subscription unchanged. A malformed
///   message is `notRequest`.
/// - `VTCStreamDisable` clears it, silently, whether or not one was
///   active. The `"vtc"` path of `ChatStreamEnable` is unaffected.
/// - Closing the connection clears it and stops delivery: "the
///   subscription is immediately cancelled. No events are buffered for
///   later delivery" (§Subscription Lifecycle).
pub struct VtcStreamHandler<B: VtcBackend> {
    shared: Arc<Shared<B>>,
    hub: VtcEventHub,
    limits: VtcStreamLimits,
    task: Option<JoinHandle<()>>,
}

impl<B: VtcBackend> fmt::Debug for VtcStreamHandler<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VtcStreamHandler")
            .field("recipient_account_id", &self.shared.recipient_account_id)
            .field("subscription", &self.shared.subscription())
            .field("chat_alias", &self.shared.alias.is_on())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<B: VtcBackend> VtcStreamHandler<B> {
    /// The handler of a connection authenticated as `caller`, whose own
    /// account is `recipient_account_id` (the account whose Chat
    /// capability and contact list the blocked-sender checks consult, as
    /// [`should_deliver`] takes it), receiving events from `hub`.
    ///
    /// The default [`VtcStreamLimits`] apply.
    pub fn new(
        backend: Arc<B>,
        caller: B::CallerCtx,
        recipient_account_id: Id,
        hub: &VtcEventHub,
    ) -> Self {
        let mut every = VtcEphemeralSubscription::default();
        every
            .replace_with(&VtcStreamEnable::default())
            .expect("a VTCStreamEnable with null eventTypes is always valid");
        Self {
            shared: Arc::new(Shared {
                backend,
                caller,
                recipient_account_id,
                subscription: Mutex::new(VtcEphemeralSubscription::default()),
                alias: VtcChatAlias::default(),
                every,
            }),
            hub: hub.clone(),
            limits: VtcStreamLimits::default(),
            task: None,
        }
    }

    /// Replace the outbound rate limits.
    pub fn with_limits(mut self, limits: VtcStreamLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The `"vtc"` switch of this connection's `ChatStreamEnable`
    /// subscription; off until set.
    pub fn chat_alias(&self) -> VtcChatAlias {
        self.shared.alias.clone()
    }

    /// The connection's current `VTCStreamEnable` subscription.
    pub fn subscription(&self) -> VtcEphemeralSubscription {
        self.shared.subscription()
    }

    fn set(&self, subscription: VtcEphemeralSubscription) {
        *self
            .shared
            .subscription
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = subscription;
    }

    fn enable(&self, message: &ExtensionMessage) -> Result<(), JmapError> {
        let enable: VtcStreamEnable = message.deserialize()?;
        let mut next = self.subscription();
        next.replace_with(&enable)?;
        self.set(next);
        Ok(())
    }
}

impl<B: VtcBackend> ExtensionHandler for VtcStreamHandler<B> {
    fn message_types(&self) -> &[&str] {
        &[ENABLE, DISABLE]
    }

    fn start(&mut self, events: ExtensionSender) {
        let rx = self.hub.tx.subscribe();
        self.task = Some(tokio::spawn(forward(
            Arc::clone(&self.shared),
            self.limits,
            rx,
            events,
        )));
    }

    async fn handle(&mut self, message: ExtensionMessage) -> Result<(), JmapError> {
        if message.message_type == ENABLE {
            self.enable(&message)
        } else {
            self.set(VtcEphemeralSubscription::default());
            Ok(())
        }
    }

    async fn close(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.set(VtcEphemeralSubscription::default());
    }
}

impl<B: VtcBackend> Drop for VtcStreamHandler<B> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Media-state limiter key: (accountId, callId, participantId).
type MediaKey = (Id, Id, Id);
/// Active-speaker limiter key: (accountId, callId).
type SpeakerKey = (Id, Id);

/// Delivers the hub's events to one connection until it closes.
async fn forward<B: VtcBackend>(
    shared: Arc<Shared<B>>,
    limits: VtcStreamLimits,
    mut rx: broadcast::Receiver<Published>,
    events: ExtensionSender,
) {
    let mut media: Limiter<MediaKey, Published> =
        Limiter::new(limits.media_state.map(|per| Window {
            max: 1,
            per,
            keep_latest: true,
        }));
    let mut speaker: Limiter<SpeakerKey, ()> =
        Limiter::new(limits.active_speaker.map(|per| Window {
            max: 2,
            per,
            keep_latest: false,
        }));
    loop {
        tokio::select! {
            () = events.closed() => return,
            () = sleep_until(media.next_due()) => {
                for (_, held) in media.take_due(Instant::now()) {
                    if shared.admits(&held).await {
                        send(&events, &held.event);
                    }
                }
            }
            received = rx.recv() => match received {
                Ok(published) => {
                    if !shared.admits(&published).await {
                        continue;
                    }
                    let now = Instant::now();
                    let pass = match &published.event {
                        VtcEvent::MediaState(e) => {
                            let key = (e.account_id.clone(), e.call_id.clone(), e.participant_id.clone());
                            media.offer(key, published.clone(), now).is_some()
                        }
                        VtcEvent::ActiveSpeaker(e) => {
                            let key = (e.account_id.clone(), e.call_id.clone());
                            speaker.offer(key, (), now).is_some()
                        }
                        _ => true,
                    };
                    if pass {
                        send(&events, &published.event);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
        }
    }
}

/// Queues `event`, dropping it if the client is too slow.
fn send(events: &ExtensionSender, event: &VtcEvent) {
    let _ = events.try_send(event);
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;
    use crate::CallScope;
    use crate::memory::MemoryBackend;
    use serde_json::{Value, json};

    fn handler() -> VtcStreamHandler<MemoryBackend> {
        VtcStreamHandler::new(
            Arc::new(MemoryBackend::new()),
            Id::from("user:alice@example.com"),
            Id::from("a1"),
            &VtcEventHub::new(8),
        )
    }

    fn message(ty: &str, body: Value) -> ExtensionMessage {
        let Value::Object(body) = body else {
            panic!("object")
        };
        ExtensionMessage::new(ty, body)
    }

    #[test]
    fn registers_the_two_control_messages() {
        assert_eq!(
            handler().message_types(),
            ["VTCStreamEnable", "VTCStreamDisable"]
        );
    }

    /// §VTCStreamEnable: a new message "replaces the previous ephemeral
    /// subscription in its entirety"; §VTCStreamDisable "MUST silently
    /// succeed if no ephemeral VTC subscription is currently active".
    #[tokio::test]
    async fn enable_replaces_and_disable_clears() {
        let mut h = handler();
        h.handle(message(ENABLE, json!({"callIds": ["c1"]})))
            .await
            .unwrap();
        let filter = h.subscription().filter().cloned().unwrap();
        assert_eq!(filter.call_ids(), &CallScope::Only(vec![Id::from("c1")]));
        h.handle(message(ENABLE, json!({}))).await.unwrap();
        assert_eq!(
            h.subscription().filter().unwrap().call_ids(),
            &CallScope::All
        );
        h.handle(message(DISABLE, json!({}))).await.unwrap();
        assert!(h.subscription().filter().is_none());
        h.handle(message(DISABLE, json!({}))).await.unwrap();
    }

    /// §VTCStreamEnable: only unrecognized `eventTypes` is
    /// `invalidArguments` and "MUST NOT update the current ephemeral
    /// subscription state".
    #[tokio::test]
    async fn invalid_event_types_leave_the_subscription_alone() {
        let mut h = handler();
        h.handle(message(ENABLE, json!({"eventTypes": ["VTCRingEvent"]})))
            .await
            .unwrap();
        let err = h
            .handle(message(ENABLE, json!({"eventTypes": ["Bogus"]})))
            .await
            .unwrap_err();
        assert_eq!(err.error_type.as_str(), "invalidArguments");
        assert!(h.subscription().filter().unwrap().includes("VTCRingEvent"));
        let err = h
            .handle(message(ENABLE, json!({"callIds": "c1"})))
            .await
            .unwrap_err();
        assert_eq!(err.error_type.as_str(), "notRequest");
    }

    /// §Subscription Independence: "`VTCStreamDisable` cancels only the
    /// VTC-specific subscription ... It does not affect VTC events
    /// delivered through `ChatStreamEnable`."
    #[tokio::test]
    async fn disable_leaves_the_chat_alias_alone() {
        let mut h = handler();
        let alias = h.chat_alias();
        alias.set(true);
        h.handle(message(DISABLE, json!({}))).await.unwrap();
        assert!(alias.is_on());
        assert!(h.chat_alias().is_on());
    }

    #[test]
    fn default_limits_are_the_drafts() {
        let l = VtcStreamLimits::default();
        assert_eq!(l.media_state_window_value(), Some(Duration::from_secs(1)));
        assert_eq!(
            l.active_speaker_window_value(),
            Some(Duration::from_secs(1))
        );
        assert_eq!(l.media_state_window(None).media_state_window_value(), None);
    }
}
