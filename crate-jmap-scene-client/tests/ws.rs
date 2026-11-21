//! `SceneWsExt` over a real WebSocket, driven by the frame examples of
//! draft-atwood-jmap-scene-wss-00 (jmap-chat-spec 6bcd9d8; line numbers
//! cite that revision). Every JSON constant is copied from the draft; the
//! oracle is the draft text, never the code under test.
//!
//! A tokio-tungstenite server on 127.0.0.1 plays the draft's server: it
//! sends the server frames verbatim and returns each text frame the client
//! sent, which the test compares with the draft's client frames.

use futures::{SinkExt as _, StreamExt as _};
use jmap_base_client::{DEFAULT_WS_MAX_MESSAGE_BYTES, connect_ws_allow_plaintext};
use jmap_base_client::{WsFrame, WsSession};
use jmap_scene_client::{SceneWsExt, SceneWsFrame};
use jmap_scene_types::{
    AvatarEventKind, ObjectEventKind, SceneEventType, SceneInteractionEvent, SceneStreamEnable,
};
use jmap_types::Id;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

/// L1049-1053 §Complete Example "Enable Scene Event Stream".
const ENABLE: &str = r#"{
  "@type": "SceneStreamEnable",
  "regionIds": ["01J5ABC0000000000000000001"],
  "eventTypes": null
}"#;

/// L1061-1069 "Another Avatar Enters".
const AVATAR_ENTERED: &str = r#"{
  "@type": "SceneAvatarEvent",
  "accountId": "account-xyz",
  "regionId": "01J5ABC0000000000000000001",
  "avatarId": "01J5AV00000000000000000002",
  "event": "entered",
  "userId": "user:bob@example.com",
  "displayName": "Bob Martinez"
}"#;

/// L1077-1085 "Object Placed".
const OBJECT_CREATED: &str = r#"{
  "@type": "SceneObjectEvent",
  "accountId": "account-xyz",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5QBJ0000000000000000042",
  "event": "created",
  "updatedBy": "user:bob@example.com",
  "position": [8.0, 1.2, -3.5]
}"#;

/// L1106-1114 "User Interaction".
const INTERACTION: &str = r#"{
  "@type": "SceneInteractionEvent",
  "accountId": "account-xyz",
  "regionId": "01J5ABC0000000000000000001",
  "objectId": "01J5QBJ0000000000000000001",
  "userId": "user:alice@example.com",
  "action": "click",
  "data": null
}"#;

/// L1122-1129 "State Change (Interleaved)".
const STATE_CHANGE: &str = r#"{
  "@type": "StateChange",
  "changed": {
    "account-xyz": {
      "SceneObject": "s301"
    }
  }
}"#;

/// L1137-1145 "Bob Leaves".
const AVATAR_LEFT: &str = r#"{
  "@type": "SceneAvatarEvent",
  "accountId": "account-xyz",
  "regionId": "01J5ABC0000000000000000001",
  "avatarId": "01J5AV00000000000000000002",
  "event": "left",
  "userId": "user:bob@example.com",
  "displayName": "Bob Martinez"
}"#;

/// L1153-1155 "Disable Scene Stream".
const DISABLE: &str = r#"{
  "@type": "SceneStreamDisable"
}"#;

/// L587-592 "Client sends" an `eventTypes` of only an unrecognized value.
const ENABLE_UNRECOGNIZED_ONLY: &str = r#"{
  "@type": "SceneStreamEnable",
  "id": "e1",
  "regionIds": ["01J5ABC0000000000000000001"],
  "eventTypes": ["SceneFooBarEvent"]
}"#;

