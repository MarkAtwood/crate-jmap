//! Bearer-token authentication middleware (bd:JMAP-cf7p.6).
//!
//! The testjig protects every routed endpoint behind a hardcoded
//! bearer token (default `test-token`). Real consumers building a
//! JMAP server on the workspace's library kit bring their own auth
//! integration — OAuth, JWT, mTLS, whatever — and configure their own
//! middleware. The testjig's middleware exists only to make it
//! ergonomic to point real JMAP clients (which all expect *some*
//! credential) at the in-process jig without hand-rolling a bypass
//! for every test.
//!
//! # Token sources
//!
//! - `POST /jmap`: `Authorization: Bearer <token>` header only.
//! - `GET /.well-known/jmap`: also requires the bearer token.
//!   RFC 8620 §2 describes the Session resource as the result of an
//!   *authenticated* GET, so the discovery endpoint is not anonymous.
//!   Consumers that need anonymous discovery layer their own
//!   middleware on top of this crate's `router` and explicitly bypass
//!   it for `/.well-known/jmap`.
//! - `GET /ws` and `GET /events` (slices `.4` / `.5`): the
//!   Authorization header OR the `?token=<token>` query parameter.
//!   Browsers cannot set arbitrary headers on EventSource or
//!   WebSocket handshakes, so the query-parameter fallback is the
//!   conventional escape hatch for those transports.
//!
//! # Constant-time compare
//!
//! Token comparisons MUST use [`subtle::ConstantTimeEq::ct_eq`]
//! rather than `==`. A plain string compare short-circuits at the
//! first mismatched byte and gives an off-host attacker a byte-by-byte
//! timing oracle on the token; constant-time compare closes that
//! oracle. This matches the workspace's secret-comparison policy
//! (precedent: `crate-jmap-chat-server/src/space.rs:1100` invite-code
//! compare; bd:JMAP-sc1b.89).
//!
//! `ct_eq` returns `Choice(0)` cheaply when the byte slices differ in
//! length, so an attacker can still learn whether the supplied token
//! matches the stored length. For the hardcoded default
//! `test-token` (10 bytes) the length is public knowledge; operators
//! who configure a stronger token via CLI accept the same length-leak
//! tradeoff that the precedent's fixed-length invite codes accept.

use axum::{
    extract::{Request, State as AxumState},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// The testjig's default bearer token.
///
/// 10 bytes. Operators who want stronger credentials override this
/// via the `--token` CLI flag (slice `bd:JMAP-cf7p.8`). The default
/// is intentionally weak and human-typeable for curl-driven
/// smoke-testing; the testjig is not a production server.
pub const DEFAULT_BEARER_TOKEN: &str = "test-token";

/// Configured bearer token, stored as a byte sequence so the
/// middleware can call [`ConstantTimeEq::ct_eq`] on the raw bytes
/// without re-encoding on every request.
///
/// Wrapped in [`Arc`] so [`AuthState`] is cheap to clone — axum's
/// middleware layer requires its state to implement `Clone`, and
/// the testjig clones the state once per request handler invocation
/// inside `from_fn_with_state`.
#[derive(Clone)]
pub struct AuthState {
    token: Arc<[u8]>,
}

impl AuthState {
    /// Construct an [`AuthState`] from a token string.
    ///
    /// Empty tokens are accepted — operators who deliberately want to
    /// disable auth (for a fully-open localhost demo) can pass `""`,
    /// at which case clients still need to send `Authorization:
    /// Bearer ` (empty value) for the constant-time compare to match.
    /// There is no separate "auth disabled" branch; the middleware
    /// always runs the same compare.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: Arc::from(token.into().into_bytes()),
        }
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new(DEFAULT_BEARER_TOKEN)
    }
}

/// Axum middleware that gates every protected route behind a
/// constant-time bearer-token check.
///
/// The middleware runs in three phases:
///
/// 1. Try to extract the bearer token from the `Authorization`
///    header. Header parsing failures (non-UTF-8, missing `Bearer `
///    prefix) fall through to phase 2 rather than rejecting outright;
///    the query-string fallback is the alternative for SSE / WS.
/// 2. If the header did not supply a token, parse the query string
///    for `token=<value>`. SSE and WS handshakes use this path.
/// 3. Constant-time compare the supplied token (or empty bytes, if
///    no token was supplied) against the configured token. Mismatch
///    returns HTTP 401 with `WWW-Authenticate: Bearer realm="jmap"`
///    per RFC 6750 §3.
///
/// The middleware does NOT consult the request URI to special-case
/// `/.well-known/jmap` or any other route — every route reachable
/// through [`crate::http::router`] requires the same token. Slice
/// `.4` (SSE) and slice `.5` (WS) inherit this middleware for free.
pub async fn require_bearer_token(
    AxumState(state): AxumState<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    let supplied = extract_token(&request);

    // `ct_eq` returns a `subtle::Choice` whose conversion to `bool`
    // also runs in constant time. Diverging on the boolean afterwards
    // is fine — the only secret-dependent operation (the byte compare)
    // is already done in constant time.
    if state.token.ct_eq(&supplied).into() {
        next.run(request).await
    } else {
        unauthorized()
    }
}

