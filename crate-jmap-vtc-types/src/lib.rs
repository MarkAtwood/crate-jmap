//! JMAP VTC extension types (draft-atwood-jmap-vtc-00 and
//! draft-atwood-jmap-vtc-wss-00).
//!
//! Normative reference: draft-atwood-jmap-vtc-00 — the
//! `urn:ietf:params:jmap:vtc` capability, which manages voice and video
//! call state over JMAP. Media itself is out of band.
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies, no I/O, no method semantics.
//! - Depends on RFC 8620 types only; no dependency on the Chat, Calendars
//!   or Scene type crates. Their ids are plain [`jmap_types::Id`]s.
//! - Forbids `unsafe`.
//!
//! ## Public surface
//!
//! - [`JMAP_VTC_URI`], [`VtcCapability`], [`VtcGateway`], [`VtcIceServer`],
//!   [`VtcDialInEntry`], [`MediaKind`], [`GatewayProtocol`] — the
//!   capability and its gateways (draft §Capability, §`VTCGateway`).
//! - [`VtcMediaState`], [`VtcCallPolicy`] — embedded types.
//! - [`VtcCall`], [`CallType`], [`CallState`], [`CallEndReason`].
//! - [`VtcParticipant`], [`CallRole`], [`CallResponse`], [`LobbyState`],
//!   [`JoinMethod`].
//! - [`VtcRecording`], [`RecordingState`], [`CompositionLayout`], [`VtcLivestream`],
//!   [`LivestreamState`].
//! - [`VtcCallPush`] — the ring push payload, full and content-free.
//! - [`VtcEvent`] and the eight events it carries: [`VtcRingEvent`],
//!   [`VtcCallEndEvent`] ([`RingEndReason`]), [`VtcGatewaySignal`]
//!   ([`SignalDirection`]), [`VtcParticipantEvent`]
//!   ([`ParticipantEventKind`]), [`VtcMediaStateEvent`],
//!   [`VtcActiveSpeakerEvent`], [`VtcUnmuteRequestEvent`],
//!   [`VtcRecordingStateEvent`].
//! - [`JMAP_VTC_WEBSOCKET_URI`], [`VtcStreamMessage`] over
//!   [`VtcStreamEnable`] and [`VtcStreamDisable`], and the
//!   [`VTC_EVENT_TYPES`] vocabulary — the client→server stream control
//!   frames of draft-atwood-jmap-vtc-wss-00.
//! - `query` — the `VTCCall/query`, `VTCParticipant/query`,
//!   `VTCRecording/query` and `VTCLivestream/query` filters and comparators.
//! - `JmapObject` impls for [`VtcCall`] (`"VTCCall"`) and
//!   [`VtcParticipant`] (`"VTCParticipant"`), each get/set/query, for
//!   `jmap-vtc-server`.
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types
//! ([`VtcParticipant`], [`VtcParticipantEvent`], [`VtcCallPush`],
//! [`VtcRingEvent`], [`VtcRecording`], and the subject and initiator of
//! [`VtcCall`]). `tracing` or `log` macros that interpolate one of these values
//! with `?` (debug) or `%` (display) emit its full contents to the configured
//! sink: participant user ids and display names, call subjects, who called whom
//! and when, and dial-in addresses. That data is typically regulated as
//! personal data under GDPR, CCPA, and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the participant's user id and display name.
//! tracing::debug!(participant = ?participant, "joined call");
//!
//! // GOOD — emits only the opaque ids.
//! tracing::debug!(participant_id = ?participant.id, call_id = ?participant.call_id, "joined call");
//! ```
//!
//! Credential-grade fields are already redacted: the join password in
//! [`VtcCall`], and the URL and credential in [`VtcPlaybackEndpoint`].
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod backend;
mod call;
mod capability;
mod event;
mod participant;
mod push;
mod query;
mod recording;
mod stream;
mod wire;

pub use call::{CallEndReason, CallState, CallType, VtcCall, VtcCallPolicy, VtcMediaState};
pub use capability::{
    GatewayProtocol, JMAP_VTC_URI, JMAP_VTC_WEBSOCKET_URI, MediaKind, VtcCapability,
    VtcDialInEntry, VtcGateway, VtcIceServer,
};
pub use event::{
    CallSignalKind, ParticipantEventKind, RingEndReason, SignalDirection, VtcActiveSpeakerEvent,
    VtcCallEndEvent, VtcCallSignal, VtcEvent, VtcGatewaySignal, VtcMediaStateEvent,
    VtcParticipantEvent, VtcRecordingStateEvent, VtcRingEvent, VtcUnmuteRequestEvent,
};
pub use participant::{CallResponse, CallRole, JoinMethod, LobbyState, VtcParticipant};
pub use push::VtcCallPush;
pub use query::{
    Filter, FilterOperator, Operator, VtcCallComparator, VtcCallFilter, VtcCallFilterCondition,
    VtcCallSortProperty, VtcLivestreamFilter, VtcLivestreamFilterCondition, VtcMediaComparator,
    VtcMediaSortProperty, VtcParticipantComparator, VtcParticipantFilter,
    VtcParticipantFilterCondition, VtcParticipantSortProperty, VtcRecordingFilter,
    VtcRecordingFilterCondition,
};
pub use recording::{
    CompositionLayout, LatencyHint, LivestreamFailureReason, LivestreamMode, LivestreamState,
    PlaybackProtocol, RecordingState, StreamTrackKind, VtcLivestream, VtcPlaybackEndpoint,
    VtcRecording, VtcStreamTrack, VtcTimeReference,
};
pub use stream::{
    VTC_EVENT_TYPES, VtcEventType, VtcStreamDisable, VtcStreamEnable, VtcStreamMessage,
};
