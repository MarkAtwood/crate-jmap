// Integration tests for JmapClient core methods.
// Oracle: RFC 8620 §2 (Session), §3.3 (making requests), §6 (blobs), §7 (push)
// Fixtures: tests/fixtures/jmap/*.json (hand-written from RFC 8620 examples)

use jmap_base_client::auth::NoneAuth;
use jmap_base_client::client::JmapClient;
use jmap_base_client::error::ClientError;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn session_fixture() -> serde_json::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jmap/session.json"),
    )
    .expect("cannot read session.json fixture");
    serde_json::from_str(&text).expect("session.json must be valid JSON")
}

fn call_response_fixture() -> serde_json::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jmap/call_response.json"),
    )
    .expect("cannot read call_response.json fixture");
    serde_json::from_str(&text).expect("call_response.json must be valid JSON")
}

fn minimal_request() -> jmap_types::JmapRequest {
    jmap_types::JmapRequest::new(
        vec!["urn:ietf:params:jmap:core".to_owned()],
        vec![(
            "Mailbox/get".to_owned(),
            serde_json::json!({"accountId": "A13824", "ids": null}),
            "r1".to_owned(),
        )],
        None,
    )
}

// ---------------------------------------------------------------------------
// Constructor validation (these do not need a mock server)
// ---------------------------------------------------------------------------

/// Oracle: base_url validation — empty string must be rejected.
#[test]
fn test_new_rejects_empty_url() {
    let result = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        "",
        jmap_base_client::client::ClientConfig::default(),
    )
    .map(|_| ());
    assert!(
        matches!(result, Err(ClientError::InvalidArgument(_))),
        "empty base_url must return InvalidArgument, got {result:?}"
    );
}

/// Oracle: base_url validation — ftp:// scheme must be rejected.
#[test]
fn test_new_rejects_ftp_scheme() {
    let result = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        "ftp://example.com",
        jmap_base_client::client::ClientConfig::default(),
    )
    .map(|_| ());
    assert!(
        matches!(result, Err(ClientError::InvalidArgument(_))),
        "ftp scheme must return InvalidArgument, got {result:?}"
    );
}

/// Oracle: base_url validation — URL with a path component must be rejected.
#[test]
fn test_new_rejects_url_with_path() {
    let result = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        "https://example.com/jmap",
        jmap_base_client::client::ClientConfig::default(),
    )
    .map(|_| ());
    assert!(
        matches!(result, Err(ClientError::InvalidArgument(_))),
        "base_url with path must return InvalidArgument, got {result:?}"
    );
}

/// Oracle: base_url validation — a bare https origin must be accepted.
#[test]
fn test_new_accepts_https_origin() {
    let result = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        "https://example.com",
        jmap_base_client::client::ClientConfig::default(),
    )
    .map(|_| ());
    assert!(result.is_ok(), "valid https origin must be accepted");
}

/// Oracle: config validation — request_timeout == Duration::ZERO must be rejected.
/// Duration::ZERO is version-dependent in reqwest: some versions treat it as "no timeout",
/// others as "instant timeout". Reject explicitly to eliminate this footgun.
#[test]
fn test_new_rejects_zero_request_timeout() {
    let mut config = jmap_base_client::client::ClientConfig::default();
    config.request_timeout = std::time::Duration::ZERO;
    let result = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        "https://example.com",
        config,
    )
    .map(|_| ());
    assert!(
        matches!(result, Err(ClientError::InvalidArgument(_))),
        "request_timeout == Duration::ZERO must return InvalidArgument, got {result:?}"
    );
}

