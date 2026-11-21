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
    start_with(config, hub, |c| c)
}

/// [`start`], with `extend` applied to the connection (to register
/// extension handlers).
fn start_with(
    config: ConnectionConfig,
    hub: Option<PushHub>,
    extend: impl FnOnce(Connection<TestHandler>) -> Connection<TestHandler>,
) -> Harness {
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
    let conn = extend(Connection::new(Arc::clone(&handler)).with_config(config));
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

// ---------------------------------------------------------------------------
// Extension message types
// ---------------------------------------------------------------------------
//
// Oracle: the rule the JMAP Chat, VTC and Scene WebSocket drafts add to
// RFC 8887 (draft-atwood-jmap-chat-wss-00 §Update to RFC 8887, §Handling
// Unknown Message Types; the Stream Enable "success is silent and failure
// produces a `RequestError`" text; the Scene draft's `regionIds` member on
// a `forbidden` RequestError). The toy types below stand in for them.

/// What the toy extension saw, shared with the test.
#[derive(Default)]
struct ToyLog {
    seen: std::sync::Mutex<Vec<String>>,
    enabled: std::sync::atomic::AtomicBool,
    sender: std::sync::Mutex<Option<ExtensionSender>>,
    gate: Notify,
}

impl ToyLog {
    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn note(&self, entry: impl Into<String>) {
        self.seen.lock().unwrap().push(entry.into());
    }

    fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// A server-initiated event: sent only while the stream is enabled,
    /// as the drafts require.
    fn publish(&self, n: u64) {
        if self.enabled()
            && let Some(sender) = &*self.sender.lock().unwrap()
        {
            sender
                .try_send(&json!({"@type": "ToyEvent", "n": n}))
                .unwrap();
        }
    }
}

struct Toy {
    log: Arc<ToyLog>,
    events: Option<ExtensionSender>,
}

#[derive(serde::Deserialize)]
struct ToyEnable {
    topics: Vec<String>,
}

impl ExtensionHandler for Toy {
    fn message_types(&self) -> &[&str] {
        &["ToyStreamEnable", "ToyStreamDisable", "ToySlow", "ToyBurst"]
    }

    fn start(&mut self, events: ExtensionSender) {
        *self.log.sender.lock().unwrap() = Some(events.clone());
        self.events = Some(events);
    }

    async fn handle(&mut self, message: ExtensionMessage) -> Result<(), JmapError> {
        self.log.note(message.message_type.clone());
        let enabled = &self.log.enabled;
        match message.message_type.as_str() {
            "ToyStreamEnable" => {
                let enable: ToyEnable = message.deserialize()?;
                if enable.topics.is_empty() {
                    let mut err = JmapError::invalid_arguments("no topics");
                    err.extra.insert("regionIds".to_owned(), json!(["r1"]));
                    err.extra.insert("type".to_owned(), json!("spoofed"));
                    return Err(err);
                }
                enabled.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            "ToyStreamDisable" => enabled.store(false, std::sync::atomic::Ordering::SeqCst),
            "ToySlow" => self.log.gate.notified().await,
            "ToyBurst" => {
                let events = self.events.as_ref().unwrap();
                let results: Vec<bool> = (0..3)
                    .map(|n| {
                        events
                            .try_send(&json!({"@type": "ToyEvent", "n": n}))
                            .is_ok()
                    })
                    .collect();
                self.log.note(format!("{results:?}"));
            }
            other => unreachable!("{other}"),
        }
        Ok(())
    }

    async fn close(&mut self) {
        self.log.note("close");
    }
}

fn start_toy(config: ConnectionConfig) -> (Harness, Arc<ToyLog>) {
    let log = Arc::new(ToyLog::default());
    let toy = Toy {
        log: Arc::clone(&log),
        events: None,
    };
    (start_with(config, None, |c| c.with_extension(toy)), log)
}

/// Sends a request and waits for its Response, so everything the client
/// sent before has been read.
async fn sync(h: &mut Harness, id: &str) {
    h.send(req(id, "Core/echo"));
    assert_eq!(h.recv().await["requestId"], id);
}

/// Waits until the toy has seen `n` messages.
async fn seen_at_least(log: &ToyLog, n: usize) {
    while log.seen().len() < n {
        tokio::task::yield_now().await;
    }
}

/// A registered `@type` parses to an [`ExtensionMessage`] without `@type`
/// and with its `id`; RFC 8887 types still parse as before.
#[test]
fn extension_types_parse_to_extension_messages() {
    let text = r#"{"@type":"ToyStreamEnable","id":"e1","topics":["a"]}"#;
    let Parsed::Extension(m) = parse_message(text, &cfg(), |t| t == "ToyStreamEnable").unwrap()
    else {
        panic!("not an extension message")
    };
    assert_eq!(m.message_type, "ToyStreamEnable");
    assert_eq!(m.id.as_deref(), Some("e1"));
    assert_eq!(Value::Object(m.body), json!({"id": "e1", "topics": ["a"]}));
    let text = r#"{"@type":"WebSocketPushDisable"}"#;
    assert!(matches!(
        parse_message(text, &cfg(), |_| true).unwrap(),
        Parsed::Core(ClientMessage::PushDisable(_))
    ));
}

/// The extension control flow: enable is silent, events flow only while
/// enabled, disable stops them, and the connection's Close runs `close`
/// and closes the queue.
#[tokio::test]
async fn extension_enable_events_disable_close() {
    let (mut h, log) = start_toy(cfg());
    h.send(json!({"@type": "ToyStreamEnable", "topics": ["a"]}));
    sync(&mut h, "s1").await;
    log.publish(1);
    assert_eq!(h.recv().await, json!({"@type": "ToyEvent", "n": 1}));

    h.send(json!({"@type": "ToyStreamDisable"}));
    sync(&mut h, "s2").await;
    log.publish(2);
    sync(&mut h, "s3").await;
    assert_eq!(log.seen(), ["ToyStreamEnable", "ToyStreamDisable"]);

    h.to_server.send(Ok(Inbound::Close)).unwrap();
    h.task.await.unwrap().unwrap();
    assert_eq!(log.seen().last().map(String::as_str), Some("close"));
    let sender = log.sender.lock().unwrap().clone().unwrap();
    assert!(sender.is_closed());
    assert!(matches!(
        sender.try_send(&json!({"@type": "ToyEvent"})),
        Err(ExtensionSendError::Closed)
    ));
}

/// "A client-to-server message with an unrecognized `@type` value is a
/// request-level error ... `notRequest`", also with extensions
/// registered; the connection stays open.
#[tokio::test]
async fn unregistered_type_is_still_not_request() {
    let (mut h, _log) = start_toy(cfg());
    h.send(json!({"@type": "ChatStreamEnable", "id": "c1", "dataTypes": ["typing"]}));
    let e = h.recv().await;
    assert_eq!(e["@type"], "RequestError");
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:notRequest");
    assert_eq!(e["requestId"], "c1");
    sync(&mut h, "after").await;
}

/// "failure produces a `RequestError`" carrying the message's `id`; the
/// error's extra members (the Scene draft's `regionIds`) are kept, except
/// ones the driver writes itself; `prepare_problem` runs.
#[tokio::test]
async fn extension_error_is_a_request_error() {
    let (mut h, log) = start_toy(cfg());
    h.send(json!({"@type": "ToyStreamEnable", "id": "e1", "topics": []}));
    assert_eq!(
        h.recv().await,
        json!({
            "@type": "RequestError",
            "requestId": "e1",
            "type": "urn:ietf:params:jmap:error:invalidArguments",
            "status": 400,
            "title": "T",
            "detail": "no topics",
            "regionIds": ["r1"]
        })
    );
    assert!(!log.enabled());

    // A malformed body, through ExtensionMessage::deserialize.
    h.send(json!({"@type": "ToyStreamEnable", "id": "e2", "topics": "a"}));
    let e = h.recv().await;
    assert_eq!(e["type"], "urn:ietf:params:jmap:error:notRequest");
    assert_eq!(e["requestId"], "e2");
}

/// While an extension message is handled, later client messages wait,
/// but Responses of earlier Requests are still sent.
#[tokio::test]
async fn extension_messages_hold_back_later_messages_only() {
    let (mut h, log) = start_toy(cfg());
    h.send(req("before", "wait"));
    h.send(json!({"@type": "ToySlow"}));
    h.send(req("after", "Core/echo"));
    seen_at_least(&log, 1).await;
    h.handler.gate.notify_one();
    assert_eq!(h.recv().await["requestId"], "before");
    let early =
        tokio::time::timeout(std::time::Duration::from_millis(100), h.from_server.recv()).await;
    assert!(early.is_err(), "nothing until ToySlow returns: {early:?}");
    log.gate.notify_one();
    assert_eq!(h.recv().await["requestId"], "after");
}

/// The queue is bounded: `try_send` past the capacity fails with `Full`.
#[tokio::test]
async fn extension_queue_is_bounded() {
    let (mut h, log) = start_toy(cfg().extension_queue_capacity(1));
    h.send(json!({"@type": "ToyBurst"}));
    assert_eq!(h.recv().await, json!({"@type": "ToyEvent", "n": 0}));
    sync(&mut h, "s").await;
    assert_eq!(log.seen(), ["ToyBurst", "[true, false, false]"]);
}

/// `close` runs when `run` ends with an error too, after the running
/// `handle` finished.
#[tokio::test]
async fn close_runs_after_errors_and_running_handles() {
    let (h, log) = start_toy(cfg());
    h.send(json!({"@type": "ToySlow"}));
    h.send(req("r", "Core/echo"));
    seen_at_least(&log, 1).await;
    // The client stops reading: the Response to `r` cannot be written.
    let Harness {
        to_server,
        from_server,
        task,
        ..
    } = h;
    drop(from_server);
    log.gate.notify_one();
    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(ConnectionError::Send(_))),
        "{result:?}"
    );
    assert_eq!(log.seen(), ["ToySlow", "close"]);
    drop(to_server);
}