/// L604-609 "Server responds".
const REQUEST_ERROR: &str = r#"{
  "@type": "RequestError",
  "requestId": "e1",
  "type": "urn:ietf:params:jmap:error:invalidArguments",
  "detail": "eventTypes contains no recognized values"
}"#;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// Start a server that, for each step, either sends the frame or reads
/// one client text frame, then closes. Returns the client session and a
/// handle yielding the client frames in order.
async fn serve(steps: Vec<Step>) -> (WsSession, JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut received = Vec::new();
        for step in steps {
            match step {
                Step::Send(text) => ws.send(Message::Text(text.into())).await.unwrap(),
                Step::Recv => match ws.next().await.unwrap().unwrap() {
                    Message::Text(t) => received.push(json(&t)),
                    other => panic!("expected a text frame, got {other:?}"),
                },
            }
        }
        ws.close(None).await.unwrap();
        received
    });
    let session =
        connect_ws_allow_plaintext(&format!("ws://{addr}/"), None, DEFAULT_WS_MAX_MESSAGE_BYTES)
            .await
            .unwrap();
    (session, server)
}

enum Step {
    Send(&'static str),
    Recv,
}

/// A Session advertising the Session Object Example's capabilities
/// (draft-atwood-jmap-scene-wss-00 §Capability), or only the first two
/// without `scene_ws`.
fn session_with(scene_ws: bool) -> jmap_base_client::Session {
    let mut caps = serde_json::json!({
        "urn:ietf:params:jmap:websocket": { "url": "wss://scene.example.com/jmap/ws/", "supportsPush": true },
        "urn:ietf:params:jmap:scene": {}
    });
    if scene_ws {
        caps["urn:ietf:params:jmap:scene:websocket"] = serde_json::json!({});
    }
    serde_json::from_value(serde_json::json!({
        "capabilities": caps,
        "accounts": {},
        "primaryAccounts": {},
        "username": "u",
        "apiUrl": "https://scene.example.com/api/",
        "downloadUrl": "https://scene.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "https://scene.example.com/ul/{accountId}/",
        "eventSourceUrl": "https://scene.example.com/sse/",
        "state": "s1"
    }))
    .unwrap()
}

fn scene_session() -> jmap_base_client::Session {
    session_with(true)
}

async fn next(ws: &mut WsSession) -> SceneWsFrame {
    ws.next_scene_frame().await.unwrap().unwrap()
}

#[tokio::test]
async fn complete_example_session() {
    let (mut ws, server) = serve(vec![
        Step::Recv,
        Step::Send(AVATAR_ENTERED),
        Step::Send(OBJECT_CREATED),
        Step::Recv,
        Step::Send(INTERACTION),
        Step::Send(STATE_CHANGE),
        Step::Send(AVATAR_LEFT),
        Step::Recv,
    ])
    .await;

    let mut enable = SceneStreamEnable::default();
    enable.region_ids = Some(vec![Id::from("01J5ABC0000000000000000001")]);
    ws.send_scene_stream_enable(&scene_session(), &enable)
        .await
        .unwrap();

    match next(&mut ws).await {
        SceneWsFrame::Avatar(e) => {
            // L202-203 `event`: "`\"entered\"`, `\"left\"`, or `\"ejected\"`".
            assert_eq!(e.event, AvatarEventKind::Entered);
            assert_eq!(e.region_id, Id::from("01J5ABC0000000000000000001"));
            assert_eq!(e.avatar_id, Id::from("01J5AV00000000000000000002"));
            assert_eq!(e.user_id, "user:bob@example.com");
            assert_eq!(e.display_name, "Bob Martinez");
        }
        other => panic!("expected SceneAvatarEvent, got {other:?}"),
    }
    match next(&mut ws).await {
        SceneWsFrame::Object(e) => {
            assert_eq!(e.event, ObjectEventKind::Created);
            assert_eq!(e.object_id, Id::from("01J5QBJ0000000000000000042"));
            assert_eq!(e.updated_by.as_deref(), Some("user:bob@example.com"));
            assert_eq!(e.position, Some([8.0, 1.2, -3.5]));
        }
        other => panic!("expected SceneObjectEvent, got {other:?}"),
    }

    // L171-173: "a client can submit an interaction by sending a
    // `SceneInteractionEvent` frame to the server".
    let click = SceneInteractionEvent::new(
        Id::from("account-xyz"),
        Id::from("01J5ABC0000000000000000001"),
        Id::from("01J5QBJ0000000000000000001"),
        "user:alice@example.com".to_owned(),
        "click".to_owned(),
    );
    ws.send_scene_interaction(&click).await.unwrap();

    // "the server delivers each accepted interaction to subscribers as a
    // `SceneInteractionEvent`" (L173-174).
    match next(&mut ws).await {
        SceneWsFrame::Interaction(e) => {
            assert_eq!(e.object_id, Id::from("01J5QBJ0000000000000000001"));
            assert_eq!(e.user_id, "user:alice@example.com");
            assert_eq!(e.action, "click");
            assert_eq!(e.data, None);
        }
        other => panic!("expected SceneInteractionEvent, got {other:?}"),
    }
    // L669-671: Scene frames "are interleaved with `StateChange`,
    // `Response`, and other ephemeral event frames ... on the same
    // WebSocket connection".
    match next(&mut ws).await {
        SceneWsFrame::Other(WsFrame::StateChange(sc)) => assert_eq!(
            sc.changed[&Id::from("account-xyz")]["SceneObject"].as_ref(),
            "s301"
        ),
        other => panic!("expected StateChange, got {other:?}"),
    }
    match next(&mut ws).await {
        SceneWsFrame::Avatar(e) => assert_eq!(e.event, AvatarEventKind::Left),
        other => panic!("expected SceneAvatarEvent, got {other:?}"),
    }

    ws.send_scene_stream_disable().await.unwrap();
    assert!(ws.next_scene_frame().await.is_none());

    let sent = server.await.unwrap();
    assert_eq!(sent, [json(ENABLE), json(INTERACTION), json(DISABLE)]);
}

#[tokio::test]
async fn request_error_unknown_and_malformed_frames_pass_through() {
    let (mut ws, server) = serve(vec![
        Step::Recv,
        Step::Send(REQUEST_ERROR),
        // L987-989: an unrecognized `@type` "SHOULD silently ignore the
        // message and SHOULD NOT close the WebSocket connection".
        Step::Send(r#"{"@type":"SceneWeatherEvent","regionId":"01J5ABC0000000000000000001"}"#),
        // L202 `event` (String): a number is not a SceneAvatarEvent.
        Step::Send(
            r#"{"@type":"SceneAvatarEvent","accountId":"account-xyz","regionId":"01J5ABC0000000000000000001","avatarId":"user:bob@example.com","event":1,"userId":"user:bob@example.com","displayName":"Bob Martinez"}"#,
        ),
        Step::Send(AVATAR_LEFT),
    ])
    .await;

    let mut enable = SceneStreamEnable::default();
    enable.region_ids = Some(vec![Id::from("01J5ABC0000000000000000001")]);
    enable.event_types = Some(vec![SceneEventType::Other("SceneFooBarEvent".to_owned())]);
    enable.id = Some("e1".to_owned());
    ws.send_scene_stream_enable(&scene_session(), &enable)
        .await
        .unwrap();

    // L562-563: "the server MUST respond with a `RequestError` frame".
    match next(&mut ws).await {
        SceneWsFrame::Other(WsFrame::RequestError(e)) => {
            // L450-452: the error "carries this value as its `requestId`".
            assert_eq!(e.request_id.as_deref(), Some("e1"));
            assert_eq!(
                e.problem_type,
                "urn:ietf:params:jmap:error:invalidArguments"
            );
            assert_eq!(
                e.detail.as_deref(),
                Some("eventTypes contains no recognized values")
            );
        }
        other => panic!("expected RequestError, got {other:?}"),
    }
    match next(&mut ws).await {
        SceneWsFrame::Other(WsFrame::Unknown { type_name, .. }) => {
            assert_eq!(type_name, "SceneWeatherEvent");
        }
        other => panic!("expected an unknown frame, got {other:?}"),
    }
    match next(&mut ws).await {
        SceneWsFrame::Other(WsFrame::Unknown { type_name, raw }) => {
            assert_eq!(type_name, "SceneAvatarEvent");
            assert_eq!(raw["event"], 1);
        }
        other => panic!("expected the malformed frame unparsed, got {other:?}"),
    }
    // L612-613: "This error does not close the WebSocket connection".
    match next(&mut ws).await {
        SceneWsFrame::Avatar(e) => assert_eq!(e.event, AvatarEventKind::Left),
        other => panic!("expected SceneAvatarEvent, got {other:?}"),
    }
    assert!(ws.next_scene_frame().await.is_none());

    assert_eq!(server.await.unwrap(), [json(ENABLE_UNRECOGNIZED_ONLY)]);
}

/// jmap-scene-wss-guide §Checking capabilities: with
/// `urn:ietf:params:jmap:websocket` and `urn:ietf:params:jmap:scene` but
/// not `urn:ietf:params:jmap:scene:websocket`, "clients MUST NOT send
/// `SceneStreamEnable`": the call fails and the server receives nothing.
#[tokio::test]
async fn stream_enable_refused_without_scene_websocket() {
    let (mut ws, server) = serve(vec![Step::Recv]).await;
    let err = ws
        .send_scene_stream_enable(&session_with(false), &SceneStreamEnable::default())
        .await
        .expect_err("must refuse");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidArgument(_)),
        "{err:?}"
    );
    assert!(!jmap_scene_client::supports_scene_websocket(&session_with(
        false
    )));
    assert!(jmap_scene_client::supports_scene_websocket(&session_with(
        true
    )));
    // The next frame sent is the first the server sees, so the refused
    // enable never reached it.
    ws.send_scene_stream_disable().await.unwrap();
    let received = server.await.unwrap();
    assert_eq!(received.len(), 1, "{received:?}");
    assert_eq!(received[0]["@type"], "SceneStreamDisable", "{received:?}");
}

