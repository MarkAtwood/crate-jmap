//! `SceneStreamEnable` / `SceneStreamDisable`, the Scene ephemeral events
//! they turn on, and client-sent `SceneInteractionEvent` frames, as a
//! [`jmap_websocket`] extension (draft-atwood-jmap-scene-wss-00).
//!
//! - The deployment keeps one [`SceneEventHub`] and publishes every Scene
//!   event to it with [`SceneEventHub::publish`]: avatar and object
//!   events, the interactions accepted by `SceneObject/interact`, and the
//!   region events accepted by `SceneRegion/broadcast` or originated by
//!   the server. Every event carries `accountId`, "the id of the region's hosting
//!   account" (§Ephemeral Event Types).
//! - For each WebSocket connection it builds a [`SceneStreamHandler`] with
//!   the connection's caller and the caller's own account, and registers
//!   it with
//!   [`Connection::with_extension`](jmap_websocket::server::Connection::with_extension)
//!   next to advertising `urn:ietf:params:jmap:scene:websocket`.
//!
//! For each published event the handler asks [`should_deliver`], at
//! delivery time, with the event's `accountId` as the hosting account,
//! applies the draft's rate limit on object updates
//! ([`SceneStreamLimits`]), and queues the frame with
//! [`ExtensionSender::try_send`], which drops it if the client is too
//! slow.
//!
//! A `SceneInteractionEvent` frame from the client goes through
//! [`validate_interaction`] and the hub's per-user rate limit, and is then
//! published to the hub for every connection, the sender's included, to
//! decide on. A frame that fails any check, or does not parse, is
//! discarded: "the server discards the frame and returns no error to the
//! sender" (§Submitting Interactions).
//!
//! `ChatStreamEnable` with `"scene"` in `dataTypes` is a second path to
//! the same events (§Interoperability with JMAP Chat WebSocket). The Chat
//! handler switches it through the [`SceneChatAlias`] of this handler
//! (`jmap-chat-scene-server` wires the two), so both paths are decided
//! here, once per event: the union is delivered without duplicates.
//!
//! Observer subscriptions (`observe: true`, jmap-chat-spec 93f2258) are
//! recognized when [`SceneBackend::supports_observe`] is `true`, the
//! server advertising `{"supportsObserve": true}`; otherwise `observe` is
//! ignored, as the draft requires. An observing `SceneStreamEnable` needs
//! `regionIds` (`invalidArguments` without), looks each listed region up
//! in [`SceneBackend::observer_accounts`], and fails with a `forbidden`
//! `RequestError` whose `regionIds` member lists the accessible regions
//! that refuse observers (`allowObservers` `false`, or
//! [`SceneBackend::may_observe_region`]), leaving the subscription as it
//! was.
//!
//! For `observerCount` the handler reports to
//! [`SceneBackend::set_observing`] every `(account, region)` pair its
//! observer subscription covers: each listed region in each of the
//! [`SceneBackend::observer_accounts`] when the subscription is enabled,
//! whether or not the user has access to it then, and, from the first
//! event for it that reaches the handler, each listed region of any
//! other account ("delivery is not limited to those accounts"). It
//! releases a pair when a later `SceneStreamEnable`, `SceneStreamDisable`
//! or the connection's close stops covering it. Which of the reported
//! users count (access, `allowObservers`, policy, an active avatar) the
//! backend decides, as delivery does, so the count follows delivery.
//!
//! Credential expiry is the deployment's (§Credential Expiry): the
//! handler does not know when credentials expire; close the connection
//! then.
//!
//! ```
//! use std::sync::Arc;
//! use jmap_scene_server::SceneBackend;
//! use jmap_scene_server::websocket::{SceneEventHub, SceneStreamHandler};
//! use jmap_types::Id;
//! use jmap_websocket::server::{Connection, DispatcherHandler};
//!
//! /// Called once per accepted connection; `hub` is the server's.
//! fn wire<B: SceneBackend<CallerCtx = ()>>(
//!     backend: Arc<B>,
//!     hub: &SceneEventHub,
//!     connection: Connection<DispatcherHandler<()>>,
//! ) -> Connection<DispatcherHandler<()>> {
//!     let scene = SceneStreamHandler::new(backend, (), Id::from("acct-alice"), hub);
//!     connection.with_extension(scene)
//! }
//! ```

mod limit;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use jmap_scene_types::{ObjectEventKind, SceneEvent, SceneInteractionEvent, SceneStreamEnable};
use jmap_types::{Id, JmapError};
use jmap_websocket::server::{ExtensionHandler, ExtensionMessage, ExtensionSender};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::SceneBackend;
use crate::SceneStreamFilter;
use crate::ephemeral::{
    SceneEphemeralSubscription, observer_regions, should_deliver, validate_interaction,
};
use limit::{Limiter, Window, sleep_until};

