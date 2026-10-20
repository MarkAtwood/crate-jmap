//! Round-trips the JSON examples of draft-atwood-jmap-scene-wss-00 that
//! are instances of this crate's types, verbatim. Each constant is copied
//! byte-for-byte from the draft at jmap-chat-spec 0cd3af0; the line numbers
//! cite that revision. The oracle is the draft text, never the code under
//! test: each example is parsed as the typed value, which must serialize
//! back to the same JSON (null and absent kept distinct).
//!
//! Numbers compare by value: RFC 8259 §6 gives JSON one number type, so
//! the draft's `0` and the `0.0` an `f64` coordinate serializes as are the
//! same JSON value.
//!
//! The `RequestError` (L545), `VTCStreamEnable` (L897),
//! `WebSocketPushEnable` (L952) and `StateChange` (L1044) examples are not
//! instances of these types.

use jmap_scene_types::{
    AvatarEventKind, ObjectEventKind, SceneEvent, SceneStreamMessage, JMAP_SCENE_URI,
    JMAP_SCENE_WEBSOCKET_URI,
};
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

/// Parse `text` as `T`, assert it re-serializes to the same JSON value,
/// and return it.
fn round_trip<T: DeserializeOwned + Serialize>(text: &str) -> T {
    let json: Value = serde_json::from_str(text).unwrap();
    let typed: T = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        numbers_as_f64(&serde_json::to_value(&typed).unwrap()),
        numbers_as_f64(&json),
        "{text}"
    );
    typed
}

/// draft-atwood-jmap-scene-wss-00 L142-L151.
const SESSION_L142: &str = r#"{
  "capabilities": {
    "urn:ietf:params:jmap:websocket": {
      "url": "wss://scene.example.com/jmap/ws/",
      "supportsPush": true
    },
    "urn:ietf:params:jmap:scene": {},
    "urn:ietf:params:jmap:scene:websocket": {}
  }
}"#;

/// draft-atwood-jmap-scene-wss-00 L190-L197.
const EVENT_L190: &str = r#"{
  "@type": "SceneAvatarEvent",
  "regionId": "01J5ABC0000000000000000001",
  "avatarId": "user:alice@example.com",
  "event": "entered",
  "userId": "user:alice@example.com",
  "displayName": "Alice Chen"
}"#;

/// draft-atwood-jmap-scene-wss-00 L231-L238.
const EVENT_L231: &str = r#"{
  "@type": "SceneObjectEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000042",
  "event": "created",
  "updatedBy": "user:bob@example.com",
  "position": [15.0, 0, -5.0]
}"#;

/// draft-atwood-jmap-scene-wss-00 L244-L251.
const EVENT_L244: &str = r#"{
  "@type": "SceneObjectEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000042",
  "event": "destroyed",
  "updatedBy": "user:bob@example.com",
  "position": null
}"#;

/// draft-atwood-jmap-scene-wss-00 L289-L296.
const EVENT_L289: &str = r#"{
  "@type": "SceneInteractionEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000001",
  "userId": "user:alice@example.com",
  "action": "click",
  "data": null
}"#;

/// draft-atwood-jmap-scene-wss-00 L302-L309.
const EVENT_L302: &str = r#"{
  "@type": "SceneInteractionEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000007",
  "userId": "user:alice@example.com",
  "action": "com.example.puzzle.solve",
  "data": {"pieceIndex": 4, "rotation": 90}
}"#;

/// draft-atwood-jmap-scene-wss-00 L328-L338.
const EVENT_L328: &str = r#"{
  "@type": "SceneInteractionEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000077",
  "userId": "user:alice@example.com",
  "action": "com.example.game.cast-spell",
  "data": {
    "spellId": "fireball",
    "targetId": "01J5OBJ0000000000000000033"
  }
}"#;

/// draft-atwood-jmap-scene-wss-00 L407-L411.
const STREAM_L407: &str = r#"{
  "@type": "SceneStreamEnable",
  "regionIds": ["01J5ABC0000000000000000001"],
  "eventTypes": ["SceneAvatarEvent", "SceneObjectEvent"]
}"#;

/// draft-atwood-jmap-scene-wss-00 L427-L431.
const STREAM_L427: &str = r#"{
  "@type": "SceneStreamEnable",
  "regionIds": ["01J5ABC0000000000000000001"],
  "eventTypes": ["SceneInteractionEvent"]
}"#;

