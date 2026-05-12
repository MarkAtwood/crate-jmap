//! Axum router for the testjig's foundation HTTP endpoints.
//!
//! Mounts two routes per RFC 8620:
//!
//! - `POST /jmap` — the API endpoint (RFC 8620 §3). Parses the request
//!   body, dispatches each method call through [`Dispatcher`], and
//!   serialises the response envelope back to the client.
//! - `GET /.well-known/jmap` — the Session resource (RFC 8620 §2.2).
//!   Returns the hardcoded Session JSON built by
//!   [`crate::session::session_json`].
//!
//! # Slice scope (bd:JMAP-cf7p.2)
//!
//! This slice wires the two foundation routes and registers a single
//! built-in `Core/echo` handler (RFC 8620 §4) so the dispatcher can
//! demonstrate end-to-end request/response flow without any backend.
//!
//! The 8 extension MemoryBackends, SSE / WebSocket endpoints, and the
//! bearer-auth middleware land in subsequent slices
//! (bd:JMAP-cf7p.3 / .4 / .5 / .6). Until then, any method other than
//! `Core/echo` returns `unknownMethod` per RFC 8620 §3.6.2.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State as AxumState,
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use http_body_util::LengthLimitError;
use jmap_server::{parse_request, request_error, Dispatcher, JmapError, JmapHandler, RequestError};
use jmap_types::{Invocation, State};
use serde_json::Value;

use crate::session;

/// Maximum size, in bytes, of a single `POST /jmap` request body.
///
/// Set to 10 MiB, matching the RFC 8620 §2 `maxSizeRequest` suggested
/// minimum that the testjig advertises in its Session. Exceeding this
/// limit produces a JMAP `requestTooLarge` response (HTTP 400) per
/// RFC 8620 §3.6.1 — `requestTooLarge` is a request-level JMAP error
/// type, not a generic HTTP 413.
pub const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

/// Maximum number of method calls in a single JMAP request.
///
/// Matches the RFC 8620 §2 `maxCallsInRequest` suggested minimum that
/// the testjig advertises. Exceeding this produces
/// `limit("maxCallsInRequest")` per RFC 8620 §3.6.1.
pub const MAX_CALLS_IN_REQUEST: usize = 16;

/// Application state shared with axum route handlers.
///
/// Wrapped in [`Arc`] internally so cloning the router state is cheap
/// even though `Dispatcher<()>` does not itself implement `Clone`.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    /// The JMAP method dispatcher. Caller context is `()` because the
    /// testjig is single-user — there is no per-request identity to
    /// thread through. Extension MemoryBackends use
    /// `type CallerCtx = ();` so this dispatcher is compatible with
    /// every handler the testjig will mount in later slices.
    dispatcher: Dispatcher<()>,
}

impl AppState {
    /// Build the testjig's application state with the foundation
    /// `Core/echo` handler registered.
    ///
    /// Slice bd:JMAP-cf7p.3 will extend this to register the 8
    /// extension reference MemoryBackend handlers.
    pub fn new() -> Self {
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        dispatcher.register("Core/echo", Arc::new(EchoHandler));
        Self {
            inner: Arc::new(AppStateInner { dispatcher }),
        }
    }

