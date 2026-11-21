//! `ChatStreamEnable` / `ChatStreamDisable` and the ephemeral events they
//! turn on, as a [`jmap_websocket`] extension (draft-atwood-jmap-chat-wss-00
//! §Ephemeral Chat Events).
//!
//! - The deployment keeps one [`ChatEventHub`] and publishes every
//!   `ChatTypingEvent` and `ChatPresenceEvent` to it, one per recipient
//!   account (the event's `accountId`).
//! - For each WebSocket connection it builds a [`ChatStreamHandler`] with
//!   the connection's caller and accounts, and registers it with
//!   [`Connection::with_extension`](jmap_websocket::server::Connection::with_extension)
//!   next to advertising `urn:ietf:params:jmap:chat:websocket`.
//!
//! The handler keeps the connection's [`EphemeralSubscription`]. For each
//! published event it checks, at delivery time, the subscription's
//! `chatIds` / `contactIds` scope, [`should_deliver_typing`] or
//! [`project_presence`], and the draft's rate limits
//! ([`ChatStreamLimits`]), then queues the frame with
//! [`ExtensionSender::try_send`], which drops it if the client is too
//! slow ("events above this rate MAY be silently dropped").
//!
//! Credential expiry is the deployment's: "A server that does not close
//! the connection when credentials expire SHOULD NOT deliver further
//! ephemeral events on that connection" (§Authentication and Identity).
//! The handler does not know when credentials expire; close the
//! connection then.
//!
//! ```
//! use std::sync::Arc;
//! use jmap_chat_server::ChatBackend;
//! use jmap_chat_server::websocket::{ChatEventHub, ChatStreamHandler};
//! use jmap_types::Id;
//! use jmap_websocket::server::{Connection, DispatcherHandler};
//!
//! /// Called once per accepted connection; `hub` is the server's.
//! fn wire<B: ChatBackend<CallerCtx = ()>>(
//!     backend: Arc<B>,
//!     hub: &ChatEventHub,
//!     connection: Connection<DispatcherHandler<()>>,
//! ) -> Connection<DispatcherHandler<()>> {
//!     let chat = ChatStreamHandler::new(backend, (), [Id::from("acct-owner")], hub);
//!     connection.with_extension(chat)
//! }
//! ```

mod limit;

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use jmap_chat_types::{ChatPresenceEvent, ChatStreamEnable, ChatTypingEvent, EphemeralMessage};
use jmap_types::{Id, JmapError};
use jmap_websocket::server::{ExtensionHandler, ExtensionMessage, ExtensionSender};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::ChatBackend;
use crate::ephemeral::{
    CompanionStreams, EphemeralSubscription, project_presence, should_deliver_typing,
};
use limit::{Limiter, Window, sleep_until};

/// The `@type` of the message that enables ephemeral Chat events.
const ENABLE: &str = "ChatStreamEnable";
/// The `@type` of the message that disables them.
const DISABLE: &str = "ChatStreamDisable";

/// One event published to a [`ChatEventHub`].
#[derive(Debug, Clone)]
enum ChatEvent {
    Typing(ChatTypingEvent),
    Presence(ChatPresenceEvent),
}

/// The server-wide source of ephemeral Chat events: the deployment
/// publishes each event here once, and every connection's
/// [`ChatStreamHandler`] decides whether its client receives it.
///
/// Publish one event per recipient account, with that account as
/// `accountId`: for typing, the account holding the recipient's Chat
/// record and `senderId` as the recipient sees it; for presence, the
/// account holding the recipient's `ChatContact`. A connection takes the
/// events whose `accountId` is one of its accounts.
///
/// The hub keeps the last `capacity` events for connections that fall
/// behind; a connection that falls further behind skips the events it
/// missed, which the draft allows for ephemeral events. Clones share the
/// hub.
#[derive(Clone)]
pub struct ChatEventHub {
    tx: broadcast::Sender<ChatEvent>,
}

impl fmt::Debug for ChatEventHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatEventHub")
            .field("connections", &self.tx.receiver_count())
            .finish()
    }
}

impl ChatEventHub {
    /// A hub that buffers `capacity` events per slow connection.
    ///
    /// # Panics
    ///
    /// If `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Offer a `ChatTypingEvent` to every connection. With none
    /// connected it is dropped.
    pub fn publish_typing(&self, event: ChatTypingEvent) {
        let _ = self.tx.send(ChatEvent::Typing(event));
    }