/// draft-atwood-jmap-scene-wss-00 L458-L465.
const STREAM_L458: &str = r#"{
  "@type": "SceneStreamEnable",
  "regionIds": [
    "01J5ABC0000000000000000001",
    "01J5ABC0000000000000000002"
  ],
  "eventTypes": null
}"#;

/// draft-atwood-jmap-scene-wss-00 L529-L533.
const STREAM_L529: &str = r#"{
  "@type": "SceneStreamEnable",
  "regionIds": ["01J5ABC0000000000000000001"],
  "eventTypes": ["SceneFooBarEvent"]
}"#;

/// draft-atwood-jmap-scene-wss-00 L586-L588.
const STREAM_L586: &str = r#"{
  "@type": "SceneStreamDisable"
}"#;

/// draft-atwood-jmap-scene-wss-00 L665-L672.
const EVENT_L665: &str = r#"{
  "@type": "SceneObjectEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000050",
  "event": "updated",
  "updatedBy": null,
  "position": [10.0, 0, 0]
}"#;

/// draft-atwood-jmap-scene-wss-00 L679-L686.
const EVENT_L679: &str = r#"{
  "@type": "SceneObjectEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000050",
  "event": "updated",
  "updatedBy": null,
  "position": [10.5, 0, 0]
}"#;

/// draft-atwood-jmap-scene-wss-00 L795-L802.
const EVENT_L795: &str = r#"{
  "@type": "SceneAvatarEvent",
  "regionId": "01J5ABC0000000000000000001",
  "avatarId": "user:mallory@example.com",
  "event": "entered",
  "userId": "user:mallory@example.com",
  "displayName": "Mallory Nguyen"
}"#;

/// draft-atwood-jmap-scene-wss-00 L889-L893.
const STREAM_L889: &str = r#"{
  "@type": "SceneStreamEnable",
  "regionIds": ["01J5ABC0000000000000000001"],
  "eventTypes": null
}"#;

/// draft-atwood-jmap-scene-wss-00 L978-L982.
const STREAM_L978: &str = r#"{
  "@type": "SceneStreamEnable",
  "regionIds": ["01J5ABC0000000000000000001"],
  "eventTypes": null
}"#;

/// draft-atwood-jmap-scene-wss-00 L990-L997.
const EVENT_L990: &str = r#"{
  "@type": "SceneAvatarEvent",
  "regionId": "01J5ABC0000000000000000001",
  "avatarId": "user:bob@example.com",
  "event": "entered",
  "userId": "user:bob@example.com",
  "displayName": "Bob Martinez"
}"#;

/// draft-atwood-jmap-scene-wss-00 L1005-L1012.
const EVENT_L1005: &str = r#"{
  "@type": "SceneObjectEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000042",
  "event": "created",
  "updatedBy": "user:bob@example.com",
  "position": [8.0, 1.2, -3.5]
}"#;

/// draft-atwood-jmap-scene-wss-00 L1029-L1036.
const EVENT_L1029: &str = r#"{
  "@type": "SceneInteractionEvent",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5OBJ0000000000000000001",
  "userId": "user:alice@example.com",
  "action": "click",
  "data": null
}"#;

/// draft-atwood-jmap-scene-wss-00 L1059-L1066.
const EVENT_L1059: &str = r#"{
  "@type": "SceneAvatarEvent",
  "regionId": "01J5ABC0000000000000000001",
  "avatarId": "user:bob@example.com",
  "event": "left",
  "userId": "user:bob@example.com",
  "displayName": "Bob Martinez"
}"#;

/// draft-atwood-jmap-scene-wss-00 L1074-L1076.
const STREAM_L1074: &str = r#"{
  "@type": "SceneStreamDisable"
}"#;

const EVENTS: &[&str] = &[
    EVENT_L190,
    EVENT_L231,
    EVENT_L244,
    EVENT_L289,
    EVENT_L302,
    EVENT_L328,
    EVENT_L665,
    EVENT_L679,
    EVENT_L795,
    EVENT_L990,
    EVENT_L1005,
    EVENT_L1029,
    EVENT_L1059,
];

const STREAMS: &[&str] = &[
    STREAM_L407,
    STREAM_L427,
    STREAM_L458,
    STREAM_L529,
    STREAM_L586,
    STREAM_L889,
    STREAM_L978,
    STREAM_L1074,
];

#[test]
fn every_event_example_round_trips_to_a_known_variant() {
    for text in EVENTS {
        let e: SceneEvent = round_trip(text);
        assert!(
            !matches!(e, SceneEvent::Unknown { .. }),
            "decoded as Unknown: {text}"
        );
    }
}

