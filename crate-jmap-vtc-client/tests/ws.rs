//! `VtcWsExt` over a real WebSocket, driven by the frame examples of
//! draft-atwood-jmap-vtc-wss-00 (jmap-chat-spec 6bcd9d8; line numbers cite
//! that revision). Every JSON constant is copied from the draft or, where
//! the draft gives none, hand-written from the cited sentences; the oracle
//! is the draft text, never the code under test.
//!
//! A tokio-tungstenite server on 127.0.0.1 plays the draft's server: it
//! sends the server frames verbatim and returns each text frame the client
//! sent, which the test compares with the draft's client frames.

use futures::{SinkExt as _, StreamExt as _};
use jmap_base_client::{connect_ws_allow_plaintext, DEFAULT_WS_MAX_MESSAGE_BYTES};
use jmap_base_client::{WsFrame, WsSession};
use jmap_types::Id;
use jmap_vtc_client::{VtcWsExt, VtcWsFrame};
use jmap_vtc_types::{CallRole, ParticipantEventKind, RecordingState, VtcEvent, VtcStreamEnable};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const CALL: &str = "01J4XKZQN4MWVT8PPBEHTJ3AB0";

/// L489-494 §Complete Example "Enable VTC Event Stream".
const ENABLE: &str = r#"{
  "@type": "VTCStreamEnable",
  "callIds": ["01J4XKZQN4MWVT8PPBEHTJ3AB0"],
  "eventTypes": null
}"#;

/// L501-510 "Participant Joins".
const PARTICIPANT_JOINED: &str = r#"{
  "@type": "VTCParticipantEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "participantId": "01J4XMVQK7NRYS9PPCEHTK4CD0",
  "event": "joined",
  "displayName": "Alice Chen",
  "role": "participant"
}"#;

/// L517-529 "Media State Update".
const MEDIA_STATE: &str = r#"{
  "@type": "VTCMediaStateEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "participantId": "01J4XMVQK7NRYS9PPCEHTK4CD0",
  "mediaState": {
    "audio": false,
    "video": true,
    "screen": false,
    "raisedHand": false
  }
}"#;

/// L536-542 "Active Speaker Change".
const ACTIVE_SPEAKER: &str = r#"{
  "@type": "VTCActiveSpeakerEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "participantId": "01J4XNZRM8QWVT0RRDFJVS5EF0"
}"#;

/// L549-557 "Recording Starts".
const RECORDING: &str = r#"{
  "@type": "VTCRecordingStateEvent",
  "accountId": "bob-account",
  "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
  "recordingId": "01J4XP2SN9RXVV1SSEGJVT6GH0",
  "state": "recording",
  "initiatedBy": "user:bob@example.com"
}"#;

/// L564-572 "State Change (Interleaved)".
const STATE_CHANGE: &str = r#"{
  "@type": "StateChange",
  "changed": {
    "bob-account": {
      "VTCParticipant": "t101"
    }
  }
}"#;

/// L579-583 "Disable VTC Stream".
const DISABLE: &str = r#"{
  "@type": "VTCStreamDisable"
}"#;

/// L221-227: an `eventTypes` that "is an empty array" draws an
/// `invalidArguments` `RequestError`; L172-174: the frame's `id` comes
/// back "as its `requestId`". Hand-written; the draft has no example.
const ENABLE_EMPTY_EVENT_TYPES: &str = r#"{
  "@type": "VTCStreamEnable",
  "id": "enable-1",
  "callIds": null,
  "eventTypes": []
}"#;

/// RFC 8887 §4.3.4 `RequestError` answering `ENABLE_EMPTY_EVENT_TYPES`.
const REQUEST_ERROR: &str = r#"{
  "@type": "RequestError",
  "requestId": "enable-1",
  "type": "urn:ietf:params:jmap:error:invalidArguments"
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

async fn next(ws: &mut WsSession) -> VtcWsFrame {
    ws.next_vtc_frame().await.unwrap().unwrap()
}

