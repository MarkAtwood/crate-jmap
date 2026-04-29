use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

use crate::{Invocation, JmapError};

/// Wrap a method-level error as an error `Invocation` for `methodResponses`.
///
/// Per RFC 8620 §3.6.2, error invocations always use `"error"` as the method
/// name regardless of the original method.  Only `call_id` is echoed.
/// Method-level errors are returned inside `methodResponses` with HTTP 200 —
/// they are NOT returned as top-level HTTP errors.
pub fn error_invocation(call_id: &str, err: JmapError) -> Invocation {
    // JmapError derives Serialize with String/Option<String> fields only;
    // serde_json::to_value cannot fail for this type.
    let err_value = serde_json::to_value(&err).expect("JmapError::Serialize is infallible");
    ("error".to_owned(), err_value, call_id.to_owned())
}

/// Map a [`JmapError`] type string to the appropriate HTTP status code.
///
/// Error type strings are per RFC 8620 §7.1.  Only request-level errors should
/// flow through here; method-level errors stay inside `methodResponses` at HTTP
/// 200 and never reach this function.
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
#[derive(Debug)]
pub struct RequestError {
    status: StatusCode,
    err: JmapError,
}

impl IntoResponse for RequestError {
    fn into_response(self) -> Response {
        let status = self.status;
        let err = self.err;
        // RFC 8620 §3.6.1 requires RFC 7807 Problem Details format with full URN type.
        let type_urn = format!("urn:ietf:params:jmap:error:{}", err.error_type);
        let mut obj = serde_json::Map::new();
        obj.insert("type".to_owned(), serde_json::Value::String(type_urn));
        obj.insert(
            "status".to_owned(),
            serde_json::Value::Number(status.as_u16().into()),
        );
        // For "limit" errors, RFC 8620 §3.6.1 REQUIRES a "limit" property naming
        // the exceeded limit.  By convention (see JmapError::limit()), the limit
        // name is stored in the description field.  Use JmapError::limit(name) —
        // never set error_type = "limit" manually — to ensure this invariant holds.
        if err.error_type == "limit" {
            let limit_name = err.description.as_deref().unwrap_or("unknown");
            obj.insert(
                "limit".to_owned(),
                serde_json::Value::String(limit_name.to_owned()),
            );
        } else if let Some(detail) = &err.description {
            obj.insert(
                "detail".to_owned(),
                serde_json::Value::String(detail.clone()),
            );
        }
        let body = serde_json::Value::Object(obj).to_string();
        (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            body,
        )
            .into_response()
    }
}

/// Convenience constructor: wrap a [`JmapError`] in a [`RequestError`],
/// deriving the HTTP status code automatically.
pub fn request_error(err: JmapError) -> RequestError {
    let status = error_status(&err);
    RequestError { status, err }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

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
    #[tokio::test]
    async fn request_error_type_is_full_urn() {
        use axum::body::to_bytes;
        let re = request_error(JmapError::not_request());
        let resp = re.into_response();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["type"], "urn:ietf:params:jmap:error:notRequest",
            "type must be full URN"
        );
    }

    /// Oracle: RFC 7807 §3.1 — status field must equal the HTTP status code.
    #[tokio::test]
    async fn request_error_status_field_matches_http_status() {
        use axum::body::to_bytes;
        let re = request_error(JmapError::not_request());
        let resp = re.into_response();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["status"], 400, "status field must match HTTP code");
    }

    /// Oracle: RFC 8620 §3.6.1 — limit errors MUST include "limit" property.
    #[tokio::test]
    async fn request_error_limit_includes_limit_property() {
        use axum::body::to_bytes;
        let re = request_error(JmapError::limit("maxCallsInRequest"));
        let resp = re.into_response();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["limit"], "maxCallsInRequest",
            "limit property must name the exceeded limit"
        );
        assert_eq!(body["type"], "urn:ietf:params:jmap:error:limit");
    }
}
