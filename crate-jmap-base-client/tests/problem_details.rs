//! RFC 9457 problem details from every JMAP HTTP resource become
//! `ClientError::Problem` (draft-atwood-jmap-updates-00 §7.2).
//!
//! Oracle: RFC 8620 §3.6.1.1 (the `limit` example body), RFC 9457 §3
//! (`application/problem+json`, `type`, `status`), and the draft §7.2
//! ("Every HTTP resource of a JMAP server SHOULD return a problem details
//! object ... the Session resource, the event source ..."; clients act on
//! `type` and the status). Bodies are hand-written.

use jmap_base_client::{
    ClientConfig, ClientError, DefaultTransport, DownloadBlobParams, JmapClient, JmapUrlTemplate,
    NoneAuth,
};
use jmap_types::MethodErrorType;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> JmapClient {
    JmapClient::new(
        DefaultTransport,
        NoneAuth,
        &server.uri(),
        ClientConfig::default(),
    )
    .unwrap()
}

fn problem(status: u16, body: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.to_string(), "application/problem+json")
}

/// RFC 8620 §3.6.1.1: a `limit` request error from the API resource.
#[tokio::test]
async fn api_request_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(problem(
            400,
            json!({
                "type": "urn:ietf:params:jmap:error:limit",
                "limit": "maxSizeRequest",
                "status": 400,
                "detail": "The request is larger than the server is willing to process."
            }),
        ))
        .mount(&server)
        .await;
    let req = jmap_types::JmapRequest::new(vec!["urn:ietf:params:jmap:core".into()], vec![], None);
    let err = client(&server)
        .call(&format!("{}/api/", server.uri()), &req)
        .await
        .unwrap_err();
    let ClientError::Problem { status, details } = err else {
        panic!("expected Problem, got {err:?}");
    };
    assert_eq!(status, 400);
    assert_eq!(details.jmap_error_type(), Some(MethodErrorType::Limit));
    assert_eq!(details.limit.as_deref(), Some("maxSizeRequest"));
}

/// The draft §7.2: the Session resource too.
#[tokio::test]
async fn session_resource() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(problem(
            503,
            json!({ "type": "about:blank", "status": 503, "title": "Service Unavailable" }),
        ))
        .mount(&server)
        .await;
    match client(&server).fetch_session().await {
        Err(ClientError::Problem {
            status: 503,
            details,
        }) => {
            assert_eq!(details.problem_type, "about:blank");
        }
        other => panic!("expected Problem 503, got {other:?}"),
    }
}

/// The draft §7.2: the download resource; `type` may be absent
/// (RFC 9457 §3.1.1: then "about:blank").
#[tokio::test]
async fn download_resource() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/download/a1/b1/f"))
        .respond_with(problem(404, json!({ "status": 404, "title": "Not Found" })))
        .mount(&server)
        .await;
    let template = JmapUrlTemplate::new(format!(
        "{}/download/{{accountId}}/{{blobId}}/{{name}}",
        server.uri()
    ));
    let err = client(&server)
        .download_blob(DownloadBlobParams {
            download_url_template: &template,
            account_id: &jmap_types::Id::from("a1"),
            blob_id: &jmap_types::Id::from("b1"),
            name: "f",
            accept_type: None,
            expected_sha256: None,
        })
        .await
        .unwrap_err();
    match err {
        ClientError::Problem {
            status: 404,
            details,
        } => {
            assert_eq!(details.problem_type, "about:blank");
        }
        other => panic!("expected Problem 404, got {other:?}"),
    }
}

/// The draft §7.2: the event source.
#[tokio::test]
async fn event_source() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/events"))
        .respond_with(problem(
            400,
            json!({ "type": "about:blank", "status": 400 }),
        ))
        .mount(&server)
        .await;
    let err = client(&server)
        .subscribe_events(&format!("{}/events", server.uri()), None)
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(err, ClientError::Problem { status: 400, .. }),
        "{err:?}"
    );
}

/// An error without a problem details body, or with a body that does not
/// parse, stays the opaque `Http` error.
#[tokio::test]
async fn other_error_bodies_stay_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(
            ResponseTemplate::new(500).set_body_raw("not json", "application/problem+json"),
        )
        .mount(&server)
        .await;
    let c = client(&server);
    assert!(matches!(c.fetch_session().await, Err(ClientError::Http(_))));
    assert!(matches!(c.fetch_session().await, Err(ClientError::Http(_))));
}

/// 401 keeps its existing `AuthFailed` mapping even with a problem body.
#[tokio::test]
async fn unauthorized_stays_auth_failed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jmap"))
        .respond_with(problem(
            401,
            json!({ "type": "about:blank", "status": 401 }),
        ))
        .mount(&server)
        .await;
    assert!(matches!(
        client(&server).fetch_session().await,
        Err(ClientError::AuthFailed(401))
    ));
}
