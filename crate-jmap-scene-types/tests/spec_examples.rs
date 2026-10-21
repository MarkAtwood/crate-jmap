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

use jmap_scene_types::query::{
    SceneAssetComparator, SceneAssetFilter, SceneAvatarComparator, SceneAvatarFilter,
    SceneObjectComparator, SceneObjectFilter, SceneRegionComparator, SceneRegionFilter,
    WithinBounds, WithinRadius,
};
use jmap_scene_types::{
    AccessPolicy, GeoAnchor, PhysicsMode, SceneAvatar, SceneBounds, SceneCapability, SceneObject,
    SceneRegion, ViewHint, JMAP_SCENE_URI,
};
use jmap_types::Id;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

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
    let b: SceneBounds = round_trip(&parse(SCENE_BOUNDS));
    assert_eq!(b.min, [-500.0, -10.0, -500.0]);
    assert_eq!(b.max, [500.0, 200.0, 500.0]);
}

#[test]
fn geo_anchor_examples() {
    // Oracle: L307-L309 `null` is "Earth, WGS84 ellipsoid", or a URL, or a
    // DID.
    for (text, frame, hint) in [
        (GEO_SEATTLE, None, ViewHint::Ar),
        (
            GEO_MIDDLE_EARTH,
            Some("https://maps.example.com/worlds/middle-earth"),
            ViewHint::ThreeD,
        ),
        (
            GEO_ARRAKIS,
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
        serde_json::from_value(parse(GEO_SEATTLE)["geoAnchor"].clone()).unwrap();
    assert_eq!(seattle.latitude, 47.6301);
    assert_eq!(seattle.longitude, -122.3154);
    assert_eq!(seattle.altitude, Some(130.0));
}

#[test]
fn scene_region_example() {
    let r: SceneRegion = round_trip(&parse(SCENE_REGION));
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
    for (text, call) in [(REGION_GET_RESPONSE, 0), (LIFECYCLE_REGION_GET, 1)] {
        let v = parse(text);
        let args = &v.get("methodResponses").unwrap_or(&v)[call][1];
        let list = args["list"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        for item in list {
            round_trip::<SceneRegion>(item);
        }
    }
    let v = parse(REGION_GET_RESPONSE);
    let second: SceneRegion =
        serde_json::from_value(v["methodResponses"][0][1]["list"][1].clone()).unwrap();
    assert_eq!(second.access_policy, AccessPolicy::Invite);
    assert_eq!(second.environment, None);
}

#[test]
fn admin_region_created_example() {
    let v = parse(ADMIN_REGION_CREATED);
    let r: SceneRegion = round_trip(&v[0][1]["created"]["r0"]);
    assert_eq!(r.spawn_position, [0.0, 0.0, 50.0]);
    assert_eq!(r.active_call_id, Some(None));
}

#[test]
fn scene_object_example() {
    let o: SceneObject = round_trip(&parse(SCENE_OBJECT));
    assert_eq!(o.position, [12.5, 0.0, -3.2]);
    assert_eq!(o.orientation, [0.0, 0.707, 0.0, 0.707]);
    assert_eq!(o.physics_mode, PhysicsMode::Static);
    assert_eq!(o.owner_id.as_deref(), Some("user:curator@example.com"));
}

#[test]
fn object_get_response_example() {
    let v = parse(OBJECT_GET_RESPONSE);
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
    let a: SceneAvatar = round_trip(&parse(SCENE_AVATAR));
    assert_eq!(a.user_id, "user:alice@example.com");
    assert_eq!(a.left_at, None);

    let v = parse(LIFECYCLE_AVATAR_CREATED);
    let created: SceneAvatar = round_trip(&v[0][1]["created"]["av0"]);
    assert_eq!(created.position, [0.0, 0.0, 10.0]);
}

#[test]
fn session_capability_example() {
    let v = parse(SESSION);
    // Oracle: L156 "an empty JSON object `{}`".
    assert_eq!(v["capabilities"][JMAP_SCENE_URI], json!({}));
    let cap: SceneCapability =
        round_trip(&v["accounts"]["alice-account"]["accountCapabilities"][JMAP_SCENE_URI]);
    assert!(!cap.may_create_region);
    assert!(cap.may_create_object);
    assert_eq!(
        cap.supported_visual_types,
        ["model/gltf-binary", "image/png"]
    );
    assert_eq!(cap.max_avatars_per_region, Some(100));
    assert_eq!(cap.max_asset_size_bytes, Some(52_428_800));
}

#[test]
fn asset_query_example() {
    let args = first_call_args(&parse(ASSET_QUERY)).clone();
    round_trip::<SceneAssetFilter>(&args["filter"]);
    for c in args["sort"].as_array().unwrap() {
        round_trip::<SceneAssetComparator>(c);
    }
}

#[test]
fn region_query_examples() {
    for text in [
        REGION_QUERY_BY_POPULATION,
        REGION_QUERY_SIMULATED,
        LIFECYCLE_REGION_QUERY,
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
    let args = first_call_args(&parse(OBJECT_QUERY_RADIUS)).clone();
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

    let args = first_call_args(&parse(OBJECT_QUERY_BOUNDS)).clone();
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
    let args = first_call_args(&parse(AVATAR_QUERY_RADIUS)).clone();
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
const SCENE_BOUNDS: &str = r##"{
  "min": [-500, -10, -500],
  "max": [500, 200, 500]
}"##;

/// AR region anchored to a park in Seattle (draft-atwood-jmap-scene-00 L333-L347).
const GEO_SEATTLE: &str = r##"{
  "name": "Volunteer Park AR Garden",
  "viewHint": "ar",
  "geoAnchor": {
    "referenceFrame": null,
    "latitude": 47.6301,
    "longitude": -122.3154,
    "altitude": 130.0,
    "heading": 0
  },
  "bounds": {
    "min": [-50, 0, -50],
    "max": [50, 30, 50]
  }
}"##;

/// region anchored to a game world map (draft-atwood-jmap-scene-00 L353-L367).
const GEO_MIDDLE_EARTH: &str = r##"{
  "name": "Rivendell Courtyard",
  "viewHint": "3d",
  "geoAnchor": {
    "referenceFrame": "https://maps.example.com/worlds/middle-earth",
    "latitude": 33.21,
    "longitude": -12.84,
    "altitude": 200.0,
    "heading": 45
  },
  "bounds": {
    "min": [-100, 0, -100],
    "max": [100, 50, 100]
  }
}"##;

/// region anchored to a DID-identified world (draft-atwood-jmap-scene-00 L373-L387).
const GEO_ARRAKIS: &str = r##"{
  "name": "Arrakeen Spaceport",
  "viewHint": "ar",
  "geoAnchor": {
    "referenceFrame": "did:web:example.com:worlds:arrakis",
    "latitude": 0.0,
    "longitude": 0.0,
    "altitude": 0.0,
    "heading": 0
  },
  "bounds": {
    "min": [-500, 0, -500],
    "max": [500, 100, 500]
  }
}"##;

/// SceneRegion example (draft-atwood-jmap-scene-00 at 6bcd9d8, L489-L518).
const SCENE_REGION: &str = r##"{
  "id": "01J5ABC0000000000000000001",
  "accountId": "account-xyz",
  "ownerId": "user:curator@example.com",
  "name": "Gallery East Wing",
  "description": "Contemporary sculpture exhibition",
  "bounds": {
    "min": [-50, 0, -50],
    "max": [50, 10, 50]
  },
  "environment": {
    "skyColor": "#87CEEB",
    "ambientIntensity": 0.6,
    "gravity": 9.81
  },
  "simulationUri": "wss://sim.example.com/regions/01J5ABC",
  "viewHint": "3d",
  "spawnPosition": [0, 0, 10],
  "spawnOrientation": [0, 0, 0, 1],
  "activeAvatarCount": 3,
  "accessPolicy": "public",
  "invitedUserIds": [],
  "createdAt": "2026-06-01T10:00:00Z",
  "updatedAt": "2026-06-05T14:30:00Z",
  "worldState": null,
  "chatId": "01J5ABC0000000000000000099",
  "spaceId": null,
  "channelId": null,
  "activeCallId": null
}"##;

