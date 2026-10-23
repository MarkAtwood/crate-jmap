//! draft-atwood-jmap-vtc-00 ephemeral WebSocket events: §WebSocket:
//! VTCRingEvent, §VTCCallEndEvent, §VTCGatewaySignal and §In-Call
//! Ephemeral Events.
//!
//! The event structs carry no `@type`; [`VtcEvent`] adds and dispatches
//! on it, as `jmap_chat_types::EphemeralMessage` does for Chat events.

use jmap_types::{Id, UTCDate, impl_string_enum};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{CallRole, GatewayProtocol, MediaKind, RecordingState, VtcMediaState};

/// The `endReason` of a [`VtcCallEndEvent`]: why ringing stopped on this
/// device (draft-atwood-jmap-vtc-00 §VTCCallEndEvent).
///
/// "This is a per-device ring-disposition enum, distinct from
/// VTCCall.endReason" ([`CallEndReason`](crate::CallEndReason)).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RingEndReason {
    /// `"answered_elsewhere"`: the same user answered on another device.
    AnsweredElsewhere,
    /// `"declined"`.
    Declined,
    /// `"cancelled"`: the initiator withdrew the ring.
    Cancelled,
    /// `"timeout"`.
    Timeout,
    /// `"failed"`.
    Failed,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(RingEndReason, "a VTC ring end reason string",
    "answered_elsewhere" => AnsweredElsewhere,
    "declined" => Declined,
    "cancelled" => Cancelled,
    "timeout" => Timeout,
    "failed" => Failed,
);

/// The `direction` of a [`VtcGatewaySignal`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SignalDirection {
    /// `"inbound"`: from the external party toward the call.
    Inbound,
    /// `"outbound"`: from a moderator toward the external party.
    Outbound,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(SignalDirection, "a VTC gateway signal direction string",
    "inbound" => Inbound,
    "outbound" => Outbound,
);

/// The `event` of a [`VtcParticipantEvent`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParticipantEventKind {
    /// `"joined"`.
    Joined,
    /// `"left"`.
    Left,
    /// `"kicked"`.
    Kicked,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(ParticipantEventKind, "a VTC participant event string",
    "joined" => Joined,
    "left" => Left,
    "kicked" => Kicked,
);

/// An incoming ring on a WebSocket connection
/// (draft-atwood-jmap-vtc-00 §WebSocket: VTCRingEvent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcRingEvent {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The ringing call.
    pub call_id: Id,
    /// The userId of the caller.
    pub initiator_id: String,
    /// The media types for this call.
    pub media_types: Vec<MediaKind>,
    /// The media-layer entry point. Untrusted (§VTCCall `joinUri`).
    pub join_uri: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Tells a ringing device to stop ringing
/// (draft-atwood-jmap-vtc-00 §VTCCallEndEvent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcCallEndEvent {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The call.
    pub call_id: Id,
    /// Why ringing stopped for this device.
    pub end_reason: RingEndReason,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A protocol-specific signal between a gateway and a call participant,
/// relayed without interpretation (draft-atwood-jmap-vtc-00
/// §VTCGatewaySignal).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcGatewaySignal {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The call.
    pub call_id: Id,
    /// The gateway participant.
    pub participant_id: Id,
    /// The gateway protocol.
    pub protocol: GatewayProtocol,
    /// The signal type, defined by the external protocol, e.g. `"dtmf"`.
    pub signal: String,
    /// Signal-specific payload, opaque to JMAP VTC.
    pub data: serde_json::Map<String, serde_json::Value>,
    /// Inbound or outbound.
    pub direction: SignalDirection,
    /// Time the signal was generated.
    pub timestamp: UTCDate,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A participant joined, left or was kicked
/// (draft-atwood-jmap-vtc-00 §VTCParticipantEvent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcParticipantEvent {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The call.
    pub call_id: Id,
    /// The participant.
    pub participant_id: Id,
    /// Joined, left or kicked.
    pub event: ParticipantEventKind,
    /// The participant's display name at event time.
    pub display_name: String,
    /// The participant's role at event time.
    pub role: CallRole,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A participant's media state changed
/// (draft-atwood-jmap-vtc-00 §VTCMediaStateEvent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcMediaStateEvent {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The call.
    pub call_id: Id,
    /// The participant.
    pub participant_id: Id,
    /// The updated media state.
    pub media_state: VtcMediaState,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The active (dominant) speaker changed
/// (draft-atwood-jmap-vtc-00 §VTCActiveSpeakerEvent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcActiveSpeakerEvent {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The call.
    pub call_id: Id,
    /// The current active speaker.
    pub participant_id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A moderator asked a participant to unmute
