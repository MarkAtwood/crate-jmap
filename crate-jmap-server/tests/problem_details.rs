//! RFC 9457 problem details for any JMAP HTTP resource
//! (draft-atwood-jmap-updates-00 §7.2).
//!
//! Oracle: RFC 9457 §3 (media type `application/problem+json`, members
//! `type`, `status`, `title`, `detail`, `instance`), §4.2.1
//! (`about:blank`), RFC 8620 §3.6.1 (JMAP request-level types and the
//! `limit` member) and RFC 8620 §3.8 / the draft §7.2 (Content-Language
//! when `title`/`detail` are localized). Expected bodies are hand-written.

use http::StatusCode;
use jmap_server::{RequestError, problem_response, request_error};
use jmap_types::JmapError;
use jmap_types::problem::ProblemDetails;
use serde_json::{Value, json};

fn body(resp: &http::Response<String>) -> Value {
    serde_json::from_str(resp.body()).unwrap()
}

/// A non-API resource (e.g. the Session resource without credentials)
/// returns an `about:blank` problem with the status phrase as title.
#[test]
fn about_blank_for_a_plain_status() {
    let resp = problem_response(
        StatusCode::UNAUTHORIZED,
        &ProblemDetails::for_status(401).with_title("Unauthorized"),
        None,
    );
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(resp.headers()["content-type"], "application/problem+json");
    assert!(resp.headers().get("content-language").is_none());
    assert_eq!(
        body(&resp),
        json!({ "type": "about:blank", "status": 401, "title": "Unauthorized" })
    );
}

/// A localized title is labelled with Content-Language; `instance` is
/// passed through.
#[test]
fn localized_title_sets_content_language() {
    let resp = problem_response(
        StatusCode::NOT_FOUND,
        &ProblemDetails::for_status(404)
            .with_title("Introuvable")
            .with_instance("/download/a1/b1/x"),
        Some("fr"),
    );
    assert_eq!(resp.headers()["content-language"], "fr");
    assert_eq!(body(&resp)["instance"], "/download/a1/b1/x");
}

/// A Content-Language value that is not a valid header is left out
/// rather than failing the error response.
#[test]
fn invalid_content_language_is_dropped() {
    let resp = problem_response(
        StatusCode::BAD_REQUEST,
        &ProblemDetails::for_status(400),
        Some("fr\n"),
    );
    assert!(resp.headers().get("content-language").is_none());
}

/// RFC 8620 §3.6.1.1 `limit` example shape through `RequestError`; its
/// `problem_details` can take a title before sending.
#[test]
fn request_error_problem_details() {
    let err: RequestError = request_error(JmapError::limit("maxSizeRequest"));
    assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    let details = err.problem_details().with_title("Request too large");
    assert_eq!(
        serde_json::to_value(&details).unwrap(),
        json!({
            "type": "urn:ietf:params:jmap:error:limit",
            "status": 400,
            "title": "Request too large",
            "limit": "maxSizeRequest"
        })
    );
    assert_eq!(
        body(&err.into_response()),
        json!({ "type": "urn:ietf:params:jmap:error:limit", "status": 400, "limit": "maxSizeRequest" })
    );
}
