//! HTTP response helpers for JMAP request-level errors (RFC 8620 §3.6.1, RFC 7807).

use http::{header, Response, StatusCode};
use serde::Serialize;

use crate::{Invocation, JmapError};

/// RFC 7807 Problem Details body for JMAP request-level errors.
///
/// `type` and `status` are always present.  `limit` is present for `limit`
/// errors (RFC 8620 §3.6.1 requires naming the exceeded limit).  `detail` is
/// present for other errors that carry a description.
#[derive(Serialize)]
struct ProblemDetails<'a> {
    #[serde(rename = "type")]
    type_urn: String,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
}

/// Wrap a method-level error as an error `Invocation` for `methodResponses`.
///
/// Per RFC 8620 §3.6.2, error invocations always use `"error"` as the method
/// name regardless of the original method.  Only `call_id` is echoed.
/// Method-level errors are returned inside `methodResponses` with HTTP 200 —
/// they are NOT returned as top-level HTTP errors.
pub fn error_invocation(call_id: &str, err: JmapError) -> Invocation {
    // JmapError uses #[derive(Serialize)] with only String, Option<String>, and
    // Option<Id> fields — all JSON-serializable primitives with string keys.
    // serde_json::to_value only fails when a Serialize impl produces a non-string
    // map key; the derived impl for JmapError cannot do this.  The fallback is a
    // defensive measure against a future jmap-types change that breaks the invariant;
    // it prevents a panic at the cost of slightly less specific error information.
    let err_value = serde_json::to_value(&err).unwrap_or_else(
        |_| serde_json::json!({"type": "serverFail", "description": "internal error"}),
    );
    ("error".to_owned(), err_value, call_id.to_owned())
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
    match err.error_type.as_str() {
        // RFC 8620 §3.6.1 request-level errors → 400.
        "notJSON" | "notRequest" | "limit" | "unknownCapability" | "invalidArguments"
        | "requestTooLarge" => StatusCode::BAD_REQUEST,
        "forbidden" => StatusCode::FORBIDDEN,
        "serverFail" => StatusCode::INTERNAL_SERVER_ERROR,
        "serverUnavailable" => StatusCode::SERVICE_UNAVAILABLE,
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
/// with the RFC 7807 Problem Details body.  Any HTTP framework that works with
/// the `http` crate (axum, hyper, warp, etc.) accepts this directly.
#[derive(Debug)]
pub struct RequestError {
    status: StatusCode,
    err: JmapError,
}

impl RequestError {
    /// Convert into an HTTP response with an RFC 7807 Problem Details body.
    ///
    /// The `Content-Type` is `application/problem+json`.  The body is a JSON
    /// object with at minimum `"type"` (full URN) and `"status"` fields per
    /// RFC 7807 §3.1, plus `"limit"` for `limit` errors (RFC 8620 §3.6.1).
    pub fn into_response(self) -> Response<String> {
        let status = self.status;
        let err = self.err;
        // RFC 8620 §3.6.1 requires RFC 7807 Problem Details format with full URN type.
        // For "limit" errors, RFC 8620 §3.6.1 REQUIRES a "limit" property naming
        // the exceeded limit.  By convention (see JmapError::limit()), the limit
        // name is stored in the description field.  Use JmapError::limit(name) —
        // never set error_type = "limit" manually — to ensure this invariant holds.
        let (limit, detail) = if err.error_type == "limit" {
            (Some(err.description.as_deref().unwrap_or("unknown")), None)
        } else {
            (None, err.description.as_deref())
        };
        let details = ProblemDetails {
            type_urn: format!("urn:ietf:params:jmap:error:{}", err.error_type),
            status: status.as_u16(),
            limit,
            detail,
        };
        // ProblemDetails only contains String, u16, and Option<&str> fields —
        // all JSON-serializable; to_json() cannot fail here.
        let body = serde_json::to_string(&details).expect("ProblemDetails is infallible");
        // Builder only fails for invalid status codes or header values; both are
        // controlled here and known-valid, so this cannot panic.
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/problem+json")
            .body(body)
            .expect("valid status code and Content-Type header")
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
    use crate::Id;
    use http::StatusCode;

    // -----------------------------------------------------------------------
    // error_invocation
    // -----------------------------------------------------------------------

    /// Oracle: RFC 8620 §3.6.2 — error invocations must use the literal method name "error".
    /// The call_id must be echoed from the request.
    #[test]
    fn error_invocation_structure() {
        let inv = error_invocation("c0", JmapError::unknown_method());
        assert_eq!(inv.0, "error");
        assert_eq!(inv.2, "c0");
    }

    /// Oracle: RFC 8620 §7.1 — error args object must have a "type" field.
    #[test]
    fn error_invocation_args_contains_type() {
        let inv = error_invocation("c0", JmapError::unknown_method());
        // inv.1 is already a serde_json::Value — index directly.
        assert_eq!(inv.1["type"], "unknownMethod");
    }

    /// Oracle: RFC 8620 §7.1 — serverFail error type string and description field.
    #[test]
    fn error_invocation_server_fail() {
        let inv = error_invocation("y", JmapError::server_fail("boom"));
        assert_eq!(inv.1["type"], "serverFail");
        assert_eq!(inv.1["description"], "boom");
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

    /// Oracle: RFC 8620 §3.6.1 + RFC 7807 — Content-Type must be application/problem+json.
    #[test]
    fn request_error_content_type_is_problem_json() {
        let re = request_error(JmapError::not_request());
        let resp = re.into_response();
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/problem+json"),
            "Content-Type must be application/problem+json per RFC 7807"
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

    /// Oracle: RFC 7807 §3.1 — status field must equal the HTTP status code.
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
            JmapError::unknown_capability(),
            JmapError::forbidden(),
            JmapError::server_fail("test"),
            JmapError::server_unavailable(),
            JmapError::server_partial_fail(),
            JmapError::unknown_method(),
            JmapError::invalid_arguments("x"),
            JmapError::invalid_result_reference(),
            JmapError::not_found(),
            JmapError::account_not_found(),
            JmapError::account_not_supported_by_method(),
            JmapError::account_read_only(),
            JmapError::request_too_large(),
            JmapError::singleton(),
            JmapError::will_destroy(),
            JmapError::invalid_patch(),
            JmapError::invalid_properties(),
            JmapError::too_large(),
            JmapError::rate_limit(),
            JmapError::over_quota(),
            JmapError::state_mismatch(),
            JmapError::cannot_calculate_changes(),
            JmapError::anchor_not_found(),
            JmapError::unsupported_sort(),
            JmapError::unsupported_filter(),
            JmapError::too_many_changes(),
            JmapError::from_account_not_found(),
            JmapError::from_account_not_supported_by_method(),
            JmapError::already_exists(Id::from("existing-1")),
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
