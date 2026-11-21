//! `SceneRegion/broadcast` request and response
//! (draft-atwood-jmap-scene-00 at jmap-chat-spec 93f2258,
//! §SceneRegion/broadcast).
//!
//! The method sends region events: "ephemeral, deployment-defined
//! announcements about a region as a whole", with "no originating user,
//! avatar, or object". Like `SceneObject/interact` it changes no record or
//! state string, and "The response has the shape of the
//! `SceneObject/interact` response". Validation is the server's job, not
//! these types'.

use std::collections::BTreeMap;

use jmap_types::{Id, SetError};
use serde::{Deserialize, Serialize};

/// One region event submitted with `SceneRegion/broadcast`: a
/// `SceneRegionBroadcast` object.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneRegionBroadcast {
    /// "The SceneRegion the event is about."
    pub region_id: Id,
    /// "The event name, in reverse-domain notation (e.g.,
    /// `\"com.example.game.round-start\"`). This specification defines no
    /// event names." Kept verbatim.
    pub event: String,
    /// "Event-specific payload, opaque to this specification. Default:
    /// `null`."
    #[serde(default)]
    pub data: Option<serde_json::Map<String, serde_json::Value>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SceneRegionBroadcast {
    /// Construct a [`SceneRegionBroadcast`] from its required fields.
    ///
    /// `data` defaults to `None` (`null`).
    pub fn new(region_id: Id, event: String) -> Self {
        Self {
            region_id,
            event,
            data: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Request arguments of `SceneRegion/broadcast`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneRegionBroadcastRequest {
    /// "The hosting account of the regions."
    pub account_id: Id,
    /// "A map from a client-chosen id to a [`SceneRegionBroadcast`]
    /// object." Keyed by `String`, like the creation-id maps of `/set`.
    pub events: BTreeMap<String, SceneRegionBroadcast>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SceneRegionBroadcastRequest {
    /// Construct a [`SceneRegionBroadcastRequest`] from its required
    /// fields.
    pub fn new(account_id: Id, events: BTreeMap<String, SceneRegionBroadcast>) -> Self {
        Self {
            account_id,
            events,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `SceneRegion/broadcast`.
///
/// `accepted` and `notAccepted` are "`null` if none"; both serialize as
/// `null` rather than being left out.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneRegionBroadcastResponse {
    /// "The account used for the call."
    pub account_id: Id,
    /// "The client ids of the events the server accepted, or `null` if
    /// none."
    #[serde(default)]
    pub accepted: Option<Vec<String>>,
    /// "A map from client id to a [`SetError`] for each event the server
    /// did not accept, or `null` if none."
    #[serde(default)]
    pub not_accepted: Option<BTreeMap<String, SetError>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SceneRegionBroadcastResponse {
    /// Construct a [`SceneRegionBroadcastResponse`] with nothing accepted
    /// or refused.
    pub fn new(account_id: Id) -> Self {
        Self {
            account_id,
            accepted: None,
            not_accepted: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Hand-written from the argument list of jmap-chat-spec 93f2258
    /// §SceneRegion/broadcast and the `SceneRegionEvent` example of
    /// draft-atwood-jmap-scene-wss-00 ("a round starts").
    fn request() -> serde_json::Value {
        json!({
            "accountId": "account-xyz",
            "events": {
                "e0": {
                    "regionId": "01J5ABC0000000000000000001",
                    "event": "com.example.game.round-start",
                    "data": {"round": 6, "buyTimeSeconds": 20}
                },
                "e1": {
                    "regionId": "01J5ABC0000000000000000001",
                    "event": "com.example.game.timer-expired",
                    "data": null
                }
            }
        })
    }

    #[test]
    fn decodes_and_round_trips_a_request() {
        let raw = request();
        let req: SceneRegionBroadcastRequest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(req.events["e0"].event, "com.example.game.round-start");
        assert_eq!(req.events["e0"].data.as_ref().unwrap()["round"], 6);
        assert_eq!(req.events["e1"].data, None);
        assert_eq!(serde_json::to_value(&req).unwrap(), raw);
    }

    #[test]
    fn data_defaults_to_null() {
        // "`data` (Object|null): Event-specific payload ... Default:
        // `null`."
        let b: SceneRegionBroadcast = serde_json::from_value(json!({
            "regionId": "R1",
            "event": "com.example.game.over"
        }))
        .unwrap();
        assert_eq!(
            b,
            SceneRegionBroadcast::new(Id::from("R1"), "com.example.game.over".into())
        );
        assert_eq!(serde_json::to_value(&b).unwrap()["data"], json!(null));
    }

    #[test]
    fn response_round_trips_and_empty_carries_nulls() {
        // "`accepted` (Id[]|null) ... or `null` if none"; "`notAccepted`
        // (Id[SetError]|null) ... or `null` if none".
        let raw = json!({
            "accountId": "account-xyz",
            "accepted": ["e0"],
            "notAccepted": {"e1": {"type": "forbidden"}}
        });
        let resp: SceneRegionBroadcastResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(resp.accepted, Some(vec!["e0".to_owned()]));
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);
        assert_eq!(
            serde_json::to_value(SceneRegionBroadcastResponse::new(Id::from("a"))).unwrap(),
            json!({"accountId": "a", "accepted": null, "notAccepted": null})
        );
    }
}
