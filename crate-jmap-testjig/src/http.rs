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
use jmap_types::{Id, Invocation, State};
use serde_json::Value;

use crate::auth::{require_bearer_token, AuthState};
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
/// The [`AuthState`] is its own field (not behind the same `Arc`)
/// because axum's `from_fn_with_state` middleware extracts it
/// separately from the per-route handler state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) inner: Arc<AppStateInner>,
    auth: AuthState,
}

/// The 8 reference MemoryBackends, kept alive alongside the dispatcher
/// they registered handlers on.
///
/// `register_*_handlers` clones an `Arc<MemoryBackend>` into every
/// handler closure, so the backends would stay alive purely through
/// the dispatcher even if these fields were dropped. We retain the
/// typed Arcs here for two reasons:
///
/// 1. The SSE poller (slice bd:JMAP-cf7p.4) calls
///    `MemoryBackend::get_state::<O>(...)` per known [`JmapObject`]
///    type to assemble the StateChange map. Reaching the backend
///    through the dispatcher would require a typed handler probe and
///    is significantly more awkward; a direct Arc reference is the
///    minimum viable surface.
/// 2. Future slices (e.g. WebSocket push) need the same access.
///
/// [`JmapObject`]: jmap_server::JmapObject
pub(crate) struct AppStateInner {
    /// The JMAP method dispatcher. Caller context is `()` because the
    /// testjig is single-user — there is no per-request identity to
    /// thread through. Extension MemoryBackends use
    /// `type CallerCtx = ();` so this dispatcher is compatible with
    /// every handler the testjig will mount in later slices.
    pub(crate) dispatcher: Dispatcher<()>,
    pub(crate) mail: Arc<jmap_mail_server::memory::MemoryBackend>,
    pub(crate) chat: Arc<jmap_chat_server::memory::MemoryBackend>,
    pub(crate) calendars: Arc<jmap_calendars_server::memory::MemoryBackend>,
    pub(crate) tasks: Arc<jmap_tasks_server::memory::MemoryBackend>,
    pub(crate) contacts: Arc<jmap_contacts_server::memory::MemoryBackend>,
    pub(crate) filenode: Arc<jmap_filenode_server::memory::MemoryBackend>,
    pub(crate) sharing: Arc<jmap_sharing_server::memory::MemoryBackend>,
    pub(crate) metadata: Arc<jmap_metadata_server::memory::MemoryBackend>,
}

impl AppState {
    /// Build the testjig's application state with the foundation
    /// `Core/echo` handler registered and the default bearer token
    /// (`test-token`) configured for the auth middleware.
    ///
    /// Slice bd:JMAP-cf7p.3 will extend this to register the 8
    /// extension reference MemoryBackend handlers.
    pub fn new() -> Self {
        Self::with_token(crate::auth::DEFAULT_BEARER_TOKEN)
    }