/// The `@type` of the message that enables ephemeral Scene events.
const ENABLE: &str = "SceneStreamEnable";
/// The `@type` of the message that disables them.
const DISABLE: &str = "SceneStreamDisable";
/// The `@type` of a client-submitted interaction.
const INTERACTION: &str = "SceneInteractionEvent";

/// Interaction limiter key: (`accountId`, `regionId`, `userId`).
type InteractionKey = (Id, Id, String);

/// The server-wide source of Scene ephemeral events: the deployment
/// publishes each event here, and every connection's
/// [`SceneStreamHandler`] decides whether its client receives it.
///
/// The hub also holds the per-user interaction rate limit, so that it
/// counts a user's frames over all of that user's connections:
/// "`SceneInteractionEvent`: no more than five events per user per region
/// per second. Frames above this rate MAY be silently dropped"
/// (§Rate Limiting). Frames over it are dropped.
///
/// The hub keeps the last `capacity` events for connections that fall
/// behind; a connection that falls further behind skips the events it
/// missed. Clones share the hub.
#[derive(Clone)]
pub struct SceneEventHub {
    tx: broadcast::Sender<SceneEvent>,
    interactions: Arc<Mutex<Limiter<InteractionKey, ()>>>,
}

impl fmt::Debug for SceneEventHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SceneEventHub")
            .field("connections", &self.tx.receiver_count())
            .finish_non_exhaustive()
    }
}

impl SceneEventHub {
    /// A hub that buffers `capacity` events per slow connection, with the
    /// draft's interaction limit of five per user per region per second.
    ///
    /// # Panics
    ///
    /// If `capacity` is 0.
    pub fn new(capacity: usize) -> Self {
        Self::with_interaction_window(capacity, Some(Duration::from_secs(1)))
    }

    /// As [`new`](Self::new), with at most five client-submitted
    /// interactions per user per region in `window`; `None` for no
    /// limit.
    ///
    /// # Panics
    ///
    /// If `capacity` is 0.
    pub fn with_interaction_window(capacity: usize, window: Option<Duration>) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        let rule = window.map(|per| Window {
            max: 5,
            per,
            keep_latest: false,
        });
        Self {
            tx,
            interactions: Arc::new(Mutex::new(Limiter::new(rule))),
        }
    }

    /// Offer `event` to every connection. With none connected it is
    /// dropped.
    pub fn publish(&self, event: SceneEvent) {
        let _ = self.tx.send(event);
    }

    /// Whether a client-submitted interaction keyed `key` is within the
    /// rate limit, counting it if so.
    fn admit_interaction(&self, key: InteractionKey) -> bool {
        self.interactions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .offer(key, (), Instant::now())
            .is_some()
    }
}

/// The outbound rate limit a [`SceneStreamHandler`] applies, per
/// connection. The default is the draft's (§Rate Limiting):
/// "`SceneObjectEvent` with `event: "updated"`: no more than two events
/// per object per second. Events above this rate MAY be silently dropped;
/// the most recent state SHOULD be delivered when the rate window
/// reopens." The latest one over the limit is held and sent when the
/// window reopens, after the delivery checks run again.
///
/// The interaction limit is per user, so it is the [`SceneEventHub`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneStreamLimits {
    object_update: Option<Duration>,
}

impl Default for SceneStreamLimits {
    fn default() -> Self {
        Self {
            object_update: Some(Duration::from_secs(1)),
        }
    }
}

impl SceneStreamLimits {
    /// The object-update window: at most two `"updated"` events per object
    /// in it, the latest one over the limit sent when it reopens; `None`
    /// for no limit.
    pub fn object_update_window(mut self, window: Option<Duration>) -> Self {
        self.object_update = window;
        self
    }

    /// The object-update window.
    pub fn object_update_window_value(&self) -> Option<Duration> {
        self.object_update
    }
}

/// The `"scene"` switch of one connection's `ChatStreamEnable`
/// subscription, shared with that connection's [`SceneStreamHandler`].
///
/// While on, the handler delivers every Scene event the user may
/// receive, as a `SceneStreamEnable` with `regionIds` and `eventTypes`
/// both `null` would: the `"scene"` path "Subscribes to all Scene event
/// types for all regions in which the user has an active avatar"
/// (draft-atwood-jmap-scene-wss-00 §Interoperability with JMAP Chat
/// WebSocket). That covers every `SceneStreamEnable` subscription
/// without `observe`. An observer subscription also reaches listed
/// regions without an avatar ("It has no observer form", jmap-chat-spec
/// 93f2258), so for an event the `"scene"` path does not admit the
/// handler still asks the connection's own observer subscription. Either
/// way the union the server "MUST deliver" is decided once per event:
/// "if an event matches both subscriptions, it is delivered exactly once"
/// (§Subscription Union).
/// `SceneStreamDisable` does not touch it, and turning it off leaves the
/// `SceneStreamEnable` subscription in place (§Subscription
/// Independence).
///
/// Set it to `jmap_chat_server::EphemeralSubscription::scene` after every
/// change of the Chat subscription; `jmap-chat-scene-server` does.
#[derive(Debug, Clone, Default)]
pub struct SceneChatAlias {
    on: Arc<AtomicBool>,
}

