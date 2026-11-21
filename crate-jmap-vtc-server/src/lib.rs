//! JMAP VTC method handlers (draft-atwood-jmap-vtc-00).
//!
//! # Usage
//!
//! Implement [`VtcBackend`] for your storage layer, then call
//! [`register_vtc_handlers`] to wire the method names into a
//! [`jmap_server::Dispatcher`]:
//!
//! ```rust,no_run
//! # use std::sync::Arc;
//! # use jmap_vtc_server::{VtcBackend, register_vtc_handlers};
//! # use jmap_server::Dispatcher;
//! # fn example<B: VtcBackend<CallerCtx = ()> + 'static>(backend: B) {
//! let mut dispatcher: Dispatcher<()> = Dispatcher::new();
//! register_vtc_handlers(&mut dispatcher, Arc::new(backend));
//! # }
//! ```
//!
//! # Scope
//!
//! The `VTCCall/*` methods: create with the capability gates, the state
//! machine, ending a call, the call-level moderator actions and `/query`.
//! Ending a call, by `VTCCall/set` or by the server through [`end_call`],
//! also ends its breakout rooms and stops its active recordings and
//! livestreams.
//! [`expire_ring`] applies the ring timeout when the deployment's timer
//! fires.
//! The `VTCParticipant/*` methods: joining, gateway dial-out, answering
//! and declining a ring (with the `answered_elsewhere` or `declined` fork
//! to that user's other devices), media state under the call policy, leaving and
//! reconnecting, the participant-level moderator actions (mute, ask to
//! unmute, kick, lobby admission, role, breakout assignment), the outbound
//! `gatewayData/pendingSignal` relay, the call transitions they cause, and
//! `/query` scoped by `callId` or `userId`.
//! The `VTCRecording/*` and `VTCLivestream/*` methods: moderator-only
//! `/set` with each type's state machine, the `VTCRecordingStateEvent`
//! consent signal, and `streamKey` redaction in `VTCLivestream/get`.
//! Media work happens outside the stored write: the handlers hand each
//! start, pause, resume and stop to [`VtcBackend::run_media_effect`] as a
//! [`MediaEffect`] after the write, and the backend reports what its media
//! pipeline did (a livestream going live or failing, a recording becoming
//! available) through [`media_report::MediaReporter`].
//! [`VtcEphemeralSubscription`] holds the per-connection `VTCStreamEnable`
//! subscription state of draft-atwood-jmap-vtc-wss-00 and the
//! [`should_deliver`] delivery-time check for its events.
//! The crate depends on RFC 8620 only; the Chat binding (membership,
//! `start_call`, `Chat.activeCallId`) lives in a separate binding crate.
//!
//! # `websocket` feature (ephemeral events over RFC 8887)
//!
//! Enable `websocket` for the `websocket` module: the `jmap-websocket`
//! extension handler for `VTCStreamEnable` / `VTCStreamDisable`
//! (draft-atwood-jmap-vtc-wss-00) and the server-wide hub the deployment
//! publishes events to. Off by default, so a deployment without the WebSocket
//! transport does not pull in `jmap-websocket`.
//!
//! # `memory` feature (reference implementation)
//!
//! Enable the `memory` feature to expose `memory::MemoryBackend`, the
//! in-memory reference implementation of [`VtcBackend`] used by this
//! crate's tests. **Not production.** Its API may break in any minor
//! release while the crate is pre-1.0.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

use std::sync::Arc;

use jmap_server::{
    CallContext, ClosureHandler, ConditionalBackend, Dispatcher, HandlerFuture, JmapHandler,
    RegistrationError, SetSupport,
};

mod backend;
mod call;
mod ephemeral;
mod helpers;
mod history;
mod media;
pub mod media_report;
/// In-memory reference implementation of [`VtcBackend`].
///
/// Gated behind `feature = "memory"`. Not production.
#[cfg(feature = "memory")]
pub mod memory;
mod participant;
pub mod query;
#[cfg(feature = "websocket")]
pub mod websocket;

pub use backend::{MediaEffect, StatefulObject, VtcBackend, call_already_active_on_chat};
pub use call::{
    RING_TIMEOUT, end_call, expire_ring, handle_vtc_call_changes, handle_vtc_call_get,
    handle_vtc_call_query, handle_vtc_call_query_changes, handle_vtc_call_set,
};
pub use ephemeral::{CallScope, VtcEphemeralSubscription, VtcStreamFilter, should_deliver};
pub use history::register_vtc_handlers_with_history;
pub use media::{
    handle_vtc_livestream_changes, handle_vtc_livestream_get, handle_vtc_livestream_query,
    handle_vtc_livestream_query_changes, handle_vtc_livestream_set, handle_vtc_recording_changes,
    handle_vtc_recording_get, handle_vtc_recording_query, handle_vtc_recording_query_changes,
    handle_vtc_recording_set,
};
pub use participant::{
    after_leave, handle_vtc_participant_changes, handle_vtc_participant_get,
    handle_vtc_participant_query, handle_vtc_participant_query_changes, handle_vtc_participant_set,
};

/// Capability URI for `urn:ietf:params:jmap:vtc`.
pub use jmap_vtc_types::JMAP_VTC_URI;

