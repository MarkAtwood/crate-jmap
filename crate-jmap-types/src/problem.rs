//! RFC 9457 problem details, the error body of every JMAP HTTP resource.
//!
//! RFC 8620 §3.6.1 sends request-level errors as problem details (citing
//! RFC 7807, which RFC 9457 obsoletes). draft-atwood-jmap-updates-00 §7.2
//! asks that every JMAP HTTP resource do the same on a 4xx or 5xx: the
//! Session resource, the API, upload, download, the event source, the
//! WebSocket opening handshake, and the resources extensions add. Types
//! that JMAP registers carry the `urn:ietf:params:jmap:error:` prefix
//! (RFC 8620 §9.5); a problem with no specific type uses `about:blank`
//! (RFC 9457 §4.2.1).
//!
//! `title` and `detail` are display text: a server MAY localize them
//! from Accept-Language and then labels the response with
//! Content-Language. Clients act on [`ProblemDetails::problem_type`] and
//! the HTTP status, never on `title` or `detail`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{JmapError, MethodErrorType};

/// The media type of a problem details body (RFC 9457 §3).
pub const PROBLEM_JSON: &str = "application/problem+json";

/// The prefix of JMAP problem type URIs (RFC 8620 §9.5: the JMAP Error
/// Codes registry).
pub const JMAP_ERROR_URN_PREFIX: &str = "urn:ietf:params:jmap:error:";

/// The type of a problem with no more specific type (RFC 9457 §4.2.1):
/// its `title` should be the HTTP status phrase.
pub const ABOUT_BLANK: &str = "about:blank";

/// An RFC 9457 problem details object, with the `limit` member RFC 8620
/// §3.6.1 adds for `limit` errors.
///
/// Build one with [`ProblemDetails::new`], [`ProblemDetails::for_status`]
/// or [`ProblemDetails::for_jmap_error`] and the `with_*` setters, or
/// deserialize it from a response body.
///
/// ```rust
/// use jmap_types::problem::ProblemDetails;
///
/// let p = ProblemDetails::for_jmap_error(&jmap_types::JmapError::not_json(), 400)
///     .with_title("Not JSON");
/// assert_eq!(p.problem_type, "urn:ietf:params:jmap:error:notJSON");
/// assert_eq!(p.jmap_error_type().unwrap().to_string(), "notJSON");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProblemDetails {
    /// `type`: a URI naming the problem type (RFC 9457 §3.1.1); absent
    /// on the wire means [`ABOUT_BLANK`].
    #[serde(rename = "type", default = "about_blank")]
    pub problem_type: String,
    /// `status`: the HTTP status code (RFC 9457 §3.1.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// `title`: a short summary of the problem type (RFC 9457 §3.1.3);
    /// display text, MAY be localized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `detail`: an explanation of this occurrence (RFC 9457 §3.1.4);
    /// display text, MAY be localized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// `instance`: a URI reference for this occurrence (RFC 9457 §3.1.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// `limit`: the name of the exceeded limit of a JMAP `limit` error
    /// (RFC 8620 §3.6.1, MUST for that type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    /// Extension members (RFC 9457 §3.2), e.g. RFC 8887's `requestId`.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

fn about_blank() -> String {
    ABOUT_BLANK.to_owned()
}

impl ProblemDetails {
    /// A problem of type `problem_type` with HTTP status `status`.
    pub fn new(problem_type: impl Into<String>, status: u16) -> Self {
        Self {
            problem_type: problem_type.into(),
            status: Some(status),
            title: None,
            detail: None,
            instance: None,
            limit: None,
            extra: Map::new(),
        }
    }

    /// An [`ABOUT_BLANK`] problem for an HTTP status with no more specific
    /// type, e.g. 401 or 404 (RFC 9457 §4.2.1). Set the title to the
    /// status phrase with [`Self::with_title`].
    pub fn for_status(status: u16) -> Self {
        Self::new(ABOUT_BLANK, status)
    }

    /// The problem for a JMAP request-level error (RFC 8620 §3.6.1): type
    /// `urn:ietf:params:jmap:error:<type>`, `detail` from the error's
    /// description, and `limit` from [`JmapError::limit_name`] for a
    /// `limit` error (`"unknown"` if unset, since RFC 8620 requires it).
    pub fn for_jmap_error(err: &JmapError, status: u16) -> Self {
        let mut p = Self::new(format!("{JMAP_ERROR_URN_PREFIX}{}", err.error_type), status);
        p.detail = err.description.clone();
        if err.error_type == MethodErrorType::Limit {
            p.limit = Some(
                err.limit_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
            );
        }
        p
    }

    /// Set [`title`](Self::title).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set [`detail`](Self::detail).
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Set [`instance`](Self::instance).
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// The JMAP error type when [`problem_type`](Self::problem_type) is a
    /// `urn:ietf:params:jmap:error:` URI, compared exactly
    /// (draft-atwood-jmap-updates-00 §7.1); `None` otherwise.
    pub fn jmap_error_type(&self) -> Option<MethodErrorType> {
        self.problem_type
            .strip_prefix(JMAP_ERROR_URN_PREFIX)
            .map(MethodErrorType::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// RFC 8620 §3.6.1.1 example: a `limit` problem.
    #[test]
    fn rfc8620_limit_example_round_trips() {
        let body = json!({
            "type": "urn:ietf:params:jmap:error:limit",
            "limit": "maxSizeRequest",
            "status": 400,
            "detail": "The request is larger than the server is willing to process."
        });
        let p: ProblemDetails = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(p.jmap_error_type(), Some(MethodErrorType::Limit));
        assert_eq!(p.limit.as_deref(), Some("maxSizeRequest"));
        assert_eq!(serde_json::to_value(&p).unwrap(), body);
    }

    /// RFC 9457 §3.1.1: "When this member is not present, its value is
    /// assumed to be "about:blank"."
    #[test]
    fn absent_type_is_about_blank() {
        let p: ProblemDetails = serde_json::from_value(json!({ "status": 404 })).unwrap();
        assert_eq!(p.problem_type, ABOUT_BLANK);
        assert_eq!(p.jmap_error_type(), None);
    }

    /// RFC 9457 §3.2 extension members round-trip.
    #[test]
    fn extension_members_round_trip() {
        let body = json!({ "type": "about:blank", "requestId": "r1" });
        let p: ProblemDetails = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(p.extra["requestId"], "r1");
        assert_eq!(serde_json::to_value(&p).unwrap(), body);
    }

    #[test]
    fn for_jmap_error_limit_without_name_says_unknown() {
        let mut e = JmapError::limit("x");
        e.limit_name = None;
        assert_eq!(
            ProblemDetails::for_jmap_error(&e, 400).limit.as_deref(),
            Some("unknown")
        );
    }
}