/// Pull the bearer token from the request, preferring the
/// `Authorization` header and falling back to the `?token=` query
/// parameter.
///
/// Returns the raw bytes the caller supplied, or an empty `Vec` if
/// no token was found. The empty fallback ensures the downstream
/// `ct_eq` always has well-defined inputs — it never short-circuits
/// on `None`, which would re-introduce a timing oracle on "header
/// present vs absent".
fn extract_token(request: &Request) -> Vec<u8> {
    if let Some(header_value) = request.headers().get(header::AUTHORIZATION) {
        if let Some(token) = parse_bearer_header(header_value) {
            return token;
        }
    }
    parse_query_token(request).unwrap_or_default()
}

/// Parse `Authorization: Bearer <token>` and return the token bytes.
///
/// Returns `None` if the header value is not valid UTF-8, does not
/// start with the case-insensitive `Bearer ` scheme prefix, or has no
/// token following the prefix. The Bearer scheme name is matched
/// case-insensitively per RFC 6750 §2.1.
fn parse_bearer_header(value: &HeaderValue) -> Option<Vec<u8>> {
    let s = value.to_str().ok()?;
    // RFC 6750 §2.1: scheme is case-insensitive; token follows a
    // single space. We accept leading whitespace tolerantly because
    // some client libraries pad the header.
    let trimmed = s.trim_start();
    let rest = trimmed.strip_prefix("Bearer ").or_else(|| {
        // Case-insensitive fallback. Doing the cheap exact match
        // first keeps the common-case fast.
        if trimmed.len() < 7 {
            None
        } else {
            let (scheme, rest) = trimmed.split_at(7);
            if scheme.eq_ignore_ascii_case("Bearer ") {
                Some(rest)
            } else {
                None
            }
        }
    })?;
    let token = rest.trim_start();
    if token.is_empty() {
        None
    } else {
        Some(token.as_bytes().to_vec())
    }
}

/// Best-effort parse of `?token=...` from the request URI.
///
/// Returns `None` if the query string is absent or has no `token=`
/// parameter. Empty `token=` (i.e. `?token=`) yields
/// `Some(Vec::new())`, which still goes through `ct_eq` against the
/// configured token; a non-empty configured token will not match the
/// empty bytes.
///
/// Hand-rolled rather than using `serde_urlencoded` (transitive via
/// axum) so the testjig does not take a direct dep on a serde adapter
/// just to read one optional string field. Honors
/// application/x-www-form-urlencoded percent-decoding for the
/// `token` value; malformed percent-encoding (`%` not followed by two
/// hex digits, or non-UTF-8 byte sequences) is treated as "no token
/// supplied" and falls through to the `ct_eq` mismatch path.
fn parse_query_token(request: &Request) -> Option<Vec<u8>> {
    let query = request.uri().query()?;
    for pair in query.split('&') {
        // Skip empty pairs (e.g. trailing `&` or `&&`).
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        if key == "token" {
            // application/x-www-form-urlencoded: `+` decodes to space
            // before percent-decoding per the URL Living Standard
            // §application/x-www-form-urlencoded.
            let value_pluses = value.replace('+', " ");
            let decoded = percent_decode(value_pluses.as_bytes())?;
            return Some(decoded);
        }
    }
    None
}