/// SceneObject example (draft-atwood-jmap-scene-00 L517-L539).
const SCENE_OBJECT: &str = r##"{
  "id": "01J5OBJ0000000000000000001",
  "regionId": "01J5ABC0000000000000000001",
  "parentId": null,
  "name": "Sculpture: Convergence",
  "position": [12.5, 0, -3.2],
  "orientation": [0, 0.707, 0, 0.707],
  "scale": [1, 1, 1],
  "visualRef": "blob-gltf-sculpture-001",
  "visualType": "model/gltf-binary",
  "assetUri": "https://cdn.example.com/assets/sculpture-001.glb",
  "physicsMode": "static",
  "interactable": true,
  "visible": true,
  "ownerId": "user:curator@example.com",
  "createdAt": "2026-06-01T12:00:00Z",
  "updatedAt": "2026-06-01T12:00:00Z",
  "worldState": {
    "artist": "Elena Voss",
    "year": 2025,
    "medium": "Bronze"
  }
}"##;

/// SceneAvatar example (draft-atwood-jmap-scene-00 L586-L601).
const SCENE_AVATAR: &str = r##"{
  "id": "user:alice@example.com",
  "regionId": "01J5ABC0000000000000000001",
  "userId": "user:alice@example.com",
  "displayName": "Alice Chen",
  "position": [5.2, 0, -1.8],
  "orientation": [0, 0.383, 0, 0.924],
  "visualRef": "blob-avatar-alice-001",
  "visualType": "model/gltf-binary",
  "joinedAt": "2026-06-05T14:35:00Z",
  "leftAt": null,
  "worldState": {
    "animation": "idle",
    "nametag": true
  }
}"##;

