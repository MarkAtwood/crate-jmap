//! draft-atwood-jmap-scene-wss-00 §Scene Stream Control Messages — the
//! client→server `SceneStreamEnable` and `SceneStreamDisable` frames and
//! the `eventTypes` vocabulary.
//!
//! The frame structs carry no `@type`; [`SceneStreamMessage`] adds and
//! dispatches on it, as [`SceneEvent`](crate::SceneEvent) does for the
//! events.

use jmap_types::Id;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::event::{AVATAR, INTERACTION, OBJECT};

/// The recognized values of [`SceneStreamEnable::event_types`]
/// (draft-atwood-jmap-scene-wss-00 §SceneStreamEnable).
///
/// "Unrecognized values in `eventTypes` that appear alongside recognized
/// values MUST be silently ignored"; an `eventTypes` that is empty or holds
/// only unrecognized values is an `invalidArguments` `RequestError`.
pub const SCENE_EVENT_TYPES: &[&str] = &[AVATAR, OBJECT, INTERACTION];

/// Client→server: start or replace the ephemeral Scene event subscription
/// on this WebSocket connection (draft-atwood-jmap-scene-wss-00
/// §SceneStreamEnable).
///
/// Both filters default to `null`, so [`Default`] subscribes to every
/// event of every region in which the user has an active avatar. `None`
/// serializes as an explicit `null`; an absent property deserializes as
/// `None`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneStreamEnable {
    /// Client-specified identifier for this frame. A `RequestError` this
    /// frame causes carries it as its `requestId` (RFC 8887 §4.3.4).
    /// Absent unless the client sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// SceneRegion ids to receive events for; `None` means every region in
    /// which the user has an active avatar.
    #[serde(default)]
    pub region_ids: Option<Vec<Id>>,
    /// Event type names to receive; `None` means all. Recognized values
    /// are [`SCENE_EVENT_TYPES`]; other values are kept verbatim.
    #[serde(default)]
    pub event_types: Option<Vec<String>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Client→server: cancel all ephemeral Scene event delivery on this