    /// Build the testjig's application state with a custom bearer
    /// token (rather than the [`crate::auth::DEFAULT_BEARER_TOKEN`]).
    ///
    /// Constructs all 8 reference MemoryBackends from the workspace's
    /// extension-server crates, registers the testjig's single
    /// account ([`session::ACCOUNT_ID`]) on each, and mounts every
    /// crate's `register_*_handlers` function on a single dispatcher
    /// alongside the built-in `Core/echo` handler.
    ///
    /// All 8 reference backends use `type CallerCtx = ();`, which
    /// lines up with the testjig's single-hardcoded-principal posture.
    pub fn with_token(token: impl Into<String>) -> Self {
        let mut dispatcher: Dispatcher<()> = Dispatcher::new();
        dispatcher.register("Core/echo", Arc::new(EchoHandler));
        let backends = register_all_extensions(&mut dispatcher);
        Self {
            inner: Arc::new(AppStateInner {
                dispatcher,
                mail: backends.mail,
                chat: backends.chat,
                calendars: backends.calendars,
                tasks: backends.tasks,
                contacts: backends.contacts,
                filenode: backends.filenode,
                sharing: backends.sharing,
                metadata: backends.metadata,
            }),
            auth: AuthState::new(token),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the axum router with the foundation routes mounted and the
/// bearer-token middleware applied to every route.
///
/// Routes (all gated behind the bearer token from [`AppState`]):
///
/// - `GET /.well-known/jmap` → `get_session` (Session resource,
///   RFC 8620 §2)
/// - `POST /jmap` → `post_jmap` (API endpoint, RFC 8620 §3)
/// - `GET /events` → [`crate::sse::get_events`] (RFC 8620 §7.3
///   EventSource)
/// - `GET /ws` → [`crate::ws::get_ws`] (RFC 8887 JMAP-over-WebSocket)
///
/// `get_session` and `post_jmap` are module-private; the SSE and WS
/// handlers are named via their `pub` paths so consumers reading the
/// rendered docs can follow them.
pub fn router(state: AppState) -> Router {
    let auth = state.auth.clone();
    Router::new()
        .route("/.well-known/jmap", get(get_session))
        .route("/jmap", post(post_jmap))
        .route("/events", get(crate::sse::get_events))
        .route("/ws", get(crate::ws::get_ws))
        .route_layer(axum::middleware::from_fn_with_state(
            auth,
            require_bearer_token,
        ))
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

/// The typed [`Arc<MemoryBackend>`] handles produced by
/// [`register_all_extensions`].
///
/// Returned (rather than dropped) so the caller can retain typed
/// references on [`AppStateInner`]; the SSE poller (slice
/// bd:JMAP-cf7p.4) needs direct access to call
/// `MemoryBackend::get_state::<O>(...)` per [`JmapObject`] type.
///
/// [`JmapObject`]: jmap_server::JmapObject
struct ExtensionBackends {
    mail: Arc<jmap_mail_server::memory::MemoryBackend>,
    chat: Arc<jmap_chat_server::memory::MemoryBackend>,
    calendars: Arc<jmap_calendars_server::memory::MemoryBackend>,
    tasks: Arc<jmap_tasks_server::memory::MemoryBackend>,
    contacts: Arc<jmap_contacts_server::memory::MemoryBackend>,
    filenode: Arc<jmap_filenode_server::memory::MemoryBackend>,
    sharing: Arc<jmap_sharing_server::memory::MemoryBackend>,
    metadata: Arc<jmap_metadata_server::memory::MemoryBackend>,
}

/// Construct one reference [`MemoryBackend`] from each extension-server
/// crate, register the testjig's single account on it, and mount its
/// handlers on the dispatcher.
///
/// Backend lifetimes: `register_*_handlers` clones the `Arc<B>` into
/// every registered handler closure, so the backend stays alive as
/// long as the dispatcher does. The returned [`ExtensionBackends`]
/// adds a second owner so the caller can poll state directly without
/// reaching through the dispatcher.
///
/// Account registration: five of the eight reference backends
/// (`mail`, `chat`, `calendars`, `tasks`, `contacts`) expose
/// `register_account(&Id)` for explicit per-account initialisation;
/// `filenode` uses a builder-style `with_account(&str)`; `sharing`
/// and `metadata` accept a slice of account ids at construction via
/// `new_with_accounts`. The three different shapes pre-date this
/// slice — a follow-up sweep could normalise them, but `.3` does not
/// in-scope that.
fn register_all_extensions(dispatcher: &mut Dispatcher<()>) -> ExtensionBackends {
    let account = Id::from(session::ACCOUNT_ID);

    // Mail (RFC 8621): Mailbox, Thread, Email, SearchSnippet,
    // Identity, EmailSubmission, VacationResponse.
    let mail = Arc::new(jmap_mail_server::memory::MemoryBackend::new());
    mail.register_account(&account);
    jmap_mail_server::register_mail_handlers(dispatcher, Arc::clone(&mail));

    // Chat (draft-atwood-jmap-chat-00): Chat, Message, Space,
    // SpaceBan, ChatContact, ReadPosition, CustomEmoji, SpaceInvite,
    // PresenceStatus.
    let chat = Arc::new(jmap_chat_server::memory::MemoryBackend::new());
    chat.register_account(&account);
    jmap_chat_server::register_chat_handlers(dispatcher, Arc::clone(&chat));

    // Calendars (draft-ietf-jmap-calendars): Calendar, CalendarEvent,
    // Participant, ParticipantIdentity.
    let calendars = Arc::new(jmap_calendars_server::memory::MemoryBackend::new());
    calendars.register_account(&account);
    jmap_calendars_server::register_calendars_handlers(dispatcher, Arc::clone(&calendars));

    // Tasks (draft-ietf-jmap-tasks): Task, TaskList.
    let tasks = Arc::new(jmap_tasks_server::memory::MemoryBackend::new());
    tasks.register_account(&account);
    jmap_tasks_server::register_tasks_handlers(dispatcher, Arc::clone(&tasks));

    // Contacts (draft-ietf-jmap-contacts): ContactCard, AddressBook.
    let contacts = Arc::new(jmap_contacts_server::memory::MemoryBackend::new());
    contacts.register_account(&account);
    jmap_contacts_server::register_contacts_handlers(dispatcher, Arc::clone(&contacts));

    // FileNode (draft-atwood-jmap-chat-filenode-00): FileNode tree.
    // Uses a builder shape — `with_account` consumes self and returns
    // the seeded backend, which we then wrap in an Arc.
    let filenode = Arc::new(
        jmap_filenode_server::memory::MemoryBackend::new().with_account(session::ACCOUNT_ID),
    );
    jmap_filenode_server::register_filenode_handlers(dispatcher, Arc::clone(&filenode));

    // Sharing (RFC 9670): Principal, Permission.
    let sharing = Arc::new(
        jmap_sharing_server::memory::MemoryBackend::new_with_accounts(&[session::ACCOUNT_ID]),
    );
    jmap_sharing_server::register_sharing_handlers(dispatcher, Arc::clone(&sharing));

    // Metadata (draft-ietf-jmap-metadata): Metadata, Annotation.
    let metadata = Arc::new(
        jmap_metadata_server::memory::MemoryBackend::new_with_accounts(&[session::ACCOUNT_ID]),
    );
    jmap_metadata_server::register_metadata_handlers(dispatcher, Arc::clone(&metadata));

    ExtensionBackends {
        mail,
        chat,
        calendars,
        tasks,
        contacts,
        filenode,
        sharing,
        metadata,
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

    /// The bearer token every test helper attaches to outbound
    /// requests. Matches the default carried by `AppState::new`.
    const TEST_TOKEN: &str = crate::auth::DEFAULT_BEARER_TOKEN;

    /// Drive an authenticated request through the router and return
    /// `(StatusCode, parsed JSON body)`. The default `Authorization:
    /// Bearer test-token` header lets the bearer-auth middleware pass
    /// so the test exercises the route logic itself.
    async fn send(
        router: Router,
        method: http::Method,
        path: &str,
        body: Option<&str>,
    ) -> (StatusCode, Value) {
        send_with_token(router, method, path, body, Some(TEST_TOKEN)).await
    }

    /// Same as [`send`] but allows the caller to omit the bearer
    /// token (to exercise the 401 path) or supply a custom one
    /// (to exercise mismatch).
    async fn send_with_token(
        router: Router,
        method: http::Method,
        path: &str,
        body: Option<&str>,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = http::Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(tok) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {tok}"));
        }
        let req = builder
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
        // 401 responses return a non-JSON body; callers that want to
        // inspect the 401 body should consume the bytes directly. For
        // the JSON-body tests, parse and surface the value.
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
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
    /// well-formed Session object as `application/json` when the
    /// caller authenticates correctly.
    #[tokio::test]
    async fn get_session_returns_session_json_at_200() {
        let state = AppState::new();
        let router = router(state);
        let req = http::Request::builder()
            .method(http::Method::GET)
            .uri("/.well-known/jmap")
            .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
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

    /// Oracle: bd:JMAP-cf7p.6 design decision — `GET /.well-known/jmap`
    /// requires the bearer token. Anonymous discovery is not honored.
    #[tokio::test]
    async fn get_session_without_auth_returns_401() {
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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Oracle: bd:JMAP-cf7p.6 — `POST /jmap` requires the bearer
    /// token. Anonymous calls receive 401 (not a JMAP error envelope
    /// because the auth middleware fires before any JMAP machinery).
    #[tokio::test]
    async fn post_jmap_without_auth_returns_401() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {}, "c1"]]
        }))
        .unwrap();
        let (status, _) =
            send_with_token(router, http::Method::POST, "/jmap", Some(&req_body), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// Oracle: bd:JMAP-cf7p.6 — supplying the wrong bearer token
    /// also returns 401.
    #[tokio::test]
    async fn post_jmap_wrong_token_returns_401() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {}, "c1"]]
        }))
        .unwrap();
        let (status, _) = send_with_token(
            router,
            http::Method::POST,
            "/jmap",
            Some(&req_body),
            Some("wrong-token"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// Oracle: bd:JMAP-cf7p.3 — every extension's reference
    /// MemoryBackend is mounted on the dispatcher and the testjig's
    /// hardcoded account is registered on it. Sample one method
    /// from the canonical extension-server template (Mailbox/get,
    /// RFC 8621 §2.2) and confirm the dispatcher returns a
    /// well-formed /get response, not `unknownMethod` and not an
    /// `accountNotFound` error.
    #[tokio::test]
    async fn post_jmap_mailbox_get_works_against_mounted_mail_backend() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [
                ["Mailbox/get", {"accountId": session::ACCOUNT_ID, "ids": null}, "c1"]
            ]
        }))
        .unwrap();
        let (status, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
        assert_eq!(status, StatusCode::OK);
        let inv = &body["methodResponses"][0];
        assert_eq!(
            inv[0], "Mailbox/get",
            "Mailbox/get must dispatch to the mail backend"
        );
        assert_eq!(
            inv[1]["accountId"],
            session::ACCOUNT_ID,
            "successful /get response must echo accountId"
        );
        assert!(
            inv[1]["list"].is_array(),
            "successful /get response must carry a list array per RFC 8620 §5.1"
        );
    }

    /// Oracle: bd:JMAP-cf7p.3 — same as the Mailbox/get test but for
    /// Chat/get (draft-atwood-jmap-chat-00) to confirm the chat
    /// reference MemoryBackend is mounted on the same dispatcher.
    #[tokio::test]
    async fn post_jmap_chat_get_works_against_mounted_chat_backend() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:chat"],
            "methodCalls": [
                ["Chat/get", {"accountId": session::ACCOUNT_ID, "ids": null}, "c1"]
            ]
        }))
        .unwrap();
        let (status, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
        assert_eq!(status, StatusCode::OK);
        let inv = &body["methodResponses"][0];
        assert_eq!(inv[0], "Chat/get");
        assert_eq!(inv[1]["accountId"], session::ACCOUNT_ID);
        assert!(inv[1]["list"].is_array());
    }

    /// Oracle: RFC 8620 §3.3 — methodCalls processed in order in a
    /// single envelope, each independently dispatched to the right
    /// extension backend. Cross-extension batches are a primary
    /// reason JMAP exists (one round trip across data types).
    #[tokio::test]
    async fn post_jmap_cross_extension_batch() {
        let state = AppState::new();
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": [
                "urn:ietf:params:jmap:core",
                "urn:ietf:params:jmap:mail",
                "urn:ietf:params:jmap:chat",
            ],
            "methodCalls": [
                ["Mailbox/get", {"accountId": session::ACCOUNT_ID, "ids": null}, "c1"],
                ["Chat/get", {"accountId": session::ACCOUNT_ID, "ids": null}, "c2"],
                ["Core/echo", {"trailing": true}, "c3"],
            ]
        }))
        .unwrap();
        let (status, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
        assert_eq!(status, StatusCode::OK);
        let calls = body["methodResponses"].as_array().unwrap();
        assert_eq!(calls.len(), 3, "all three calls must produce a response");
        assert_eq!(calls[0][0], "Mailbox/get");
        assert_eq!(calls[1][0], "Chat/get");
        assert_eq!(calls[2][0], "Core/echo");
        // Ordering: each response carries its original call_id.
        assert_eq!(calls[0][2], "c1");
        assert_eq!(calls[1][2], "c2");
        assert_eq!(calls[2][2], "c3");
        // Core/echo must echo args unchanged.
        assert_eq!(calls[2][1], json!({"trailing": true}));
    }

    /// Oracle: bd:JMAP-cf7p.3 — every one of the 8 extension-server
    /// crates has at least one `/get` method registered on the
    /// dispatcher. Probe one representative method per extension to
    /// confirm the dispatcher routes it (rather than returning
    /// `unknownMethod`). We don't assert success — we only assert
    /// the response method-name is NOT `error` with type
    /// `unknownMethod`, which would mean the handler wasn't
    /// registered. Some extensions may return method-level errors
    /// for /get with null ids on an empty store; that's still a
    /// successful dispatch.
    #[tokio::test]
    async fn post_jmap_all_eight_extension_get_methods_dispatch() {
        // (method name, capability URI) per extension.
        let probes = [
            ("Mailbox/get", "urn:ietf:params:jmap:mail"),
            ("Chat/get", "urn:ietf:params:jmap:chat"),
            ("Calendar/get", "urn:ietf:params:jmap:calendars"),
            ("Task/get", "urn:ietf:params:jmap:tasks"),
            ("ContactCard/get", "urn:ietf:params:jmap:contacts"),
            ("FileNode/get", "urn:ietf:params:jmap:filenode"),
            ("Principal/get", "urn:ietf:params:jmap:sharing"),
            ("Metadata/get", "urn:ietf:params:jmap:metadata"),
        ];
        for (method, capability) in probes {
            let state = AppState::new();
            let router = router(state);
            let req_body = serde_json::to_string(&json!({
                "using": ["urn:ietf:params:jmap:core", capability],
                "methodCalls": [[method, {"accountId": session::ACCOUNT_ID, "ids": null}, "c1"]]
            }))
            .unwrap();
            let (status, body) = send(router, http::Method::POST, "/jmap", Some(&req_body)).await;
            assert_eq!(status, StatusCode::OK, "probe {method} status");
            let inv = &body["methodResponses"][0];
            // If the handler was not registered the dispatcher would
            // emit ('error', {type: 'unknownMethod'}, call_id).
            let is_unknown = inv[0] == "error" && inv[1]["type"] == "unknownMethod";
            assert!(
                !is_unknown,
                "{method} must be registered on the dispatcher; got response: {inv}"
            );
        }
    }

    /// Oracle: bd:JMAP-cf7p.6 — a custom token configured via
    /// `AppState::with_token` is honored end-to-end (the
    /// CLI-supplied-token path that slice `.8` will surface).
    #[tokio::test]
    async fn post_jmap_custom_configured_token_authorizes() {
        let state = AppState::with_token("custom-jig-token");
        let router = router(state);
        let req_body = serde_json::to_string(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {}, "c1"]]
        }))
        .unwrap();
        let (status, body) = send_with_token(
            router,
            http::Method::POST,
            "/jmap",
            Some(&req_body),
            Some("custom-jig-token"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["methodResponses"][0][0], "Core/echo");
    }
}
