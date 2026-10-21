//! `GraphEditWsExt` over a real WebSocket, checked against
//! draft-atwood-jmap-graph-edit-00 §WebSocket Stream (jmap-chat-spec
//! 6bcd9d8; line numbers cite that revision). The enable frame is copied
//! from the draft's example; the other frames are hand-written from the
//! draft's property lists. The oracle is the draft text, never the code
//! under test.
//!
//! A tokio-tungstenite server on 127.0.0.1 plays the server: it sends the
//! server frames verbatim and returns each text frame the client sent.

use futures::{SinkExt as _, StreamExt as _};
use jmap_base_client::ws::{connect_ws_allow_plaintext, DEFAULT_WS_MAX_MESSAGE_BYTES};
use jmap_base_client::{WsFrame, WsSession};
use jmap_graph_client::{GraphEditWsExt, GraphEditWsFrame};
use jmap_graph_types::edit::GraphEditStreamEnable;
use jmap_types::Id;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

/// L447-L452 "Subscribing to a node".
const ENABLE: &str = r#"{
  "@type": "GraphEditStreamEnable",
  "nodeIds": ["N1"],
  "dataTypes": ["ops", "cursors"],
  "id": "sub1"
}"#;

/// L311: "whose only property is `@type`".
const DISABLE: &str = r#"{ "@type": "GraphEditStreamDisable" }"#;

/// L315-L324 GraphNodeOpEvent carrying the L392-L405 op.
const OP_EVENT: &str = r#"{
  "@type": "GraphNodeOpEvent",
  "accountId": "A1",
  "op": {
    "id": "P42",
    "graphId": "G1",
    "nodeId": "N1",
    "seq": 42,
    "baseSeq": 41,
    "data": "AAECAwQF",
    "created": "2026-10-20T17:00:00Z",
    "createdBy": "U7"
  }
}"#;

/// L351-L366 GraphCursorEvent.
const CURSOR_EVENT: &str = r#"{
  "@type": "GraphCursorEvent",
  "accountId": "A1",
  "nodeId": "N1",
  "senderId": "U7",
  "data": "AQI"
}"#;

/// L287: "Standard `StateChange` notifications for GraphNodeOp continue as
/// usual" (RFC 8887 §4.3.5 shape).
const STATE_CHANGE: &str = r#"{
  "@type": "StateChange",
  "changed": { "A1": { "GraphNodeOp": "s42" } }
}"#;

/// A GraphNodeOpEvent missing its required `op` (L323).
const MALFORMED_OP_EVENT: &str = r#"{
  "@type": "GraphNodeOpEvent",
  "accountId": "A1"
}"#;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

enum Step {
    Send(&'static str),
    Recv,
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

async fn next(ws: &mut WsSession) -> GraphEditWsFrame {
    ws.next_graph_edit_frame()
        .await
        .expect("connection must stay open")
        .expect("frame must be received")
}

#[tokio::test]
async fn enable_receive_and_disable() {
    let (mut ws, server) = serve(vec![
        Step::Recv,
        Step::Send(OP_EVENT),
        Step::Send(STATE_CHANGE),
        Step::Send(CURSOR_EVENT),
        Step::Send(MALFORMED_OP_EVENT),
        Step::Recv,
    ])
    .await;

    let mut enable = GraphEditStreamEnable::new(
        vec![Id::from("N1")],
        vec!["ops".to_owned(), "cursors".to_owned()],
    );
    enable.id = Some("sub1".to_owned());
    ws.send_graph_edit_stream_enable(&enable).await.unwrap();

    match next(&mut ws).await {
        GraphEditWsFrame::Op(e) => {
            assert_eq!(e.account_id, Id::from("A1"));
            assert_eq!(e.op.seq, 42);
            assert_eq!(e.op.node_id, Id::from("N1"));
            assert_eq!(e.op.data, "AAECAwQF");
        }
        other => panic!("expected an op event, got {other:?}"),
    }
    assert!(
        matches!(
            next(&mut ws).await,
            GraphEditWsFrame::Other(WsFrame::StateChange(_))
        ),
        "StateChange must pass through as Other"
    );
    match next(&mut ws).await {
        GraphEditWsFrame::Cursor(e) => {
            assert_eq!(e.node_id, Id::from("N1"));
            assert_eq!(e.sender_id, Some(Id::from("U7")));
            assert_eq!(e.data, "AQI");
        }
        other => panic!("expected a cursor event, got {other:?}"),
    }
    match next(&mut ws).await {
        GraphEditWsFrame::Other(WsFrame::Unknown { type_name, .. }) => {
            assert_eq!(type_name, "GraphNodeOpEvent");
        }
        other => panic!("a malformed event must pass through as Other, got {other:?}"),
    }

    ws.send_graph_edit_stream_disable().await.unwrap();

    let sent = server.await.unwrap();
    assert_eq!(sent, vec![json(ENABLE), json(DISABLE)]);
}