/// (draft-atwood-jmap-vtc-00 §VTCUnmuteRequestEvent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcUnmuteRequestEvent {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The call.
    pub call_id: Id,
    /// The participant asked to unmute.
    pub participant_id: Id,
    /// The userId of the requesting moderator.
    pub requested_by: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Recording state changed: "This is the consent signal"
/// (draft-atwood-jmap-vtc-00 §VTCRecordingStateEvent, §Recording Consent).
///
/// "Exactly one of `recordingId` and `livestreamId` is present"; this is
/// documented, not enforced on deserialize.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcRecordingStateEvent {
    /// The id of the call's hosting account, "the account that contains
    /// the VTCCall and its VTCParticipant records" (draft-atwood-jmap-vtc-wss-00
    /// §Event Types); usually not the recipient's personal account.
    pub account_id: Id,
    /// The call.
    pub call_id: Id,
    /// The recording; absent when the event reports a livestream bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<Id>,
    /// The livestream, when the event reports a key-holding livestream
    /// bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub livestream_id: Option<Id>,
    /// The new state: `"recording"`, `"paused"` or `"stopped"`.
    pub state: RecordingState,
    /// The userId of the participant who initiated the change.
    pub initiated_by: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Any VTC ephemeral event, discriminated by its `@type`.
