//! `ChatWsExt` over a real WebSocket, driven by the frame examples of
//! draft-atwood-jmap-chat-wss-00 (jmap-chat-spec 6c38e2f; line numbers
//! cite that revision). Every server JSON constant is copied from the
//! draft; the oracle is the draft text, never the code under test.
//!
//! A tokio-tungstenite server on 127.0.0.1 plays the draft's server: it
//! sends the server frames verbatim and returns each text frame the client
//! sent, which the test compares with hand-written client frames.

use futures::{SinkExt as _, StreamExt as _};
use jmap_base_client::{
    ClientError, DEFAULT_WS_MAX_MESSAGE_BYTES, WsFrame, WsSession, connect_ws_allow_plaintext,
};
use jmap_chat_client::{ChatWsExt, ChatWsFrame};
use jmap_chat_types::{ChatStreamEnable, Clearable, Presence};
use jmap_types::Id;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// L199-204 §Enabling Ephemeral Push.
const ENABLE: &str = r#"{
  "@type": "ChatStreamEnable",
  "dataTypes": ["typing", "presence"],
  "chatIds": null,
  "contactIds": null
}"#;

/// The L199-204 frame without its `null` members. L188: "An absent
/// `chatIds` or `contactIds` is equivalent to `null`".
const ENABLE_SENT: &str = r#"{
  "@type": "ChatStreamEnable",
  "dataTypes": ["typing", "presence"]
}"#;

/// L217-219 §Disabling Ephemeral Push.
const DISABLE: &str = r#"{
  "@type": "ChatStreamDisable"
}"#;

/// L242-248 §ChatTypingEvent.
const TYPING: &str = r#"{
  "@type": "ChatTypingEvent",
  "accountId": "u1",
  "chatId": "01J3XKZQN4MWVT8PPBEHTJ3HX0",
  "senderId": "c8xq2m",
  "typing": true
}"#;

/// L287-295 §ChatPresenceEvent.
const PRESENCE: &str = r#"{
  "@type": "ChatPresenceEvent",
  "accountId": "u1",
  "contactId": "c8xq2m",
  "presence": "away",
  "lastActiveAt": "2026-04-26T12:00:00Z",
  "statusText": "In a meeting",
  "statusEmoji": null
}"#;

/// §Example (L394-402): the `StateChange` after the typing event.
const STATE_CHANGE: &str = r#"{
  "@type": "StateChange",
  "changed": {
    "u1": {
      "Message": "d35ecb040aab"
    }
  },
  "pushState": "bbb"
}"#;

/// L194: an empty `dataTypes` gets "a `RequestError` frame ... with a
/// `type` of `\"urn:ietf:params:jmap:error:invalidArguments\"`"; L186: it
/// "carries this value as its `requestId`".
const ENABLE_EMPTY_SENT: &str = r#"{
  "@type": "ChatStreamEnable",
  "id": "enable-1",
  "dataTypes": []
}"#;

/// RFC 8887 §4.3.4 `RequestError` for [`ENABLE_EMPTY_SENT`].
const REQUEST_ERROR: &str = r#"{
  "@type": "RequestError",
  "requestId": "enable-1",
  "type": "urn:ietf:params:jmap:error:invalidArguments",
  "status": 400
}"#;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

enum Step {
    Send(&'static str),
    Recv,
}

/// Start a server that, for each step, either sends the frame or reads
/// one client text frame, then closes with `close`. Returns the client
/// session and a handle yielding the client frames in order.
async fn serve(steps: Vec<Step>, close: Option<CloseFrame>) -> (WsSession, JoinHandle<Vec<Value>>) {
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
        ws.close(close).await.unwrap();
        // Keep the socket open until the client has read the Close frame.
        let _ = ws.next().await;
        received
    });
    let session =
        connect_ws_allow_plaintext(&format!("ws://{addr}/"), None, DEFAULT_WS_MAX_MESSAGE_BYTES)
            .await
            .unwrap();
    (session, server)
}

async fn next(ws: &mut WsSession) -> ChatWsFrame {
    ws.next_chat_frame().await.unwrap().unwrap()
}

