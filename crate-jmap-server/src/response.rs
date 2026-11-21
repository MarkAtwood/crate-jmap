//! HTTP response helpers for JMAP request-level errors (RFC 8620 §3.6.1) and
//! RFC 9457 problem details for any JMAP HTTP resource
//! (draft-atwood-jmap-updates-00 §7.2).

use http::{Response, StatusCode, header};

use crate::{Invocation, JmapError};
use jmap_types::MethodErrorType;
use jmap_types::problem::ProblemDetails;

/// An HTTP error response with an RFC 9457 problem details body, for any
/// JMAP HTTP resource (draft-atwood-jmap-updates-00 §7.2: the Session
/// resource, API, upload, download, event source, WebSocket opening
/// handshake and extension resources SHOULD all return one on a 4xx or
/// 5xx).
///
/// The `Content-Type` is `application/problem+json`. When the caller
/// localized `title` or `detail` from the request's Accept-Language
/// (see [`JmapAccounts::accept_language`](crate::JmapAccounts::accept_language)),
/// pass the language as `content_language`: it labels the response with
/// `Content-Language` (RFC 8620 §3.8).
///
/// ```rust
/// use http::StatusCode;
/// use jmap_types::problem::ProblemDetails;
///
/// let resp = jmap_server::problem_response(
///     StatusCode::UNAUTHORIZED,
///     &ProblemDetails::for_status(401).with_title("Nicht autorisiert"),
///     Some("de"),
/// );
/// assert_eq!(resp.headers()["content-type"], "application/problem+json");
/// assert_eq!(resp.headers()["content-language"], "de");
/// ```
///
/// # Panics
///
/// Does not panic in practice. Serializing `details` cannot fail (it holds
/// only strings, an integer and a string-keyed map), and the response is
/// built from a typed [`StatusCode`], a constant `Content-Type` and a
/// `Content-Language` that is added only when it parses as a header value.
pub fn problem_response(
    status: StatusCode,
    details: &ProblemDetails,
    content_language: Option<&str>,
) -> Response<String> {
    // ProblemDetails holds only strings, an integer and a string-keyed map:
    // serialization cannot fail.
    let body = serde_json::to_string(details).expect("ProblemDetails is infallible");
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, jmap_types::problem::PROBLEM_JSON);
    if let Some(lang) = content_language
        && let Ok(value) = header::HeaderValue::from_str(lang)
    {
        builder = builder.header(header::CONTENT_LANGUAGE, value);
    }
    // The status and the headers set above are valid.
    builder.body(body).expect("valid status code and headers")
}

/// Wrap a method-level error as an error `Invocation` for `methodResponses`.
///
/// Per RFC 8620 §3.6.2, error invocations always use `"error"` as the method
/// name regardless of the original method.  Only `call_id` is echoed.
/// Method-level errors are returned inside `methodResponses` with HTTP 200 —
/// they are NOT returned as top-level HTTP errors.
///
/// # Panics
///
/// Panics if `err` does not serialize to JSON. The derived `Serialize` for
/// [`JmapError`] writes only a type string, optional string and integer
/// fields and a string-keyed map of extra properties, none of which can
/// fail, so this does not happen; the test
/// `jmap_error_all_constructors_serialize` covers every constructor. A panic
/// here would mean a change to [`JmapError`] broke that invariant.
pub fn error_invocation(call_id: &str, err: JmapError) -> Invocation {
    // JmapError uses #[derive(Serialize)] with only String, Option<String>,
    // and Option<Id> fields — all JSON-serializable primitives with string
    // keys. serde_json::to_value only fails when a Serialize impl produces a
    // non-string map key; the derived impl for JmapError cannot do this. The
    // exhaustive contract test `jmap_error_all_constructors_serialize` in
    // this module exercises every public JmapError constructor and proves
    // the invariant.
    //
    // bd:JMAP-jfia.35 — previously this was an `unwrap_or_else` fallback to
    // a generic serverFail. That fallback was actively harmful: a future
    // jmap-types change that broke the Serialize invariant would silently
    // rewrite the original error to "internal error" with no log signal.
    // `.expect()` instead lets a panic surface, get caught by the
    // dispatcher's `task::spawn` isolation, and produce a serverFail
    // invocation with the failing-variant identity visible in the panic
    // payload. Louder is safer than silent corruption.
    let err_value = serde_json::to_value(&err)
        .expect("JmapError Serialize invariant — see jmap_error_all_constructors_serialize");
    Invocation::new("error", err_value, call_id)
}