/// jmap-scene-wss-guide.md L428-430 (jmap-chat-spec 120db2c), §Extra
/// fields: "Clients MUST tolerate unknown fields in event objects. Future
/// spec revisions or deployment extensions may add fields. Parse only the
/// fields you recognize; ignore the rest."
///
/// Each frame is a draft example (L1061, L1077, L1106 above) with
/// hand-added unknown fields, some with structured values. Each still
/// arrives as its typed variant with the recognized fields intact.
#[tokio::test]
async fn events_with_unknown_fields_are_tolerated() {
    let (mut ws, server) = serve(vec![
        Step::Send(
            r#"{"@type":"SceneAvatarEvent","accountId":"account-xyz","regionId":"01J5ABC0000000000000000001","avatarId":"01J5AV00000000000000000002","event":"entered","userId":"user:bob@example.com","displayName":"Bob Martinez","futureField":{"nested":[1,2,3]},"x-vendor-mood":"cheerful"}"#,
        ),
        Step::Send(
            r#"{"@type":"SceneObjectEvent","accountId":"account-xyz","regionId":"01J5ABC0000000000000000001","objectId":"01J5QBJ0000000000000000042","event":"created","updatedBy":"user:bob@example.com","position":[8.0,1.2,-3.5],"velocity":[0.0,0.0,1.0],"x-vendor-tick":42}"#,
        ),
        Step::Send(
            r#"{"@type":"SceneInteractionEvent","accountId":"account-xyz","regionId":"01J5ABC0000000000000000001","objectId":"01J5QBJ0000000000000000001","userId":"user:alice@example.com","action":"click","data":null,"handedness":"left","x-vendor-latencyMs":12.5}"#,
        ),
    ])
    .await;

    match next(&mut ws).await {
        SceneWsFrame::Avatar(e) => {
            assert_eq!(e.event, AvatarEventKind::Entered);
            assert_eq!(e.avatar_id, Id::from("01J5AV00000000000000000002"));
            assert_eq!(e.user_id, "user:bob@example.com");
            assert_eq!(e.display_name, "Bob Martinez");
        }
        other => panic!("expected SceneAvatarEvent, got {other:?}"),
    }
    match next(&mut ws).await {
        SceneWsFrame::Object(e) => {
            assert_eq!(e.event, ObjectEventKind::Created);
            assert_eq!(e.object_id, Id::from("01J5QBJ0000000000000000042"));
            assert_eq!(e.updated_by.as_deref(), Some("user:bob@example.com"));
            assert_eq!(e.position, Some([8.0, 1.2, -3.5]));
        }
        other => panic!("expected SceneObjectEvent, got {other:?}"),
    }
    match next(&mut ws).await {
        SceneWsFrame::Interaction(e) => {
            assert_eq!(e.object_id, Id::from("01J5QBJ0000000000000000001"));
            assert_eq!(e.user_id, "user:alice@example.com");
            assert_eq!(e.action, "click");
            assert_eq!(e.data, None);
        }
        other => panic!("expected SceneInteractionEvent, got {other:?}"),
    }
    assert!(ws.next_scene_frame().await.is_none());
    assert!(server.await.unwrap().is_empty());
}