/// Oracle: config validation — max_call_body == 0 must be rejected with InvalidArgument.
#[test]
fn test_new_rejects_zero_max_call_body() {
    let mut config = jmap_base_client::client::ClientConfig::default();
    config.max_call_body = 0;
    let result = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        "https://example.com",
        config,
    )
    .map(|_| ());
    assert!(
        matches!(result, Err(ClientError::InvalidArgument(_))),
        "max_call_body == 0 must return InvalidArgument, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// fetch_session
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §2 — fetch_session returns a correctly parsed Session
/// with the apiUrl, uploadUrl, downloadUrl, eventSourceUrl, state, and username
/// from the hand-written RFC 8620 fixture.
#[tokio::test]
async fn test_fetch_session_returns_session() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(ResponseTemplate::new(200).set_body_json(session_fixture()))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let session = client
        .fetch_session()
        .await
        .expect("fetch_session must succeed");

    // Oracle: RFC 8620 §2.1 — values from fixture, not from code under test
    assert_eq!(session.username, "john@example.com");
    assert_eq!(session.api_url, "https://jmap.example.com/api/");
    assert_eq!(
        session.upload_url,
        "https://jmap.example.com/upload/{accountId}/"
    );
    assert_eq!(
        session.download_url,
        "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}"
    );
    assert_eq!(
        session.event_source_url,
        "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}"
    );
    assert_eq!(session.state, "75128aab4b1b");
    assert!(
        session.accounts.contains_key("A13824"),
        "accounts must contain A13824"
    );
}

/// Oracle: security requirement — fetch_session response body capped at 1 MiB.
/// A response body of 1 MiB + 1 byte must return ClientError::ResponseTooLarge.
#[tokio::test]
async fn test_fetch_session_size_cap() {
    let oversized_body = "x".repeat(1024 * 1024 + 1);
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(ResponseTemplate::new(200).set_body_string(oversized_body))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let err = client
        .fetch_session()
        .await
        .expect_err("oversized response must fail");
    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}

/// Oracle: RFC 8620 §2 — HTTP 401 from session endpoint must surface as
/// ClientError::AuthFailed(401).
#[tokio::test]
async fn test_fetch_session_401_returns_auth_failed() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let err = client.fetch_session().await.expect_err("401 must fail");
    assert!(
        matches!(err, ClientError::AuthFailed(401)),
        "expected AuthFailed(401), got {err:?}"
    );
}

/// Oracle: session URL validation — a Session whose apiUrl has a non-http
/// scheme must return ClientError::InvalidArgument.
#[tokio::test]
async fn test_fetch_session_rejects_non_http_api_url() {
    let server = MockServer::start().await;

    let mut body = session_fixture();
    body["apiUrl"] = serde_json::Value::String("ftp://example.com/api/".to_owned());

    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let err = client
        .fetch_session()
        .await
        .expect_err("ftp apiUrl must fail");
    assert!(
        matches!(err, ClientError::InvalidSession(_)),
        "expected InvalidSession for ftp apiUrl, got {err:?}"
    );
}