/// Map a [`JmapError`] type string to the appropriate HTTP status code.
///
/// Error type strings are per RFC 8620 §7.1.
///
/// # Request-level errors only
///
/// Only request-level errors should flow through this function.  Method-level
/// errors (`accountNotFound`, `notFound`, `unknownMethod`, etc.) belong in
/// `methodResponses` at HTTP 200 via [`error_invocation`] — they must never
/// reach `error_status`.  Passing a method-level error here is a caller bug;
/// the catch-all maps unrecognized types to 500 rather than silently returning
/// a wrong status code.
///
/// Request-level error types (safe to pass here): `notJSON`, `notRequest`,
/// `limit`, `unknownCapability`, `invalidArguments`, `requestTooLarge`,
/// `forbidden`, `serverFail`, `serverUnavailable`.
pub fn error_status(err: &JmapError) -> StatusCode {
    match err.error_type {
        // RFC 8620 §3.6.1 request-level errors → 400.
        MethodErrorType::NotJson
        | MethodErrorType::NotRequest
        | MethodErrorType::Limit
        | MethodErrorType::UnknownCapability
        | MethodErrorType::InvalidArguments
        | MethodErrorType::RequestTooLarge => StatusCode::BAD_REQUEST,
        MethodErrorType::Forbidden => StatusCode::FORBIDDEN,
        MethodErrorType::ServerFail => StatusCode::INTERNAL_SERVER_ERROR,
        MethodErrorType::ServerUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        // Any unrecognized type is an internal bug, not a client error.
        // The most common mistake is passing a method-level error (e.g. "accountNotFound",
        // "notFound") to request_error() — those must stay in methodResponses at HTTP 200
        // via error_invocation() per RFC 8620 §3.6.2.
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A request-level JMAP error response: HTTP status code + JMAP error body.
///
/// Used when an error occurs before method dispatch (e.g., parse failure,
/// unknown capability).  Derives the HTTP status from the error type via
/// [`error_status`].  Use [`request_error`] to construct.
///
/// Call [`RequestError::into_response`] to produce an `http::Response<String>`
/// with the RFC 9457 problem details body.  Any HTTP framework that works with
/// the `http` crate (axum, hyper, warp, etc.) accepts this directly.
#[derive(Debug)]
// bd:JMAP-wlip.31 — both fields are private today, so #[non_exhaustive]
// is functionally a no-op for outside-crate construction / matching.
// Kept deliberately as a forward-compat signal: a future field that
// becomes `pub` (e.g. an `extras` map paralleling SetError.extra) does
// NOT then need to add the attribute as a separate change. The
// attribute also documents intent at the type-definition site without
// requiring readers to chase down the field visibility.
#[non_exhaustive]
pub struct RequestError {
    status: StatusCode,
    err: JmapError,
}

impl std::fmt::Display for RequestError {
    /// Render as `"<status>: <error_type>[: <description>]"` for log/diagnostic
    /// output. The HTTP status is included because [`RequestError`] is the
    /// request-level error type and the status is the most actionable field
    /// for operators.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.err.description.as_deref() {
            Some(desc) => write!(
                f,
                "{}: {}: {}",
                self.status.as_u16(),
                self.err.error_type,
                desc
            ),
            None => write!(f, "{}: {}", self.status.as_u16(), self.err.error_type),
        }
    }
}

impl std::error::Error for RequestError {}

impl RequestError {
    /// The RFC 9457 problem details of this error (RFC 8620 §3.6.1): type
    /// `urn:ietf:params:jmap:error:<type>`, `status`, `detail` from the
    /// description, and `limit` for `limit` errors. Add a localized `title`
    /// and send it with [`problem_response`].
    pub fn problem_details(&self) -> ProblemDetails {
        ProblemDetails::for_jmap_error(&self.err, self.status.as_u16())
    }

    /// The HTTP status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Convert into an HTTP response with an RFC 9457 problem details body
    /// ([`Self::problem_details`], [`problem_response`]) and no
    /// `Content-Language`.
    pub fn into_response(self) -> Response<String> {
        problem_response(self.status, &self.problem_details(), None)
    }
}

/// Convenience constructor: wrap a [`JmapError`] in a [`RequestError`],
/// deriving the HTTP status code automatically.
///
/// # Request-level errors only
///
/// Pass only request-level errors (see [`error_status`] for the full list).
/// Method-level errors must go through [`error_invocation`] instead.
pub fn request_error(err: JmapError) -> RequestError {
    let status = error_status(&err);
    RequestError { status, err }
}