#[tokio::test]
async fn complete_example_session() {
    let (mut ws, server) = serve(vec![
        Step::Recv,
        Step::Send(PARTICIPANT_JOINED),
        Step::Send(MEDIA_STATE),
        Step::Send(ACTIVE_SPEAKER),
        Step::Send(RECORDING),
        Step::Send(STATE_CHANGE),
        Step::Recv,
    ])
    .await;

    let mut enable = VtcStreamEnable::default();
    enable.call_ids = Some(vec![Id::from(CALL)]);
    ws.send_vtc_stream_enable(&enable).await.unwrap();

    match next(&mut ws).await {
        VtcWsFrame::Event(VtcEvent::Participant(e)) => {
            assert_eq!(e.account_id, Id::from("bob-account"));
            assert_eq!(e.call_id, Id::from(CALL));
            assert_eq!(e.participant_id, Id::from("01J4XMVQK7NRYS9PPCEHTK4CD0"));
            assert_eq!(e.event, ParticipantEventKind::Joined);
            assert_eq!(e.display_name, "Alice Chen");
            assert_eq!(e.role, CallRole::Participant);
        }
        other => panic!("expected VTCParticipantEvent, got {other:?}"),
    }
    match next(&mut ws).await {
        VtcWsFrame::Event(VtcEvent::MediaState(e)) => {
            // L515: "A participant mutes their microphone".
            assert!(!e.media_state.audio);
            assert!(e.media_state.video);
            assert!(!e.media_state.screen);
            assert!(!e.media_state.raised_hand);
        }
        other => panic!("expected VTCMediaStateEvent, got {other:?}"),
    }
    match next(&mut ws).await {
        VtcWsFrame::Event(VtcEvent::ActiveSpeaker(e)) => {
            assert_eq!(e.participant_id, Id::from("01J4XNZRM8QWVT0RRDFJVS5EF0"));
        }
        other => panic!("expected VTCActiveSpeakerEvent, got {other:?}"),
    }
    match next(&mut ws).await {
        VtcWsFrame::Event(VtcEvent::RecordingState(e)) => {
            assert_eq!(e.recording_id, Some(Id::from("01J4XP2SN9RXVV1SSEGJVT6GH0")));
            assert_eq!(e.state, RecordingState::Recording);
            assert_eq!(e.initiated_by, "user:bob@example.com");
        }
        other => panic!("expected VTCRecordingStateEvent, got {other:?}"),
    }
    // L274-275: event frames "are interleaved with `StateChange` and
    // `Response` frames on the same WebSocket connection".
    match next(&mut ws).await {
        VtcWsFrame::Other(WsFrame::StateChange(sc)) => assert_eq!(
            sc.changed[&Id::from("bob-account")]["VTCParticipant"].as_ref(),
            "t101"
        ),
        other => panic!("expected StateChange, got {other:?}"),
    }

    ws.send_vtc_stream_disable().await.unwrap();
    assert!(ws.next_vtc_frame().await.is_none());

    assert_eq!(server.await.unwrap(), [json(ENABLE), json(DISABLE)]);
}

#[tokio::test]
async fn request_error_unknown_and_malformed_frames_pass_through() {
    let (mut ws, server) = serve(vec![
        Step::Recv,
        Step::Send(REQUEST_ERROR),
        // L443-446: an unrecognized `@type` "SHOULD silently ignore the
        // message and SHOULD NOT close the WebSocket connection".
        Step::Send(r#"{"@type":"VTCWeatherEvent","callId":"01J4XKZQN4MWVT8PPBEHTJ3AB0"}"#),
        // L507 `event` is a string ("joined"); a number is not a
        // VTCParticipantEvent.
        Step::Send(
            r#"{"@type":"VTCParticipantEvent","accountId":"bob-account","callId":"01J4XKZQN4MWVT8PPBEHTJ3AB0","participantId":"01J4XMVQK7NRYS9PPCEHTK4CD0","event":1,"displayName":"Alice Chen","role":"participant"}"#,
        ),
        Step::Send(ACTIVE_SPEAKER),
    ])
    .await;

    let mut enable = VtcStreamEnable::default();
    enable.id = Some("enable-1".to_owned());
    enable.event_types = Some(Vec::new());
    ws.send_vtc_stream_enable(&enable).await.unwrap();

    match next(&mut ws).await {
        VtcWsFrame::Other(WsFrame::RequestError(e)) => {
            assert_eq!(e.request_id.as_deref(), Some("enable-1"));
            assert_eq!(
                e.problem_type,
                "urn:ietf:params:jmap:error:invalidArguments"
            );
        }
        other => panic!("expected RequestError, got {other:?}"),
    }
    match next(&mut ws).await {
        VtcWsFrame::Other(WsFrame::Unknown { type_name, .. }) => {
            assert_eq!(type_name, "VTCWeatherEvent");
        }
        other => panic!("expected an unknown frame, got {other:?}"),
    }
    match next(&mut ws).await {
        VtcWsFrame::Other(WsFrame::Unknown { type_name, raw }) => {
            assert_eq!(type_name, "VTCParticipantEvent");
            assert_eq!(raw["event"], 1);
        }
        other => panic!("expected the malformed frame unparsed, got {other:?}"),
    }
    match next(&mut ws).await {
        VtcWsFrame::Event(VtcEvent::ActiveSpeaker(e)) => {
            assert_eq!(e.call_id, Id::from(CALL));
        }
        other => panic!("expected VTCActiveSpeakerEvent, got {other:?}"),
    }
    assert!(ws.next_vtc_frame().await.is_none());

    assert_eq!(server.await.unwrap(), [json(ENABLE_EMPTY_EVENT_TYPES)]);
}
