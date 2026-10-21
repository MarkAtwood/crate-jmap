//! `SceneObject/interact` request and response
//! (draft-atwood-jmap-scene-00 §SceneObject/interact).
//!
//! The method submits interactions with interactable objects. It is
//! ephemeral: it changes no record or state string, and its response
//! "follows the shape of `/set`" so that failures are standard SetErrors.
//! Validation is the server's job, not these types'.

use std::collections::BTreeMap;

use jmap_types::{Id, SetError};
use serde::{Deserialize, Serialize};

/// One interaction submitted with `SceneObject/interact`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneInteraction {
    /// The SceneRegion containing the object.
    pub region_id: Id,
    /// The SceneObject being interacted with.
    pub object_id: Id,
    /// The interaction type, with the values of
    /// [`SceneInteractionEvent::action`](crate::SceneInteractionEvent::action):
    /// `"click"`, `"grab"`, `"release"`, `"activate"`, or a
    /// deployment-specific reverse-domain value. Kept verbatim.
    pub action: String,
    /// Action-specific payload, opaque to the draft. Default `None`
    /// (`null`).
    #[serde(default)]
    pub data: Option<serde_json::Map<String, serde_json::Value>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SceneInteraction {
    /// Construct a [`SceneInteraction`] from its required fields.
    ///
    /// `data` defaults to `None` (`null`).
    pub fn new(region_id: Id, object_id: Id, action: String) -> Self {
        Self {
            region_id,
            object_id,
            action,
            data: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// Request arguments of `SceneObject/interact`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneObjectInteractRequest {
    /// "The hosting account of the regions."
    pub account_id: Id,
    /// "A map from a client-chosen id to a SceneInteraction object." Keyed
    /// by `String`, like the creation-id maps of `/set`.
    pub interactions: BTreeMap<String, SceneInteraction>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SceneObjectInteractRequest {
    /// Construct a [`SceneObjectInteractRequest`] from its required fields.
    pub fn new(account_id: Id, interactions: BTreeMap<String, SceneInteraction>) -> Self {
        Self {
            account_id,
            interactions,
            extra: serde_json::Map::new(),
        }
    }
}

/// Response arguments of `SceneObject/interact`.
///
/// `accepted` and `notAccepted` are "`null` if none"; both serialize as
/// `null` rather than being left out.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneObjectInteractResponse {
    /// "The account used for the call."
    pub account_id: Id,
    /// "The client ids of the interactions the server accepted, or `null`
    /// if none."
    #[serde(default)]
    pub accepted: Option<Vec<String>>,
    /// "A map from client id to a SetError for each interaction the server
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

impl SceneObjectInteractResponse {
    /// Construct a [`SceneObjectInteractResponse`] with nothing accepted
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

    /// The example request of draft-atwood-jmap-scene-00 §SceneObject/interact
    /// (jmap-chat-spec 6bcd9d8 L2063-2091), arguments only.
    fn draft_request() -> serde_json::Value {
        json!({
            "accountId": "acct-001",
            "interactions": {
                "i0": {
                    "regionId": "01J5ABC0000000000000000001",
                    "objectId": "01J5QBJ0000000000000000002",
                    "action": "click",
                    "data": null
                },
                "i1": {
                    "regionId": "01J5ABC0000000000000000001",
                    "objectId": "01J5QBJ0000000000000000003",
                    "action": "grab",
                    "data": {"hand": "left"}
                }
            }
        })
    }

    #[test]
    fn decodes_and_round_trips_the_draft_request() {
        let raw = draft_request();
        let req: SceneObjectInteractRequest = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(req.account_id.as_ref(), "acct-001");
        let i1 = &req.interactions["i1"];
        assert_eq!(i1.object_id.as_ref(), "01J5QBJ0000000000000000003");
        assert_eq!(i1.action, "grab");
        assert_eq!(i1.data.as_ref().unwrap()["hand"], "left");
        assert_eq!(req.interactions["i0"].data, None);
        assert_eq!(serde_json::to_value(&req).unwrap(), raw);
    }

    #[test]
    fn data_defaults_to_null() {
        // L2038-2039 "`data` (Object|null): ... Default: `null`."
        let i: SceneInteraction = serde_json::from_value(json!({
            "regionId": "01J5ABC0000000000000000001",
            "objectId": "01J5QBJ0000000000000000002",
            "action": "activate"
        }))
        .unwrap();
        assert_eq!(i.data, None);
    }

    #[test]
    fn decodes_and_round_trips_the_draft_response() {
        // L2093-2110: "the second object has `interactable: false`".
        let raw = json!({
            "accountId": "acct-001",
            "accepted": ["i0"],
            "notAccepted": {
                "i1": {
                    "type": "forbidden",
                    "description": "The object is not interactable."
                }
            }
        });
        let resp: SceneObjectInteractResponse = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(resp.accepted, Some(vec!["i0".to_owned()]));
        assert_eq!(
            resp.not_accepted.as_ref().unwrap()["i1"]
                .error_type
                .as_str(),
            "forbidden"
        );
        assert_eq!(serde_json::to_value(&resp).unwrap(), raw);
    }

    #[test]
    fn empty_response_carries_explicit_nulls() {
        // L2046-2050: `accepted` and `notAccepted` are "`null` if none".
        let resp = SceneObjectInteractResponse::new(Id::from("acct-001"));
        assert_eq!(
            serde_json::to_value(&resp).unwrap(),
            json!({ "accountId": "acct-001", "accepted": null, "notAccepted": null })
        );
    }
}