/// Oracle: session URL validation — each of uploadUrl, downloadUrl, and
/// eventSourceUrl with a non-http scheme must return ClientError::InvalidArgument.
#[tokio::test]
async fn test_fetch_session_rejects_non_http_other_urls() {
    // A fresh MockServer per iteration ensures each request receives exactly
    // the intended body with one field set to ftp://, regardless of wiremock
    // mock-matching order when multiple mocks are registered on the same path.
    for field in &["uploadUrl", "downloadUrl", "eventSourceUrl"] {
        let server = MockServer::start().await;
        let mut body = session_fixture();
        body[*field] = serde_json::Value::String("ftp://example.com/bad".to_owned());

        Mock::given(method("GET"))
            .and(path("/.well-known/jmap"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let client = JmapClient::new(
            jmap_base_client::auth::DefaultTransport,
            NoneAuth,
            &server.uri(),
            jmap_base_client::client::ClientConfig::default(),
        )
        .expect("client construction must succeed");

        let err = client
            .fetch_session()
            .await
            .expect_err(&format!("ftp {field} must fail"));
        assert!(
            matches!(err, ClientError::InvalidSession(_)),
            "expected InvalidSession for ftp {field}, got {err:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// call
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §3.3/§3.4 — a successful POST to apiUrl returns a
/// JmapResponse parsed from the hand-written call_response.json fixture.
#[tokio::test]
async fn test_call_round_trip() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(call_response_fixture()))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let api_url = format!("{}/api/", server.uri());
    let resp = client
        .call(&api_url, &minimal_request())
        .await
        .expect("call must succeed");

    // Oracle: values from call_response.json fixture
    assert_eq!(resp.session_state, "sess1");
    assert_eq!(resp.method_responses.len(), 1);
    assert_eq!(resp.method_responses[0].0, "Mailbox/get");
    assert_eq!(resp.method_responses[0].2, "r1");
}

/// Oracle: security requirement — call response body capped at 8 MiB.
/// A response body of 8 MiB + 1 byte must return ClientError::ResponseTooLarge.
#[tokio::test]
async fn test_call_size_cap() {
    let oversized_body = "x".repeat(8 * 1024 * 1024 + 1);
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(oversized_body))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let api_url = format!("{}/api/", server.uri());
    let err = client
        .call(&api_url, &minimal_request())
        .await
        .expect_err("oversized response must fail");
    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}

/// Oracle: RFC 8620 §3.3 — HTTP 401 from apiUrl must surface as
/// ClientError::AuthFailed(401).
#[tokio::test]
async fn test_call_401_returns_auth_failed() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let api_url = format!("{}/api/", server.uri());
    let err = client
        .call(&api_url, &minimal_request())
        .await
        .expect_err("401 must fail");
    assert!(
        matches!(err, ClientError::AuthFailed(401)),
        "expected AuthFailed(401), got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// upload_blob
// ---------------------------------------------------------------------------

/// Oracle: security requirement — upload_blob response body capped at 1 MiB.
/// A server returning an oversized upload response must yield ResponseTooLarge.
#[tokio::test]
async fn test_upload_blob_response_size_cap() {
    let oversized_body = "x".repeat(1024 * 1024 + 1);
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/upload/account1/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(oversized_body))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let template = format!("{}/upload/{{accountId}}/", server.uri());
    let err = client
        .upload_blob(
            &template,
            "account1",
            bytes::Bytes::from(b"hello".to_vec()),
            "application/octet-stream",
        )
        .await
        .expect_err("oversized upload response must fail");
    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// download_blob
// ---------------------------------------------------------------------------

/// Oracle: security requirement — download_blob response body capped at 64 MiB.
/// A response body of 64 MiB + 1 byte must return ClientError::ResponseTooLarge.
#[tokio::test]
async fn test_download_blob_size_cap() {
    let oversized_body = vec![b'x'; 64 * 1024 * 1024 + 1];
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/download/account1/blob-abc/file.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(oversized_body))
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let template = format!(
        "{}/download/{{accountId}}/{{blobId}}/{{name}}",
        server.uri()
    );
    let err = client
        .download_blob(jmap_base_client::DownloadBlobParams {
            download_url_template: &template,
            account_id: "account1",
            blob_id: "blob-abc",
            name: "file.bin",
            accept_type: None,
            expected_sha256: None,
        })
        .await
        .expect_err("oversized download must fail");
    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// subscribe_events — SSE line-ending normalization
// ---------------------------------------------------------------------------

/// Oracle: RFC 8895 §9 — SSE lines terminated with CRLF must parse
/// identically to LF-terminated lines.
///
/// A server sending HTTP/1.1 CRLF-terminated SSE is realistic; the client
/// must normalize \r\n → \n before handing the block to parse_sse_block.
#[tokio::test]
async fn test_subscribe_events_crlf_line_endings() {
    use futures::StreamExt as _;
    use jmap_base_client::sse::SseEvent;

    // CRLF-terminated SSE block ending in the double-CRLF frame delimiter.
    let crlf_body = "event: state\r\ndata: {\"changed\":{}}\r\n\r\n";

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_bytes(crlf_body.as_bytes().to_vec()),
        )
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let event_url = format!("{}/events", server.uri());
    let mut stream = client
        .subscribe_events(&event_url, None)
        .await
        .expect("subscribe_events must succeed");

    let frame = stream
        .next()
        .await
        .expect("stream must yield at least one frame")
        .expect("frame must not be an error");

    // Oracle: the "state" event type must be recognized after CRLF normalization.
    assert!(
        matches!(frame.event, SseEvent::StateChange(_)),
        "CRLF-terminated state event must parse as StateChange, got {:?}",
        frame.event
    );
}

/// Oracle: RFC 8895 §9 — SSE frame terminated by LF + CRLF blank line (\n\r\n)
/// must parse correctly. This combination is not detected by \n\n (LFs are
/// separated by \r) and must be caught by the explicit \n\r\n search.
#[tokio::test]
async fn test_subscribe_events_lf_crlf_frame_delimiter() {
    use futures::StreamExt as _;
    use jmap_base_client::sse::SseEvent;

    // LF-terminated field lines, CRLF-terminated blank line: \n\r\n delimiter.
    let body = "event: state\ndata: {\"changed\":{}}\n\r\n";

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_bytes(body.as_bytes().to_vec()),
        )
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let event_url = format!("{}/events", server.uri());
    let mut stream = client
        .subscribe_events(&event_url, None)
        .await
        .expect("subscribe_events must succeed");

    let frame = stream
        .next()
        .await
        .expect("stream must yield at least one frame")
        .expect("frame must not be an error");

    // Oracle: state event must be recognized after \n\r\n delimiter.
    assert!(
        matches!(frame.event, SseEvent::StateChange(_)),
        "LF+CRLF-terminated state event must parse as StateChange, got {:?}",
        frame.event
    );
}

/// Oracle: RFC 8895 §9 — SSE lines terminated with bare CR must parse
/// identically to LF-terminated lines (CR-only is a valid line terminator).
#[tokio::test]
async fn test_subscribe_events_cr_line_endings() {
    use futures::StreamExt as _;
    use jmap_base_client::sse::SseEvent;

    // CR-only-terminated SSE block ending in the double-CR frame delimiter.
    let cr_body = "event: state\rdata: {\"changed\":{}}\r\r";

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_bytes(cr_body.as_bytes().to_vec()),
        )
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let event_url = format!("{}/events", server.uri());
    let mut stream = client
        .subscribe_events(&event_url, None)
        .await
        .expect("subscribe_events must succeed");

    let frame = stream
        .next()
        .await
        .expect("stream must yield at least one frame")
        .expect("frame must not be an error");

    // Oracle: the "state" event type must be recognized after CR normalization.
    assert!(
        matches!(frame.event, SseEvent::StateChange(_)),
        "CR-terminated state event must parse as StateChange, got {:?}",
        frame.event
    );
}

