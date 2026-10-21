//! draft-atwood-jmap-scene-wss-00 §Ephemeral Event Types — the
//! `SceneAvatarEvent`, `SceneObjectEvent` and `SceneInteractionEvent`
//! WebSocket frames.
//!
//! The event structs carry no `@type`; [`SceneEvent`] adds and dispatches
//! on it. Events "are not persisted as JMAP objects and carry no state
//! token".

use jmap_types::{impl_string_enum, Id};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Position;

/// The `event` of a [`SceneAvatarEvent`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AvatarEventKind {
    /// `"entered"`.
    Entered,
    /// `"left"`.
    Left,
    /// `"ejected"`.
    Ejected,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(AvatarEventKind, "a Scene avatar event string",
    "entered" => Entered,
    "left" => Left,
    "ejected" => Ejected,
);

/// The `event` of a [`SceneObjectEvent`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectEventKind {
    /// `"created"`.
    Created,
    /// `"updated"`.
    Updated,
    /// `"destroyed"`.
    Destroyed,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(ObjectEventKind, "a Scene object event string",
    "created" => Created,
    "updated" => Updated,
    "destroyed" => Destroyed,
);

/// An avatar entered, left or was ejected from a region
/// (draft-atwood-jmap-scene-wss-00 §SceneAvatarEvent). Server→client only.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneAvatarEvent {
    /// The region's hosting account: "the account in which the SceneRegion
    /// was created and which holds its SceneObject and SceneAvatar records"
    /// (draft-atwood-jmap-scene-wss-00 §Ephemeral Event Types). Region and
    /// object ids are unique only within it.
    pub account_id: Id,
    /// The SceneRegion.
    pub region_id: Id,
    /// The SceneAvatar.
    pub avatar_id: Id,
    /// Entered, left or ejected.
    pub event: AvatarEventKind,
    /// The authenticated user identity.
    pub user_id: String,
    /// The avatar's display name at event time.
    pub display_name: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An object in a subscribed region was created, updated or destroyed
/// (draft-atwood-jmap-scene-wss-00 §SceneObjectEvent). Server→client only;
/// the full object state is available via `SceneObject/get`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneObjectEvent {
    /// The region's hosting account: "the account in which the SceneRegion
    /// was created and which holds its SceneObject and SceneAvatar records"
    /// (draft-atwood-jmap-scene-wss-00 §Ephemeral Event Types). Region and
    /// object ids are unique only within it.
    pub account_id: Id,
    /// The SceneRegion.
    pub region_id: Id,
    /// The SceneObject.
    pub object_id: Id,
    /// Created, updated or destroyed.
    pub event: ObjectEventKind,
    /// The userId of the user who made the change; `None` (`null`) for
    /// server-initiated changes such as physics simulation.
    pub updated_by: Option<String>,
    /// The object's position after the change; `None` (`null`) for
    /// `"destroyed"`. Pairing with [`event`](Self::event) is not enforced.
    pub position: Option<Position>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A user interacted with an interactable object
