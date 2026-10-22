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
//! - [`JMAP_VTC_URI`], [`VtcCapability`], [`VtcGateway`],
//!   [`VtcDialInEntry`], [`MediaKind`], [`GatewayProtocol`] — the
//!   capability and its gateways (draft §Capability, §VTCGateway).
//! - [`VtcMediaState`], [`VtcCallPolicy`] — embedded types.
//! - [`VtcCall`], [`CallType`], [`CallState`], [`CallEndReason`].
//! - [`VtcParticipant`], [`CallRole`], [`CallResponse`], [`LobbyState`],
//!   [`JoinMethod`].
//! - [`VtcRecording`], [`RecordingState`], [`VtcLivestream`],
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
    GatewayProtocol, MediaKind, VtcCapability, VtcDialInEntry, VtcGateway, JMAP_VTC_URI,
    JMAP_VTC_WEBSOCKET_URI,
};
pub use event::{
    ParticipantEventKind, RingEndReason, SignalDirection, VtcActiveSpeakerEvent, VtcCallEndEvent,
    VtcEvent, VtcGatewaySignal, VtcMediaStateEvent, VtcParticipantEvent, VtcRecordingStateEvent,
    VtcRingEvent, VtcUnmuteRequestEvent,
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
pub use recording::{LivestreamState, RecordingState, VtcLivestream, VtcRecording};
pub use stream::{
    VtcEventType, VtcStreamDisable, VtcStreamEnable, VtcStreamMessage, VTC_EVENT_TYPES,
};
