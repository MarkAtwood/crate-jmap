//! Round-trips the JSON examples of draft-atwood-jmap-scene-00 that are
//! instances of this crate's types, verbatim. Each constant is copied
//! byte-for-byte from the draft at jmap-chat-spec 3227c0f; the line numbers
//! cite that revision. The oracle is the draft text, never the code under
//! test: each example is parsed as plain JSON, parsed as the typed value,
//! and the typed value must serialize back to the same JSON (null and
//! absent kept distinct).
//!
//! Numbers compare by value: RFC 8259 §6 gives JSON one number type, so
//! the draft's `0` and the `0.0` an `f64` coordinate serializes as are the
//! same JSON value. Every other part of the comparison is exact.
//!
//! The `/set` create arguments and the `created` entries holding only
//! server-set properties (L635-L694, L1038-L1060, L1510-L1531,
//! L1943-L1956, L3113-L3203) are not instances of these types; the method
//! beads test them.

mod common;

use jmap_scene_types::{
    AccessPolicy, GeoAnchor, JMAP_SCENE_URI, PhysicsMode, SceneAvatar, SceneBounds,
    SceneCapability, SceneObject, SceneRegion, ViewHint,
};
use jmap_scene_types::{
    SceneAssetComparator, SceneAssetFilter, SceneAvatarComparator, SceneAvatarFilter,
    SceneObjectComparator, SceneObjectFilter, SceneRegionComparator, SceneRegionFilter,
    WithinBounds, WithinRadius,
};
use jmap_types::Id;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// `v` with every number replaced by its `f64` value (RFC 8259 §6).
fn numbers_as_f64(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(numbers_as_f64).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), numbers_as_f64(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Parse `json` as `T`, assert it re-serializes to the same JSON value,
/// and return it.
fn round_trip<T: DeserializeOwned + Serialize>(json: &Value) -> T {
    let typed: T = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        numbers_as_f64(&serde_json::to_value(&typed).unwrap()),
        numbers_as_f64(json)
    );
    typed
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// The arguments of the first method call, from either a full request
/// (`methodCalls`) or a bare invocation list.
fn first_call_args(v: &Value) -> &Value {
    v.get("methodCalls")
        .unwrap_or(v)
        .get(0)
        .unwrap()
        .get(1)
        .unwrap()
}

#[test]
fn numbers_compare_by_value_only() {
    // The helper must not hide any difference but the number spelling.
    assert_eq!(
        numbers_as_f64(&json!([0, 1.5])),
        numbers_as_f64(&json!([0.0, 1.5]))
    );
    assert_ne!(
        numbers_as_f64(&json!([0, 1.5])),
        numbers_as_f64(&json!([0, 1.6]))
    );
    assert_ne!(
        numbers_as_f64(&json!({"a": null})),
        numbers_as_f64(&json!({}))
    );
}

#[test]
fn scene_bounds_example() {
    let b: SceneBounds = round_trip(&parse(&scene_bounds()));
    assert_eq!(b.min, [-500.0, -10.0, -500.0]);
    assert_eq!(b.max, [500.0, 200.0, 500.0]);
}

#[test]
fn geo_anchor_examples() {
    // Oracle: L307-L309 `null` is "Earth, WGS84 ellipsoid", or a URL, or a
    // DID.
    for (text, frame, hint) in [
        (&geo_seattle(), None, ViewHint::Ar),
        (
            &geo_middle_earth(),
            Some("https://maps.example.com/worlds/middle-earth"),
            ViewHint::ThreeD,
        ),
        (
            &geo_arrakis(),
            Some("did:web:example.com:worlds:arrakis"),
            ViewHint::Ar,
        ),
    ] {
        let v = parse(text);
        let anchor: GeoAnchor = round_trip(&v["geoAnchor"]);
        assert_eq!(anchor.reference_frame.as_deref(), frame);
        round_trip::<SceneBounds>(&v["bounds"]);
        let h: ViewHint = round_trip(&v["viewHint"]);
        assert_eq!(h, hint);
    }
    let seattle: GeoAnchor =
        serde_json::from_value(parse(&geo_seattle())["geoAnchor"].clone()).unwrap();
    assert_eq!(seattle.latitude, 47.6301);
    assert_eq!(seattle.longitude, -122.3154);
    assert_eq!(seattle.altitude, Some(130.0));
}

#[test]
fn scene_region_example() {
    let r: SceneRegion = round_trip(&parse(&scene_region()));
    assert_eq!(r.view_hint, Some(ViewHint::ThreeD));
    assert_eq!(r.access_policy, AccessPolicy::Public);
    assert_eq!(r.spawn_position, [0.0, 0.0, 10.0]);
    assert_eq!(r.active_avatar_count, 3);
    assert_eq!(
        r.chat_id,
        Some(Some(Id::from("01J5ABC0000000000000000099")))
    );
    assert_eq!(r.space_id, Some(None));
    assert_eq!(r.world_state, Some(None));
    assert_eq!(r.input_uri, None);
    assert_eq!(r.geo_anchor, None);
}

#[test]
fn region_get_response_examples() {
    for (text, call) in [(&region_get_response(), 0), (&lifecycle_region_get(), 1)] {
        let v = parse(text);
        let args = &v.get("methodResponses").unwrap_or(&v)[call][1];
        let list = args["list"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        for item in list {
            round_trip::<SceneRegion>(item);
        }
    }
    let v = parse(&region_get_response());
    let second: SceneRegion =
        serde_json::from_value(v["methodResponses"][0][1]["list"][1].clone()).unwrap();
    assert_eq!(second.access_policy, AccessPolicy::Invite);
    assert_eq!(second.environment, None);
}

#[test]
fn admin_region_created_example() {
    let v = parse(&admin_region_created());
    let r: SceneRegion = round_trip(&v[0][1]["created"]["r0"]);
    assert_eq!(r.spawn_position, [0.0, 0.0, 50.0]);
    assert_eq!(r.active_call_id, Some(None));
}

#[test]
fn scene_object_example() {
    let o: SceneObject = round_trip(&parse(&scene_object()));
    assert_eq!(o.position, [12.5, 0.0, -3.2]);
    assert_eq!(o.orientation, [0.0, 0.707, 0.0, 0.707]);
    assert_eq!(o.physics_mode, PhysicsMode::Static);
    assert_eq!(o.owner_id.as_deref(), Some("user:curator@example.com"));
}

#[test]
fn object_get_response_example() {
    let v = parse(&object_get_response());
    let list = v["methodResponses"][0][1]["list"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    for item in list {
        round_trip::<SceneObject>(item);
    }
    let kiosk: SceneObject = serde_json::from_value(list[1].clone()).unwrap();
    assert_eq!(kiosk.scale, [0.8, 0.8, 0.8]);
    assert_eq!(kiosk.world_state, None);
}

#[test]
fn scene_avatar_examples() {
    let a: SceneAvatar = round_trip(&parse(&scene_avatar()));
    assert_eq!(a.user_id, "user:alice@example.com");
    assert_eq!(a.left_at, None);

    let v = parse(&lifecycle_avatar_created());
    let created: SceneAvatar = round_trip(&v[0][1]["created"]["av0"]);
    assert_eq!(created.position, [0.0, 0.0, 10.0]);
}

#[test]
fn session_capability_example() {
    let v = parse(&session());
    // Oracle: L156 "an empty JSON object `{}`".
    assert_eq!(v["capabilities"][JMAP_SCENE_URI], json!({}));
    let cap: SceneCapability =
        round_trip(&v["accounts"]["gallery-account"]["accountCapabilities"][JMAP_SCENE_URI]);
    assert!(!cap.may_create_region);
    assert!(cap.may_create_object);
    assert_eq!(
        cap.supported_visual_types,
        ["model/gltf-binary", "image/png"]
    );
    assert_eq!(cap.max_avatars_per_region, Some(100));
    assert_eq!(cap.max_asset_size_bytes, Some(52_428_800));
    assert!(cap.supports_scene_asset);
}

#[test]
fn asset_query_example() {
    let args = first_call_args(&parse(&asset_query())).clone();
    round_trip::<SceneAssetFilter>(&args["filter"]);
    for c in args["sort"].as_array().unwrap() {
        round_trip::<SceneAssetComparator>(c);
    }
}

#[test]
fn region_query_examples() {
    for text in [
        &region_query_by_population(),
        &region_query_simulated(),
        &lifecycle_region_query(),
    ] {
        let args = first_call_args(&parse(text)).clone();
        round_trip::<SceneRegionFilter>(&args["filter"]);
        for c in args["sort"].as_array().unwrap() {
            round_trip::<SceneRegionComparator>(c);
        }
    }
}

#[test]
fn object_query_examples() {
    let args = first_call_args(&parse(&object_query_radius())).clone();
    let filter: SceneObjectFilter = round_trip(&args["filter"]);
    let SceneObjectFilter::Condition(cond) = filter else {
        panic!("expected a condition")
    };
    assert_eq!(cond.interactable, Some(true));
    assert_eq!(
        cond.within_radius,
        Some(WithinRadius::new([5.0, 0.0, -2.0], 20.0))
    );
    for c in args["sort"].as_array().unwrap() {
        round_trip::<SceneObjectComparator>(c);
    }

    let args = first_call_args(&parse(&object_query_bounds())).clone();
    let filter: SceneObjectFilter = round_trip(&args["filter"]);
    let SceneObjectFilter::Condition(cond) = filter else {
        panic!("expected a condition")
    };
    assert_eq!(
        cond.within_bounds,
        Some(WithinBounds::new([-15.0, 0.0, -15.0], [15.0, 10.0, 15.0]))
    );
    for c in args["sort"].as_array().unwrap() {
        round_trip::<SceneObjectComparator>(c);
    }
}

#[test]
fn avatar_query_example() {
    let args = first_call_args(&parse(&avatar_query_radius())).clone();
    let filter: SceneAvatarFilter = round_trip(&args["filter"]);
    let SceneAvatarFilter::Condition(cond) = filter else {
        panic!("expected a condition")
    };
    assert_eq!(cond.is_active, Some(true));
    assert_eq!(
        cond.within_radius,
        Some(WithinRadius::new([10.0, 0.0, -5.0], 30.0))
    );
    for c in args["sort"].as_array().unwrap() {
        round_trip::<SceneAvatarComparator>(c);
    }
}

/// SceneBounds example (draft-atwood-jmap-scene-00 L200-L203).
fn scene_bounds() -> String {
    common::fixture("scene_bounds.json")
}

/// AR region anchored to a park in Seattle (draft-atwood-jmap-scene-00 L333-L347).
fn geo_seattle() -> String {
    common::fixture("geo_seattle.json")
}

/// region anchored to a game world map (draft-atwood-jmap-scene-00 L353-L367).
fn geo_middle_earth() -> String {
    common::fixture("geo_middle_earth.json")
}

/// region anchored to a DID-identified world (draft-atwood-jmap-scene-00 L373-L387).
fn geo_arrakis() -> String {
    common::fixture("geo_arrakis.json")
}

/// SceneRegion example (draft-atwood-jmap-scene-00 at 6bcd9d8, L489-L518).
fn scene_region() -> String {
    common::fixture("scene_region.json")
}

/// SceneObject example (draft-atwood-jmap-scene-00 L517-L539).
fn scene_object() -> String {
    common::fixture("scene_object.json")
}

/// SceneAvatar example (draft-atwood-jmap-scene-00 L586-L601).
fn scene_avatar() -> String {
    common::fixture("scene_avatar.json")
}

/// SceneAsset/query example (draft-atwood-jmap-scene-00 L829-L840).
fn asset_query() -> String {
    common::fixture("asset_query.json")
}

/// SceneRegion/get response (draft-atwood-jmap-scene-00 at 6bcd9d8, L969-L1036).
fn region_get_response() -> String {
    common::fixture("region_get_response.json")
}

/// SceneRegion/query, public regions sorted by population (draft-atwood-jmap-scene-00 L1285-L1300).
fn region_query_by_population() -> String {
    common::fixture("region_query_by_population.json")
}

/// SceneRegion/query, public regions with a simulation layer (draft-atwood-jmap-scene-00 L1326-L1336).
fn region_query_simulated() -> String {
    common::fixture("region_query_simulated.json")
}

/// SceneObject/get response (draft-atwood-jmap-scene-00 L1373-L1425).
fn object_get_response() -> String {
    common::fixture("object_get_response.json")
}

/// SceneObject/query, withinRadius (draft-atwood-jmap-scene-00 L1807-L1827).
fn object_query_radius() -> String {
    common::fixture("object_query_radius.json")
}

/// SceneObject/query, withinBounds (draft-atwood-jmap-scene-00 L1853-L1872).
fn object_query_bounds() -> String {
    common::fixture("object_query_bounds.json")
}

/// SceneAvatar/query, proximity (draft-atwood-jmap-scene-00 L2264-L2284).
fn avatar_query_radius() -> String {
    common::fixture("avatar_query_radius.json")
}

/// Session object, lifecycle Step 1 (draft-atwood-jmap-scene-00 L3011-L3047 at
/// jmap-chat-spec 6bcd9d8).
fn session() -> String {
    common::fixture("session.json")
}

/// SceneRegion/query and /get request, lifecycle Step 2 (draft-atwood-jmap-scene-00 L2769-L2785).
fn lifecycle_region_query() -> String {
    common::fixture("lifecycle_region_query.json")
}

/// SceneRegion/query and /get response, lifecycle Step 2 (draft-atwood-jmap-scene-00 L2791-L2862).
fn lifecycle_region_get() -> String {
    common::fixture("lifecycle_region_get.json")
}

/// SceneAvatar/set response, lifecycle Step 3 (draft-atwood-jmap-scene-00 L2891-L2918).
fn lifecycle_avatar_created() -> String {
    common::fixture("lifecycle_avatar_created.json")
}

/// SceneRegion/set response, region administration Step 1 (draft-atwood-jmap-scene-00 at 6bcd9d8, L3368-L3410).
fn admin_region_created() -> String {
    common::fixture("admin_region_created.json")
}