/// An extension message must be an object with a string `@type`, and not
/// pose as an RFC 8887 server message.
#[test]
fn extension_sender_checks_the_message() {
    let (tx, mut rx) = mpsc::channel(4);
    let sender = ExtensionSender::new(tx);
    for bad in [
        json!({"@type": "Response"}),
        json!({"@type": "StateChange"}),
        json!({"@type": "RequestError"}),
        json!({"@type": 1}),
        json!({"n": 1}),
        json!([1]),
    ] {
        assert!(
            matches!(
                sender.try_send(&bad),
                Err(ExtensionSendError::NotAnExtensionMessage)
            ),
            "{bad}"
        );
    }
    sender.try_send(&json!({"@type": "ToyEvent"})).unwrap();
    assert_eq!(rx.try_recv().unwrap(), r#"{"@type":"ToyEvent"}"#);
}

fn test_connection() -> Connection<TestHandler> {
    Connection::new(Arc::new(TestHandler {
        gate: Notify::new(),
    }))
}

#[test]
#[should_panic(expected = "is an RFC 8887 type")]
fn registering_a_core_type_panics() {
    struct Bad;
    impl ExtensionHandler for Bad {
        fn message_types(&self) -> &[&str] {
            &["WebSocketPushEnable"]
        }
        async fn handle(&mut self, _: ExtensionMessage) -> Result<(), JmapError> {
            Ok(())
        }
    }
    let _ = test_connection().with_extension(Bad);
}

#[test]
#[should_panic(expected = "registered twice")]
fn registering_a_type_twice_panics() {
    let toy = || Toy {
        log: Arc::default(),
        events: None,
    };
    let _ = test_connection()
        .with_extension(toy())
        .with_extension(toy());
}

// ---------------------------------------------------------------------------
// Closing from the deployment (RFC 8887 §4.2)
// ---------------------------------------------------------------------------

/// [`start_toy`], also returning the connection's [`CloseHandle`].
fn start_closable() -> (Harness, Arc<ToyLog>, CloseHandle) {
    let log = Arc::new(ToyLog::default());
    let toy = Toy {
        log: Arc::clone(&log),
        events: None,
    };
    let mut handle = None;
    let h = start_with(cfg(), None, |c| {
        handle = Some(c.close_handle());
        c.with_extension(toy)
    });
    (h, log, handle.unwrap())
}

async fn next_outbound(h: &mut Harness) -> Outbound {
    tokio::time::timeout(std::time::Duration::from_secs(5), h.from_server.recv())
        .await
        .expect("a message")
        .expect("open")
}

/// §4.2: on expired credentials the server "MAY send a Close frame with a
/// status code of 1008 (Policy Violation)". The Close carries the code and
/// reason given, `run` returns `Ok`, the extension's `close` ran, and
/// nothing queued is sent after the Close: a Request still running and an
/// extension event queued after the request are dropped.
#[tokio::test]
async fn close_handle_sends_close_and_shuts_down() {
    let (mut h, log, handle) = start_closable();
    h.send(json!({"@type": "ToyStreamEnable", "topics": ["t"]}));
    sync(&mut h, "s1").await;
    h.send(req("slow", "wait"));
    sync(&mut h, "s2").await;

    assert!(handle.close(CloseCode::POLICY_VIOLATION, "credentials expired"));
    log.publish(7);
    h.handler.gate.notify_one();

    assert_eq!(
        next_outbound(&mut h).await,
        Outbound::Close {
            code: 1008,
            reason: "credentials expired".to_owned()
        }
    );
    let Harness {
        mut from_server,
        task,
        to_server,
        ..
    } = h;
    task.await.unwrap().unwrap();
    assert_eq!(log.seen(), ["ToyStreamEnable", "close"]);
    // `run` returned and dropped the sink: nothing followed the Close.
    assert_eq!(from_server.recv().await, None);
    drop(to_server);
}

/// The first request wins; a later one, through any clone, is ignored.
#[tokio::test]
async fn second_close_is_ignored() {
    let (mut h, _log, handle) = start_closable();
    assert!(handle.close(CloseCode::POLICY_VIOLATION, "first"));
    assert!(!handle.clone().close(CloseCode::GOING_AWAY, "second"));
    assert_eq!(
        next_outbound(&mut h).await,
        Outbound::Close {
            code: 1008,
            reason: "first".to_owned()
        }
    );
    h.task.await.unwrap().unwrap();
}

/// After the client's Close ended `run`, `close` does nothing and says so.
#[tokio::test]
async fn close_after_the_client_closed_does_nothing() {
    let (h, log, handle) = start_closable();
    h.to_server.send(Ok(Inbound::Close)).unwrap();
    let Harness {
        mut from_server,
        task,
        to_server,
        ..
    } = h;
    task.await.unwrap().unwrap();
    assert!(!handle.close(CloseCode::POLICY_VIOLATION, "late"));
    assert_eq!(from_server.recv().await, None, "no Close was sent");
    assert_eq!(log.seen(), ["close"]);
    drop(to_server);
}

/// RFC 6455 §5.5: a control frame payload is at most 125 bytes, so the
/// reason is cut to 123 bytes, here inside a 2-byte character.
#[tokio::test]
async fn close_reason_is_cut_to_fit_a_control_frame() {
    let (mut h, _log, handle) = start_closable();
    let reason = format!("{}é", "x".repeat(122));
    assert!(handle.close(CloseCode::new(4001).unwrap(), reason));
    assert_eq!(
        next_outbound(&mut h).await,
        Outbound::Close {
            code: 4001,
            reason: "x".repeat(122)
        }
    );
}