    /// Borrow the dispatcher (e.g. to register additional handlers
    /// during integration-test setup). Returns `None` if the state has
    /// been cloned and the cloned references are still alive; callers
    /// must mutate the dispatcher before sharing the state.
    ///
    /// Slice bd:JMAP-cf7p.3 will switch the dispatcher to be
    /// pre-populated at construction time, at which point this
    /// borrow-mut accessor is no longer needed.
    pub fn dispatcher_mut(&mut self) -> Option<&mut Dispatcher<()>> {
        Arc::get_mut(&mut self.inner).map(|inner| &mut inner.dispatcher)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the axum router with the foundation routes mounted.
///
/// Routes:
///
/// - `GET /.well-known/jmap` → [`get_session`]
/// - `POST /jmap` → [`post_jmap`]
///
/// Slice bd:JMAP-cf7p.4 will add `GET /events` (SSE).
/// Slice bd:JMAP-cf7p.5 will add `GET /ws` (WebSocket).
/// Slice bd:JMAP-cf7p.6 will wrap the router with the bearer-auth
/// middleware.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/.well-known/jmap", get(get_session))
        .route("/jmap", post(post_jmap))
        .with_state(state)
}

/// `GET /.well-known/jmap` — return the Session resource (RFC 8620 §2).
///
/// The body is pinned per [`crate::session::session_json`]; this
/// handler simply serialises it with `application/json` Content-Type.
async fn get_session() -> Response {
    let body = serde_json::to_string(&session::session_json())
        .expect("session_json builds only JSON-safe primitives — Serialize is infallible");
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    resp
}

/// `POST /jmap` — dispatch a JMAP request envelope (RFC 8620 §3).
///
/// Body-handling pipeline:
///
/// 1. Drain the request body with a 10 MiB cap. Bodies exceeding the
///    cap surface as `LengthLimitError`, which maps to JMAP
///    `requestTooLarge` (HTTP 400 per RFC 8620 §3.6.1).
/// 2. Parse the bytes as JSON. Parse failure → `notJSON` (HTTP 400).
/// 3. Validate the JMAP request envelope via [`parse_request`] with
///    `MAX_CALLS_IN_REQUEST`. Schema or call-count failures surface as
///    `notRequest` / `limit` (HTTP 400).
/// 4. Dispatch through the registered handlers. The response envelope
///    serialises as `application/json` (HTTP 200), with method-level
///    errors embedded inside `methodResponses` per RFC 8620 §3.6.2.
///
/// Capability validation (`check_known_capabilities`) is deliberately
/// **not** enforced at this slice — the testjig advertises 9 URIs but
/// only the core method `Core/echo` is registered. Requiring opt-in
/// would refuse any client that requests `urn:ietf:params:jmap:mail`
/// even when it only calls `Core/echo`. Slice bd:JMAP-cf7p.3 may
/// re-enable the check once the extension MemoryBackends mount their
/// methods.
async fn post_jmap(
    AxumState(state): AxumState<AppState>,
    body: Body,
) -> Result<Response, ApiError> {
    let bytes = axum::body::to_bytes(body, MAX_REQUEST_BYTES)
        .await
        .map_err(map_body_error)?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| ApiError::from(JmapError::not_json()))?;
    let request = parse_request(value, MAX_CALLS_IN_REQUEST).map_err(ApiError::from)?;

    let response = state
        .inner
        .dispatcher
        .dispatch(request, (), State::from(session::STATE))
        .await;

    let body = serde_json::to_string(&response)
        .expect("JmapResponse is built from JSON-safe primitives — Serialize is infallible");
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok(resp)
}

/// Classify an [`axum_core::Error`] produced by [`axum::body::to_bytes`].
///
/// The error chain contains a [`LengthLimitError`] when the body
/// exceeded `MAX_REQUEST_BYTES`; that maps to JMAP `requestTooLarge`
/// per RFC 8620 §3.6.1. Any other body-read failure (connection
/// reset, malformed Transfer-Encoding, etc.) is a transport-layer
/// fault that does not have a clean JMAP error mapping; surface it as
/// `notJSON` since the dispatcher will never see well-formed JSON.
fn map_body_error(err: axum_core::Error) -> ApiError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&err);
    while let Some(e) = source {
        if e.is::<LengthLimitError>() {
            return ApiError::from(JmapError::request_too_large());
        }
        source = e.source();
    }
    ApiError::from(JmapError::not_json())
}

/// Local newtype wrapping [`RequestError`] so we can impl
/// [`IntoResponse`] for it without violating Rust's orphan rule.
///
/// `RequestError::into_response` already produces a correctly-shaped
/// RFC 7807 Problem Details response with `application/problem+json`
/// Content-Type and the right HTTP status; we just adapt the body
/// type from `http::Response<String>` to axum's preferred shape.
struct ApiError(RequestError);