/// SceneAsset/query example (draft-atwood-jmap-scene-00 L829-L840).
const ASSET_QUERY: &str = r##"[["SceneAsset/query", {
  "accountId": "account-xyz",
  "filter": {
    "mediaType": "model/gltf-binary",
    "name": "sculpture",
    "createdAfter": "2026-03-01T00:00:00Z",
    "createdBefore": "2026-06-01T00:00:00Z"
  },
  "sort": [{"property": "createdAt", "isAscending": false}],
  "position": 0,
  "limit": 25
}, "0"]]"##;

/// SceneRegion/get response (draft-atwood-jmap-scene-00 at 6bcd9d8, L969-L1036).
const REGION_GET_RESPONSE: &str = r##"{
  "methodResponses": [
    ["SceneRegion/get", {
      "accountId": "acct-001",
      "state": "state-region-47",
      "list": [
        {
          "id": "01J5ABC0000000000000000001",
          "accountId": "acct-001",
          "ownerId": "user:curator@example.com",
          "name": "Gallery East Wing",
          "description": "Contemporary sculpture exhibition",
          "bounds": {
            "min": [-50, 0, -50],
            "max": [50, 10, 50]
          },
          "environment": {
            "skyColor": "#87CEEB",
            "ambientIntensity": 0.6,
            "gravity": 9.81
          },
          "simulationUri": "wss://sim.example.com/regions/01J5ABC0001",
          "viewHint": "3d",
          "spawnPosition": [0, 0, 10],
          "spawnOrientation": [0, 0, 0, 1],
          "activeAvatarCount": 3,
          "accessPolicy": "public",
          "invitedUserIds": [],
          "createdAt": "2026-06-01T10:00:00Z",
          "updatedAt": "2026-06-05T14:30:00Z",
          "chatId": "01J5CHAT000000000000000099",
          "spaceId": null,
          "channelId": null,
          "activeCallId": null
        },
        {
          "id": "01J5ABC0000000000000000002",
          "accountId": "acct-001",
          "ownerId": "user:curator@example.com",
          "name": "Design Review Room",
          "description": null,
          "bounds": {
            "min": [-20, 0, -20],
            "max": [20, 8, 20]
          },
          "environment": null,
          "simulationUri": "wss://sim.example.com/regions/01J5ABC0002",
          "viewHint": "3d",
          "spawnPosition": [0, 0, 5],
          "spawnOrientation": [0, 0, 0, 1],
          "activeAvatarCount": 0,
          "accessPolicy": "invite",
          "invitedUserIds": [
            "user:alice@example.com",
            "user:bob@example.com"
          ],
          "createdAt": "2026-06-02T08:00:00Z",
          "updatedAt": "2026-06-02T08:00:00Z",
          "chatId": null,
          "spaceId": null,
          "channelId": null,
          "activeCallId": null
        }
      ],
      "notFound": []
    }, "r0"]
  ]
}"##;