/// Oracle: security requirement — subscribe_events must reject a 200 response
/// whose Content-Type is not text/event-stream. A misconfigured server returning
/// application/json would silently produce no events; return UnexpectedResponse instead.
#[tokio::test]
async fn test_subscribe_events_rejects_wrong_content_type() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/json")
                .set_body_bytes(b"{}".to_vec()),
        )
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let event_url = format!("{}/events", server.uri());
    // BoxStream is not Debug, so expect_err is unavailable; use match.
    let result = client.subscribe_events(&event_url, None).await;
    match result {
        Ok(_) => panic!("wrong Content-Type must fail before streaming starts"),
        Err(ref e) => assert!(
            matches!(e, ClientError::UnexpectedResponse(_)),
            "expected UnexpectedResponse for wrong Content-Type, got {e:?}"
        ),
    }
}

/// Oracle: regression for bd:JMAP-6lsm.2 — RFC 7231 §3.1.1.1 / RFC 9110 §8.3
/// say the media-type essence is bounded by ';', SP, HTAB, or end-of-string.
/// A naive `starts_with("text/event-stream")` accepts "text/event-streamish",
/// silently produces no events, and the caller sees an apparently-quiet
/// stream. The fix must reject the suffix-extension case before streaming
/// starts. The independent oracle is the spec; the test feeds a hand-written
/// invalid Content-Type that *would* have passed the old prefix check.
#[tokio::test]
async fn test_subscribe_events_rejects_event_stream_suffix() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(
            ResponseTemplate::new(200)
                // Trailing "ish" makes the subtype "event-streamish", not
                // "event-stream". Pre-fix code would accept this.
                .insert_header("Content-Type", "text/event-streamish")
                .set_body_bytes(b"data: hi\n\n".to_vec()),
        )
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let event_url = format!("{}/events", server.uri());
    let result = client.subscribe_events(&event_url, None).await;
    match result {
        Ok(_) => panic!("text/event-streamish must be rejected, not silently accepted"),
        Err(ref e) => assert!(
            matches!(e, ClientError::UnexpectedResponse(_)),
            "expected UnexpectedResponse for streamish suffix, got {e:?}"
        ),
    }
}