/// Decode `%HH` sequences in a byte slice into the corresponding
/// raw bytes. Returns `None` if a `%` is not followed by exactly
/// two ASCII hex digits.
fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            if i + 2 >= input.len() {
                return None;
            }
            let hi = hex_digit(input[i + 1])?;
            let lo = hex_digit(input[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Build the canonical 401 response with the RFC 6750 §3 challenge.
fn unauthorized() -> Response {
    let mut resp = (
        StatusCode::UNAUTHORIZED,
        "missing or invalid bearer token\n",
    )
        .into_response();
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        // Static realm; sufficient for the test jig's purposes.
        HeaderValue::from_static("Bearer realm=\"jmap\""),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body as AxumBody;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    fn protected_router(state: AuthState) -> Router {
        Router::new()
            .route("/protected", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn_with_state(
                state,
                require_bearer_token,
            ))
    }

    /// Oracle: RFC 6750 §3 — a request to a protected resource that
    /// supplies no Authorization header must receive 401 Unauthorized
    /// with a `WWW-Authenticate: Bearer ...` challenge.
    #[tokio::test]
    async fn no_auth_header_returns_401() {
        let app = protected_router(AuthState::new("expected"));
        let req = http::Request::builder()
            .uri("/protected")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("Bearer "));
    }

    /// Oracle: the configured token authenticates successfully via
    /// the `Authorization: Bearer ...` header.
    #[tokio::test]
    async fn correct_token_via_header_passes() {
        let app = protected_router(AuthState::new("expected-token"));
        let req = http::Request::builder()
            .uri("/protected")
            .header(header::AUTHORIZATION, "Bearer expected-token")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Oracle: a non-matching token returns 401 even with the right
    /// scheme name. This catches simple typo-style attacks.
    #[tokio::test]
    async fn wrong_token_via_header_returns_401() {
        let app = protected_router(AuthState::new("expected-token"));
        let req = http::Request::builder()
            .uri("/protected")
            .header(header::AUTHORIZATION, "Bearer wrong-token")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Oracle: RFC 6750 §2.1 — Bearer scheme name is case-insensitive.
    /// The middleware must accept `bearer`, `BEARER`, etc.
    #[tokio::test]
    async fn bearer_scheme_is_case_insensitive() {
        for variant in ["Bearer", "bearer", "BEARER", "BeArEr"] {
            let app = protected_router(AuthState::new("tok"));
            let req = http::Request::builder()
                .uri("/protected")
                .header(header::AUTHORIZATION, format!("{variant} tok"))
                .body(AxumBody::empty())
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "Bearer scheme must be case-insensitive per RFC 6750 §2.1 ({variant})"
            );
        }
    }

    /// Oracle: a request that uses a different auth scheme (Basic,
    /// Digest, etc.) must be rejected — the testjig only honors
    /// Bearer.
    #[tokio::test]
    async fn non_bearer_scheme_returns_401() {
        let app = protected_router(AuthState::new("tok"));
        let req = http::Request::builder()
            .uri("/protected")
            .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Oracle: the `?token=` query-string fallback authenticates
    /// when the Authorization header is absent. This is the browser
    /// SSE / WS path (slices `.4` / `.5`).
    #[tokio::test]
    async fn correct_token_via_query_passes() {
        let app = protected_router(AuthState::new("expected-token"));
        let req = http::Request::builder()
            .uri("/protected?token=expected-token")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Oracle: a mismatched query-string token returns 401.
    #[tokio::test]
    async fn wrong_token_via_query_returns_401() {
        let app = protected_router(AuthState::new("expected-token"));
        let req = http::Request::builder()
            .uri("/protected?token=wrong")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Oracle: the Authorization header MUST take precedence when
    /// both header and query are present. This avoids ambiguous
    /// behavior when a client supplies both (rare but possible
    /// during SSE reconnect with a header-setting library).
    #[tokio::test]
    async fn header_beats_query_when_header_valid() {
        let app = protected_router(AuthState::new("correct"));
        let req = http::Request::builder()
            .uri("/protected?token=wrong-from-query")
            .header(header::AUTHORIZATION, "Bearer correct")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "correct token in header must authorize even when query token is wrong"
        );
    }

    /// Oracle: when the header is unparseable (no Bearer prefix), the
    /// middleware MUST fall back to the query string rather than
    /// rejecting outright. This lets SSE / WS browsers that send a
    /// non-Bearer cookie header still authenticate via `?token=`.
    #[tokio::test]
    async fn unparseable_header_falls_through_to_query() {
        let app = protected_router(AuthState::new("correct"));
        let req = http::Request::builder()
            .uri("/protected?token=correct")
            .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Oracle: `Authorization: Bearer ` with no token returns 401.
    #[tokio::test]
    async fn empty_bearer_token_returns_401() {
        let app = protected_router(AuthState::new("nonempty"));
        let req = http::Request::builder()
            .uri("/protected")
            .header(header::AUTHORIZATION, "Bearer ")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Oracle: configuring an empty token authorizes a client that
    /// sends `Bearer ` with no value. The middleware does not have a
    /// special "disabled" branch — empty configured tokens just
    /// compare equal to empty supplied tokens.
    #[tokio::test]
    async fn empty_configured_token_round_trips() {
        let app = protected_router(AuthState::new(""));
        // Empty configured token matches empty supplied token sent via
        // the query fallback (header would be rejected by the
        // "no token after Bearer" check, by design — operators who
        // configure empty tokens are using the query path).
        let req = http::Request::builder()
            .uri("/protected?token=")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Oracle: query strings with malformed percent-encoding return
    /// 401 (the middleware treats parse failure as "no token
    /// supplied" and then mismatches).
    #[tokio::test]
    async fn malformed_query_returns_401() {
        let app = protected_router(AuthState::new("tok"));
        let req = http::Request::builder()
            // `%` without two hex digits is invalid percent-encoding.
            .uri("/protected?token=%ZZ")
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Oracle: the default AuthState honors the DEFAULT_BEARER_TOKEN
    /// constant. Curl-driven smoke tests rely on this.
    #[tokio::test]
    async fn default_token_authorizes_default_state() {
        let app = protected_router(AuthState::default());
        let req = http::Request::builder()
            .uri("/protected")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {DEFAULT_BEARER_TOKEN}"),
            )
            .body(AxumBody::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