/// draft-atwood-jmap-scene-wss-00 at jmap-chat-spec 93f2258,
/// §SceneRegionEvent "Example — a round starts".
const ROUND_START: &str = r#"{
  "@type": "SceneRegionEvent",
  "accountId": "account-xyz",
  "regionId": "01J5ABC0000000000000000001",
  "event": "com.example.game.round-start",
  "data": {"round": 6, "buyTimeSeconds": 20}
}"#;

/// A `SceneRegionEvent` frame arrives as [`SceneWsFrame::Region`]; one
/// without its required `event` arrives untyped in `Other`.
#[tokio::test]
async fn region_event_is_typed() {
    let (mut ws, server) = serve(vec![
        Step::Send(ROUND_START),
        Step::Send(
            r#"{"@type":"SceneRegionEvent","accountId":"account-xyz","regionId":"01J5ABC0000000000000000001","data":null}"#,
        ),
    ])
    .await;
    match next(&mut ws).await {
        SceneWsFrame::Region(e) => {
            assert_eq!(e.region_id, Id::from("01J5ABC0000000000000000001"));
            assert_eq!(e.event, "com.example.game.round-start");
            assert_eq!(e.data.as_ref().unwrap()["round"], 6);
        }
        other => panic!("expected SceneRegionEvent, got {other:?}"),
    }
    assert!(matches!(
        next(&mut ws).await,
        SceneWsFrame::Other(WsFrame::Unknown { .. })
    ));
    assert!(ws.next_scene_frame().await.is_none());
    assert!(server.await.unwrap().is_empty());
}