/// Oracle: positive case for bd:JMAP-6lsm.2 — Content-Type with a parameter
/// like `text/event-stream; charset=utf-8` MUST be accepted (RFC 7231
/// §3.1.1.1 allows parameters after ';'). The bugfix splits on ';' or
/// whitespace and compares the essence; without that split, a parameterised
/// header would still pass the (now stricter) check, but pinning this case
/// keeps the boundary explicit so a future "tighten further" mistake doesn't
/// silently break parameterised media types.
#[tokio::test]
async fn test_subscribe_events_accepts_charset_parameter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream; charset=utf-8")
                // Single complete event so the stream returns Some(Ok(_)).
                .set_body_bytes(b"data: hi\n\n".to_vec()),
        )
        .mount(&server)
        .await;

    let client = JmapClient::new(
        jmap_base_client::auth::DefaultTransport,
        NoneAuth,
        &server.uri(),
        jmap_base_client::client::ClientConfig::default(),
    )
    .expect("client construction must succeed");

    let event_url = format!("{}/events", server.uri());
    // Should construct the stream successfully even with the charset param.
    let _stream = client
        .subscribe_events(&event_url, None)
        .await
        .expect("subscribe_events must accept text/event-stream; charset=utf-8");
}

// ---------------------------------------------------------------------------
// extract_response
// ---------------------------------------------------------------------------

/// Oracle: RFC 8620 §3.4 — extract_response finds the matching invocation
/// by call_id and deserializes its arguments.
#[test]
fn test_extract_response_success() {
    let resp = jmap_types::JmapResponse::new(
        vec![(
            "Mailbox/get".to_owned(),
            serde_json::json!({"accountId": "A13824", "state": "s1", "list": [], "notFound": []}),
            "r1".to_owned(),
        )],
        "sess1".into(),
        None,
    );

    let val = jmap_base_client::client::extract_response::<serde_json::Value>(&resp, "r1");
    assert!(val.is_ok(), "extract_response must succeed: {val:?}");
}

/// Oracle: RFC 8620 §3.4 — extract_response returns ClientError::MethodNotFound
/// when no invocation with the given call_id exists.
#[test]
fn test_extract_response_not_found() {
    let resp = jmap_types::JmapResponse::new(
        vec![(
            "Mailbox/get".to_owned(),
            serde_json::json!({}),
            "r1".to_owned(),
        )],
        "sess1".into(),
        None,
    );

    let err = jmap_base_client::client::extract_response::<serde_json::Value>(&resp, "r99")
        .expect_err("wrong call_id must fail");
    assert!(
        matches!(err, ClientError::MethodNotFound(_)),
        "expected MethodNotFound, got {err:?}"
    );
}

/// Oracle: RFC 8620 §3.6.1 — when an invocation has method name "error",
/// extract_response returns ClientError::MethodError with type and description.
#[test]
fn test_extract_response_method_error() {
    let resp = jmap_types::JmapResponse::new(
        vec![(
            "error".to_owned(),
            serde_json::json!({"type": "serverFail", "description": "oops"}),
            "r1".to_owned(),
        )],
        "sess1".into(),
        None,
    );

    let err = jmap_base_client::client::extract_response::<serde_json::Value>(&resp, "r1")
        .expect_err("error invocation must fail");
    assert!(
        matches!(
            &err,
            ClientError::MethodError { error_type, description }
                if error_type == "serverFail" && description.as_deref() == Some("oops")
        ),
        "expected MethodError{{serverFail, Some(\"oops\")}}, got {err:?}"
    );
}