    /// Offer a `ChatPresenceEvent` to every connection. With none
    /// connected it is dropped.
    pub fn publish_presence(&self, event: ChatPresenceEvent) {
        let _ = self.tx.send(ChatEvent::Presence(event));
    }
}

/// The outbound rate limits a [`ChatStreamHandler`] applies, per
/// connection.
///
/// The default is the draft's:
///
/// - typing: "no more than one `ChatTypingEvent` per sender per chat per
///   3 seconds SHOULD be delivered; events above this rate MAY be
///   silently dropped" (draft-atwood-jmap-chat-wss-00 §`ChatTypingEvent`).
///   Events over the limit are dropped, except `typing: false`, which is
///   always delivered and does not count against the window, so a client
///   never misses a stop (decision 66).
/// - presence: "no more than one `ChatPresenceEvent` per contact per 30
///   seconds SHOULD be delivered; events above this rate MAY be silently
///   dropped, but the most recent presence state SHOULD be delivered when
///   the rate window reopens" (§`ChatPresenceEvent`). The latest event over
///   the limit is held and sent when the window reopens, after the
///   delivery checks run again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatStreamLimits {
    typing: Option<Duration>,
    presence: Option<Duration>,
}

impl Default for ChatStreamLimits {
    fn default() -> Self {
        Self {
            typing: Some(Duration::from_secs(3)),
            presence: Some(Duration::from_secs(30)),
        }
    }
}

impl ChatStreamLimits {
    /// The typing window: at most one event per sender per Chat in it;
    /// `None` for no limit.
    pub fn typing_window(mut self, window: Option<Duration>) -> Self {
        self.typing = window;
        self
    }

    /// The presence window: at most one event per contact in it, the
    /// latest one over the limit sent when it reopens; `None` for no
    /// limit.
    pub fn presence_window(mut self, window: Option<Duration>) -> Self {
        self.presence = window;
        self
    }

    /// The typing window.
    pub fn typing_window_value(&self) -> Option<Duration> {
        self.typing
    }

    /// The presence window.
    pub fn presence_window_value(&self) -> Option<Duration> {
        self.presence
    }
}

/// Called with the connection's subscription after it changes.
type Observer = Box<dyn FnMut(&EphemeralSubscription) + Send>;

/// What the forwarding task needs.
struct Shared<B: ChatBackend> {
    backend: Arc<B>,
    caller: B::CallerCtx,
    accounts: HashSet<Id>,
    subscription: Mutex<EphemeralSubscription>,
}