/// SceneRegion/query, public regions sorted by population (draft-atwood-jmap-scene-00 L1285-L1300).
const REGION_QUERY_BY_POPULATION: &str = r##"{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"],
  "methodCalls": [
    ["SceneRegion/query", {
      "accountId": "acct-001",
      "filter": {
        "accessPolicy": "public"
      },
      "sort": [
        {"property": "activeAvatarCount", "isAscending": false}
      ],
      "position": 0,
      "limit": 10
    }, "r0"]
  ]
}"##;

/// SceneRegion/query, public regions with a simulation layer (draft-atwood-jmap-scene-00 L1326-L1336).
const REGION_QUERY_SIMULATED: &str = r##"[["SceneRegion/query", {
  "accountId": "account-xyz",
  "filter": {
    "accessPolicy": "public",
    "hasSimulationUri": true,
    "createdAfter": "2026-01-01T00:00:00Z"
  },
  "sort": [{"property": "createdAt", "isAscending": false}],
  "position": 0,
  "limit": 50
}, "0"]]"##;

/// SceneObject/get response (draft-atwood-jmap-scene-00 L1373-L1425).
const OBJECT_GET_RESPONSE: &str = r##"{
  "methodResponses": [
    ["SceneObject/get", {
      "accountId": "acct-001",
      "state": "state-obj-33",
      "list": [
        {
          "id": "01J5OBJ0000000000000000001",
          "regionId": "01J5ABC0000000000000000001",
          "parentId": null,
          "name": "Sculpture: Convergence",
          "position": [12.5, 0, -3.2],
          "orientation": [0, 0.707, 0, 0.707],
          "scale": [1, 1, 1],
          "visualRef": "blob-gltf-sculpture-001",
          "visualType": "model/gltf-binary",
          "assetUri": "https://cdn.example.com/assets/sculpture-001.glb",
          "physicsMode": "static",
          "interactable": true,
          "visible": true,
          "ownerId": "user:curator@example.com",
          "createdAt": "2026-06-01T12:00:00Z",
          "updatedAt": "2026-06-01T12:00:00Z",
          "worldState": {
            "artist": "Elena Voss",
            "year": 2025,
            "medium": "Bronze"
          }
        },
        {
          "id": "01J5OBJ0000000000000000002",
          "regionId": "01J5ABC0000000000000000001",
          "parentId": null,
          "name": "Info Kiosk",
          "position": [0, 0, 8],
          "orientation": [0, 0, 0, 1],
          "scale": [0.8, 0.8, 0.8],
          "visualRef": "blob-gltf-kiosk-001",
          "visualType": "model/gltf-binary",
          "assetUri": "https://cdn.example.com/assets/kiosk-001.glb",
          "physicsMode": "static",
          "interactable": true,
          "visible": true,
          "ownerId": "user:curator@example.com",
          "createdAt": "2026-06-01T12:30:00Z",
          "updatedAt": "2026-06-03T09:15:00Z",
          "worldState": null
        }
      ],
      "notFound": []
    }, "o0"]
  ]
}"##;

/// SceneObject/query, withinRadius (draft-atwood-jmap-scene-00 L1807-L1827).
const OBJECT_QUERY_RADIUS: &str = r##"{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"],
  "methodCalls": [
    ["SceneObject/query", {
      "accountId": "acct-001",
      "filter": {
        "regionId": "01J5ABC0000000000000000001",
        "interactable": true,
        "withinRadius": {
          "center": [5.0, 0, -2.0],
          "radius": 20
        }
      },
      "sort": [
        {"property": "name", "isAscending": true}
      ],
      "position": 0,
      "limit": 25
    }, "o0"]
  ]
}"##;

