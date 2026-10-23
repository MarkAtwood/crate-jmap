//! draft-atwood-jmap-scene-00 §SceneRegion, including GeoAnchor.

use jmap_types::{Id, UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::coordinates::identity_orientation;
use crate::{Orientation, Position, SceneBounds};

/// The advisory `viewHint` rendering mode of a [`SceneRegion`]
/// (draft-atwood-jmap-scene-00 §SceneRegion Object Fields).
///
/// "Clients MUST NOT fail when encountering an unrecognized value; they
/// SHOULD fall back to `\"3d\"`." Deployment-specific reverse-domain hints
/// land in `Other` and round-trip verbatim. A `null` hint, "treated as
/// `\"3d\"`", is `None` on [`SceneRegion::view_hint`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ViewHint {
    /// `"3d"`: standard 3D perspective.
    ThreeD,
    /// `"2d-topdown"`: top-down 2D view.
    TwoDTopDown,
    /// `"2d-side"`: side-scrolling 2D view.
    TwoDSide,
    /// `"ar"`: augmented-reality overlay anchored via `geoAnchor`.
    Ar,
    /// A reverse-domain or other value not defined by the draft.
    Other(String),
}

impl_string_enum!(ViewHint, "a Scene view hint string",
    "3d" => ThreeD,
    "2d-topdown" => TwoDTopDown,
    "2d-side" => TwoDSide,
    "ar" => Ar,
);

/// Who may enter a [`SceneRegion`]: its `accessPolicy`
/// (draft-atwood-jmap-scene-00 §SceneRegion Object Fields). Draft default
/// `"public"`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AccessPolicy {
    /// `"public"`: any authenticated user.
    #[default]
    Public,
    /// `"invite"`: the users listed in [`SceneRegion::invited_user_ids`].
    Invite,
    /// `"space"`: members of the bound Space (with JMAP Chat).
    Space,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(AccessPolicy, "a Scene access policy string",
    "public" => Public,
    "invite" => Invite,
    "space" => Space,
);

/// Anchors a region's local coordinate origin to a point in a reference
/// frame (draft-atwood-jmap-scene-00 §SceneRegion, `geoAnchor`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoAnchor {
    /// The coordinate reference frame: `None` (`null`) is Earth WGS84;
    /// otherwise a URL, DID, URN UUID, DNS name or reverse-domain
    /// identifier. "Clients that do not recognize the `referenceFrame`
    /// value MUST NOT attempt to render the region in AR mode."
    pub reference_frame: Option<String>,
    /// Latitude in decimal degrees (WGS84: -90.0 to 90.0).
    pub latitude: f64,
    /// Longitude in decimal degrees (WGS84: -180.0 to 180.0).
    pub longitude: f64,
    /// Altitude in meters above the reference frame's datum; `None`
    /// (`null`) when unknown, treated as surface level.
    pub altitude: Option<f64>,
    /// Clockwise degrees from North to the region's negative Z axis,
    /// viewed from above. Draft default `0`: negative Z points North,
    /// positive X East and positive Z South.
    #[serde(default)]
    pub heading: f64,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A discrete bounded spatial environment users can enter
