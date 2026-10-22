//! Integration tests for `JmapClient::subscribe_events_reconnecting`.

use std::time::Duration;

use futures::StreamExt as _;
use jmap_base_client::ClientError;
use jmap_base_client::SseEvent;
use jmap_base_client::SseReconnectPolicy;
use jmap_base_client::{ClientConfig, JmapClient};
use jmap_base_client::{DefaultTransport, NoneAuth};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> JmapClient {
    JmapClient::new(
        DefaultTransport,
        NoneAuth,
        &server.uri(),
        ClientConfig::default(),
    )
    .expect("client construction must succeed")
}

fn fast_policy() -> SseReconnectPolicy {
    let mut p = SseReconnectPolicy::default();
    p.initial_delay = Duration::from_millis(1);
    p.max_delay = Duration::from_millis(4);
    p
}

fn sse(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_owned(), "text/event-stream")
}

/// The `Email` state of account `a1` carried by a StateChange frame.
fn email_state(event: &SseEvent) -> String {
    match event {
        SseEvent::StateChange(sc) => sc.changed["a1"]["Email"].to_string(),
        other => panic!("expected StateChange, got {other:?}"),
    }
}

async fn request_count(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .expect("request recording is on")
        .len()
}

/// Oracle: RFC 8620 §7.3 — "When a new connection is made to the
/// event-source endpoint, a client following the server-sent events
/// specification will send a Last-Event-ID HTTP header field with the last
/// id it saw". The first connection delivers event id `e1` and closes; the
/// reconnect must carry `Last-Event-ID: e1` and continue the same stream.
#[tokio::test]
async fn reconnects_after_close_with_last_event_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .and(header("Last-Event-ID", "e1"))
        .respond_with(sse(
            "id: e2\nevent: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"a1\":{\"Email\":\"s2\"}}}\n\n",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(sse(
            "id: e1\nevent: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"a1\":{\"Email\":\"s1\"}}}\n\n",
        ))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let url = format!("{}/events", server.uri());
    let mut stream = client(&server).subscribe_events_reconnecting(&url, None, fast_policy());

    let first = stream.next().await.expect("first item").expect("first ok");
    assert_eq!(first.id.as_deref(), Some("e1"));
    assert_eq!(email_state(&first.event), "s1");
    let second = stream
        .next()
        .await
        .expect("stream must survive the close")
        .expect("second ok");
    assert_eq!(second.id.as_deref(), Some("e2"));
    assert_eq!(email_state(&second.event), "s2");
}

/// Oracle: RFC 8620 §7.3 (Last-Event-ID, as above) — a caller resuming a
/// stream it consumed earlier passes the saved id, which must go out on the
/// very first connection.
#[tokio::test]
async fn initial_last_event_id_is_sent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .and(header("Last-Event-ID", "saved-7"))
        .respond_with(sse(
            "event: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"a1\":{\"Email\":\"s8\"}}}\n\n",
        ))
        .mount(&server)
        .await;

    let url = format!("{}/events", server.uri());
    let mut stream =
        client(&server).subscribe_events_reconnecting(&url, Some("saved-7"), fast_policy());
    let frame = stream.next().await.expect("item").expect("ok");
    assert_eq!(email_state(&frame.event), "s8");
}

/// Oracle: RFC 9110 §15.6.4 — 503 Service Unavailable means "the server is
/// currently unable to handle the request due to a temporary overload or
/// scheduled maintenance, which will likely be alleviated after some
/// delay". Two 503s must be retried, and the third attempt's frame
/// delivered.
#[tokio::test]
async fn retries_service_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(sse(
            "event: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"a1\":{\"Email\":\"s3\"}}}\n\n",
        ))
        .mount(&server)
        .await;

    let url = format!("{}/events", server.uri());
    let mut stream = client(&server).subscribe_events_reconnecting(&url, None, fast_policy());
    let frame = stream
        .next()
        .await
        .expect("item")
        .expect("ok after retries");
    assert_eq!(email_state(&frame.event), "s3");
    assert_eq!(request_count(&server).await, 3);
}