/// Oracle: RFC 8620 §3.2 + §3.6.1 — when a server emits both a success
/// invocation and an "error" invocation under the same call_id, the error
/// MUST take precedence. Returning the success would silently lose the
/// failure indication. Independent oracle: hand-built JmapResponse with
/// the documented multi-invocation pattern from RFC 8620 §3.2 line 876–880.
#[test]
fn test_extract_response_error_after_success_takes_precedence() {
    let resp = jmap_types::JmapResponse::new(
        vec![
            (
                "Mailbox/get".to_owned(),
                serde_json::json!({
                    "accountId": "A1",
                    "state": "s1",
                    "list": [],
                    "notFound": []
                }),
                "r1".to_owned(),
            ),
            (
                "error".to_owned(),
                serde_json::json!({"type": "serverFail", "description": "implicit op failed"}),
                "r1".to_owned(),
            ),
        ],
        "sess1".into(),
        None,
    );

    let err = jmap_base_client::client::extract_response::<serde_json::Value>(&resp, "r1")
        .expect_err("error sibling must surface even with a success present");
    assert!(
        matches!(
            &err,
            ClientError::MethodError { error_type, .. } if error_type == "serverFail"
        ),
        "expected MethodError{{serverFail}}, got {err:?}"
    );
}

/// Oracle: RFC 8620 §5.8 example (lines 3158–3180) — a `Foo/copy` with
/// `onSuccessDestroyOriginal: true` produces both the primary `Foo/copy`
/// response and an implicit `Foo/set` response, both with the same call_id.
/// When all matching invocations are successes, extract_response returns
/// the FIRST one (the primary response). Independent oracle: spec example
/// JSON shape, hand-built here.
#[test]
fn test_extract_response_first_success_when_no_error() {
    let resp = jmap_types::JmapResponse::new(
        vec![
            (
                "Todo/copy".to_owned(),
                serde_json::json!({
                    "fromAccountId": "x",
                    "accountId": "y",
                    "created": {"k5122": {"id": "DAf97"}},
                    "oldState": "c1d64ecb038c",
                    "newState": "33844835152b"
                }),
                "0".to_owned(),
            ),
            (
                "Todo/set".to_owned(),
                serde_json::json!({
                    "accountId": "x",
                    "oldState": "871903",
                    "newState": "871909",
                    "destroyed": ["a"]
                }),
                "0".to_owned(),
            ),
        ],
        "sess1".into(),
        None,
    );

    let v = jmap_base_client::client::extract_response::<serde_json::Value>(&resp, "0")
        .expect("must succeed when all matches are successes");
    assert_eq!(
        v["fromAccountId"], "x",
        "primary (first) response must be the Todo/copy result, got {v}"
    );
    assert!(
        v.get("destroyed").is_none(),
        "must NOT be the Todo/set result (which has 'destroyed' but no 'fromAccountId')"
    );
}

/// Oracle: extension of the §3.2 multi-response rule — error precedence
/// applies even when the error appears after several successful matches.
/// Catches a regression where the implementation might short-circuit on
/// the first match.
#[test]
fn test_extract_response_error_after_multiple_successes() {
    let resp = jmap_types::JmapResponse::new(
        vec![
            (
                "Todo/copy".to_owned(),
                serde_json::json!({"fromAccountId": "x"}),
                "r1".to_owned(),
            ),
            (
                "Todo/set".to_owned(),
                serde_json::json!({"accountId": "x"}),
                "r1".to_owned(),
            ),
            (
                "error".to_owned(),
                serde_json::json!({"type": "rateLimit"}),
                "r1".to_owned(),
            ),
        ],
        "sess1".into(),
        None,
    );

    let err = jmap_base_client::client::extract_response::<serde_json::Value>(&resp, "r1")
        .expect_err("trailing error must take precedence over earlier successes");
    assert!(
        matches!(
            &err,
            ClientError::MethodError { error_type, .. } if error_type == "rateLimit"
        ),
        "expected MethodError{{rateLimit}}, got {err:?}"
    );
}
