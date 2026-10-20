//! draft-atwood-jmap-vtc-wss-00 §VTC Stream Control Messages — the
//! client→server `VTCStreamEnable` and `VTCStreamDisable` frames and the
//! `eventTypes` vocabulary.
//!
//! The frame structs carry no `@type`; [`VtcStreamMessage`] adds and
//! dispatches on it, as [`VtcEvent`](crate::VtcEvent) does for the
//! server→client events.

use jmap_types::Id;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::event::{
    ACTIVE_SPEAKER, CALL_END, GATEWAY_SIGNAL, MEDIA_STATE, PARTICIPANT, RECORDING_STATE, RING,
    UNMUTE_REQUEST,
};

/// The recognized values of [`VtcStreamEnable::event_types`]
/// (draft-atwood-jmap-vtc-wss-00 §VTCStreamEnable).
///
/// "Unrecognized values in `eventTypes` that appear alongside recognized
/// values MUST be silently ignored"; an `eventTypes` that is empty or holds
/// only unrecognized values is an `invalidArguments` `RequestError`.
pub const VTC_EVENT_TYPES: &[&str] = &[
    RING,
    CALL_END,
    GATEWAY_SIGNAL,
    PARTICIPANT,
    MEDIA_STATE,
    ACTIVE_SPEAKER,
    UNMUTE_REQUEST,
    RECORDING_STATE,
];

/// Client→server: start or replace the ephemeral VTC event subscription on
/// this WebSocket connection (draft-atwood-jmap-vtc-wss-00
/// §VTCStreamEnable).
///
/// Both filters default to `null`, so [`Default`] subscribes to every
/// event of every call the user is in. `None` serializes as an explicit
/// `null`; an absent property deserializes as `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VtcStreamEnable {
    /// VTCCall ids to receive events for; `None` means every call in which
    /// the user is a current participant.
    #[serde(default)]
    pub call_ids: Option<Vec<Id>>,
    /// Event type names to receive; `None` means all. Recognized values
    /// are [`VTC_EVENT_TYPES`]; other values are kept verbatim.
    #[serde(default)]
    pub event_types: Option<Vec<String>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Client→server: cancel all ephemeral VTC event delivery on this