/// Oracle: RFC 9110 §15.5.2 — 401 means the request "lacks valid
/// authentication credentials"; resending the same credentials cannot fix
/// it. The stream must yield `AuthFailed(401)` once, end, and not retry.
#[tokio::test]
async fn auth_failure_is_fatal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let url = format!("{}/events", server.uri());
    let mut stream = client(&server).subscribe_events_reconnecting(&url, None, fast_policy());
    assert!(matches!(
        stream.next().await,
        Some(Err(ClientError::AuthFailed(401)))
    ));
    assert!(
        stream.next().await.is_none(),
        "stream must end after a fatal error"
    );
    assert_eq!(request_count(&server).await, 1);
}

/// Oracle: `SseReconnectPolicy::max_retries` docs — "Consecutive failed
/// attempts to retry before giving up". With `Some(2)` against a server
/// that always answers 503, there must be exactly 1 + 2 attempts, then the
/// last error, then the end of the stream.
#[tokio::test]
async fn gives_up_after_max_retries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let mut policy = fast_policy();
    policy.max_retries = Some(2);
    let url = format!("{}/events", server.uri());
    let mut stream = client(&server).subscribe_events_reconnecting(&url, None, policy);
    match stream.next().await {
        Some(Err(ClientError::Http(e))) => assert_eq!(e.status(), Some(503)),
        other => panic!("expected the final 503, got {other:?}"),
    }
    assert!(stream.next().await.is_none());
    assert_eq!(request_count(&server).await, 3);
}

/// Oracle: `SseReconnectPolicy::max_retries` docs — a connection that ends
/// without delivering any frame counts as a failed attempt (otherwise a
/// server that accepts and immediately closes would be hammered forever).
/// With `Some(1)`: 2 empty connections, then a clean end with no error.
#[tokio::test]
async fn empty_connections_count_as_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(sse(""))
        .mount(&server)
        .await;

    let mut policy = fast_policy();
    policy.max_retries = Some(1);
    let url = format!("{}/events", server.uri());
    let mut stream = client(&server).subscribe_events_reconnecting(&url, None, policy);
    let end = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("empty connections must exhaust max_retries, not loop forever");
    assert!(end.is_none());
    assert_eq!(request_count(&server).await, 2);
}

/// Oracle: RFC 9110 §5.5 — a field value is `*field-content` built from
/// `field-vchar = VCHAR / obs-text` (plus SP / HTAB), so a `Last-Event-ID`
/// containing U+0001 can never be sent. The SSE id `a\x01b` the server
/// hands out makes every reconnect fail the same way, so after delivering
/// the frame the stream must yield one error and end, not back off forever
/// (the default policy has no retry cap).
#[tokio::test]
async fn unsendable_server_event_id_is_fatal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(sse(
            "id: a\u{1}b\nevent: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"a1\":{\"Email\":\"s1\"}}}\n\n",
        ))
        .mount(&server)
        .await;

    let url = format!("{}/events", server.uri());
    let mut stream = client(&server).subscribe_events_reconnecting(&url, None, fast_policy());
    let frame = stream.next().await.expect("item").expect("ok");
    assert_eq!(email_state(&frame.event), "s1");
    let rest = tokio::time::timeout(Duration::from_secs(5), async {
        (stream.next().await, stream.next().await)
    })
    .await
    .expect("an unsendable Last-Event-ID must end the stream, not loop forever");
    assert!(
        matches!(rest.0, Some(Err(ClientError::Http(_)))),
        "expected the header error, got {:?}",
        rest.0
    );
    assert!(rest.1.is_none(), "stream must end after a fatal error");
    assert_eq!(request_count(&server).await, 1);
}

/// Oracle: RFC 9110 §5.5 (as above) — a caller-supplied initial
/// `Last-Event-ID` with a control character is invalid input; the first
/// item must be the error, the stream must then end, and nothing may reach
/// the server.
#[tokio::test]
async fn unsendable_initial_last_event_id_is_fatal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(sse(""))
        .mount(&server)
        .await;

    let url = format!("{}/events", server.uri());
    let mut stream =
        client(&server).subscribe_events_reconnecting(&url, Some("a\u{1}b"), fast_policy());
    let rest = tokio::time::timeout(Duration::from_secs(5), async {
        (stream.next().await, stream.next().await)
    })
    .await
    .expect("an unsendable Last-Event-ID must end the stream, not loop forever");
    assert!(
        matches!(rest.0, Some(Err(ClientError::Http(_)))),
        "expected the header error, got {:?}",
        rest.0
    );
    assert!(rest.1.is_none(), "stream must end after a fatal error");
    assert_eq!(request_count(&server).await, 0);
}