/// SceneObject/query, withinBounds (draft-atwood-jmap-scene-00 L1853-L1872).
const OBJECT_QUERY_BOUNDS: &str = r##"{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"],
  "methodCalls": [
    ["SceneObject/query", {
      "accountId": "acct-001",
      "filter": {
        "regionId": "01J5ABC0000000000000000001",
        "withinBounds": {
          "min": [-15, 0, -15],
          "max": [15, 10, 15]
        }
      },
      "sort": [
        {"property": "createdAt", "isAscending": true}
      ],
      "position": 0,
      "limit": 50
    }, "o0"]
  ]
}"##;

/// SceneAvatar/query, proximity (draft-atwood-jmap-scene-00 L2264-L2284).
const AVATAR_QUERY_RADIUS: &str = r##"{
  "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:scene"],
  "methodCalls": [
    ["SceneAvatar/query", {
      "accountId": "acct-001",
      "filter": {
        "regionId": "01J5ABC0000000000000000001",
        "isActive": true,
        "withinRadius": {
          "center": [10.0, 0, -5.0],
          "radius": 30
        }
      },
      "sort": [
        {"property": "joinedAt", "isAscending": true}
      ],
      "position": 0,
      "limit": 50
    }, "a0"]
  ]
}"##;

/// Session object, lifecycle Step 1 (draft-atwood-jmap-scene-00 L2732-L2759).
const SESSION: &str = r##"{
  "capabilities": {
    "urn:ietf:params:jmap:core": {},
    "urn:ietf:params:jmap:scene": {}
  },
  "accounts": {
    "alice-account": {
      "name": "alice@example.com",
      "accountCapabilities": {
        "urn:ietf:params:jmap:scene": {
          "mayCreateRegion": false,
          "mayCreateObject": true,
          "supportedVisualTypes": [
            "model/gltf-binary",
            "image/png"
          ],
          "maxRegionsPerAccount": 10,
          "maxObjectsPerRegion": 5000,
          "maxAvatarsPerRegion": 100,
          "maxAssetSizeBytes": 52428800
        }
      }
    }
  },
  "apiUrl": "https://jmap.example.com/api/",
  "uploadUrl": "https://jmap.example.com/upload/{accountId}/",
  "downloadUrl": "https://jmap.example.com/download/{accountId}/{blobId}/{name}"
}"##;

/// SceneRegion/query and /get request, lifecycle Step 2 (draft-atwood-jmap-scene-00 L2769-L2785).
const LIFECYCLE_REGION_QUERY: &str = r##"[["SceneRegion/query", {
  "accountId": "alice-account",
  "filter": {
    "accessPolicy": "public",
    "hasActiveAvatars": true
  },
  "sort": [{"property": "activeAvatarCount", "isAscending": false}],
  "limit": 10
}, "0"],
["SceneRegion/get", {
  "accountId": "alice-account",
  "#ids": {
    "resultOf": "0",
    "name": "SceneRegion/query",
    "path": "/ids"
  }
}, "1"]]"##;