impl From<JmapError> for RequestError {
    /// Convert a [`JmapError`] into a [`RequestError`], deriving the HTTP
    /// status code automatically via [`error_status`].
    ///
    /// Enables `?` propagation in functions returning `Result<_, RequestError>`.
    /// Pass only request-level errors; see [`error_status`] for the safe list.
    fn from(err: JmapError) -> Self {
        request_error(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;

    // -----------------------------------------------------------------------
    // error_invocation
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §3.6.2 — error invocations must use the literal method name "error".
    /// The call_id must be echoed from the request.
    #[test]
    fn error_invocation_structure() {
        let inv = error_invocation("c0", JmapError::unknown_method());
        assert_eq!(inv.name, "error");
        assert_eq!(inv.call_id, "c0");
    }

    /// Oracle: RFC 8620 §7.1 — error args object must have a "type" field.
    #[test]
    fn error_invocation_args_contains_type() {
        let inv = error_invocation("c0", JmapError::unknown_method());
        // inv.arguments is already a serde_json::Value — index directly.
        assert_eq!(inv.arguments["type"], "unknownMethod");
    }

    /// Oracle: RFC 8620 §7.1 — serverFail error type string and description field.
    #[test]
    fn error_invocation_server_fail() {
        let inv = error_invocation("y", JmapError::server_fail("boom"));
        assert_eq!(inv.arguments["type"], "serverFail");
        assert_eq!(inv.arguments["description"], "boom");
    }

    // -----------------------------------------------------------------------
    // error_status
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §3.6.1 — unknownCapability is a request-level error → 400.
    #[test]
    fn error_status_unknown_capability_is_400() {
        let e: JmapError =
            serde_json::from_value(serde_json::json!({"type": "unknownCapability"})).unwrap();
        assert_eq!(error_status(&e), StatusCode::BAD_REQUEST);
    }

    /// Oracle: RFC 8620 §7.1 — invalidArguments → 400.
    #[test]
    fn error_status_invalid_arguments_is_400() {
        assert_eq!(
            error_status(&JmapError::invalid_arguments("x")),
            StatusCode::BAD_REQUEST
        );
    }

    /// Oracle: RFC 8620 §3.6.1 limit concept — requestTooLarge → 400.
    #[test]
    fn error_status_request_too_large_is_400() {
        assert_eq!(
            error_status(&JmapError::request_too_large()),
            StatusCode::BAD_REQUEST
        );
    }

    /// Oracle: RFC 8620 §7.1 — forbidden → HTTP 403.
    #[test]
    fn error_status_forbidden_is_403() {
        assert_eq!(error_status(&JmapError::forbidden()), StatusCode::FORBIDDEN);
    }

    /// Oracle: RFC 8620 §3.6.1 — accountNotFound is method-level (stays HTTP 200 in
    /// methodResponses).  Passing it to error_status is a caller bug; the catch-all
    /// maps it to 500 rather than silently returning a wrong HTTP status.
    #[test]
    fn error_status_account_not_found_is_500() {
        assert_eq!(
            error_status(&JmapError::account_not_found()),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    /// Oracle: RFC 8620 §7.1 — serverFail → HTTP 500.
    #[test]
    fn error_status_server_fail_is_500() {
        assert_eq!(
            error_status(&JmapError::server_fail("x")),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    /// Oracle: unknown error types are server-side bugs, not client mistakes → 500.
    #[test]
    fn error_status_unknown_type_is_500() {
        let e: JmapError =
            serde_json::from_value(serde_json::json!({"type": "totallyMadeUp"})).unwrap();
        assert_eq!(error_status(&e), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // -----------------------------------------------------------------------
    // RequestError / request_error
    // -----------------------------------------------------------------------

    /// Oracle: request_error calls error_status to derive the HTTP status code.
    #[test]
    fn request_error_derives_status() {
        let re = request_error(JmapError::invalid_arguments("bad"));
        assert_eq!(re.into_response().status(), StatusCode::BAD_REQUEST);
    }

    /// Oracle: IntoResponse for RequestError must set HTTP status from the contained StatusCode.
    #[test]
    fn request_error_into_response_status_code() {
        let re = request_error(JmapError::invalid_arguments("bad"));
        let resp = re.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// Oracle: RFC 8620 §3.6.1 + RFC 9457 §3 — Content-Type must be application/problem+json.
    #[test]
    fn request_error_content_type_is_problem_json() {
        let re = request_error(JmapError::not_request());
        let resp = re.into_response();
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/problem+json"),
            "Content-Type must be application/problem+json per RFC 9457"
        );
    }

    /// Oracle: RFC 8620 §3.6.1 — type field must be a full URN.
    #[test]
    fn request_error_type_is_full_urn() {
        let body: serde_json::Value = serde_json::from_str(
            &request_error(JmapError::not_request())
                .into_response()
                .into_body(),
        )
        .unwrap();
        assert_eq!(
            body["type"], "urn:ietf:params:jmap:error:notRequest",
            "type must be full URN"
        );
    }

    /// Oracle: RFC 9457 §3.1.2 — status field must equal the HTTP status code.
    #[test]
    fn request_error_status_field_matches_http_status() {
        let body: serde_json::Value = serde_json::from_str(
            &request_error(JmapError::not_request())
                .into_response()
                .into_body(),
        )
        .unwrap();
        assert_eq!(body["status"], 400, "status field must match HTTP code");
    }

    /// Oracle: RFC 8620 §3.6.1 — limit errors MUST include "limit" property.
    #[test]
    fn request_error_limit_includes_limit_property() {
        let body: serde_json::Value = serde_json::from_str(
            &request_error(JmapError::limit("maxCallsInRequest"))
                .into_response()
                .into_body(),
        )
        .unwrap();
        assert_eq!(
            body["limit"], "maxCallsInRequest",
            "limit property must name the exceeded limit"
        );
        assert_eq!(body["type"], "urn:ietf:params:jmap:error:limit");
    }

    /// Oracle: RFC 8620 §3.6.1 ("limit" property naming the limit) and
    /// RFC 9457 §3.1.4 ("detail"). A limit error with a description carries
    /// both. Expected object is hand-written.
    #[test]
    fn request_error_limit_with_description_includes_limit_and_detail() {
        let mut err = JmapError::limit("maxCallsInRequest");
        err.description = Some("17 calls exceed 16".to_owned());
        let body: serde_json::Value =
            serde_json::from_str(&request_error(err).into_response().into_body()).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "type": "urn:ietf:params:jmap:error:limit",
                "status": 400,
                "limit": "maxCallsInRequest",
                "detail": "17 calls exceed 16",
            })
        );
    }

    // -----------------------------------------------------------------------
    // JmapError serialization invariant (guards the .expect() in error_invocation)
    // -----------------------------------------------------------------------

    /// Oracle: error_invocation depends on JmapError being infallibly serializable.
    /// This test exercises every JmapError constructor to catch any future regression
    /// in jmap-types that breaks the invariant.
    #[test]
    fn jmap_error_all_constructors_serialize() {
        let errors = vec![
            JmapError::not_json(),
            JmapError::not_request(),
            JmapError::limit("maxCallsInRequest"),
            JmapError::unknown_capability_with_detail("urn:example:unknown"),
            JmapError::forbidden(),
            JmapError::server_fail("test"),
            JmapError::server_unavailable(),
            JmapError::server_partial_fail(),
            JmapError::unknown_method(),
            JmapError::invalid_arguments("x"),
            JmapError::invalid_result_reference(),
            JmapError::account_not_found(),
            JmapError::account_not_supported_by_method(),
            JmapError::account_read_only(),
            JmapError::request_too_large(),
            JmapError::too_large(),
            JmapError::rate_limit(),
            JmapError::state_mismatch(),
            JmapError::cannot_calculate_changes(),
            JmapError::anchor_not_found(),
            JmapError::unsupported_sort(),
            JmapError::unsupported_filter(),
            JmapError::too_many_changes_with_limit(100),
            JmapError::from_account_not_found(),
            JmapError::from_account_not_supported_by_method(),
            JmapError::custom("customErrorType"),
        ];
        for err in &errors {
            let v = serde_json::to_value(err);
            assert!(v.is_ok(), "JmapError variant failed to serialize: {err:?}");
        }
    }

    // -----------------------------------------------------------------------
    // From<JmapError> for RequestError
    // -----------------------------------------------------------------------

    /// Oracle: From<JmapError> must produce the same result as request_error().
    #[test]
    fn from_jmap_error_matches_request_error() {
        let via_from: RequestError = JmapError::invalid_arguments("x").into();
        let via_fn = request_error(JmapError::invalid_arguments("x"));
        assert_eq!(
            via_from.into_response().status(),
            via_fn.into_response().status()
        );
    }
}