impl<B: ChatBackend> Shared<B> {
    fn subscription(&self) -> EphemeralSubscription {
        self.subscription
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether this connection receives `event` now. A backend error
    /// drops it.
    async fn typing(&self, event: &ChatTypingEvent) -> bool {
        let covered = self
            .subscription()
            .typing()
            .is_some_and(|scope| scope.covers(&event.chat_id));
        covered
            && self.accounts.contains(&event.account_id)
            && should_deliver_typing(&*self.backend, &self.caller, &event.account_id, event)
                .await
                .unwrap_or(false)
    }

    /// The frame this connection receives for `event` now, if any. A
    /// backend error drops it.
    async fn presence(&self, event: &ChatPresenceEvent) -> Option<ChatPresenceEvent> {
        let covered = self
            .subscription()
            .presence()
            .is_some_and(|scope| scope.covers(&event.contact_id));
        if !covered || !self.accounts.contains(&event.account_id) {
            return None;
        }
        project_presence(&*self.backend, &self.caller, &event.account_id, event)
            .await
            .ok()
            .flatten()
    }
}

/// The `ChatStreamEnable` / `ChatStreamDisable` handler of one WebSocket
/// connection; see the [module docs](self).
///
/// - `ChatStreamEnable` replaces the subscription through
///   [`EphemeralSubscription::replace_with`] with the
///   [`CompanionStreams`] given to
///   [`with_companions`](Self::with_companions). Its error becomes the
///   `RequestError` (`invalidArguments`, with the message's `id` as
///   `requestId`) and leaves the subscription unchanged. A malformed
///   message is `notRequest`.
/// - `ChatStreamDisable` clears it, silently, whether or not one was
///   active.
/// - Closing the connection clears it and stops delivery.
///
/// The companion values `"vtc"` and `"scene"` only switch the companion
/// handler's alias on: register
/// [`on_subscription_change`](Self::on_subscription_change) observers for
/// them (`jmap-chat-vtc-server` and `jmap-chat-scene-server` do).
pub struct ChatStreamHandler<B: ChatBackend> {
    shared: Arc<Shared<B>>,
    hub: ChatEventHub,
    companions: CompanionStreams,
    limits: ChatStreamLimits,
    observers: Vec<Observer>,
    task: Option<JoinHandle<()>>,
}

impl<B: ChatBackend> fmt::Debug for ChatStreamHandler<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatStreamHandler")
            .field("accounts", &self.shared.accounts)
            .field("subscription", &self.shared.subscription())
            .field("companions", &self.companions)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<B: ChatBackend> ChatStreamHandler<B> {
    /// The handler of a connection authenticated as `caller`, receiving
    /// the events of `accounts` (the accounts whose Chats and
    /// `ChatContact`s the owner sees, usually the owner's own) from `hub`.
    ///
    /// No companion value is recognized and the default
    /// [`ChatStreamLimits`] apply.
    pub fn new(
        backend: Arc<B>,
        caller: B::CallerCtx,
        accounts: impl IntoIterator<Item = Id>,
        hub: &ChatEventHub,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                caller,
                accounts: accounts.into_iter().collect(),
                subscription: Mutex::new(EphemeralSubscription::default()),
            }),
            hub: hub.clone(),
            companions: CompanionStreams::default(),
            limits: ChatStreamLimits::default(),
            observers: Vec::new(),
            task: None,
        }
    }

    /// The companion WebSocket capabilities the server advertises, which
    /// decide whether `"vtc"` and `"scene"` in `dataTypes` are recognized.
    pub fn with_companions(mut self, companions: CompanionStreams) -> Self {
        self.companions = companions;
        self
    }

    /// The companion capabilities in effect.
    pub fn companions(&self) -> CompanionStreams {
        self.companions
    }

    /// Replace the outbound rate limits.
    pub fn with_limits(mut self, limits: ChatStreamLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Call `observer` with the subscription each time it changes: after
    /// a successful `ChatStreamEnable`, after `ChatStreamDisable`, and
    /// with the empty subscription when the connection closes. Observers
    /// run in the order added, before the next client message is read.
    pub fn on_subscription_change(
        mut self,
        observer: impl FnMut(&EphemeralSubscription) + Send + 'static,
    ) -> Self {
        self.observers.push(Box::new(observer));
        self
    }

    /// The connection's current subscription.
    pub fn subscription(&self) -> EphemeralSubscription {
        self.shared.subscription()
    }

    /// Replace the subscription and tell the observers.
    fn set(&mut self, subscription: EphemeralSubscription) {
        *self
            .shared
            .subscription
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = subscription.clone();
        for observer in &mut self.observers {
            observer(&subscription);
        }
    }

    fn enable(&mut self, message: &ExtensionMessage) -> Result<(), JmapError> {
        let enable: ChatStreamEnable = message.deserialize()?;
        let mut next = self.subscription();
        next.replace_with(&enable, self.companions)?;
        self.set(next);
        Ok(())
    }
}

impl<B: ChatBackend> ExtensionHandler for ChatStreamHandler<B> {
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
            self.set(EphemeralSubscription::default());
            Ok(())
        }
    }

    async fn close(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.set(EphemeralSubscription::default());
    }
}

impl<B: ChatBackend> Drop for ChatStreamHandler<B> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Typing limiter key: (accountId, chatId, senderId).
type TypingKey = (Id, Id, Id);
/// Presence limiter key: (accountId, contactId).
type PresenceKey = (Id, Id);