/// SceneRegion/query and /get response, lifecycle Step 2 (draft-atwood-jmap-scene-00 L2791-L2862).
const LIFECYCLE_REGION_GET: &str = r##"[["SceneRegion/query", {
  "accountId": "alice-account",
  "queryState": "s100",
  "canCalculateChanges": true,
  "position": 0,
  "total": 2,
  "ids": [
    "01JXKR5M0G3QVTA8N2BWFP7Y01",
    "01JXKR5M0G3QVTA8N2BWFP7Y02"
  ]
}, "0"],
["SceneRegion/get", {
  "accountId": "alice-account",
  "state": "s101",
  "list": [
    {
      "id": "01JXKR5M0G3QVTA8N2BWFP7Y01",
      "accountId": "alice-account",
      "ownerId": "user:curator@example.com",
      "name": "Gallery East Wing",
      "description": "Contemporary sculpture exhibition",
      "bounds": {
        "min": [-50, 0, -50],
        "max": [50, 10, 50]
      },
      "environment": {
        "skyColor": "#87CEEB",
        "ambientIntensity": 0.6,
        "gravity": 9.81
      },
      "simulationUri": "wss://sim.example.com/regions/01JXKR5M",
      "viewHint": "3d",
      "spawnPosition": [0, 0, 10],
      "spawnOrientation": [0, 0, 0, 1],
      "activeAvatarCount": 3,
      "accessPolicy": "public",
      "invitedUserIds": [],
      "createdAt": "2026-05-01T10:00:00Z",
      "updatedAt": "2026-06-05T14:30:00Z",
      "chatId": null,
      "spaceId": null,
      "channelId": null,
      "activeCallId": null
    },
    {
      "id": "01JXKR5M0G3QVTA8N2BWFP7Y02",
      "accountId": "alice-account",
      "ownerId": "user:curator@example.com",
      "name": "Rooftop Lounge",
      "description": "Social hangout with city skyline",
      "bounds": {
        "min": [-30, 0, -30],
        "max": [30, 5, 30]
      },
      "environment": {
        "skyColor": "#1a1a2e",
        "ambientIntensity": 0.3,
        "gravity": 9.81
      },
      "simulationUri": "wss://sim.example.com/regions/01JXKR5N",
      "viewHint": "3d",
      "spawnPosition": [0, 0, 0],
      "spawnOrientation": [0, 0, 0, 1],
      "activeAvatarCount": 1,
      "accessPolicy": "public",
      "invitedUserIds": [],
      "createdAt": "2026-05-15T08:00:00Z",
      "updatedAt": "2026-06-04T22:10:00Z",
      "chatId": null,
      "spaceId": null,
      "channelId": null,
      "activeCallId": null
    }
  ],
  "notFound": []
}, "1"]]"##;

/// SceneAvatar/set response, lifecycle Step 3 (draft-atwood-jmap-scene-00 L2891-L2918).
const LIFECYCLE_AVATAR_CREATED: &str = r##"[["SceneAvatar/set", {
  "accountId": "alice-account",
  "oldState": "s200",
  "newState": "s201",
  "created": {
    "av0": {
      "id": "user:alice@example.com",
      "regionId": "01JXKR5M0G3QVTA8N2BWFP7Y01",
      "userId": "user:alice@example.com",
      "displayName": "Alice Chen",
      "position": [0, 0, 10],
      "orientation": [0, 0, 0, 1],
      "visualRef": "blob-avatar-alice-001",
      "visualType": "model/gltf-binary",
      "joinedAt": "2026-06-06T09:15:00Z",
      "leftAt": null,
      "worldState": {
        "animation": "idle",
        "nametag": true
      }
    }
  },
  "updated": null,
  "destroyed": null,
  "notCreated": null,
  "notUpdated": null,
  "notDestroyed": null
}, "0"]]"##;

/// SceneRegion/set response, region administration Step 1 (draft-atwood-jmap-scene-00 at 6bcd9d8, L3368-L3410).
const ADMIN_REGION_CREATED: &str = r##"[["SceneRegion/set", {
  "accountId": "admin-account",
  "oldState": "s300",
  "newState": "s301",
  "created": {
    "r0": {
      "id": "01JXKRBN7KWMPS46GFHDJT9R10",
      "accountId": "admin-account",
      "ownerId": "user:admin@example.com",
      "name": "Product Launch Hall",
      "description": "Annual product launch event venue",
      "bounds": {
        "min": [-100, 0, -60],
        "max": [100, 20, 60]
      },
      "viewHint": "3d",
      "accessPolicy": "public",
      "invitedUserIds": [],
      "spawnPosition": [0, 0, 50],
      "spawnOrientation": [0, 0, 0, 1],
      "simulationUri": "wss://sim.example.com/regions/launch-hall",
      "environment": {
        "skyColor": "#000022",
        "ambientIntensity": 0.4,
        "gravity": 9.81,
        "fogDensity": 0.01,
        "fogColor": "#111133"
      },
      "activeAvatarCount": 0,
      "createdAt": "2026-06-06T08:00:00Z",
      "updatedAt": "2026-06-06T08:00:00Z",
      "chatId": null,
      "spaceId": null,
      "channelId": null,
      "activeCallId": null
    }
  },
  "updated": null,
  "destroyed": null,
  "notCreated": null,
  "notUpdated": null,
  "notDestroyed": null
}, "0"]]"##;
