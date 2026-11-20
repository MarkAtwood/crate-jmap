//! Oracles: RFC 8887 §4.3.1-§4.3.5 and RFC 8620 §3.6.1; expected JSON is
//! written by hand from those sections.

use super::*;
use jmap_types::{Id, StateChange};
use serde_json::json;
use std::collections::BTreeMap;
use std::convert::Infallible;
use tokio::sync::{Notify, mpsc};

fn cfg() -> ConnectionConfig {
    ConnectionConfig::default()
}

fn err_json(text: &str) -> Value {
    serde_json::to_value(parse_client_message(text, &cfg()).unwrap_err()).unwrap()
}

/// §4.3.1 / RFC 8620 §3.6.1: a message that is not JSON is `notJSON`
/// (400), and with no id to echo `requestId` is `null` as in the §4.4
/// example, which this reproduces exactly.
#[test]
fn not_json_matches_the_rfc_example() {
    assert_eq!(
        err_json("The quick brown fox jumps\n over the lazy dog."),
        json!({
            "@type": "RequestError",
            "requestId": null,
            "type": "urn:ietf:params:jmap:error:notJSON",
            "status": 400,
            "detail": "The request did not parse as I-JSON."
        })
    );
}

/// RFC 8620 §3.6.1 `notRequest`: JSON that is not a valid Request. The
/// `id` is echoed as `requestId` when it is a string (§4.3.4).
#[test]
fn not_request_cases_echo_a_string_id() {
    for (text, id) in [
        (r#"[1,2]"#, Value::Null),
        (r#"{"id":"x1"}"#, json!("x1")),
        (r#"{"@type":"Bogus","id":"x2"}"#, json!("x2")),
        (r#"{"@type":"Response","id":"x3"}"#, json!("x3")),
        (r#"{"@type":"Request","id":"x4","using":"no"}"#, json!("x4")),
        (
            r#"{"@type":"Request","id":5,"using":[],"methodCalls":[]}"#,
            Value::Null,
        ),
        (
            r#"{"@type":"WebSocketPushEnable","id":"x6","dataTypes":{}}"#,
            json!("x6"),
        ),
    ] {
        let v = err_json(text);
        assert_eq!(v["@type"], "RequestError", "{text}");
        assert_eq!(v["type"], "urn:ietf:params:jmap:error:notRequest", "{text}");
        assert_eq!(v["status"], 400, "{text}");
        assert_eq!(v["requestId"], id, "{text}");
    }
}

/// RFC 8620 §3.6.1: exceeding `maxCallsInRequest` is a `limit` error with
/// a `limit` property naming it.
#[test]
fn too_many_calls_is_a_limit_error() {
    let config = cfg().max_calls_in_request(1);
    let text =
        r#"{"@type":"Request","id":"R","using":[],"methodCalls":[["a",{},"0"],["b",{},"1"]]}"#;
    let v = serde_json::to_value(parse_client_message(text, &config).unwrap_err()).unwrap();
    assert_eq!(v["type"], "urn:ietf:params:jmap:error:limit");
    assert_eq!(v["limit"], "maxCallsInRequest");
    assert_eq!(v["requestId"], "R");
}

/// RFC 8620 §3.6.1.1: a too-large request is a `limit` error naming
/// `maxSizeRequest`; it is not parsed, so no id is known.
#[test]
fn too_large_is_a_limit_error() {
    let config = cfg().max_size_request(Some(10));
    let text = r#"{"@type":"Request","id":"R","using":[],"methodCalls":[]}"#;
    let v = serde_json::to_value(parse_client_message(text, &config).unwrap_err()).unwrap();
    assert_eq!(v["limit"], "maxSizeRequest");
    assert_eq!(v["requestId"], Value::Null);
}

/// §4.3.2: `@type` and `id` are WebSocket members, not part of the
/// RFC 8620 Request, so they are not left in `extra`.
#[test]
fn request_parses_without_ws_members_in_extra() {
    let text = r#"{"@type":"Request","id":"R1","using":["urn:ietf:params:jmap:core"],"methodCalls":[["Core/echo",{"a":1},"c"]]}"#;
    let ClientMessage::Request(r) = parse_client_message(text, &cfg()).unwrap() else {
        panic!("not a Request")
    };
    assert_eq!(r.id.as_deref(), Some("R1"));
    assert!(r.request.extra.is_empty(), "{:?}", r.request.extra);
    assert_eq!(r.request.method_calls[0].name, "Core/echo");
}

fn change(account: &str, types: &[(&str, &str)]) -> StateChange {
    let mut by_type = BTreeMap::new();
    for (t, s) in types {
        by_type.insert((*t).to_owned(), State::from(*s));
    }
    let mut changed = BTreeMap::new();
    changed.insert(Id::from(account), by_type);
    StateChange::new(changed)
}

/// §4.3.5.2: only listed types are sent; "Other types are omitted"; and a
/// notification is "only ... sent if the data for one of these types
/// changes".
#[test]
fn filter_keeps_only_listed_types() {
    let c = WsStateChange::new(change("a", &[("Email", "e1"), ("Mailbox", "m1")]), None);
    let only_mailbox = filter_state_change(c.clone(), Some(&["Mailbox".to_owned()])).unwrap();
    assert_eq!(
        serde_json::to_value(&only_mailbox).unwrap(),
        json!({"@type": "StateChange", "changed": {"a": {"Mailbox": "m1"}}})
    );
    assert!(filter_state_change(c.clone(), Some(&["Thread".to_owned()])).is_none());
    assert!(filter_state_change(c.clone(), Some(&[])).is_none());
    assert_eq!(filter_state_change(c.clone(), None), Some(c));
}

// ---------------------------------------------------------------------------
// PushHub
// ---------------------------------------------------------------------------

async fn next(stream: &mut BoxStream<'static, WsStateChange>) -> WsStateChange {
    tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("a notification")
        .expect("stream open")
}

fn types_of(c: &WsStateChange) -> Value {
    serde_json::to_value(&c.state_change.changed).unwrap()
}

/// §4.3.5.2: with no `pushState`, only later changes are sent.
#[tokio::test]
async fn hub_sends_later_changes() {
    let mut hub = PushHub::new(8);
    hub.publish(change("a", &[("Email", "e1")]));
    let mut s = hub.subscribe(&WebSocketPushEnable::new(None, None));
    hub.publish(change("a", &[("Mailbox", "m1")]));
    let c = next(&mut s).await;
    assert_eq!(types_of(&c), json!({"a": {"Mailbox": "m1"}}));
    assert_eq!(c.push_state, Some(hub.push_state()));
}

/// §4.3.5.2: "Upon receipt of a "pushState" token, the server SHOULD
/// immediately send all changes since that state token."
#[tokio::test]
async fn hub_replays_since_its_token() {
    let mut hub = PushHub::new(8);
    hub.publish(change("a", &[("Email", "e1")]));
    let token = hub.push_state();
    hub.publish(change("a", &[("Mailbox", "m1")]));
    hub.publish(change("a", &[("Email", "e2")]));
    let mut s = hub.subscribe(&WebSocketPushEnable::new(None, Some(token)));
    let c = next(&mut s).await;
    assert_eq!(types_of(&c), json!({"a": {"Email": "e2", "Mailbox": "m1"}}));
}

/// A token older than the kept history replays the latest state of every
/// type (a superset of the changes since it).
#[tokio::test]
async fn hub_replays_latest_after_eviction() {
    let mut hub = PushHub::new(1);
    hub.publish(change("a", &[("Email", "e1")]));
    let token = hub.push_state();
    hub.publish(change("a", &[("Mailbox", "m1")]));
    hub.publish(change("b", &[("Email", "x1")]));
    let mut s = hub.subscribe(&WebSocketPushEnable::new(None, Some(token)));
    let c = next(&mut s).await;
    assert_eq!(
        types_of(&c),
        json!({"a": {"Email": "e1", "Mailbox": "m1"}, "b": {"Email": "x1"}})
    );
}

/// A token another hub issued replays nothing.
#[tokio::test]
async fn hub_ignores_foreign_tokens() {
    let other = PushHub::new(8);
    other.publish(change("a", &[("Email", "e0")]));
    let mut hub = PushHub::new(8);
    hub.publish(change("a", &[("Email", "e1")]));
    for token in [other.push_state(), "junk".to_owned()] {
        let mut s = hub.subscribe(&WebSocketPushEnable::new(None, Some(token)));
        let none = tokio::time::timeout(std::time::Duration::from_millis(50), s.next()).await;
        assert!(none.is_err(), "nothing may be replayed");
    }
}

// ---------------------------------------------------------------------------
// The driver, over in-memory channels
// ---------------------------------------------------------------------------

/// Echoes each request's `using` back as the session state; blocks
/// requests whose first call is `wait` until released.
struct TestHandler {
    gate: Notify,
}

impl RequestHandler for TestHandler {
    async fn handle(&self, request: JmapRequest) -> Result<JmapResponse, JmapError> {
        if request
            .method_calls
            .first()
            .is_some_and(|c| c.name == "wait")
        {
            self.gate.notified().await;
        }
        if request
            .method_calls
            .first()
            .is_some_and(|c| c.name == "fail")
        {
            return Err(JmapError::unknown_capability_with_detail("urn:x"));
        }
        Ok(JmapResponse::new(
            request.method_calls,
            State::from("s"),
            None,
        ))
    }

    fn prepare_problem(&self, problem: &mut ProblemDetails) {
        problem.title = Some("T".to_owned());
    }
}

struct Harness {
    to_server: mpsc::UnboundedSender<Result<Inbound, Infallible>>,
    from_server: mpsc::UnboundedReceiver<Outbound>,
    handler: Arc<TestHandler>,
    task: tokio::task::JoinHandle<Result<(), ConnectionError>>,
}

fn start(config: ConnectionConfig, hub: Option<PushHub>) -> Harness {
    let handler = Arc::new(TestHandler {
        gate: Notify::new(),
    });
    let (to_server, rx) = mpsc::unbounded_channel();
    let (tx, from_server) = mpsc::unbounded_channel::<Outbound>();
    let incoming =
        futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|m| (m, rx)) })
            .boxed();
    let outgoing = futures::sink::unfold(tx, |tx, m: Outbound| async move {
        tx.send(m)
            .map(|()| tx)
            .map_err(|_| std::io::Error::other("closed"))
    });
    let conn = Connection::new(Arc::clone(&handler)).with_config(config);
    let task = match hub {
        Some(hub) => tokio::spawn(conn.with_push(hub).run(incoming, Box::pin(outgoing))),
        None => tokio::spawn(conn.run(incoming, Box::pin(outgoing))),
    };
    Harness {
        to_server,
        from_server,
        handler,
        task,
    }
}

impl Harness {
    fn send(&self, v: Value) {
        self.to_server
            .send(Ok(Inbound::Text(v.to_string())))
            .unwrap();
    }

    async fn recv(&mut self) -> Value {
        let m = tokio::time::timeout(std::time::Duration::from_secs(5), self.from_server.recv())
            .await
            .expect("a message")
            .expect("open");
        let Outbound::Text(t) = m else {
            panic!("{m:?}")
        };
        serde_json::from_str(&t).unwrap()
    }
}

fn req(id: &str, method: &str) -> Value {
    json!({"@type": "Request", "id": id, "using": [], "methodCalls": [[method, {}, "c"]]})
}

/// §4.3.3 `requestId` echo; §4.3.2 out-of-order processing: a request
/// blocked in the handler does not hold back a later one.
#[tokio::test]
async fn responses_echo_ids_and_may_overtake() {
    let mut h = start(cfg(), None);
    h.send(req("slow", "wait"));
    h.send(req("fast", "Core/echo"));
    let first = h.recv().await;
    assert_eq!(first["@type"], "Response");
    assert_eq!(first["requestId"], "fast");
    h.handler.gate.notify_one();
    assert_eq!(h.recv().await["requestId"], "slow");
}

/// §4.3.2: "the "maxConcurrentRequests" limit ... also applies to
/// requests made on the WebSocket connection" — RFC 8620 §3.6.1 `limit`.
#[tokio::test]
async fn over_concurrency_limit_is_a_limit_error() {
    let mut h = start(cfg().max_concurrent_requests(1), None);
    h.send(req("one", "wait"));
    h.send(req("two", "Core/echo"));
    let e = h.recv().await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["limit"], "maxConcurrentRequests");
    assert_eq!(e["requestId"], "two");
    assert_eq!(e["title"], "T", "prepare_problem runs on driver errors");
    h.handler.gate.notify_one();
    assert_eq!(h.recv().await["requestId"], "one");
}

/// A handler error is a RequestError carrying the request's id.
#[tokio::test]
async fn handler_error_is_a_request_error() {
    let mut h = start(cfg(), None);
    h.send(req("f", "fail"));
    let e = h.recv().await;
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:unknownCapability");
    assert_eq!(e["requestId"], "f");
    assert_eq!(e["title"], "T");
}

/// §4.3.1: a binary frame may close the connection with 1003.
#[tokio::test]
async fn binary_closes_with_1003() {
    let mut h = start(cfg(), None);
    h.to_server.send(Ok(Inbound::Binary)).unwrap();
    let m = h.from_server.recv().await.unwrap();
    assert!(matches!(m, Outbound::Close { code: 1003, .. }), "{m:?}");
    h.task.await.unwrap().unwrap();
}

/// §4.3.1: "the endpoint can either ignore the frame".
#[tokio::test]
async fn binary_can_be_ignored() {
    let mut h = start(cfg().binary_frames(BinaryFrames::Ignore), None);
    h.to_server.send(Ok(Inbound::Binary)).unwrap();
    h.send(req("r", "Core/echo"));
    assert_eq!(h.recv().await["requestId"], "r");
}

/// §4.3.5.2 / §4.3.5.3 over the driver: enable with `dataTypes` gets
/// only those types; disable stops pushes; a Close ends `run` cleanly.
#[tokio::test]
async fn push_enable_filter_and_disable() {
    let hub = PushHub::new(8);
    let mut h = start(cfg(), Some(hub.clone()));
    h.send(json!({"@type": "WebSocketPushEnable", "dataTypes": ["Mailbox"]}));
    // A request round trip orders the enable before the publishes.
    h.send(req("sync", "Core/echo"));
    assert_eq!(h.recv().await["requestId"], "sync");
    hub.publish(change("a", &[("Email", "e1")]));
    hub.publish(change("a", &[("Email", "e2"), ("Mailbox", "m1")]));
    let c = h.recv().await;
    assert_eq!(c["@type"], "StateChange");
    assert_eq!(c["changed"], json!({"a": {"Mailbox": "m1"}}));
    assert_eq!(c["pushState"], hub.push_state());

    h.send(json!({"@type": "WebSocketPushDisable"}));
    h.send(req("sync2", "Core/echo"));
    assert_eq!(h.recv().await["requestId"], "sync2");
    hub.publish(change("a", &[("Mailbox", "m2")]));
    h.send(req("sync3", "Core/echo"));
    assert_eq!(
        h.recv().await["requestId"],
        "sync3",
        "no StateChange after disable"
    );
    h.to_server.send(Ok(Inbound::Close)).unwrap();
    h.task.await.unwrap().unwrap();
}