/// Delivers the hub's events to one connection until it closes.
async fn forward<B: ChatBackend>(
    shared: Arc<Shared<B>>,
    limits: ChatStreamLimits,
    mut rx: broadcast::Receiver<ChatEvent>,
    events: ExtensionSender,
) {
    let window = |per, keep_latest| {
        Some(Window {
            max: 1,
            per,
            keep_latest,
        })
    };
    let mut typing: Limiter<TypingKey, ()> =
        Limiter::new(limits.typing.and_then(|p| window(p, false)));
    let mut presence: Limiter<PresenceKey, ChatPresenceEvent> =
        Limiter::new(limits.presence.and_then(|p| window(p, true)));
    loop {
        tokio::select! {
            () = events.closed() => return,
            () = sleep_until(presence.next_due()) => {
                for (_, held) in presence.take_due(Instant::now()) {
                    if let Some(frame) = shared.presence(&held).await {
                        send(&events, EphemeralMessage::Presence(frame));
                    }
                }
            }
            received = rx.recv() => match received {
                Ok(ChatEvent::Typing(event)) => {
                    if shared.typing(&event).await {
                        let key = (
                            event.account_id.clone(),
                            event.chat_id.clone(),
                            event.sender_id.clone(),
                        );
                        // decision 66: a `typing: false` is always delivered
                        // and does not count against the window.
                        if !event.typing || typing.offer(key, (), Instant::now()).is_some() {
                            send(&events, EphemeralMessage::Typing(event));
                        }
                    }
                }
                Ok(ChatEvent::Presence(event)) => {
                    if let Some(frame) = shared.presence(&event).await {
                        let key = (event.account_id.clone(), event.contact_id.clone());
                        if presence.offer(key, event, Instant::now()).is_some() {
                            send(&events, EphemeralMessage::Presence(frame));
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
        }
    }
}

/// Queues `frame`, dropping it if the client is too slow.
fn send(events: &ExtensionSender, frame: EphemeralMessage) {
    let _ = events.try_send(&frame);
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;
    use crate::ephemeral::IdScope;
    use crate::memory::MemoryBackend;
    use serde_json::{Value, json};

    fn handler() -> ChatStreamHandler<MemoryBackend<()>> {
        let hub = ChatEventHub::new(8);
        ChatStreamHandler::new(
            Arc::new(MemoryBackend::default()),
            (),
            [Id::from("a1")],
            &hub,
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
            ["ChatStreamEnable", "ChatStreamDisable"]
        );
    }

    /// §ChatStreamEnable: the subscription replaces the previous one in
    /// its entirety; ChatStreamDisable clears it.
    #[tokio::test]
    async fn enable_replaces_and_disable_clears() {
        let mut h = handler();
        h.handle(message(
            ENABLE,
            json!({"dataTypes": ["typing"], "chatIds": ["c1"]}),
        ))
        .await
        .unwrap();
        assert_eq!(
            h.subscription().typing(),
            Some(&IdScope::Only(vec![Id::from("c1")]))
        );
        h.handle(message(ENABLE, json!({"dataTypes": ["presence"]})))
            .await
            .unwrap();
        assert_eq!(h.subscription().typing(), None);
        assert_eq!(h.subscription().presence(), Some(&IdScope::All));
        h.handle(message(DISABLE, json!({}))).await.unwrap();
        assert_eq!(h.subscription(), EphemeralSubscription::default());
        // Silently succeeds with nothing active.
        h.handle(message(DISABLE, json!({}))).await.unwrap();
    }

    /// §ChatStreamEnable: only unrecognized dataTypes is invalidArguments
    /// and MUST NOT update the subscription; "vtc" is unrecognized unless
    /// the companion capability is advertised.
    #[tokio::test]
    async fn unrecognized_data_types_leave_the_subscription_alone() {
        let mut h = handler();
        h.handle(message(ENABLE, json!({"dataTypes": ["typing"]})))
            .await
            .unwrap();
        let err = h
            .handle(message(ENABLE, json!({"id": "e1", "dataTypes": ["vtc"]})))
            .await
            .unwrap_err();
        assert_eq!(err.error_type.as_str(), "invalidArguments");
        assert_eq!(h.subscription().typing(), Some(&IdScope::All));
    }

    #[tokio::test]
    async fn a_malformed_enable_is_not_request() {
        let mut h = handler();
        let err = h
            .handle(message(ENABLE, json!({"dataTypes": "typing"})))
            .await
            .unwrap_err();
        assert_eq!(err.error_type.as_str(), "notRequest");
    }

    #[tokio::test]
    async fn observers_see_every_change_and_the_close() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let mut h = handler()
            .with_companions(CompanionStreams {
                vtc: true,
                ..CompanionStreams::default()
            })
            .on_subscription_change(move |s| log.lock().unwrap().push(s.vtc()));
        h.handle(message(ENABLE, json!({"dataTypes": ["vtc"]})))
            .await
            .unwrap();
        // A failed enable does not notify.
        let _ = h.handle(message(ENABLE, json!({"dataTypes": []}))).await;
        h.handle(message(DISABLE, json!({}))).await.unwrap();
        h.handle(message(ENABLE, json!({"dataTypes": ["vtc"]})))
            .await
            .unwrap();
        h.close().await;
        assert_eq!(*seen.lock().unwrap(), [true, false, true, false]);
    }

    #[test]
    fn default_limits_are_the_drafts() {
        let l = ChatStreamLimits::default();
        assert_eq!(l.typing_window_value(), Some(Duration::from_secs(3)));
        assert_eq!(l.presence_window_value(), Some(Duration::from_secs(30)));
        let l = l.typing_window(None);
        assert_eq!(l.typing_window_value(), None);
    }
}