impl SceneChatAlias {
    /// Switch the `"scene"` path on or off.
    pub fn set(&self, on: bool) {
        self.on.store(on, Ordering::SeqCst);
    }

    /// Whether the `"scene"` path is on.
    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }
}

/// What the forwarding task needs.
struct Shared<B: SceneBackend> {
    backend: Arc<B>,
    caller: B::CallerCtx,
    recipient_account_id: Id,
    subscription: Mutex<SceneEphemeralSubscription>,
    alias: SceneChatAlias,
    /// The subscription of the `"scene"` path: everything.
    every: SceneEphemeralSubscription,
    /// The `(account, region)` pairs this connection reported observing
    /// with [`SceneBackend::set_observing`]: the pairs its observer
    /// subscription covers ([`Shared::observe`], [`Shared::observe_event`]).
    observing: tokio::sync::Mutex<Vec<(Id, Id)>>,
}

impl<B: SceneBackend> Shared<B> {
    fn subscription(&self) -> SceneEphemeralSubscription {
        self.subscription
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether this connection receives `event` now. A backend error
    /// drops it.
    ///
    /// With the `"scene"` path on, its everything-subscription decides,
    /// except that an event it refuses may still reach the connection's
    /// observer subscription, the one `SceneStreamEnable` the path does
    /// not cover.
    async fn admits(&self, event: &SceneEvent) -> bool {
        let Some(hosting) = hosting_account(event) else {
            return false;
        };
        let deliver = |subscription| {
            should_deliver(
                &*self.backend,
                &self.caller,
                hosting,
                &self.recipient_account_id,
                subscription,
                event,
            )
        };
        let own = self.subscription();
        if self.alias.is_on() {
            if deliver(&self.every).await.unwrap_or(false) {
                return true;
            }
            if !own.filter().is_some_and(SceneStreamFilter::observes) {
                return false;
            }
        }
        deliver(&own).await.unwrap_or(false)
    }

    /// Whether the connection's current observer subscription covers
    /// region `region_id`, in any account.
    fn observer_covers(&self, region_id: &Id) -> bool {
        self.subscription()
            .filter()
            .is_some_and(|f| f.observes() && f.region_ids().covers(region_id))
    }

    /// After the subscription changed: observe `listed`, the pairs a new
    /// observer subscription names in the observer accounts, keep the
    /// pairs learned from events ([`Self::observe_event`]) that the current
    /// subscription still covers, and release the rest, telling the
    /// backend of each pair started and stopped. Starts are reported
    /// first, so a region kept across two subscriptions never dips. A
    /// backend error is ignored: the count is advisory and the
    /// subscription stands.
    ///
    /// Call it after storing the new subscription: the coverage check
    /// reads it under the `observing` lock, as [`Self::observe_event`]
    /// does, so a pair that call adds concurrently is kept or released
    /// with the rest.
    async fn observe(&self, listed: Vec<(Id, Id)>) {
        let mut observing = self.observing.lock().await;
        let mut next = listed;
        for pair in observing.iter() {
            if !next.contains(pair) && self.observer_covers(&pair.1) {
                next.push(pair.clone());
            }
        }
        let previous = std::mem::replace(&mut *observing, next);
        for (account, region) in observing.iter().filter(|p| !previous.contains(p)) {
            let _ = self
                .backend
                .set_observing(&self.caller, account, region, true)
                .await;
        }
        for (account, region) in previous.iter().filter(|p| !observing.contains(p)) {
            let _ = self
                .backend
                .set_observing(&self.caller, account, region, false)
                .await;
        }
    }

    /// Before deciding on `event`: if the observer subscription covers its
    /// region and this connection has not yet reported its
    /// `(accountId, regionId)` pair, report it with
    /// [`SceneBackend::set_observing`]. That reaches the regions of
    /// accounts outside [`SceneBackend::observer_accounts`], whose events
    /// the subscription delivers too: "delivery is not limited to those
    /// accounts". The pair is reported whether or not `event` itself is
    /// delivered, since its `eventTypes` or the user's current access may
    /// refuse this one event; the backend counts only the users delivery
    /// reaches.
    ///
    /// The pair is recorded only once the backend call returns, so a
    /// forwarding task aborted during the call leaves no pair to release.
    async fn observe_event(&self, event: &SceneEvent) {
        let (Some(account), Some(region)) = (hosting_account(event), event_region(event)) else {
            return;
        };
        if !self.observer_covers(region) {
            return;
        }
        let mut observing = self.observing.lock().await;
        let pair = (account.clone(), region.clone());
        if observing.contains(&pair) || !self.observer_covers(region) {
            return;
        }
        let _ = self
            .backend
            .set_observing(&self.caller, account, region, true)
            .await;
        observing.push(pair);
    }
}

/// The event's `regionId`; `None` for an unrecognized `@type`.
fn event_region(event: &SceneEvent) -> Option<&Id> {
    Some(match event {
        SceneEvent::Avatar(e) => &e.region_id,
        SceneEvent::Object(e) => &e.region_id,
        SceneEvent::Interaction(e) => &e.region_id,
        SceneEvent::Region(e) => &e.region_id,
        _ => return None,
    })
}

/// The event's `accountId`, its region's hosting account; `None` for an
/// unrecognized `@type`.
fn hosting_account(event: &SceneEvent) -> Option<&Id> {
    Some(match event {
        SceneEvent::Avatar(e) => &e.account_id,
        SceneEvent::Object(e) => &e.account_id,
        SceneEvent::Interaction(e) => &e.account_id,
        SceneEvent::Region(e) => &e.account_id,
        _ => return None,
    })
}

/// The `SceneStreamEnable` / `SceneStreamDisable` /
/// `SceneInteractionEvent` handler of one WebSocket connection; see the
/// [module docs](self).
///
/// - `SceneStreamEnable` replaces the subscription through
///   [`SceneEphemeralSubscription::replace_with`], or
///   [`SceneEphemeralSubscription::replace_with_observe`] when
///   [`SceneBackend::supports_observe`]. Its error becomes the
///   `RequestError` (`invalidArguments`, or `forbidden` with `regionIds`
///   for a refused observer subscription, with the message's `id` as
///   `requestId`) and leaves the subscription unchanged; "This error does
///   not close the WebSocket connection". A malformed message is
///   `notRequest`.
/// - `SceneStreamDisable` clears it, silently, whether or not one was
///   active. The `"scene"` path of `ChatStreamEnable` is unaffected.
/// - `SceneInteractionEvent` is validated and relayed, never answered.
/// - Closing the connection clears the subscription and stops delivery
///   (§Subscription Lifecycle).
///
/// Each of these that changes which regions the connection observes
/// reports it to [`SceneBackend::set_observing`].
pub struct SceneStreamHandler<B: SceneBackend> {
    shared: Arc<Shared<B>>,
    hub: SceneEventHub,
    limits: SceneStreamLimits,
    task: Option<JoinHandle<()>>,
}

impl<B: SceneBackend> fmt::Debug for SceneStreamHandler<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SceneStreamHandler")
            .field("recipient_account_id", &self.shared.recipient_account_id)
            .field("subscription", &self.shared.subscription())
            .field("chat_alias", &self.shared.alias.is_on())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<B: SceneBackend> SceneStreamHandler<B> {
    /// The handler of a connection authenticated as `caller`, whose own
    /// account is `recipient_account_id` (the account whose Chat
    /// capability the blocked-sender check consults, as
    /// [`should_deliver`] takes it), receiving events from and relaying
    /// interactions to `hub`.
    ///
    /// The default [`SceneStreamLimits`] apply.
    ///
    /// # Panics
    ///
    /// Does not panic: the `expect` on the catch-all subscription holds
    /// because [`SceneEphemeralSubscription::replace_with`] fails only on an
    /// `eventTypes` array with no recognized value, and the default
    /// [`SceneStreamEnable`] has `eventTypes` `null`.
    pub fn new(
        backend: Arc<B>,
        caller: B::CallerCtx,
        recipient_account_id: Id,
        hub: &SceneEventHub,
    ) -> Self {
        let mut every = SceneEphemeralSubscription::default();
        every
            .replace_with(&SceneStreamEnable::default())
            .expect("a SceneStreamEnable with null eventTypes is always valid");
        Self {
            shared: Arc::new(Shared {
                backend,
                caller,
                recipient_account_id,
                subscription: Mutex::new(SceneEphemeralSubscription::default()),
                alias: SceneChatAlias::default(),
                every,
                observing: tokio::sync::Mutex::new(Vec::new()),
            }),
            hub: hub.clone(),
            limits: SceneStreamLimits::default(),
            task: None,
        }
    }

    /// Replace the outbound rate limit.
    pub fn with_limits(mut self, limits: SceneStreamLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The `"scene"` switch of this connection's `ChatStreamEnable`
    /// subscription; off until set.
    pub fn chat_alias(&self) -> SceneChatAlias {
        self.shared.alias.clone()
    }

    /// The connection's current `SceneStreamEnable` subscription.
    pub fn subscription(&self) -> SceneEphemeralSubscription {
        self.shared.subscription()
    }

    fn set(&self, subscription: SceneEphemeralSubscription) {
        *self
            .shared
            .subscription
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = subscription;
    }

    async fn enable(&self, message: &ExtensionMessage) -> Result<(), JmapError> {
        let enable: SceneStreamEnable = message.deserialize()?;
        let shared = &self.shared;
        let backend = &*shared.backend;
        let mut next = self.subscription();
        // "Recognized only when the server advertises `supportsObserve`".
        let observing = backend.supports_observe(&shared.caller);
        if observing {
            next.replace_with_observe(&enable)?;
        } else {
            next.replace_with(&enable)?;
        }
        let mut listed = Vec::new();
        if observing
            && enable.observe == Some(true)
            && let Some(region_ids) = &enable.region_ids
        {
            let fail = |e: B::Error| jmap_server::server_fail_from_backend(&e);
            let accounts = backend
                .observer_accounts(&shared.caller, &shared.recipient_account_id)
                .await
                .map_err(fail)?;
            let refused = observer_regions(backend, &shared.caller, &accounts, region_ids)
                .await
                .map_err(fail)?;
            if !refused.is_empty() {
                // "the server MUST respond with a `RequestError` ... with a
                // `type` of `\"urn:ietf:params:jmap:error:forbidden\"` and a
                // `regionIds` member listing the refused regions, and MUST
                // NOT update the current ephemeral subscription state".
                let mut err = JmapError::forbidden();
                err.description = Some("these regions do not admit observers".to_owned());
                err.extra
                    .insert("regionIds".to_owned(), serde_json::json!(refused));
                return Err(err);
            }
            // Every listed region in every observer account, whether the
            // user has access to it now or not: delivery re-checks access
            // per event, so a region the user gains access to later must
            // count from then on. The backend decides who counts.
            for account in &accounts {
                for region in region_ids {
                    let pair = (account.clone(), region.clone());
                    if !listed.contains(&pair) {
                        listed.push(pair);
                    }
                }
            }
        }
        self.set(next);
        shared.observe(listed).await;
        Ok(())
    }

    /// Validates a client-submitted interaction and relays it; every
    /// failure is silent.
    async fn interact(&self, message: &ExtensionMessage) {
        let Ok(mut frame) = message.deserialize::<SceneInteractionEvent>() else {
            return;
        };
        // "A frame carries the properties defined above": members the
        // server does not know, the frame's `id` among them, are not
        // relayed to other users.
        frame.extra.clear();
        let shared = &self.shared;
        let Ok(Some(relayed)) =
            validate_interaction(&*shared.backend, &shared.caller, &frame.account_id, &frame).await
        else {
            return;
        };
        let key = (
            relayed.account_id.clone(),
            relayed.region_id.clone(),
            relayed.user_id.clone(),
        );
        if self.hub.admit_interaction(key) {
            self.hub.publish(SceneEvent::Interaction(relayed));
        }
    }
}

impl<B: SceneBackend> ExtensionHandler for SceneStreamHandler<B> {
    fn message_types(&self) -> &[&str] {
        &[ENABLE, DISABLE, INTERACTION]
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
        match message.message_type.as_str() {
            ENABLE => self.enable(&message).await,
            INTERACTION => {
                self.interact(&message).await;
                Ok(())
            }
            _ => {
                self.set(SceneEphemeralSubscription::default());
                self.shared.observe(Vec::new()).await;
                Ok(())
            }
        }
    }

    async fn close(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.set(SceneEphemeralSubscription::default());
        // "The count drops when the subscription stops covering the
        // region, through ... the connection closing."
        self.shared.observe(Vec::new()).await;
    }
}

impl<B: SceneBackend> Drop for SceneStreamHandler<B> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Object-update limiter key: (`accountId`, `objectId`).
type ObjectKey = (Id, Id);

/// Delivers the hub's events to one connection until it closes.
async fn forward<B: SceneBackend>(
    shared: Arc<Shared<B>>,
    limits: SceneStreamLimits,
    mut rx: broadcast::Receiver<SceneEvent>,
    events: ExtensionSender,
) {
    let mut updates: Limiter<ObjectKey, SceneEvent> =
        Limiter::new(limits.object_update.map(|per| Window {
            max: 2,
            per,
            keep_latest: true,
        }));
    loop {
        tokio::select! {
            () = events.closed() => return,
            () = sleep_until(updates.next_due()) => {
                for (_, held) in updates.take_due(Instant::now()) {
                    if shared.admits(&held).await {
                        send(&events, &held);
                    }
                }
            }
            received = rx.recv() => match received {
                Ok(event) => {
                    shared.observe_event(&event).await;
                    if !shared.admits(&event).await {
                        continue;
                    }
                    let pass = match &event {
                        SceneEvent::Object(e) if e.event == ObjectEventKind::Updated => {
                            let key = (e.account_id.clone(), e.object_id.clone());
                            updates.offer(key, event.clone(), Instant::now()).is_some()
                        }
                        _ => true,
                    };
                    if pass {
                        send(&events, &event);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
        }
    }
}

/// Queues `event`, dropping it if the client is too slow.
fn send(events: &ExtensionSender, event: &SceneEvent) {
    let _ = events.try_send(event);
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;
    use crate::RegionScope;
    use crate::memory::MemoryBackend;
    use serde_json::{Value, json};

    fn handler(hub: &SceneEventHub) -> SceneStreamHandler<MemoryBackend> {
        SceneStreamHandler::new(
            Arc::new(MemoryBackend::new()),
            Id::from("user:alice@example.com"),
            Id::from("a1"),
            hub,
        )
    }

    fn message(ty: &str, body: Value) -> ExtensionMessage {
        let Value::Object(body) = body else {
            panic!("object")
        };
        ExtensionMessage::new(ty, body)
    }

    #[test]
    fn registers_the_control_messages_and_interactions() {
        assert_eq!(
            handler(&SceneEventHub::new(8)).message_types(),
            [
                "SceneStreamEnable",
                "SceneStreamDisable",
                "SceneInteractionEvent"
            ]
        );
    }

    /// §`SceneStreamEnable`: a new message "replaces the previous ephemeral
    /// subscription in its entirety"; §`SceneStreamDisable` "MUST silently
    /// succeed if no ephemeral Scene subscription is currently active".
    #[tokio::test]
    async fn enable_replaces_and_disable_clears() {
        let mut h = handler(&SceneEventHub::new(8));
        h.handle(message(ENABLE, json!({"regionIds": ["r1"]})))
            .await
            .unwrap();
        assert_eq!(
            h.subscription().filter().unwrap().region_ids(),
            &RegionScope::Only(vec![Id::from("r1")])
        );
        h.handle(message(ENABLE, json!({}))).await.unwrap();
        assert_eq!(
            h.subscription().filter().unwrap().region_ids(),
            &RegionScope::All
        );
        h.handle(message(DISABLE, json!({}))).await.unwrap();
        assert!(h.subscription().filter().is_none());
        h.handle(message(DISABLE, json!({}))).await.unwrap();
    }

    /// §`SceneStreamEnable`: only unrecognized `eventTypes` is
    /// `invalidArguments` and "MUST NOT update the current ephemeral
    /// subscription state". Without `supportsObserve`, `observe` is not
    /// recognized: `observe: true` with `regionIds: null` is not the
    /// invalidArguments the draft requires of an observing server.
    #[tokio::test]
    async fn invalid_event_types_and_unrecognized_observe() {
        let mut h = handler(&SceneEventHub::new(8));
        h.handle(message(ENABLE, json!({"eventTypes": ["SceneAvatarEvent"]})))
            .await
            .unwrap();
        let err = h
            .handle(message(ENABLE, json!({"eventTypes": ["SceneFooBarEvent"]})))
            .await
            .unwrap_err();
        assert_eq!(err.error_type.as_str(), "invalidArguments");
        assert!(
            h.subscription()
                .filter()
                .unwrap()
                .includes("SceneAvatarEvent")
        );
        h.handle(message(ENABLE, json!({"observe": true, "regionIds": null})))
            .await
            .unwrap();
        assert_eq!(
            h.subscription().filter().unwrap().region_ids(),
            &RegionScope::All
        );
    }

    /// §Submitting Interactions: "If any check fails, the server discards
    /// the frame and returns no error to the sender." A malformed frame
    /// and one from a user without an avatar are both silent.
    #[tokio::test]
    async fn failed_interactions_are_silent() {
        let hub = SceneEventHub::new(8);
        let mut rx = hub.tx.subscribe();
        let mut h = handler(&hub);
        h.handle(message(INTERACTION, json!({"action": 1})))
            .await
            .unwrap();
        h.handle(message(
            INTERACTION,
            json!({"accountId": "a1", "regionId": "r1", "objectId": "o1", "action": "click", "data": null}),
        ))
        .await
        .unwrap();
        assert!(rx.try_recv().is_err());
    }

    /// Which `(account, region)` pairs the handler reports to
    /// `set_observing`, against draft-atwood-jmap-scene-wss-00 at
    /// jmap-chat-spec 42aed85 §Semantics: "While an observer subscription
    /// delivers events for a region to a user without an active avatar
    /// there, the user counts toward the region's `observerCount`", and
    /// "delivery is not limited to those accounts" the server checks at
    /// enable time (bd:JMAP-1mep).
    #[cfg(feature = "test-helpers")]
    mod observing {
        use super::*;
        use jmap_scene_types::{SceneCapability, SceneRegion};
        use jmap_server::JmapReads as _;

        const OWN: &str = "a1";
        const OTHER: &str = "a2";
        const R1: &str = "r1";
        const R2: &str = "r2";
        const ALICE: &str = "user:alice@example.com";
        const CAROL: &str = "user:carol@example.com";

        /// Accounts [`OWN`] and [`OTHER`], `supportsObserve` on.
        fn backend() -> Arc<MemoryBackend> {
            let backend = MemoryBackend::new();
            let cap: SceneCapability = serde_json::from_value(json!({
                "mayCreateRegion": true,
                "mayCreateObject": true,
                "supportedVisualTypes": ["model/gltf-binary"],
                "maxRegionsPerAccount": null,
                "maxObjectsPerRegion": null,
                "maxAvatarsPerRegion": null,
                "maxAssetSizeBytes": null
            }))
            .unwrap();
            backend.register_account(&Id::from(OWN), cap.clone());
            backend.register_account(&Id::from(OTHER), cap);
            backend.set_supports_observe(true);
            Arc::new(backend)
        }

        /// Seed region `id` of `account`, owned by [`ALICE`], open to
        /// observers.
        fn region(backend: &MemoryBackend, account: &str, id: &str, policy: &str) {
            backend.insert_object_for_test(
                "SceneRegion",
                &Id::from(account),
                id,
                json!({
                    "id": id,
                    "accountId": account,
                    "ownerId": ALICE,
                    "name": id,
                    "description": null,
                    "bounds": {"min": [-20, 0, -20], "max": [20, 8, 20]},
                    "environment": null,
                    "simulationUri": null,
                    "spawnPosition": [0, 0, 5],
                    "spawnOrientation": [0, 0, 0, 1],
                    "activeAvatarCount": 0,
                    "viewHint": "3d",
                    "accessPolicy": policy,
                    "createdAt": "2026-06-01T10:00:00Z",
                    "updatedAt": "2026-06-01T10:00:00Z",
                    "allowObservers": true,
                    "observerCount": 0
                }),
            );
        }

        async fn count(backend: &MemoryBackend, account: &str, region: &str) -> u64 {
            let (list, _) = backend
                .get_objects::<SceneRegion>(
                    &Id::from(ALICE),
                    &Id::from(account),
                    Some(&[Id::from(region)]),
                    None,
                )
                .await
                .unwrap();
            list[0].observer_count.unwrap()
        }

        fn carol(backend: &Arc<MemoryBackend>) -> SceneStreamHandler<MemoryBackend> {
            SceneStreamHandler::new(
                Arc::clone(backend),
                Id::from(CAROL),
                Id::from(OWN),
                &SceneEventHub::new(8),
            )
        }

        async fn observe(h: &mut SceneStreamHandler<MemoryBackend>, regions: &[&str]) {
            h.handle(message(
                ENABLE,
                json!({"regionIds": regions, "observe": true}),
            ))
            .await
            .unwrap();
        }

        fn round_start(account: &str, region: &str) -> SceneEvent {
            serde_json::from_value(json!({
                "@type": "SceneRegionEvent",
                "accountId": account,
                "regionId": region,
                "event": "com.example.round-start",
                "data": null
            }))
            .unwrap()
        }

        async fn reported(h: &SceneStreamHandler<MemoryBackend>) -> Vec<(Id, Id)> {
            h.shared.observing.lock().await.clone()
        }

        /// (b) A listed region the user has no access to when the
        /// subscription is enabled is reported anyway: once access is
        /// granted, events flow ("events resume without a new
        /// `SceneStreamEnable` once the region admits the observer
        /// again") and the user counts.
        #[tokio::test]
        async fn a_region_the_user_gains_access_to_later_counts() {
            let backend = backend();
            region(&backend, OWN, R1, "invite");
            let mut h = carol(&backend);
            observe(&mut h, &[R1]).await;
            assert_eq!(count(&backend, OWN, R1).await, 0);
            assert!(reported(&h).await.contains(&(Id::from(OWN), Id::from(R1))));
            backend.grant_region_access(R1, CAROL);
            assert_eq!(count(&backend, OWN, R1).await, 1);
            backend.revoke_region_access(R1, CAROL);
            assert_eq!(count(&backend, OWN, R1).await, 0);
            backend.grant_region_access(R1, CAROL);
            h.close().await;
            assert_eq!(count(&backend, OWN, R1).await, 0);
        }

        /// (a) A listed region of an account outside `observer_accounts`
        /// still gets events, so its first event reports it; it is kept
        /// while a new subscription still lists it, and released when
        /// one does not. Events of unlisted regions report nothing.
        #[tokio::test]
        async fn a_region_outside_the_observer_accounts_counts_from_its_first_event() {
            let backend = backend();
            backend.restrict_observer_accounts(&[OWN]);
            region(&backend, OTHER, R1, "public");
            region(&backend, OTHER, R2, "public");
            let mut h = carol(&backend);
            observe(&mut h, &[R1]).await;
            assert_eq!(count(&backend, OTHER, R1).await, 0);

            h.shared.observe_event(&round_start(OTHER, R2)).await;
            assert_eq!(count(&backend, OTHER, R2).await, 0);
            h.shared.observe_event(&round_start(OTHER, R1)).await;
            assert!(h.shared.admits(&round_start(OTHER, R1)).await);
            assert_eq!(count(&backend, OTHER, R1).await, 1);
            // A second event is not a second report.
            h.shared.observe_event(&round_start(OTHER, R1)).await;
            let pair = (Id::from(OTHER), Id::from(R1));
            assert_eq!(reported(&h).await.iter().filter(|p| **p == pair).count(), 1);

            observe(&mut h, &[R1, R2]).await;
            assert_eq!(count(&backend, OTHER, R1).await, 1);
            observe(&mut h, &[R2]).await;
            assert_eq!(count(&backend, OTHER, R1).await, 0);
            assert!(!reported(&h).await.contains(&pair));

            h.shared.observe_event(&round_start(OTHER, R2)).await;
            assert_eq!(count(&backend, OTHER, R2).await, 1);
            h.handle(message(DISABLE, json!({}))).await.unwrap();
            assert_eq!(count(&backend, OTHER, R2).await, 0);
            assert!(reported(&h).await.is_empty());
        }

        /// A subscription without `observe` reports nothing, from enable
        /// or from events.
        #[tokio::test]
        async fn a_plain_subscription_reports_nothing() {
            let backend = backend();
            region(&backend, OWN, R1, "public");
            let mut h = carol(&backend);
            h.handle(message(ENABLE, json!({"regionIds": [R1]})))
                .await
                .unwrap();
            h.shared.observe_event(&round_start(OWN, R1)).await;
            assert!(reported(&h).await.is_empty());
            assert_eq!(count(&backend, OWN, R1).await, 0);
        }

        /// "A user with several observing connections counts once",
        /// however each connection came to report the region.
        #[tokio::test]
        async fn several_connections_count_once() {
            let backend = backend();
            backend.restrict_observer_accounts(&[OWN]);
            region(&backend, OWN, R1, "public");
            region(&backend, OTHER, R1, "public");
            let mut first = carol(&backend);
            let mut second = carol(&backend);
            observe(&mut first, &[R1]).await;
            observe(&mut second, &[R1]).await;
            for h in [&first, &second] {
                h.shared.observe_event(&round_start(OTHER, R1)).await;
            }
            assert_eq!(count(&backend, OWN, R1).await, 1);
            assert_eq!(count(&backend, OTHER, R1).await, 1);
            first.close().await;
            assert_eq!(count(&backend, OWN, R1).await, 1);
            assert_eq!(count(&backend, OTHER, R1).await, 1);
            second.close().await;
            assert_eq!(count(&backend, OWN, R1).await, 0);
            assert_eq!(count(&backend, OTHER, R1).await, 0);
        }
    }

    #[test]
    fn the_hub_limits_interactions_per_user_per_region() {
        let hub = SceneEventHub::new(8);
        let key = |region: &str| (Id::from("a1"), Id::from(region), "u".to_owned());
        for _ in 0..5 {
            assert!(hub.admit_interaction(key("r1")));
        }
        assert!(!hub.admit_interaction(key("r1")));
        assert!(hub.admit_interaction(key("r2")));
        let unlimited = SceneEventHub::with_interaction_window(8, None);
        for _ in 0..10 {
            assert!(unlimited.admit_interaction(key("r1")));
        }
    }

    #[test]
    fn default_limits_are_the_drafts() {
        let l = SceneStreamLimits::default();
        assert_eq!(l.object_update_window_value(), Some(Duration::from_secs(1)));
        assert_eq!(
            l.object_update_window(None).object_update_window_value(),
            None
        );
    }
}