/// (draft-atwood-jmap-scene-wss-00 §SceneInteractionEvent).
///
/// Bidirectional: a client submits one to the server, which validates it
/// (§Client-to-Server Submission) and relays it to the region's
/// subscribers. Validation is the server's job, not this type's.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneInteractionEvent {
    /// The region's hosting account: "the account in which the SceneRegion
    /// was created and which holds its SceneObject and SceneAvatar records"
    /// (draft-atwood-jmap-scene-wss-00 §Ephemeral Event Types). Region and
    /// object ids are unique only within it.
    pub account_id: Id,
    /// The SceneRegion.
    pub region_id: Id,
    /// The SceneObject interacted with.
    pub object_id: Id,
    /// The userId of the user who performed the interaction.
    ///
    /// Defaults to empty when absent: on a client-sent frame "a `userId`
    /// supplied in the frame is ignored" and the server sets it to the
    /// authenticated user's (draft-atwood-jmap-scene-wss-00
    /// §Client-to-Server Submission), so a frame may omit it. Always
    /// serialized.
    #[serde(default)]
    pub user_id: String,
    /// The interaction type: a registered short name (`"click"`,
    /// `"grab"`, `"release"`, `"activate"`) or a deployment-specific
    /// reverse-domain name. "Clients MUST ignore unrecognized values", so
    /// any string is kept verbatim.
    pub action: String,
    /// Action-specific payload, opaque and relayed without interpretation;
    /// `None` (`null`) when no additional data is needed.
    pub data: Option<serde_json::Map<String, serde_json::Value>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SceneInteractionEvent {
    /// Construct a [`SceneInteractionEvent`] from its required fields.
    ///
    /// `data` defaults to `None` (`null`).
    pub fn new(
        account_id: Id,
        region_id: Id,
        object_id: Id,
        user_id: String,
        action: String,
    ) -> Self {
        Self {
            account_id,
            region_id,
            object_id,
            user_id,
            action,
            data: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// A Scene ephemeral event frame, discriminated by its `@type`.
///
/// Unrecognised `@type` values deserialize to [`SceneEvent::Unknown`],
/// which keeps the discriminant and every other field so the frame
/// re-serializes verbatim. A frame with a known `@type` but a malformed
/// body is an error, not `Unknown`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum SceneEvent {
    /// `"SceneAvatarEvent"`.
    Avatar(SceneAvatarEvent),
    /// `"SceneObjectEvent"`.
    Object(SceneObjectEvent),
    /// `"SceneInteractionEvent"`.
    Interaction(SceneInteractionEvent),
    /// Any `@type` not recognized by this version of the library.
    Unknown {
        /// The original wire value of `@type`.
        type_name: String,
        /// Every other top-level field, verbatim.
        payload: serde_json::Map<String, serde_json::Value>,
    },
}

pub(crate) const AVATAR: &str = "SceneAvatarEvent";
pub(crate) const OBJECT: &str = "SceneObjectEvent";
pub(crate) const INTERACTION: &str = "SceneInteractionEvent";

impl SceneEvent {
    /// The event's `@type`, such as `"SceneAvatarEvent"`; for
    /// [`SceneEvent::Unknown`], the original wire value.
    pub fn type_name(&self) -> &str {
        match self {
            SceneEvent::Avatar(_) => AVATAR,
            SceneEvent::Object(_) => OBJECT,
            SceneEvent::Interaction(_) => INTERACTION,
            SceneEvent::Unknown { type_name, .. } => type_name,
        }
    }
}

impl Serialize for SceneEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let body = match self {
            SceneEvent::Avatar(e) => serde_json::to_value(e),
            SceneEvent::Object(e) => serde_json::to_value(e),
            SceneEvent::Interaction(e) => serde_json::to_value(e),
            SceneEvent::Unknown { payload, .. } => Ok(serde_json::Value::Object(payload.clone())),
        };
        let body = body.map_err(serde::ser::Error::custom)?;
        crate::wire::with_tag(body, self.type_name())
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for SceneEvent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::wire::from_map;
        let mut map = serde_json::Map::deserialize(d)?;
        let type_name = crate::wire::take_tag::<D::Error>(&mut map)?;
        Ok(match type_name.as_str() {
            AVATAR => SceneEvent::Avatar(from_map::<_, D>(map)?),
            OBJECT => SceneEvent::Object(from_map::<_, D>(map)?),
            INTERACTION => SceneEvent::Interaction(from_map::<_, D>(map)?),
            _ => SceneEvent::Unknown {
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

    fn parse(raw: serde_json::Value) -> SceneEvent {
        serde_json::from_value(raw).unwrap()
    }

    #[test]
    fn avatar_event_kinds_match_draft() {
        // Oracle: draft-atwood-jmap-scene-wss-00 (jmap-chat-spec 6bcd9d8)
        // L202-203 `event` (String):
        // "`\"entered\"`, `\"left\"`, or `\"ejected\"`."
        for (wire, kind) in [
            ("entered", AvatarEventKind::Entered),
            ("left", AvatarEventKind::Left),
            ("ejected", AvatarEventKind::Ejected),
        ] {
            assert_eq!(serde_json::to_value(&kind).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<AvatarEventKind>(json!(wire)).unwrap(),
                kind
            );
        }
    }

    #[test]
    fn object_event_kinds_match_draft() {
        // Oracle: L241-242 `event` (String): "`\"created\"`,
        // `\"updated\"`, or `\"destroyed\"`."
        for (wire, kind) in [
            ("created", ObjectEventKind::Created),
            ("updated", ObjectEventKind::Updated),
            ("destroyed", ObjectEventKind::Destroyed),
        ] {
            assert_eq!(serde_json::to_value(&kind).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<ObjectEventKind>(json!(wire)).unwrap(),
                kind
            );
        }
    }

    #[test]
    fn server_initiated_change_has_null_updated_by() {
        // Oracle: L244-247 `updatedBy` (String|null): "`null` for
        // server-initiated changes (e.g., physics simulation, scripted
        // behavior)." Hand-written frame.
        let raw = json!({
            "@type": "SceneObjectEvent",
            "accountId": "A",
            "regionId": "R1",
            "objectId": "O1",
            "event": "updated",
            "updatedBy": null,
            "position": [1.0, 2.0, 3.0]
        });
        let SceneEvent::Object(e) = parse(raw.clone()) else {
            panic!("wrong variant")
        };
        assert_eq!(e.event, ObjectEventKind::Updated);
        assert_eq!(e.updated_by, None);
        assert_eq!(e.position, Some([1.0, 2.0, 3.0]));
        assert_eq!(serde_json::to_value(SceneEvent::Object(e)).unwrap(), raw);
    }

    #[test]
    fn unrecognized_action_kept_verbatim() {
        // Oracle: L303-305 "Deployment-specific actions SHOULD use
        // reverse-domain notation ... Clients MUST ignore unrecognized
        // values", so an unregistered action must still decode.
        let SceneEvent::Interaction(e) = parse(json!({
            "@type": "SceneInteractionEvent",
            "accountId": "A",
            "regionId": "R1",
            "objectId": "O1",
            "userId": "user:alice@example.com",
            "action": "org.example.unknown",
            "data": null
        })) else {
            panic!("wrong variant")
        };
        assert_eq!(e.action, "org.example.unknown");
        assert_eq!(e.data, None);
    }

    #[test]
    fn position_must_be_three_numbers() {
        // Oracle: L249 `position` (Number[3]|null).
        let raw = json!({
            "@type": "SceneObjectEvent",
            "accountId": "A",
            "regionId": "R1",
            "objectId": "O1",
            "event": "updated",
            "updatedBy": null,
            "position": [1.0, 2.0]
        });
        assert!(serde_json::from_value::<SceneEvent>(raw).is_err());
    }

    #[test]
    fn unknown_type_and_extras_preserved() {
        let raw = json!({"@type": "SceneWeatherEvent", "regionId": "R1"});
        let msg = parse(raw.clone());
        assert_eq!(msg.type_name(), "SceneWeatherEvent");
        assert_eq!(serde_json::to_value(&msg).unwrap(), raw);

        let raw = json!({
            "@type": "SceneAvatarEvent",
            "accountId": "A",
            "regionId": "R1",
            "avatarId": "A1",
            "event": "left",
            "userId": "u",
            "displayName": "U",
            "vendor.example:x": 1
        });
        let msg = parse(raw.clone());
        assert!(matches!(msg, SceneEvent::Avatar(ref e)
            if e.extra.contains_key("vendor.example:x") && !e.extra.contains_key("@type")));
        assert_eq!(serde_json::to_value(&msg).unwrap(), raw);
    }

    #[test]
    fn missing_type_and_malformed_known_type_rejected() {
        // Oracle: L193-194 `@type` (String): `"SceneAvatarEvent"`; L202
        // `event` (String).
        assert!(serde_json::from_value::<SceneEvent>(json!({"regionId": "R1"})).is_err());
        assert!(serde_json::from_value::<SceneEvent>(json!({
            "@type": "SceneAvatarEvent",
            "accountId": "A",
            "regionId": "R1",
            "avatarId": "A1",
            "event": 1,
            "userId": "u",
            "displayName": "U"
        }))
        .is_err());
    }

    #[test]
    fn known_type_without_account_id_rejected() {
        // Oracle: draft-atwood-jmap-scene-wss-00 (jmap-chat-spec 6bcd9d8)
        // L178 "Every event carries an `accountId` (String) property".
        for raw in [
            json!({
                "@type": "SceneAvatarEvent",
                "regionId": "R1",
                "avatarId": "A1",
                "event": "left",
                "userId": "u",
                "displayName": "U"
            }),
            json!({
                "@type": "SceneObjectEvent",
                "regionId": "R1",
                "objectId": "O1",
                "event": "destroyed",
                "updatedBy": null,
                "position": null
            }),
            json!({
                "@type": "SceneInteractionEvent",
                "regionId": "R1",
                "objectId": "O1",
                "userId": "u",
                "action": "click",
                "data": null
            }),
        ] {
            assert!(serde_json::from_value::<SceneEvent>(raw).is_err());
        }
    }

    #[test]
    fn interaction_new_builds_draft_click_example() {
        // Oracle: draft-atwood-jmap-scene-wss-00 (jmap-chat-spec 6bcd9d8)
        // L1106-1114, the click event of the example session, and L307-311
        // "`data` (Object|null): ... `null` when no additional data is
        // needed."
        let event = SceneInteractionEvent::new(
            Id::from("account-xyz"),
            Id::from("01J5ABC0000000000000000001"),
            Id::from("01J5QBJ0000000000000000001"),
            "user:alice@example.com".to_owned(),
            "click".to_owned(),
        );
        assert_eq!(
            serde_json::to_value(SceneEvent::Interaction(event)).unwrap(),
            json!({
                "@type": "SceneInteractionEvent",
                "accountId": "account-xyz",
                "regionId": "01J5ABC0000000000000000001",
                "objectId": "01J5QBJ0000000000000000001",
                "userId": "user:alice@example.com",
                "action": "click",
                "data": null
            })
        );
    }
}
