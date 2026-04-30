// Integration tests for JmapClient core methods.
// Oracle: RFC 8620 §2 (Session), §3.3 (making requests), §6 (blobs), §7 (push)
// Fixtures: tests/fixtures/jmap/*.json (hand-written from RFC 8620 examples)

use jmap_client::auth::NoneAuth;
use jmap_client::client::JmapClient;
use jmap_client::error::ClientError;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn session_fixture() -> serde_json::Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jmap/session.json"),
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
        vec!["urn:ietf:params:jmap:core".to_string()],
        vec![(
            "Mailbox/get".to_string(),
            serde_json::json!({"accountId": "A13824", "ids": null}),
            "r1".to_string(),
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
    let result = JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, "", jmap_client::client::ClientConfig::default()).map(|_| ());
    assert!(
        matches!(result, Err(ClientError::InvalidArgument(_))),
        "empty base_url must return InvalidArgument, got {result:?}"
    );
}

/// Oracle: base_url validation — ftp:// scheme must be rejected.
#[test]
fn test_new_rejects_ftp_scheme() {
    let result = JmapClient::new(
        jmap_client::auth::DefaultTransport,
        NoneAuth,
        "ftp://example.com",
        jmap_client::client::ClientConfig::default(),
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
        jmap_client::auth::DefaultTransport,
        NoneAuth,
        "https://example.com/jmap",
        jmap_client::client::ClientConfig::default(),
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
        jmap_client::auth::DefaultTransport,
        NoneAuth,
        "https://example.com",
        jmap_client::client::ClientConfig::default(),
    )
    .map(|_| ());
    assert!(result.is_ok(), "valid https origin must be accepted");
}

/// Oracle: config validation — request_timeout == Duration::ZERO must be rejected.
/// Duration::ZERO is version-dependent in reqwest: some versions treat it as "no timeout",
/// others as "instant timeout". Reject explicitly to eliminate this footgun.
#[test]
fn test_new_rejects_zero_request_timeout() {
    let mut config = jmap_client::client::ClientConfig::default();
    config.request_timeout = std::time::Duration::ZERO;
    let result = JmapClient::new(
        jmap_client::auth::DefaultTransport,
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
    let mut config = jmap_client::client::ClientConfig::default();
    config.max_call_body = 0;
    let result = JmapClient::new(
        jmap_client::auth::DefaultTransport,
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
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
    body["apiUrl"] = serde_json::Value::String("ftp://example.com/api/".to_string());

    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
            .expect("client construction must succeed");

    let err = client
        .fetch_session()
        .await
        .expect_err("ftp apiUrl must fail");
    assert!(
        matches!(err, ClientError::InvalidArgument(_)),
        "expected InvalidArgument for ftp apiUrl, got {err:?}"
    );
}

/// Oracle: session URL validation — each of uploadUrl, downloadUrl, and
/// eventSourceUrl with a non-http scheme must return ClientError::InvalidArgument.
#[tokio::test]
async fn test_fetch_session_rejects_non_http_other_urls() {
    let server = MockServer::start().await;

    for field in &["uploadUrl", "downloadUrl", "eventSourceUrl"] {
        let mut body = session_fixture();
        body[*field] = serde_json::Value::String("ftp://example.com/bad".to_string());

        Mock::given(method("GET"))
            .and(path("/.well-known/jmap"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let client =
            JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
                .expect("client construction must succeed");

        let err = client
            .fetch_session()
            .await
            .expect_err(&format!("ftp {field} must fail"));
        assert!(
            matches!(err, ClientError::InvalidArgument(_)),
            "expected InvalidArgument for ftp {field}, got {err:?}"
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
            .expect("client construction must succeed");

    let template = format!("{}/upload/{{accountId}}/", server.uri());
    let err = client
        .upload_blob(&template, "account1", bytes::Bytes::from(b"hello".to_vec()), "application/octet-stream")
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

    let client =
        JmapClient::new(jmap_client::auth::DefaultTransport, NoneAuth, &server.uri(), jmap_client::client::ClientConfig::default())
            .expect("client construction must succeed");

    let template = format!("{}/download/{{accountId}}/{{blobId}}/{{name}}", server.uri());
    let err = client
        .download_blob(&template, "account1", "blob-abc", "file.bin", None, None)
        .await
        .expect_err("oversized download must fail");
    assert!(
        matches!(err, ClientError::ResponseTooLarge { .. }),
        "expected ResponseTooLarge, got {err:?}"
    );
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
            "Mailbox/get".to_string(),
            serde_json::json!({"accountId": "A13824", "state": "s1", "list": [], "notFound": []}),
            "r1".to_string(),
        )],
        "sess1".into(),
        None,
    );

    let val = jmap_client::client::extract_response::<serde_json::Value>(resp, "r1");
    assert!(val.is_ok(), "extract_response must succeed: {val:?}");
}

/// Oracle: RFC 8620 §3.4 — extract_response returns ClientError::MethodNotFound
/// when no invocation with the given call_id exists.
#[test]
fn test_extract_response_not_found() {
    let resp = jmap_types::JmapResponse::new(
        vec![(
            "Mailbox/get".to_string(),
            serde_json::json!({}),
            "r1".to_string(),
        )],
        "sess1".into(),
        None,
    );

    let err = jmap_client::client::extract_response::<serde_json::Value>(resp, "r99")
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
            "error".to_string(),
            serde_json::json!({"type": "serverFail", "description": "oops"}),
            "r1".to_string(),
        )],
        "sess1".into(),
        None,
    );

    let err = jmap_client::client::extract_response::<serde_json::Value>(resp, "r1")
        .expect_err("error invocation must fail");
    assert!(
        matches!(
            &err,
            ClientError::MethodError { error_type, description }
                if error_type == "serverFail" && description == "oops"
        ),
        "expected MethodError{{serverFail, oops}}, got {err:?}"
    );
}