/// A Session whose `urn:ietf:params:jmap:scene:websocket` value is `cap`.
fn session_with_ws_capability(cap: Value) -> jmap_base_client::Session {
    let mut session = session_with(true);
    session
        .capabilities
        .insert("urn:ietf:params:jmap:scene:websocket".to_owned(), cap);
    session
}

/// jmap-chat-spec 93f2258 §Capability: `supportsObserve` "`true` if the
/// server accepts observer subscriptions"; "the value `{}` advertises no
/// optional features". The client sends `observe: true` only to a server
/// that advertises it, and the frame carries `observe` as sent.
#[tokio::test]
async fn observe_needs_supports_observe() {
    let mut enable = SceneStreamEnable::default();
    enable.region_ids = Some(vec![Id::from("01J5ABC0000000000000000001")]);
    enable.observe = Some(true);

    let plain = session_with_ws_capability(serde_json::json!({}));
    let observing = session_with_ws_capability(serde_json::json!({"supportsObserve": true}));
    assert!(!jmap_scene_client::supports_scene_observe(&plain));
    assert!(jmap_scene_client::supports_scene_observe(&observing));

    let (mut ws, server) = serve(vec![Step::Recv]).await;
    let err = ws
        .send_scene_stream_enable(&plain, &enable)
        .await
        .expect_err("must refuse");
    assert!(
        matches!(err, jmap_base_client::ClientError::InvalidArgument(_)),
        "{err:?}"
    );
    ws.send_scene_stream_enable(&observing, &enable)
        .await
        .unwrap();
    let received = server.await.unwrap();
    assert_eq!(
        received,
        [serde_json::json!({
            "@type": "SceneStreamEnable",
            "regionIds": ["01J5ABC0000000000000000001"],
            "eventTypes": null,
            "observe": true
        })]
    );
}