/// connection (draft-atwood-jmap-vtc-wss-00 §VTCStreamDisable).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VtcStreamDisable {
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A VTC stream control frame, discriminated by its `@type`.
///
/// Unrecognised `@type` values deserialize to
/// [`VtcStreamMessage::Unknown`], which keeps the discriminant and every
/// other field so the frame re-serializes verbatim. A frame with a known
/// `@type` but a malformed body is an error, not `Unknown`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VtcStreamMessage {
    /// `"VTCStreamEnable"`.
    Enable(VtcStreamEnable),
    /// `"VTCStreamDisable"`.
    Disable(VtcStreamDisable),
    /// Any `@type` not recognized by this version of the library.
    Unknown {
        /// The original wire value of `@type`.
        type_name: String,
        /// Every other top-level field, verbatim.
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

const ENABLE: &str = "VTCStreamEnable";
const DISABLE: &str = "VTCStreamDisable";

impl Serialize for VtcStreamMessage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (tag, body) = match self {
            VtcStreamMessage::Enable(e) => (ENABLE, serde_json::to_value(e)),
            VtcStreamMessage::Disable(e) => (DISABLE, serde_json::to_value(e)),
            VtcStreamMessage::Unknown { type_name, payload } => (
                type_name.as_str(),
                Ok(serde_json::Value::Object(payload.clone())),
            ),
        };
        let body = body.map_err(serde::ser::Error::custom)?;
        crate::wire::with_tag(body, tag)
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for VtcStreamMessage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::wire::from_map;
        let mut map = serde_json::Map::deserialize(d)?;
        let type_name = crate::wire::take_tag::<D::Error>(&mut map)?;
        Ok(match type_name.as_str() {
            ENABLE => VtcStreamMessage::Enable(from_map::<_, D>(map)?),
            DISABLE => VtcStreamMessage::Disable(from_map::<_, D>(map)?),
            _ => VtcStreamMessage::Unknown {
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
    fn round_trip(raw: serde_json::Value) -> VtcStreamMessage {
        let msg: VtcStreamMessage = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&msg).unwrap(), raw);
        msg
    }

    #[test]
    fn enable_example_round_trips() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L157-163 example.
        let msg = round_trip(json!({
            "@type": "VTCStreamEnable",
            "callIds": ["01J4XKZQN4MWVT8PPBEHTJ3AB"],
            "eventTypes": ["VTCParticipantEvent", "VTCMediaStateEvent",
                            "VTCActiveSpeakerEvent"]
        }));
        let VtcStreamMessage::Enable(e) = msg else {
            panic!("wrong variant: {msg:?}")
        };
        assert_eq!(e.call_ids.unwrap()[0].as_ref(), "01J4XKZQN4MWVT8PPBEHTJ3AB");
        assert_eq!(
            e.event_types.unwrap(),
            [
                "VTCParticipantEvent",
                "VTCMediaStateEvent",
                "VTCActiveSpeakerEvent"
            ]
        );
    }

    #[test]
    fn enable_null_event_types_example_round_trips() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L430-434 example, whose
        // `"eventTypes": null` means "receive all event types" (L148-149).
        let msg = round_trip(json!({
            "@type": "VTCStreamEnable",
            "callIds": ["01J4XKZQN4MWVT8PPBEHTJ3AB"],
            "eventTypes": null
        }));
        assert!(matches!(msg, VtcStreamMessage::Enable(ref e) if e.event_types.is_none()));
    }

    #[test]
    fn enable_absent_filters_default_to_null() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L145 and L153 "Default:
        // `null`." for `callIds` and `eventTypes`.
        let msg: VtcStreamMessage =
            serde_json::from_value(json!({"@type": "VTCStreamEnable"})).unwrap();
        assert_eq!(msg, VtcStreamMessage::Enable(VtcStreamEnable::default()));
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({"@type": "VTCStreamEnable", "callIds": null, "eventTypes": null})
        );
    }

    #[test]
    fn enable_empty_call_ids_kept_distinct_from_null() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L174-175 "(or `callIds` is
        // an empty array), the subscription is effectively empty", unlike
        // `null` (L143-144), so `[]` must not collapse to `null`.
        let msg = round_trip(json!({
            "@type": "VTCStreamEnable",
            "callIds": [],
            "eventTypes": null
        }));
        assert!(matches!(msg, VtcStreamMessage::Enable(ref e) if e.call_ids == Some(vec![])));
    }

    #[test]
    fn enable_unrecognized_event_type_kept_verbatim() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L177-178: unrecognized
        // values "MUST be silently ignored" by the server, so the type
        // keeps them for the server to classify rather than failing.
        let msg = round_trip(json!({
            "@type": "VTCStreamEnable",
            "callIds": null,
            "eventTypes": ["VTCRingEvent", "VTCCaptionEvent"]
        }));
        assert!(matches!(msg, VtcStreamMessage::Enable(ref e)
            if e.event_types.as_deref() == Some(&["VTCRingEvent".to_owned(), "VTCCaptionEvent".to_owned()][..])));
    }

    #[test]
    fn disable_example_round_trips() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L207-211 and L516-518
        // examples.
        let msg = round_trip(json!({"@type": "VTCStreamDisable"}));
        assert_eq!(msg, VtcStreamMessage::Disable(VtcStreamDisable::default()));
    }

    #[test]
    fn event_types_vocabulary_matches_draft() {
        // Oracle: draft-atwood-jmap-vtc-wss-00 L149-153 "Recognized values
        // are: `\"VTCRingEvent\"`, `\"VTCCallEndEvent\"`,
        // `\"VTCGatewaySignal\"`, `\"VTCParticipantEvent\"`,
        // `\"VTCMediaStateEvent\"`, `\"VTCActiveSpeakerEvent\"`,
        // `\"VTCUnmuteRequestEvent\"`, and `\"VTCRecordingStateEvent\"`."
        assert_eq!(
            VTC_EVENT_TYPES,
            [
                "VTCRingEvent",
                "VTCCallEndEvent",
                "VTCGatewaySignal",
                "VTCParticipantEvent",
                "VTCMediaStateEvent",
                "VTCActiveSpeakerEvent",
                "VTCUnmuteRequestEvent",
                "VTCRecordingStateEvent"
            ]
        );
    }

    #[test]
    fn unknown_type_and_extras_preserved() {
        let msg = round_trip(json!({"@type": "WebSocketPushEnable", "dataTypes": ["VTCCall"]}));
        assert!(
            matches!(msg, VtcStreamMessage::Unknown { ref type_name, .. }
            if type_name == "WebSocketPushEnable")
        );
        let msg = round_trip(json!({"@type": "VTCStreamDisable", "vendor.example:x": 1}));
        assert!(matches!(msg, VtcStreamMessage::Disable(ref d)
            if d.extra.contains_key("vendor.example:x") && !d.extra.contains_key("@type")));
    }

    #[test]
    fn missing_type_and_malformed_known_type_rejected() {
        // Oracle: L140 "`@type` (String): MUST be `\"VTCStreamEnable\"`";
        // L142 `callIds` is "String[]|null".
        assert!(serde_json::from_value::<VtcStreamMessage>(json!({"callIds": null})).is_err());
        assert!(serde_json::from_value::<VtcStreamMessage>(
            json!({"@type": "VTCStreamEnable", "callIds": "01J4XKZQN4MWVT8PPBEHTJ3AB"})
        )
        .is_err());
    }
}