#[test]
fn every_stream_example_round_trips_to_a_known_variant() {
    for text in STREAMS {
        let m: SceneStreamMessage = round_trip(text);
        assert!(
            !matches!(m, SceneStreamMessage::Unknown { .. }),
            "decoded as Unknown: {text}"
        );
    }
}

#[test]
fn avatar_entered_example_fields() {
    // L187 "Example — avatar enters a region".
    let SceneEvent::Avatar(e) = round_trip(EVENT_L190) else {
        panic!("wrong variant")
    };
    assert_eq!(e.region_id.as_ref(), "01J5ABC0000000000000000001");
    assert_eq!(e.avatar_id.as_ref(), "user:alice@example.com");
    assert_eq!(e.event, AvatarEventKind::Entered);
    assert_eq!(e.user_id, "user:alice@example.com");
    assert_eq!(e.display_name, "Alice Chen");
}

#[test]
fn object_created_and_destroyed_example_fields() {
    // L224-226 `position`: "Present for `\"created\"` and `\"updated\"`
    // events; `null` for `\"destroyed\"`."
    let SceneEvent::Object(c) = round_trip(EVENT_L231) else {
        panic!("wrong variant")
    };
    assert_eq!(c.event, ObjectEventKind::Created);
    assert_eq!(c.updated_by.as_deref(), Some("user:bob@example.com"));
    assert_eq!(c.position, Some([15.0, 0.0, -5.0]));

    let SceneEvent::Object(d) = round_trip(EVENT_L244) else {
        panic!("wrong variant")
    };
    assert_eq!(d.event, ObjectEventKind::Destroyed);
    assert_eq!(d.position, None);
}

#[test]
fn interaction_examples_fields() {
    // L294-295 `"action": "click"`, `"data": null`.
    let SceneEvent::Interaction(click) = round_trip(EVENT_L289) else {
        panic!("wrong variant")
    };
    assert_eq!(click.action, "click");
    assert_eq!(click.data, None);

    // L307-308 reverse-domain action with an opaque payload.
    let SceneEvent::Interaction(solve) = round_trip(EVENT_L302) else {
        panic!("wrong variant")
    };
    assert_eq!(solve.action, "com.example.puzzle.solve");
    assert_eq!(
        Value::Object(solve.data.unwrap()),
        json!({"pieceIndex": 4, "rotation": 90})
    );
}

#[test]
fn stream_enable_examples_fields() {
    // L408-411 explicit regions and event types.
    let SceneStreamMessage::Enable(e) = round_trip(STREAM_L407) else {
        panic!("wrong variant")
    };
    assert_eq!(
        e.region_ids.unwrap()[0].as_ref(),
        "01J5ABC0000000000000000001"
    );
    assert_eq!(
        e.event_types.unwrap(),
        ["SceneAvatarEvent", "SceneObjectEvent"]
    );

    // L464 `"eventTypes": null`: "all three event types ... are
    // delivered" (L468-470).
    let SceneStreamMessage::Enable(m) = round_trip(STREAM_L458) else {
        panic!("wrong variant")
    };
    assert_eq!(m.region_ids.unwrap().len(), 2);
    assert_eq!(m.event_types, None);

    // L532 the unrecognized-only error case still decodes: rejecting it
    // with a `RequestError` (L537-540) is the server's job.
    let SceneStreamMessage::Enable(bad) = round_trip(STREAM_L529) else {
        panic!("wrong variant")
    };
    assert_eq!(bad.event_types.unwrap(), ["SceneFooBarEvent"]);
}

#[test]
fn stream_disable_example() {
    // L587 `{"@type": "SceneStreamDisable"}`.
    let m: SceneStreamMessage = round_trip(STREAM_L586);
    assert!(matches!(m, SceneStreamMessage::Disable(_)));
}

#[test]
fn session_example_advertises_both_scene_capabilities() {
    // L136-137 "A server advertising
    // `urn:ietf:params:jmap:scene:websocket` MUST also advertise both
    // `urn:ietf:params:jmap:websocket` and `urn:ietf:params:jmap:scene`";
    // L120-121 its value "is an empty JSON object `{}`".
    let v: Value = serde_json::from_str(SESSION_L142).unwrap();
    let caps = &v["capabilities"];
    assert_eq!(caps[JMAP_SCENE_WEBSOCKET_URI], json!({}));
    assert_eq!(caps[JMAP_SCENE_URI], json!({}));
}