#[tokio::test]
async fn enable_events_state_change_disable() {
    let (mut ws, server) = serve(
        vec![
            Step::Recv,
            Step::Send(TYPING),
            Step::Send(PRESENCE),
            Step::Send(STATE_CHANGE),
            Step::Recv,
        ],
        None,
    )
    .await;

    let enable: ChatStreamEnable = serde_json::from_str(ENABLE).unwrap();
    ws.send_stream_enable(&enable).await.unwrap();

    match next(&mut ws).await {
        ChatWsFrame::ChatTyping(e) => {
            assert_eq!(e.account_id, Id::from("u1"));
            assert_eq!(e.chat_id, Id::from("01J3XKZQN4MWVT8PPBEHTJ3HX0"));
            assert_eq!(e.sender_id, Id::from("c8xq2m"));
            assert!(e.typing);
        }
        other => panic!("expected ChatTypingEvent, got {other:?}"),
    }
    match next(&mut ws).await {
        ChatWsFrame::ChatPresence(e) => {
            assert_eq!(e.account_id, Id::from("u1"));
            assert_eq!(e.contact_id, Id::from("c8xq2m"));
            assert_eq!(e.presence, Presence::Away);
            assert_eq!(
                e.last_active_at.as_ref().map(|d| d.as_ref()),
                Some("2026-04-26T12:00:00Z")
            );
            assert_eq!(
                e.status_text,
                Some(Clearable::Set("In a meeting".to_owned()))
            );
            // L284: "`null` explicitly clears the field".
            assert_eq!(e.status_emoji, Some(Clearable::Clear));
        }
        other => panic!("expected ChatPresenceEvent, got {other:?}"),
    }
    // L165: ephemeral frames "are interleaved with `StateChange` and
    // `Response` frames on the same WebSocket connection".
    match next(&mut ws).await {
        ChatWsFrame::Other(WsFrame::StateChange(sc)) => assert_eq!(
            sc.changed[&Id::from("u1")]["Message"].as_ref(),
            "d35ecb040aab"
        ),
        other => panic!("expected StateChange, got {other:?}"),
    }

    ws.send_stream_disable().await.unwrap();
    assert!(ws.next_chat_frame().await.is_none());
    drop(ws);

    assert_eq!(server.await.unwrap(), [json(ENABLE_SENT), json(DISABLE)]);
}

#[tokio::test]
async fn request_error_unknown_and_malformed_frames_pass_through() {
    let (mut ws, server) = serve(
        vec![
            Step::Recv,
            Step::Send(REQUEST_ERROR),
            // L304: an unrecognized `@type` "SHOULD silently ignore the
            // message and SHOULD NOT close the WebSocket connection".
            Step::Send(r#"{"@type":"ChatWeatherEvent","accountId":"u1"}"#),
            // L238 `typing` (Boolean): a string is not a ChatTypingEvent.
            Step::Send(
                r#"{"@type":"ChatTypingEvent","accountId":"u1","chatId":"01J3XKZQN4MWVT8PPBEHTJ3HX0","senderId":"c8xq2m","typing":"yes"}"#,
            ),
            Step::Send(TYPING),
        ],
        None,
    )
    .await;

    let enable: ChatStreamEnable =
        serde_json::from_str(r#"{"id":"enable-1","dataTypes":[]}"#).unwrap();
    ws.send_stream_enable(&enable).await.unwrap();

    match next(&mut ws).await {
        ChatWsFrame::Other(frame @ WsFrame::RequestError(_)) => {
            assert_eq!(frame.request_id(), Some("enable-1"));
            let WsFrame::RequestError(e) = frame else {
                unreachable!("matched RequestError above")
            };
            assert_eq!(
                e.problem_type,
                "urn:ietf:params:jmap:error:invalidArguments"
            );
        }
        other => panic!("expected RequestError, got {other:?}"),
    }
    match next(&mut ws).await {
        ChatWsFrame::Other(WsFrame::Unknown { type_name, .. }) => {
            assert_eq!(type_name, "ChatWeatherEvent");
        }
        other => panic!("expected an unknown frame, got {other:?}"),
    }
    match next(&mut ws).await {
        ChatWsFrame::Other(WsFrame::Unknown { type_name, raw }) => {
            assert_eq!(type_name, "ChatTypingEvent");
            assert_eq!(raw["typing"], "yes");
        }
        other => panic!("expected the malformed frame unparsed, got {other:?}"),
    }
    match next(&mut ws).await {
        ChatWsFrame::ChatTyping(e) => assert!(e.typing),
        other => panic!("expected ChatTypingEvent, got {other:?}"),
    }
    assert!(ws.next_chat_frame().await.is_none());
    drop(ws);

    assert_eq!(server.await.unwrap(), [json(ENABLE_EMPTY_SENT)]);
}

/// L129: on credential expiry the server "SHOULD use Close frame status
/// code 1008 (Policy Violation) ... so that clients can reliably detect
/// and recover from credential expiry".
#[tokio::test]
async fn close_1008_surfaces_ws_closed() {
    let (mut ws, server) = serve(
        vec![],
        Some(CloseFrame {
            code: CloseCode::Policy,
            reason: "token expired".into(),
        }),
    )
    .await;

    match ws.next_chat_frame().await {
        Some(Err(ClientError::WsClosed { code, reason })) => {
            assert_eq!(code, 1008);
            assert_eq!(reason, "token expired");
        }
        other => panic!("expected WsClosed {{ code: 1008, .. }}, got {other:?}"),
    }
    drop(ws);
    assert!(server.await.unwrap().is_empty());
}