/// Register the `VTCCall/*`, `VTCParticipant/*`, `VTCRecording/*` and
/// `VTCLivestream/*` method handlers with `dispatcher`.
///
/// After this call the dispatcher handles `/get`, `/changes`, `/set`,
/// `/query` and `/queryChanges` for each of the four types.
/// [`Dispatcher::register`] silently replaces an existing handler of the
/// same name, so register overrides afterwards.
///
/// This only wires dispatch. The consumer advertises
/// [`JMAP_VTC_URI`] in its Session capabilities, with the same
/// account-level object [`VtcBackend::vtc_capability`] returns.
pub fn register_vtc_handlers<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: VtcBackend + 'static,
{
    // `ids` marks a `/set` handler that resolves `#creationId` references
    // against `CallContext::created_ids` (RFC 8620 §5.3).
    macro_rules! reg {
        ($method:expr, $handler:ident) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args: a,
                        caller: ctx,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &ctx, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
        ($method:expr, $handler:ident, ids) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args: a,
                        caller: ctx,
                        created_ids: ids,
                        ..
                    } = cx;
                    Box::pin(async move { $handler(&*b, &ctx, &ids, a).await }) as HandlerFuture
                },
            ));
            dispatcher.register($method, h);
        }};
    }

    reg!("VTCCall/get", handle_vtc_call_get);
    reg!("VTCCall/changes", handle_vtc_call_changes);
    reg!("VTCCall/set", handle_vtc_call_set, ids);
    reg!("VTCCall/query", handle_vtc_call_query);
    reg!("VTCCall/queryChanges", handle_vtc_call_query_changes);
    reg!("VTCParticipant/get", handle_vtc_participant_get);
    reg!("VTCParticipant/changes", handle_vtc_participant_changes);
    reg!("VTCParticipant/set", handle_vtc_participant_set, ids);
    reg!("VTCParticipant/query", handle_vtc_participant_query);
    reg!(
        "VTCParticipant/queryChanges",
        handle_vtc_participant_query_changes
    );
    reg!("VTCRecording/get", handle_vtc_recording_get);
    reg!("VTCRecording/changes", handle_vtc_recording_changes);
    reg!("VTCRecording/set", handle_vtc_recording_set, ids);
    reg!("VTCRecording/query", handle_vtc_recording_query);
    reg!(
        "VTCRecording/queryChanges",
        handle_vtc_recording_query_changes
    );
    reg!("VTCLivestream/get", handle_vtc_livestream_get);
    reg!("VTCLivestream/changes", handle_vtc_livestream_changes);
    reg!("VTCLivestream/set", handle_vtc_livestream_set, ids);
    reg!("VTCLivestream/query", handle_vtc_livestream_query);
    reg!(
        "VTCLivestream/queryChanges",
        handle_vtc_livestream_query_changes
    );
}

/// Opt this crate's registered Foo/set methods in to
/// draft-ietf-jmap-conditional-02 (`ifUnchangedBy` and `atomic`;
/// docs/design/DECISIONS.md decisions 1 and 17).
///
/// Call after [`register_vtc_handlers`]. Attaches [`SetSupport`] to each
/// of `VTCCall/set`, `VTCParticipant/set`, `VTCRecording/set`, `VTCLivestream/set` that is registered, and skips the rest; no handler is
/// re-registered.
///
/// The dispatcher reads preconditions through each type's `/get` and runs
/// each call in the scope `backend` opens
/// ([`ConditionalBackend::begin_set`]).
/// Under `atomic` that scope must hold back `deliver_event` deliveries and
/// [`VtcBackend::run_media_effect`] media effects until commit and check
/// participant rules on the final state; the memory backend does.
/// Advertise `urn:ietf:params:jmap:conditional` only once
/// [`Dispatcher::conditional_ready`] is true for the whole dispatcher.
pub fn register_vtc_conditional<B>(dispatcher: &mut Dispatcher<B::CallerCtx>, backend: Arc<B>)
where
    B: VtcBackend + ConditionalBackend + 'static,
{
    fn attach<C: Clone + Send + 'static>(
        dispatcher: &mut Dispatcher<C>,
        method: &str,
        support: SetSupport<C>,
    ) {
        match dispatcher.attach_set_support(method, support) {
            Ok(()) | Err(RegistrationError::NotRegistered(_)) => {}
            Err(e) => unreachable!("{method} pairs with its own data type: {e}"),
        }
    }
    use jmap_vtc_types::{VtcCall, VtcLivestream, VtcParticipant, VtcRecording};
    attach(
        dispatcher,
        "VTCCall/set",
        SetSupport::new::<VtcCall, B>(Arc::clone(&backend)),
    );
    attach(
        dispatcher,
        "VTCParticipant/set",
        SetSupport::new::<VtcParticipant, B>(Arc::clone(&backend)),
    );
    attach(
        dispatcher,
        "VTCRecording/set",
        SetSupport::new::<VtcRecording, B>(Arc::clone(&backend)),
    );
    attach(
        dispatcher,
        "VTCLivestream/set",
        SetSupport::new::<VtcLivestream, B>(Arc::clone(&backend)),
    );
}