/// (draft-atwood-jmap-scene-00 §SceneRegion).
///
/// `T|null` properties are `Option<T>` and serialize `None` as `null`.
/// Properties whose absence and `null` the draft's examples keep apart
/// are `Option<Option<T>>`: `None` when absent, `Some(None)` for `null`.
/// That covers the binding properties (`chatId`, `spaceId`, `channelId`,
/// `activeCallId`), "present only when the corresponding companion
/// capability is advertised", and `inputUri`, `geoAnchor` and
/// `worldState`, which the draft's full SceneRegion examples omit
/// (L894-954, L2808-2862, L3070-3100).
///
/// `simulationUri` and `inputUri` are peer-supplied and untrusted; nothing
/// here validates them.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneRegion {
    /// A ULID assigned at creation.
    pub id: Id,
    /// The account that owns this region.
    pub account_id: Id,
    /// "The userId of the user who created this region": the "region
    /// owner" of the draft's permission rules. Immutable and server-set.
    pub owner_id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// The spatial extent of the region.
    pub bounds: SceneBounds,
    /// Deployment-defined environment settings. "Clients MUST ignore
    /// unrecognized keys."
    pub environment: Option<serde_json::Map<String, serde_json::Value>>,
    /// The real-time simulation layer endpoint; `None` for a static scene.
    pub simulation_uri: Option<String>,
    /// High-frequency input/response channel endpoint.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_uri: Option<Option<String>>,
    /// Where new avatars appear. Draft default `[0, 0, 0]`.
    #[serde(default)]
    pub spawn_position: Position,
    /// Orientation of new avatars. Draft default `[0, 0, 0, 1]`.
    #[serde(default = "identity_orientation")]
    pub spawn_orientation: Orientation,
    /// Number of avatars currently present.
    pub active_avatar_count: u64,
    /// Advisory rendering mode; `None` (`null`) "is treated as `\"3d\"`".
    pub view_hint: Option<ViewHint>,
    /// Anchor of the region's origin in a reference frame.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub geo_anchor: Option<Option<GeoAnchor>>,
    /// Who may enter. Draft default `"public"`.
    #[serde(default)]
    pub access_policy: AccessPolicy,
    /// "The userIds admitted to the region when `accessPolicy` is
    /// `\"invite\"`. Default: `[]`." The server returns it only to callers
    /// who may update the region and `[]` to everyone else.
    #[serde(default)]
    pub invited_user_ids: Vec<String>,
    /// Time the region was created.
    pub created_at: UTCDate,
    /// Time the region was last modified.
    pub updated_at: UTCDate,
    /// Deployment-defined region-wide application state, stored and
    /// relayed without interpretation.
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub world_state: Option<Option<serde_json::Map<String, serde_json::Value>>>,
    /// Bound Chat (`urn:ietf:params:jmap:chat`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub chat_id: Option<Option<Id>>,
    /// Bound Space (`urn:ietf:params:jmap:chat`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub space_id: Option<Option<Id>>,
    /// Bound channel Chat within the Space (`urn:ietf:params:jmap:chat`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub channel_id: Option<Option<Id>>,
    /// Bound active VTCCall (`urn:ietf:params:jmap:vtc`).
    #[serde(
        default,
        deserialize_with = "crate::wire::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub active_call_id: Option<Option<Id>>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IDENTITY_ORIENTATION;
    use serde_json::json;

    /// A standalone region (no companion capabilities), hand-written from
    /// the field list of draft-atwood-jmap-scene-00 L263-402.
    fn standalone() -> serde_json::Value {
        json!({
            "id": "01J5ABC0000000000000000003",
            "accountId": "A1",
            "ownerId": "user:curator@example.com",
            "name": "Main Plaza",
            "description": null,
            "bounds": {"min": [-10.0, 0.0, -10.0], "max": [10.0, 5.0, 10.0]},
            "environment": null,
            "simulationUri": null,
            "inputUri": null,
            "spawnPosition": [1.0, 0.0, 2.0],
            "spawnOrientation": [0.0, 0.0, 0.0, 1.0],
            "activeAvatarCount": 0,
            "viewHint": null,
            "geoAnchor": null,
            "accessPolicy": "invite",
            "invitedUserIds": ["user:alice@example.com"],
            "createdAt": "2026-06-01T10:00:00Z",
            "updatedAt": "2026-06-01T10:00:00Z",
            "worldState": null
        })
    }

    #[test]
    fn view_hint_wire_values() {
        // Oracle: L297 "Values defined by this specification: `\"3d\"` ...,
        // `\"2d-topdown\"` ..., `\"2d-side\"` ..., `\"ar\"`".
        let v: Vec<ViewHint> =
            serde_json::from_value(json!(["3d", "2d-topdown", "2d-side", "ar"])).unwrap();
        assert_eq!(
            v,
            [
                ViewHint::ThreeD,
                ViewHint::TwoDTopDown,
                ViewHint::TwoDSide,
                ViewHint::Ar
            ]
        );
    }

    #[test]
    fn reverse_domain_view_hint_round_trips() {
        // Oracle: L297 "Clients MUST NOT fail when encountering an
        // unrecognized value ... Deployment-specific view hints SHOULD use
        // reverse-domain notation (e.g., `\"com.example.isometric\"`)."
        let h: ViewHint = serde_json::from_value(json!("com.example.isometric")).unwrap();
        assert_eq!(h, ViewHint::Other("com.example.isometric".to_owned()));
        assert_eq!(
            serde_json::to_value(&h).unwrap(),
            json!("com.example.isometric")
        );
    }

    #[test]
    fn access_policy_wire_values_and_default() {
        // Oracle: L393 "Values: `\"public\"` ..., `\"invite\"` ...,
        // `\"space\"` ... Default: `\"public\"`."
        let v: Vec<AccessPolicy> =
            serde_json::from_value(json!(["public", "invite", "space"])).unwrap();
        assert_eq!(
            v,
            [
                AccessPolicy::Public,
                AccessPolicy::Invite,
                AccessPolicy::Space
            ]
        );
        assert_eq!(AccessPolicy::default(), AccessPolicy::Public);
        let p: AccessPolicy = serde_json::from_value(json!("x-friends")).unwrap();
        assert_eq!(serde_json::to_value(&p).unwrap(), json!("x-friends"));
    }

    #[test]
    fn missing_defaulted_fields_take_draft_defaults() {
        // Oracle: L288 spawnPosition "Default: `[0, 0, 0]`"; L291
        // spawnOrientation "Default: `[0, 0, 0, 1]` (identity)"; L393
        // accessPolicy "Default: `\"public\"`".
        let mut raw = standalone();
        for key in ["spawnPosition", "spawnOrientation", "accessPolicy"] {
            raw.as_object_mut().unwrap().remove(key);
        }
        let r: SceneRegion = serde_json::from_value(raw).unwrap();
        assert_eq!(r.spawn_position, [0.0, 0.0, 0.0]);
        assert_eq!(r.spawn_orientation, IDENTITY_ORIENTATION);
        assert_eq!(r.access_policy, AccessPolicy::Public);
    }

    #[test]
    fn heading_defaults_to_zero() {
        // Oracle: draft-atwood-jmap-scene-00 (jmap-chat-spec 6bcd9d8)
        // L387 heading "Default: `0` (negative Z points North)".
        let a: GeoAnchor = serde_json::from_value(json!({
            "referenceFrame": null,
            "latitude": 47.6301,
            "longitude": -122.3154,
            "altitude": null
        }))
        .unwrap();
        assert_eq!(a.heading, 0.0);
        assert_eq!(a.reference_frame, None);
        assert_eq!(a.altitude, None);
    }

    #[test]
    fn geo_anchor_requires_latitude_and_longitude() {
        // Oracle: L316-320 type latitude and longitude "(Number)", not
        // nullable and without a default.
        for field in ["latitude", "longitude"] {
            let mut raw = json!({"referenceFrame": null, "latitude": 1, "longitude": 2,
                "altitude": null, "heading": 0});
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<GeoAnchor>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn standalone_region_omits_binding_fields() {
        // Oracle: L406 "The following fields are present only when the
        // corresponding companion capability is advertised".
        let raw = standalone();
        let r: SceneRegion = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(r.chat_id, None);
        assert_eq!(r.active_call_id, None);
        assert_eq!(r.input_uri, Some(None));
        assert_eq!(r.geo_anchor, Some(None));
        assert_eq!(r.world_state, Some(None));
        assert_eq!(serde_json::to_value(&r).unwrap(), raw);
    }

    #[test]
    fn null_binding_distinct_from_absent() {
        // Oracle: L409 chatId "`null` if the region has no chat binding";
        // L418 activeCallId "`null` if no call is active".
        let mut raw = standalone();
        raw["chatId"] = json!(null);
        raw["activeCallId"] = json!("01J4XKZQN4MWVT8PPBEHTJ3AB");
        let r: SceneRegion = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(r.chat_id, Some(None));
        assert_eq!(
            r.active_call_id,
            Some(Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB")))
        );
        assert_eq!(r.space_id, None);
        assert_eq!(serde_json::to_value(&r).unwrap(), raw);
    }

    #[test]
    fn nullable_fields_serialize_as_null() {
        // Oracle: L272 description, L278 environment, L281 simulationUri
        // and L296 viewHint are "|null": present properties with a null
        // value.
        let r: SceneRegion = serde_json::from_value(standalone()).unwrap();
        let out = serde_json::to_value(&r).unwrap();
        for key in ["description", "environment", "simulationUri", "viewHint"] {
            assert_eq!(out.get(key), Some(&json!(null)), "{key}");
        }
    }

    #[test]
    fn required_fields_rejected_when_missing() {
        // Oracle: L263-399 type these without "|null" and without a
        // default.
        for field in [
            "id",
            "accountId",
            "name",
            "bounds",
            "activeAvatarCount",
            "createdAt",
            "updatedAt",
        ] {
            let mut raw = standalone();
            raw.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<SceneRegion>(raw).is_err(),
                "missing {field} must be rejected"
            );
        }
    }

    #[test]
    fn owner_id_is_required() {
        // Oracle: 6bcd9d8 L327 "`ownerId` (String, immutable,
        // server-set)": neither nullable nor defaulted.
        let mut raw = standalone();
        raw.as_object_mut().unwrap().remove("ownerId");
        assert!(serde_json::from_value::<SceneRegion>(raw).is_err());
    }

    #[test]
    fn invited_user_ids_default_and_round_trip() {
        // Oracle: 6bcd9d8 L456-457 "`invitedUserIds` (String\[\]): The
        // userIds admitted to the region ... Default: `[]`."
        let raw = standalone();
        let r: SceneRegion = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(r.invited_user_ids, ["user:alice@example.com"]);
        assert_eq!(r.owner_id, "user:curator@example.com");
        assert_eq!(serde_json::to_value(&r).unwrap(), raw);

        let mut raw = standalone();
        raw.as_object_mut().unwrap().remove("invitedUserIds");
        let r: SceneRegion = serde_json::from_value(raw).unwrap();
        assert!(r.invited_user_ids.is_empty());
        assert_eq!(
            serde_json::to_value(&r).unwrap()["invitedUserIds"],
            json!([])
        );
    }

    #[test]
    fn vendor_extras_preserved() {
        let mut raw = standalone();
        raw["vendor.example:season"] = json!("winter");
        raw["geoAnchor"] = json!({"referenceFrame": "mars.example.com", "latitude": 1.5,
            "longitude": 2.5, "altitude": 3.5, "heading": 90.5, "vendor.example:datum": "x"});
        let r: SceneRegion = serde_json::from_value(raw.clone()).unwrap();
        assert!(r.extra.contains_key("vendor.example:season"));
        assert_eq!(serde_json::to_value(&r).unwrap(), raw);
    }
}