impl From<JmapError> for ApiError {
    fn from(err: JmapError) -> Self {
        Self(request_error(err))
    }
}

impl From<RequestError> for ApiError {
    fn from(err: RequestError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // RequestError.into_response() returns http::Response<String>;
        // axum's IntoResponse for String wraps it as a Body so we can
        // map the body type cleanly here.
        let (parts, body) = self.0.into_response().into_parts();
        Response::from_parts(parts, Body::from(body))
    }
}

/// Built-in `Core/echo` handler (RFC 8620 §4): returns its arguments
/// verbatim.
///
/// Lives in the testjig (not in `jmap-server`) because RFC 8620 §4
/// only requires *some* server to advertise the Core capability and
/// implement `Core/echo`; it does not place the implementation in any
/// particular crate. The kit's `jmap-server` foundation stays a pure
/// dispatcher; the test jig owns this built-in.
///
/// Real consumers may register their own `Core/echo` handler (or
/// none, since the method is not load-bearing for any non-test
/// scenario) on the same `Dispatcher` shape.
struct EchoHandler;

impl JmapHandler<()> for EchoHandler {
    fn call(
        &self,
        _method: String,
        _call_id: String,
        args: Value,
        _caller: (),
    ) -> jmap_server::HandlerFuture {
        // Per RFC 8620 §4 the response is the exact same arguments
        // object the client sent; there are no extra invocations.
        Box::pin(async move { Ok((args, Vec::<Invocation>::new())) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::http::StatusCode;
    use http_body_util::BodyExt;
    use serde_json::json;

    // Helper: drive a request through the router and return
    // (StatusCode, parsed JSON body).
    async fn send(
        router: Router,
        method: http::Method,
        path: &str,
        body: Option<&str>,
    ) -> (StatusCode, Value) {
        let req = http::Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map(|s| Body::from(s.to_owned())).unwrap_or_default())
            .expect("test fixture: request builder should succeed");
        let resp = <Router as tower::ServiceExt<_>>::oneshot(router, req)
            .await
            .expect("test fixture: router oneshot should succeed");
        let status = resp.status();
        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("test fixture: body collect should succeed")
            .to_bytes();
        let value: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("non-JSON body: {:?}", String::from_utf8_lossy(&bytes)));
        (status, value)
    }

    /// Oracle: RFC 8620 §4 — Core/echo returns its arguments verbatim.
    /// End-to-end test: build a JMAP request envelope, POST it to the
    /// router, verify the methodResponses entry echoes the request args.
    #[tokio::test]
    async fn post_jmap_core_echo_returns_args_unchanged() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [
                ["Core/echo", {"hello": "world", "n": 42}, "c1"]
            ]
        }))
        .unwrap();
        let (status, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
        assert_eq!(status, StatusCode::OK, "successful dispatch must be 200");
        let calls = body["methodResponses"]
            .as_array()
            .expect("methodResponses must be an array");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "Core/echo");
        assert_eq!(
            calls[0][1],
            json!({"hello": "world", "n": 42}),
            "Core/echo must echo args unchanged per RFC 8620 §4"
        );
        assert_eq!(calls[0][2], "c1");
    }

    /// Oracle: RFC 8620 §3.4 — `sessionState` in the response matches
    /// the value the server hands to the dispatcher (here, the
    /// testjig's pinned [`session::STATE`]).
    #[tokio::test]
    async fn post_jmap_response_carries_session_state() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [
                ["Core/echo", {}, "c1"]
            ]
        }))
        .unwrap();
        let (_, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
        assert_eq!(body["sessionState"], session::STATE);
    }

    /// Oracle: RFC 8620 §3.6.2 — `unknownMethod` is a method-level
    /// error returned inside `methodResponses` at HTTP 200, not a
    /// request-level HTTP 4xx.
    #[tokio::test]
    async fn post_jmap_unknown_method_returns_method_level_error_at_200() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [
                ["Bogus/method", {}, "c1"]
            ]
        }))
        .unwrap();
        let (status, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
        assert_eq!(status, StatusCode::OK);
        let inv = &body["methodResponses"][0];
        assert_eq!(inv[0], "error", "error invocation method name is 'error'");
        assert_eq!(inv[1]["type"], "unknownMethod");
        assert_eq!(inv[2], "c1");
    }

    /// Oracle: RFC 8620 §3.6.1 — `notJSON` is a request-level error
    /// at HTTP 400 with RFC 7807 Problem Details body.
    #[tokio::test]
    async fn post_jmap_malformed_json_returns_400_not_json() {
        let state = AppState::new();
        let router = router(state);
        let (status, body) = send(
            router,
            http::Method::POST,
            "/jmap",
            Some("this-is-not-json"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["type"], "urn:ietf:params:jmap:error:notJSON",
            "malformed body must produce notJSON per RFC 8620 §3.6.1"
        );
        assert_eq!(body["status"], 400);
    }

    /// Oracle: RFC 8620 §3.6.1 — valid JSON that does not match the
    /// JmapRequest schema produces `notRequest` (HTTP 400).
    #[tokio::test]
    async fn post_jmap_wrong_shape_returns_400_not_request() {
        let state = AppState::new();
        let router = router(state);
        let (status, body) = send(
            router,
            http::Method::POST,
            "/jmap",
            Some("\"a string, not a JMAP request envelope\""),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["type"], "urn:ietf:params:jmap:error:notRequest");
    }

    /// Oracle: RFC 8620 §3.6.1 — exceeding `maxCallsInRequest` produces
    /// the `limit` error with a `limit` property naming the exceeded
    /// limit.
    #[tokio::test]
    async fn post_jmap_too_many_calls_returns_400_limit() {
        let state = AppState::new();
        let router = router(state);
        let mut calls = Vec::with_capacity(MAX_CALLS_IN_REQUEST + 1);
        for i in 0..=MAX_CALLS_IN_REQUEST {
            calls.push(json!(["Core/echo", {}, format!("c{i}")]));
        }
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": calls,
        }))
        .unwrap();
        let (status, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["type"], "urn:ietf:params:jmap:error:limit");
        assert_eq!(body["limit"], "maxCallsInRequest");
    }

    /// Oracle: RFC 8620 §3.6.1 — bodies exceeding `maxSizeRequest`
    /// produce `requestTooLarge` (HTTP 400), not the generic HTTP 413.
    /// JMAP uses HTTP 400 + the JMAP `requestTooLarge` error type for
    /// this case per `error_status` in jmap-server.
    #[tokio::test]
    async fn post_jmap_oversize_body_returns_400_request_too_large() {
        let state = AppState::new();
        let router = router(state);
        // Build a body one byte larger than the cap. The contents do
        // not need to be valid JMAP; the body-length check fires
        // before parsing.
        let oversized = "x".repeat(MAX_REQUEST_BYTES + 1);
        let (status, body) = send(router, http::Method::POST, "/jmap", Some(&oversized)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["type"], "urn:ietf:params:jmap:error:requestTooLarge",
            "oversize body must map to requestTooLarge per RFC 8620 §3.6.1"
        );
    }

    /// Oracle: RFC 8620 §2 — `GET /.well-known/jmap` returns a
    /// well-formed Session object as `application/json`.
    #[tokio::test]
    async fn get_session_returns_session_json_at_200() {
        let state = AppState::new();
        let router = router(state);
        let req = http::Request::builder()
            .method(http::Method::GET)
            .uri("/.well-known/jmap")
            .body(Body::empty())
            .unwrap();
        let resp = <Router as tower::ServiceExt<_>>::oneshot(router, req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).expect("Session body must be JSON");
        assert_eq!(body["username"], session::USERNAME);
        assert_eq!(body["state"], session::STATE);
        assert!(body["capabilities"]["urn:ietf:params:jmap:core"].is_object());
        assert!(body["accounts"][session::ACCOUNT_ID].is_object());
    }
}