/// connection (draft-atwood-jmap-scene-wss-00 §SceneStreamDisable).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SceneStreamDisable {
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A Scene stream control frame, discriminated by its `@type`.
///
/// Unrecognised `@type` values deserialize to
/// [`SceneStreamMessage::Unknown`], which keeps the discriminant and every
/// other field so the frame re-serializes verbatim. A frame with a known
/// `@type` but a malformed body is an error, not `Unknown`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneStreamMessage {
    /// `"SceneStreamEnable"`.
    Enable(SceneStreamEnable),
    /// `"SceneStreamDisable"`.
    Disable(SceneStreamDisable),
    /// Any `@type` not recognized by this version of the library.
    Unknown {
        /// The original wire value of `@type`.
        type_name: String,
        /// Every other top-level field, verbatim.
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

const ENABLE: &str = "SceneStreamEnable";
const DISABLE: &str = "SceneStreamDisable";

impl Serialize for SceneStreamMessage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (tag, body) = match self {
            SceneStreamMessage::Enable(e) => (ENABLE, serde_json::to_value(e)),
            SceneStreamMessage::Disable(e) => (DISABLE, serde_json::to_value(e)),
            SceneStreamMessage::Unknown { type_name, payload } => (
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

impl<'de> Deserialize<'de> for SceneStreamMessage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::wire::from_map;
        let mut map = serde_json::Map::deserialize(d)?;
        let type_name = crate::wire::take_tag::<D::Error>(&mut map)?;
        Ok(match type_name.as_str() {
            ENABLE => SceneStreamMessage::Enable(from_map::<_, D>(map)?),
            DISABLE => SceneStreamMessage::Disable(from_map::<_, D>(map)?),
            _ => SceneStreamMessage::Unknown {
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
    fn round_trip(raw: serde_json::Value) -> SceneStreamMessage {
        let msg: SceneStreamMessage = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&msg).unwrap(), raw);
        msg
    }

    #[test]
    fn enable_absent_filters_default_to_null() {
        // Oracle: draft-atwood-jmap-scene-wss-00 L396 and L402 "Default:
        // `null`." for `regionIds` and `eventTypes`.
        let msg: SceneStreamMessage =
            serde_json::from_value(json!({"@type": "SceneStreamEnable"})).unwrap();
        assert_eq!(
            msg,
            SceneStreamMessage::Enable(SceneStreamEnable::default())
        );
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({"@type": "SceneStreamEnable", "regionIds": null, "eventTypes": null})
        );
    }

    #[test]
    fn enable_id_is_typed_not_extra() {
        // Oracle: draft-atwood-jmap-scene-wss-00 L450-452 "`id` (String,
        // optional): A client-specified identifier for this message. A
        // `RequestError` caused by this message carries this value as its
        // `requestId`".
        let msg = round_trip(json!({
            "@type": "SceneStreamEnable",
            "id": "e1",
            "regionIds": null,
            "eventTypes": null
        }));
        assert!(matches!(msg, SceneStreamMessage::Enable(ref e)
            if e.id.as_deref() == Some("e1") && !e.extra.contains_key("id")));
    }

    #[test]
    fn enable_empty_region_ids_kept_distinct_from_null() {
        // Oracle: L497-498 "(or `regionIds` is an empty array), the
        // subscription is effectively empty", unlike `null` (L394-396), so
        // `[]` must not collapse to `null`.
        let msg = round_trip(json!({
            "@type": "SceneStreamEnable",
            "regionIds": [],
            "eventTypes": null
        }));
        assert!(matches!(msg, SceneStreamMessage::Enable(ref e) if e.region_ids == Some(vec![])));
    }

    #[test]
    fn enable_empty_event_types_kept_distinct_from_null() {
        // Oracle: L503-506 "If `eventTypes` is an empty array ... the
        // server MUST respond with a `RequestError`", while `null` means
        // all event types (L399), so the server must see `[]`.
        let msg = round_trip(json!({
            "@type": "SceneStreamEnable",
            "regionIds": null,
            "eventTypes": []
        }));
        assert!(matches!(msg, SceneStreamMessage::Enable(ref e) if e.event_types == Some(vec![])));
    }

    #[test]
    fn enable_unrecognized_event_type_kept_verbatim() {
        // Oracle: L556-559: with `["SceneFooBarEvent",
        // "SceneAvatarEvent"]` "the unrecognized value would be silently
        // ignored" by the server, so the type keeps it for the server to
        // classify rather than failing.
        let msg = round_trip(json!({
            "@type": "SceneStreamEnable",
            "regionIds": null,
            "eventTypes": ["SceneFooBarEvent", "SceneAvatarEvent"]
        }));
        assert!(matches!(msg, SceneStreamMessage::Enable(ref e)
            if e.event_types.as_deref()
                == Some(&["SceneFooBarEvent".to_owned(), "SceneAvatarEvent".to_owned()][..])));
    }

    #[test]
    fn event_types_vocabulary_matches_draft() {
        // Oracle: L400-401 "Recognized values are: `\"SceneAvatarEvent\"`,
        // `\"SceneObjectEvent\"`, and `\"SceneInteractionEvent\"`."
        assert_eq!(
            SCENE_EVENT_TYPES,
            [
                "SceneAvatarEvent",
                "SceneObjectEvent",
                "SceneInteractionEvent"
            ]
        );
    }

    #[test]
    fn unknown_type_and_extras_preserved() {
        let msg = round_trip(json!({"@type": "WebSocketPushEnable", "dataTypes": ["SceneObject"]}));
        assert!(
            matches!(msg, SceneStreamMessage::Unknown { ref type_name, .. }
            if type_name == "WebSocketPushEnable")
        );
        let msg = round_trip(json!({"@type": "SceneStreamDisable", "vendor.example:x": 1}));
        assert!(matches!(msg, SceneStreamMessage::Disable(ref d)
            if d.extra.contains_key("vendor.example:x") && !d.extra.contains_key("@type")));
    }

    #[test]
    fn missing_type_and_malformed_known_type_rejected() {
        // Oracle: L391 "`@type` (String): MUST be `\"SceneStreamEnable\"`";
        // L393 `regionIds` is "String[]|null".
        assert!(serde_json::from_value::<SceneStreamMessage>(json!({"regionIds": null})).is_err());
        assert!(serde_json::from_value::<SceneStreamMessage>(
            json!({"@type": "SceneStreamEnable", "regionIds": "01J5ABC0000000000000000001"})
        )
        .is_err());
    }
}
