//! RFC 8621 §8 VacationResponse object.
//!
//! Provides [`VacationResponse`] — a singleton object (one per account) that
//! controls the automatic out-of-office reply behaviour for the account.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Vacation-response settings for an account (RFC 8621 §8).
///
/// There is exactly one `VacationResponse` object per account; its `id` is
/// always the string `"singleton"` (RFC 8621 §8).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VacationResponse {
    /// The object id.  Always `"singleton"` (server-set; immutable).
    pub id: Id,

    /// Whether the vacation response is currently active.
    pub is_enabled: bool,

    /// Start of the active window.  If `None`, the response is effective
    /// immediately.  Only consulted when `is_enabled` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_date: Option<UTCDate>,

    /// End of the active window.  If `None`, the response is effective
    /// indefinitely.  Only consulted when `is_enabled` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_date: Option<UTCDate>,

    /// Subject line for the auto-reply.  `None` means the server should
    /// generate a suitable subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,

    /// Plaintext body for the auto-reply.  `None` means the server may derive
    /// a text part from `html_body` or generate a default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_body: Option<String>,

    /// HTML body for the auto-reply.  `None` means the server may derive an
    /// HTML part from `text_body` or send a text-only response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html_body: Option<String>,
}

impl VacationResponse {
    /// Construct a [`VacationResponse`] from its two required fields.
    ///
    /// All optional fields default to `None`.
    pub fn new(id: impl Into<Id>, is_enabled: bool) -> Self {
        Self {
            id: id.into(),
            is_enabled,
            from_date: None,
            to_date: None,
            subject: None,
            text_body: None,
            html_body: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: hand-written fixture derived from RFC 8621 §8 field descriptions.
    /// The RFC does not provide a full JSON example; the fixture was constructed
    /// from the spec's field definitions.
    #[test]
    fn vacation_response_full_roundtrip() {
        let json = r#"{"id":"singleton","isEnabled":true,"fromDate":"2024-06-01T00:00:00Z","toDate":"2024-06-30T23:59:59Z","subject":"Out of office","textBody":"I am out of the office.","htmlBody":"<p>I am out of the office.</p>"}"#;
        let vr: VacationResponse = serde_json::from_str(json).expect("must parse");
        assert_eq!(vr.id, "singleton");
        assert!(vr.is_enabled);
        assert_eq!(
            vr.from_date.as_ref().map(|d| d.as_ref()),
            Some("2024-06-01T00:00:00Z")
        );
        assert_eq!(vr.subject.as_deref(), Some("Out of office"));
        assert_eq!(vr.text_body.as_deref(), Some("I am out of the office."));
        let back = serde_json::to_string(&vr).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: disabled vacation response with no optional fields.
    /// RFC 8621 §8 states only `id` and `isEnabled` are always present.
    #[test]
    fn vacation_response_minimal_roundtrip() {
        let json = r#"{"id":"singleton","isEnabled":false}"#;
        let vr: VacationResponse = serde_json::from_str(json).expect("must parse");
        assert_eq!(vr.id, "singleton");
        assert!(!vr.is_enabled);
        assert!(vr.from_date.is_none());
        assert!(vr.to_date.is_none());
        assert!(vr.subject.is_none());
        assert!(vr.text_body.is_none());
        assert!(vr.html_body.is_none());
        let back = serde_json::to_string(&vr).expect("serialize");
        assert_eq!(back, json);
    }

    /// Oracle: RFC 8621 §8 — optional fields are omitted from serialization
    /// when None.
    #[test]
    fn vacation_response_none_fields_omitted() {
        let vr = VacationResponse {
            id: Id::from("singleton"),
            is_enabled: false,
            from_date: None,
            to_date: None,
            subject: None,
            text_body: None,
            html_body: None,
        };
        let json = serde_json::to_string(&vr).expect("serialize");
        assert!(!json.contains("fromDate"), "fromDate must be absent");
        assert!(!json.contains("toDate"), "toDate must be absent");
        assert!(!json.contains("subject"), "subject must be absent");
        assert!(!json.contains("textBody"), "textBody must be absent");
        assert!(!json.contains("htmlBody"), "htmlBody must be absent");
    }
}
