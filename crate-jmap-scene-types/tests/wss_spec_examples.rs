//! Round-trips the JSON examples of draft-atwood-jmap-scene-wss-00 that
//! are instances of this crate's types, verbatim. Each constant is copied
//! byte-for-byte from the draft at jmap-chat-spec 6bcd9d8; the line numbers
//! cite that revision. The oracle is the draft text, never the code under
//! test: each example is parsed as the typed value, which must serialize
//! back to the same JSON (null and absent kept distinct).
//!
//! Numbers compare by value: RFC 8259 §6 gives JSON one number type, so
//! the draft's `0` and the `0.0` an `f64` coordinate serializes as are the
//! same JSON value.
//!
//! The `RequestError` (L604), `VTCStreamEnable` (L963),
//! `WebSocketPushEnable` (L1023), `SceneAvatar/set` (L1034),
//! `SceneObject/get` (L1091) and `StateChange` (L1122) examples are not
//! instances of these types.

mod common;

use jmap_scene_types::{
    AvatarEventKind, ObjectEventKind, SceneEvent, SceneEventType, SceneStreamMessage,
    JMAP_SCENE_URI, JMAP_SCENE_WEBSOCKET_URI,
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

/// draft-atwood-jmap-scene-wss-00 L155-L164.
fn session_l155() -> String {
    common::fixture("session_l155.json")
}

/// draft-atwood-jmap-scene-wss-00 L214-L222.
fn event_l214() -> String {
    common::fixture("event_l214.json")
}

/// draft-atwood-jmap-scene-wss-00 L256-L264.
fn event_l256() -> String {
    common::fixture("event_l256.json")
}

/// draft-atwood-jmap-scene-wss-00 L270-L278.
fn event_l270() -> String {
    common::fixture("event_l270.json")
}

/// draft-atwood-jmap-scene-wss-00 L316-L324.
fn event_l316() -> String {
    common::fixture("event_l316.json")
}

/// draft-atwood-jmap-scene-wss-00 L330-L338.
fn event_l330() -> String {
    common::fixture("event_l330.json")
}

/// draft-atwood-jmap-scene-wss-00 L357-L368.
fn event_l357() -> String {
    common::fixture("event_l357.json")
}

/// draft-atwood-jmap-scene-wss-00 L457-L461.
fn stream_l457() -> String {
    common::fixture("stream_l457.json")
}

/// draft-atwood-jmap-scene-wss-00 L477-L481.
fn stream_l477() -> String {
    common::fixture("stream_l477.json")
}

/// draft-atwood-jmap-scene-wss-00 L500-L507.
fn stream_l500() -> String {
    common::fixture("stream_l500.json")
}

/// draft-atwood-jmap-scene-wss-00 L587-L592.
fn stream_l587() -> String {
    common::fixture("stream_l587.json")
}

/// draft-atwood-jmap-scene-wss-00 L646-L648.
fn stream_l646() -> String {
    common::fixture("stream_l646.json")
}

/// draft-atwood-jmap-scene-wss-00 L727-L735.
fn event_l727() -> String {
    common::fixture("event_l727.json")
}

/// draft-atwood-jmap-scene-wss-00 L742-L750.
fn event_l742() -> String {
    common::fixture("event_l742.json")
}

/// draft-atwood-jmap-scene-wss-00 L858-L866.
fn event_l858() -> String {
    common::fixture("event_l858.json")
}

/// draft-atwood-jmap-scene-wss-00 L955-L959.
fn stream_l955() -> String {
    common::fixture("stream_l955.json")
}

/// draft-atwood-jmap-scene-wss-00 L1049-L1053.
fn stream_l1049() -> String {
    common::fixture("stream_l955.json")
}

/// draft-atwood-jmap-scene-wss-00 L1061-L1069.
fn event_l1061() -> String {
    common::fixture("event_l1061.json")
}

/// draft-atwood-jmap-scene-wss-00 L1077-L1085.
fn event_l1077() -> String {
    common::fixture("event_l1077.json")
}

/// draft-atwood-jmap-scene-wss-00 L1106-L1114.
fn event_l1106() -> String {
    common::fixture("event_l316.json")
}

/// draft-atwood-jmap-scene-wss-00 L1137-L1145.
fn event_l1137() -> String {
    common::fixture("event_l1137.json")
}

/// draft-atwood-jmap-scene-wss-00 L1153-L1155.
fn stream_l1153() -> String {
    common::fixture("stream_l646.json")
}

fn events() -> [String; 13] {
    [
        event_l214(),
        event_l256(),
        event_l270(),
        event_l316(),
        event_l330(),
        event_l357(),
        event_l727(),
        event_l742(),
        event_l858(),
        event_l1061(),
        event_l1077(),
        event_l1106(),
        event_l1137(),
    ]
}

fn streams() -> [String; 8] {
    [
        stream_l457(),
        stream_l477(),
        stream_l500(),
        stream_l587(),
        stream_l646(),
        stream_l955(),
        stream_l1049(),
        stream_l1153(),
    ]
}

#[test]
fn every_event_example_round_trips_to_a_known_variant() {
    for text in &events() {
        let e: SceneEvent = round_trip(text);
        assert!(
            !matches!(e, SceneEvent::Unknown { .. }),
            "decoded as Unknown: {text}"
        );
    }
}

#[test]
fn every_event_example_carries_the_hosting_account_id() {
    // L178 "Every event carries an `accountId` (String) property: the id
    // of the region's hosting account". Every example uses "account-xyz".
    for text in &events() {
        let account_id = match round_trip(text) {
            SceneEvent::Avatar(e) => e.account_id,
            SceneEvent::Object(e) => e.account_id,
            SceneEvent::Interaction(e) => e.account_id,
            _ => panic!("decoded as Unknown: {text}"),
        };
        assert_eq!(account_id.as_ref(), "account-xyz", "{text}");
    }
}

#[test]
fn every_stream_example_round_trips_to_a_known_variant() {
    for text in &streams() {
        let m: SceneStreamMessage = round_trip(text);
        assert!(
            !matches!(m, SceneStreamMessage::Unknown { .. }),
            "decoded as Unknown: {text}"
        );
    }
}

#[test]
fn avatar_entered_example_fields() {
    // L211 "Example — avatar enters a region".
    let SceneEvent::Avatar(e) = round_trip(&event_l214()) else {
        panic!("wrong variant")
    };
    assert_eq!(e.region_id.as_ref(), "01J5ABC0000000000000000001");
    assert_eq!(e.avatar_id.as_ref(), "01J5AV00000000000000000001");
    assert_eq!(e.event, AvatarEventKind::Entered);
    assert_eq!(e.user_id, "user:alice@example.com");
    assert_eq!(e.display_name, "Alice Chen");
}

#[test]
fn object_created_and_destroyed_example_fields() {
    // L250-251 `position`: "Present for `\"created\"` and `\"updated\"`
    // events; `null` for `\"destroyed\"`."
    let SceneEvent::Object(c) = round_trip(&event_l256()) else {
        panic!("wrong variant")
    };
    assert_eq!(c.event, ObjectEventKind::Created);
    assert_eq!(c.updated_by.as_deref(), Some("user:bob@example.com"));
    assert_eq!(c.position, Some([15.0, 0.0, -5.0]));

    let SceneEvent::Object(d) = round_trip(&event_l270()) else {
        panic!("wrong variant")
    };
    assert_eq!(d.event, ObjectEventKind::Destroyed);
    assert_eq!(d.position, None);
}

#[test]
fn interaction_examples_fields() {
    // L322-323 `"action": "click"`, `"data": null`.
    let SceneEvent::Interaction(click) = round_trip(&event_l316()) else {
        panic!("wrong variant")
    };
    assert_eq!(click.action, "click");
    assert_eq!(click.data, None);

    // L336-337 reverse-domain action with an opaque payload.
    let SceneEvent::Interaction(solve) = round_trip(&event_l330()) else {
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
    // L458-460 explicit regions and event types.
    let SceneStreamMessage::Enable(e) = round_trip(&stream_l457()) else {
        panic!("wrong variant")
    };
    assert_eq!(
        e.region_ids.unwrap()[0].as_ref(),
        "01J5ABC0000000000000000001"
    );
    assert_eq!(
        e.event_types.unwrap(),
        [SceneEventType::Avatar, SceneEventType::Object]
    );

    // L506 `"eventTypes": null`: "all three event types ... are
    // delivered" (L510-512).
    let SceneStreamMessage::Enable(m) = round_trip(&stream_l500()) else {
        panic!("wrong variant")
    };
    assert_eq!(m.region_ids.unwrap().len(), 2);
    assert_eq!(m.event_types, None);

    // L591 the unrecognized-only error case still decodes: rejecting it
    // with a `RequestError` (L595-599) is the server's job.
    let SceneStreamMessage::Enable(bad) = round_trip(&stream_l587()) else {
        panic!("wrong variant")
    };
    assert_eq!(
        bad.event_types.unwrap(),
        [SceneEventType::Other("SceneFooBarEvent".to_owned())]
    );
    // L589 `"id": "e1"` is the value the L606 `RequestError` echoes.
    assert_eq!(bad.id.as_deref(), Some("e1"));
}

#[test]
fn stream_disable_example() {
    // L647 `{"@type": "SceneStreamDisable"}`.
    let m: SceneStreamMessage = round_trip(&stream_l646());
    assert!(matches!(m, SceneStreamMessage::Disable(_)));
}

#[test]
fn session_example_advertises_both_scene_capabilities() {
    // L148-150 "A server advertising
    // `urn:ietf:params:jmap:scene:websocket` MUST also advertise both
    // `urn:ietf:params:jmap:websocket` and `urn:ietf:params:jmap:scene`";
    // L135-137 its value "is an empty JSON object `{}`".
    let v: Value = serde_json::from_str(&session_l155()).unwrap();
    let caps = &v["capabilities"];
    assert_eq!(caps[JMAP_SCENE_WEBSOCKET_URI], json!({}));
    assert_eq!(caps[JMAP_SCENE_URI], json!({}));
}