///
/// Unrecognised `@type` values deserialize to [`VtcEvent::Unknown`], which
/// keeps the discriminant and every other field so the frame re-serializes
/// verbatim. A frame with a known `@type` but a malformed body is an error,
/// not `Unknown`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VtcEvent {
    /// `"VTCRingEvent"`.
    Ring(VtcRingEvent),
    /// `"VTCCallEndEvent"`.
    CallEnd(VtcCallEndEvent),
    /// `"VTCGatewaySignal"`.
    GatewaySignal(VtcGatewaySignal),
    /// `"VTCParticipantEvent"`.
    Participant(VtcParticipantEvent),
    /// `"VTCMediaStateEvent"`.
    MediaState(VtcMediaStateEvent),
    /// `"VTCActiveSpeakerEvent"`.
    ActiveSpeaker(VtcActiveSpeakerEvent),
    /// `"VTCUnmuteRequestEvent"`.
    UnmuteRequest(VtcUnmuteRequestEvent),
    /// `"VTCRecordingStateEvent"`.
    RecordingState(VtcRecordingStateEvent),
    /// Any `@type` not recognized by this version of the library.
    Unknown {
        /// The original wire value of `@type`.
        type_name: String,
        /// Every other top-level field, verbatim.
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

pub(crate) const RING: &str = "VTCRingEvent";
pub(crate) const CALL_END: &str = "VTCCallEndEvent";
pub(crate) const GATEWAY_SIGNAL: &str = "VTCGatewaySignal";
pub(crate) const PARTICIPANT: &str = "VTCParticipantEvent";
pub(crate) const MEDIA_STATE: &str = "VTCMediaStateEvent";
pub(crate) const ACTIVE_SPEAKER: &str = "VTCActiveSpeakerEvent";
pub(crate) const UNMUTE_REQUEST: &str = "VTCUnmuteRequestEvent";
pub(crate) const RECORDING_STATE: &str = "VTCRecordingStateEvent";

impl VtcEvent {
    /// The event's `@type`, such as `"VTCRingEvent"`; for
    /// [`VtcEvent::Unknown`], the original wire value.
    pub fn type_name(&self) -> &str {
        match self {
            VtcEvent::Ring(_) => RING,
            VtcEvent::CallEnd(_) => CALL_END,
            VtcEvent::GatewaySignal(_) => GATEWAY_SIGNAL,
            VtcEvent::Participant(_) => PARTICIPANT,
            VtcEvent::MediaState(_) => MEDIA_STATE,
            VtcEvent::ActiveSpeaker(_) => ACTIVE_SPEAKER,
            VtcEvent::UnmuteRequest(_) => UNMUTE_REQUEST,
            VtcEvent::RecordingState(_) => RECORDING_STATE,
            VtcEvent::Unknown { type_name, .. } => type_name,
        }
    }
}

impl Serialize for VtcEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let body = match self {
            VtcEvent::Ring(e) => serde_json::to_value(e),
            VtcEvent::CallEnd(e) => serde_json::to_value(e),
            VtcEvent::GatewaySignal(e) => serde_json::to_value(e),
            VtcEvent::Participant(e) => serde_json::to_value(e),
            VtcEvent::MediaState(e) => serde_json::to_value(e),
            VtcEvent::ActiveSpeaker(e) => serde_json::to_value(e),
            VtcEvent::UnmuteRequest(e) => serde_json::to_value(e),
            VtcEvent::RecordingState(e) => serde_json::to_value(e),
            VtcEvent::Unknown { payload, .. } => Ok(serde_json::Value::Object(payload.clone())),
        };
        let body = body.map_err(serde::ser::Error::custom)?;
        crate::wire::with_tag(body, self.type_name())
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for VtcEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::wire::from_map;
        let mut map = serde_json::Map::deserialize(d)?;
        let type_name = crate::wire::take_tag::<D::Error>(&mut map)?;
        Ok(match type_name.as_str() {
            RING => VtcEvent::Ring(from_map::<_, D>(map)?),
            CALL_END => VtcEvent::CallEnd(from_map::<_, D>(map)?),
            GATEWAY_SIGNAL => VtcEvent::GatewaySignal(from_map::<_, D>(map)?),
            PARTICIPANT => VtcEvent::Participant(from_map::<_, D>(map)?),
            MEDIA_STATE => VtcEvent::MediaState(from_map::<_, D>(map)?),
            ACTIVE_SPEAKER => VtcEvent::ActiveSpeaker(from_map::<_, D>(map)?),
            UNMUTE_REQUEST => VtcEvent::UnmuteRequest(from_map::<_, D>(map)?),
            RECORDING_STATE => VtcEvent::RecordingState(from_map::<_, D>(map)?),
            _ => VtcEvent::Unknown {
                type_name,
                payload: map,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Parse `raw`, check it re-serializes to itself, and return it.
    fn round_trip(raw: serde_json::Value) -> VtcEvent {
        let ev: VtcEvent = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&ev).unwrap(), raw);
        ev
    }

    #[test]
    fn ring_event_fields() {
        // Oracle: L1335-1348 field list; values from the VTCCallPush
        // example at L1279-1291.
        let ev = round_trip(json!({
            "@type": "VTCRingEvent",
            "accountId": "A1",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "initiatorId": "user:bob@example.com",
            "mediaTypes": ["audio", "video"],
            "joinUri": "https://meet.example.com/room/xkz"
        }));
        let VtcEvent::Ring(e) = ev else {
            panic!("wrong variant: {ev:?}")
        };
        assert_eq!(e.initiator_id, "user:bob@example.com");
    }

    #[test]
    fn ring_end_reason_wire_values() {
        // Oracle: draft-atwood-jmap-vtc-00.md at jmap-chat-spec 6bcd9d8,
        // L1431 "Values: `\"answered_elsewhere\"` (the same user answered on
        // a different device), `\"declined\"`, `\"cancelled\"` ...,
        // `\"timeout\"`, `\"failed\"`." The list has no `"answered"`, so it
        // is an unregistered value.
        let v: Vec<RingEndReason> = serde_json::from_value(json!([
            "answered",
            "answered_elsewhere",
            "declined",
            "cancelled",
            "timeout",
            "failed"
        ]))
        .unwrap();
        assert_eq!(
            v,
            [
                RingEndReason::Other("answered".to_owned()),
                RingEndReason::AnsweredElsewhere,
                RingEndReason::Declined,
                RingEndReason::Cancelled,
                RingEndReason::Timeout,
                RingEndReason::Failed
            ]
        );
    }

    #[test]
    fn small_enum_wire_values() {
        // Oracle: L1406 "`\"inbound\"` ... or `\"outbound\"`"; L1464
        // "`\"joined\"`, `\"left\"`, or `\"kicked\"`".
        let d: Vec<SignalDirection> =
            serde_json::from_value(json!(["inbound", "outbound"])).unwrap();
        assert_eq!(d, [SignalDirection::Inbound, SignalDirection::Outbound]);
        let k: Vec<ParticipantEventKind> =
            serde_json::from_value(json!(["joined", "left", "kicked"])).unwrap();
        assert_eq!(
            k,
            [
                ParticipantEventKind::Joined,
                ParticipantEventKind::Left,
                ParticipantEventKind::Kicked
            ]
        );
    }

    #[test]
    fn in_call_events_round_trip() {
        // Oracle: field lists of L1454-1537, with participant ids from the
        // L951-962 example.
        let ev = round_trip(json!({
            "@type": "VTCParticipantEvent",
            "accountId": "A1",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "participantId": "user:charlie@example.com",
            "event": "joined",
            "displayName": "Charlie",
            "role": "participant"
        }));
        assert!(
            matches!(ev, VtcEvent::Participant(ref e) if e.event == ParticipantEventKind::Joined)
        );

        let ev = round_trip(json!({
            "@type": "VTCMediaStateEvent",
            "accountId": "A1",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "participantId": "user:dave@example.com",
            "mediaState": {"audio": false, "video": true, "screen": false, "raisedHand": false}
        }));
        assert!(matches!(ev, VtcEvent::MediaState(ref e) if !e.media_state.audio));

        let ev = round_trip(json!({
            "@type": "VTCActiveSpeakerEvent",
            "accountId": "A1",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "participantId": "user:alice@example.com"
        }));
        assert!(matches!(ev, VtcEvent::ActiveSpeaker(_)));

        let ev = round_trip(json!({
            "@type": "VTCUnmuteRequestEvent",
            "accountId": "A1",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "participantId": "user:dave@example.com",
            "requestedBy": "user:bob@example.com"
        }));
        assert!(
            matches!(ev, VtcEvent::UnmuteRequest(ref e) if e.requested_by == "user:bob@example.com")
        );
    }

    #[test]
    fn recording_state_event_recording_or_livestream() {
        // Oracle: L1527-1531 "`recordingId` (String, optional) ... Absent
        // when the event reports a livestream bridge"; "Exactly one of
        // `recordingId` and `livestreamId` is present."
        let ev = round_trip(json!({
            "@type": "VTCRecordingStateEvent",
            "accountId": "A1",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "recordingId": "01J4XP2SN9RXVU1SSEGIUT6GH",
            "state": "recording",
            "initiatedBy": "user:bob@example.com"
        }));
        let VtcEvent::RecordingState(e) = ev else {
            panic!("wrong variant: {ev:?}")
        };
        assert_eq!(e.state, RecordingState::Recording);
        assert!(e.livestream_id.is_none());

        let ev = round_trip(json!({
            "@type": "VTCRecordingStateEvent",
            "accountId": "A1",
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB",
            "livestreamId": "01J4XQ7VN0SYWV2TTFHJVW7IJ",
            "state": "stopped",
            "initiatedBy": "user:bob@example.com"
        }));
        let VtcEvent::RecordingState(e) = ev else {
            panic!("wrong variant: {ev:?}")
        };
        assert!(e.recording_id.is_none());
    }

    #[test]
    fn unknown_type_preserved_verbatim() {
        let ev = round_trip(json!({
            "@type": "VTCCaptionEvent",
            "callId": "C1",
            "text": "hello"
        }));
        assert!(
            matches!(ev, VtcEvent::Unknown { ref type_name, .. } if type_name == "VTCCaptionEvent")
        );
    }

    #[test]
    fn missing_type_and_malformed_known_type_rejected() {
        assert!(serde_json::from_value::<VtcEvent>(json!({"callId": "C1"})).is_err());
        assert!(serde_json::from_value::<VtcEvent>(json!({"@type": 7, "callId": "C1"})).is_err());
        // A known @type whose required `endReason` is missing is an error,
        // not Unknown.
        assert!(
            serde_json::from_value::<VtcEvent>(json!({"@type": "VTCCallEndEvent", "callId": "C1"}))
                .is_err()
        );
    }

    #[test]
    fn known_type_without_account_id_rejected() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L296-298 "every event in
        // this table carries an `accountId` (String) property".
        assert!(
            serde_json::from_value::<VtcEvent>(json!({
                "@type": "VTCActiveSpeakerEvent",
                "callId": "C1",
                "participantId": "P1"
            }))
            .is_err()
        );
    }

    #[test]
    fn type_name_is_the_wire_type() {
        // Oracle: the `@type` written in each hand-written frame.
        let ev: VtcEvent = serde_json::from_value(json!({
            "@type": "VTCActiveSpeakerEvent",
            "accountId": "A1",
            "callId": "C1",
            "participantId": "P1"
        }))
        .unwrap();
        assert_eq!(ev.type_name(), "VTCActiveSpeakerEvent");
        let ev: VtcEvent =
            serde_json::from_value(json!({"@type": "VTCCaptionEvent", "callId": "C1"})).unwrap();
        assert_eq!(ev.type_name(), "VTCCaptionEvent");
    }

    #[test]
    fn vendor_extras_preserved_without_type_tag() {
        let ev = round_trip(json!({
            "@type": "VTCActiveSpeakerEvent",
            "accountId": "A1",
            "callId": "C1",
            "participantId": "P1",
            "vendor.example:level": 0.8
        }));
        let VtcEvent::ActiveSpeaker(e) = ev else {
            panic!("wrong variant: {ev:?}")
        };
        assert!(e.extra.contains_key("vendor.example:level"));
        assert!(!e.extra.contains_key("@type"));
    }
}
