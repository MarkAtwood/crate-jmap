use serde::{Deserialize, Serialize};
use thiserror::Error;

/// JMAP method-level error, serializable for inclusion in `methodResponses`.
///
/// See RFC 8620 §3.6.2 for the standard error type strings.
/// The JSON key is `"type"` (not `"error_type"`) per RFC 8620.
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
#[error("{error_type}")]
#[non_exhaustive]
pub struct JmapError {
    /// Error type string per RFC 8620 §3.6.2.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable description. Omitted from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl JmapError {
    // RFC 8620 §7.1 — "invalidArguments"
    pub fn invalid_arguments(desc: impl Into<String>) -> Self {
        Self {
            error_type: "invalidArguments".into(),
            description: Some(desc.into()),
        }
    }

    // RFC 8620 §7.1 — "forbidden"
    pub fn forbidden() -> Self {
        Self {
            error_type: "forbidden".into(),
            description: None,
        }
    }

    // RFC 8620 §7.1 — "notFound"
    pub fn not_found() -> Self {
        Self {
            error_type: "notFound".into(),
            description: None,
        }
    }

    // RFC 8620 §5.1 — "accountNotFound"
    pub fn account_not_found() -> Self {
        Self {
            error_type: "accountNotFound".into(),
            description: None,
        }
    }

    // RFC 8620 §5.1 — "accountNotSupportedByMethod"
    pub fn account_not_supported_by_method() -> Self {
        Self {
            error_type: "accountNotSupportedByMethod".into(),
            description: None,
        }
    }

    // RFC 8620 §5.1 — "accountReadOnly"
    pub fn account_read_only() -> Self {
        Self {
            error_type: "accountReadOnly".into(),
            description: None,
        }
    }

    // RFC 8620 §7.1 — "serverUnavailable"
    pub fn server_unavailable() -> Self {
        Self {
            error_type: "serverUnavailable".into(),
            description: None,
        }
    }

    // RFC 8620 §7.1 — "serverFail"
    pub fn server_fail(desc: impl Into<String>) -> Self {
        Self {
            error_type: "serverFail".into(),
            description: Some(desc.into()),
        }
    }

    // RFC 8620 §7.1 — "serverPartialFail"
    pub fn server_partial_fail() -> Self {
        Self {
            error_type: "serverPartialFail".into(),
            description: None,
        }
    }

    // RFC 8620 §7.1 — "unknownMethod"
    pub fn unknown_method() -> Self {
        Self {
            error_type: "unknownMethod".into(),
            description: None,
        }
    }

    // RFC 8620 §7.1 — "invalidResultReference"
    pub fn invalid_result_reference() -> Self {
        Self {
            error_type: "invalidResultReference".into(),
            description: None,
        }
    }

    // RFC 8620 §5.5 — "cannotCalculateChanges"
    pub fn cannot_calculate_changes() -> Self {
        Self {
            error_type: "cannotCalculateChanges".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "stateMismatch"
    pub fn state_mismatch() -> Self {
        Self {
            error_type: "stateMismatch".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "tooLarge"
    pub fn too_large() -> Self {
        Self {
            error_type: "tooLarge".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "requestTooLarge"
    pub fn request_too_large(desc: impl Into<String>) -> Self {
        Self {
            error_type: "requestTooLarge".into(),
            description: Some(desc.into()),
        }
    }

    // RFC 8620 §7.1 — "unknownCapability"
    pub fn unknown_capability(cap: impl Into<String>) -> Self {
        Self {
            error_type: "unknownCapability".into(),
            description: Some(cap.into()),
        }
    }

    // RFC 8620 §5.3 — "overQuota"
    pub fn over_quota() -> Self {
        Self {
            error_type: "overQuota".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "rateLimit"
    pub fn rate_limit() -> Self {
        Self {
            error_type: "rateLimit".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "invalidPatch"
    pub fn invalid_patch() -> Self {
        Self {
            error_type: "invalidPatch".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "willDestroy"
    pub fn will_destroy() -> Self {
        Self {
            error_type: "willDestroy".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "invalidProperties"
    pub fn invalid_properties() -> Self {
        Self {
            error_type: "invalidProperties".into(),
            description: None,
        }
    }

    // RFC 8620 §5.3 — "singleton"
    pub fn singleton() -> Self {
        Self {
            error_type: "singleton".into(),
            description: None,
        }
    }

    // RFC 8620 §5.5 — "unsupportedFilter"
    pub fn unsupported_filter() -> Self {
        Self {
            error_type: "unsupportedFilter".into(),
            description: None,
        }
    }

    // RFC 8620 §5.5 — "anchorNotFound"
    pub fn anchor_not_found() -> Self {
        Self {
            error_type: "anchorNotFound".into(),
            description: None,
        }
    }

    // RFC 8620 §5.4 — "alreadyExists"
    pub fn already_exists() -> Self {
        Self {
            error_type: "alreadyExists".into(),
            description: None,
        }
    }

    // RFC 8620 §5.4 — "fromAccountNotFound"
    pub fn from_account_not_found() -> Self {
        Self {
            error_type: "fromAccountNotFound".into(),
            description: None,
        }
    }

    // RFC 8620 §5.4 — "fromAccountNotSupportedByMethod"
    pub fn from_account_not_supported_by_method() -> Self {
        Self {
            error_type: "fromAccountNotSupportedByMethod".into(),
            description: None,
        }
    }

    // RFC 8620 §5.5 — "unsupportedSort"
    pub fn unsupported_sort() -> Self {
        Self {
            error_type: "unsupportedSort".into(),
            description: None,
        }
    }

    // RFC 8620 §5.6 — "tooManyChanges"
    pub fn too_many_changes() -> Self {
        Self {
            error_type: "tooManyChanges".into(),
            description: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent oracle: RFC 8620 §7.1 specifies these exact type strings.

    #[test]
    fn invalid_arguments_serializes_type_and_description() {
        let e = JmapError::invalid_arguments("ids field is required");
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains("\"type\""),
            "must use 'type' key per RFC 8620"
        );
        assert!(json.contains("\"invalidArguments\""));
        assert!(json.contains("\"description\""));
        assert!(json.contains("ids field is required"));
    }

    #[test]
    fn forbidden_omits_description() {
        let e = JmapError::forbidden();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"forbidden\""));
        assert!(
            !json.contains("\"description\""),
            "None description must be omitted"
        );
    }

    #[test]
    fn not_found_type_string() {
        let e = JmapError::not_found();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"notFound\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn account_not_found_type_string() {
        let e = JmapError::account_not_found();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"accountNotFound\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn account_not_supported_by_method_type_string() {
        let e = JmapError::account_not_supported_by_method();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"accountNotSupportedByMethod\""));
    }

    #[test]
    fn account_read_only_type_string() {
        let e = JmapError::account_read_only();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"accountReadOnly\""));
    }

    #[test]
    fn server_unavailable_type_string() {
        let e = JmapError::server_unavailable();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"serverUnavailable\""));
    }

    #[test]
    fn server_fail_includes_description() {
        let e = JmapError::server_fail("internal error");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"serverFail\""));
        assert!(json.contains("internal error"));
    }

    #[test]
    fn server_partial_fail_type_string() {
        let e = JmapError::server_partial_fail();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"serverPartialFail\""));
    }

    #[test]
    fn unknown_method_type_string() {
        let e = JmapError::unknown_method();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"unknownMethod\""));
    }

    #[test]
    fn invalid_result_reference_type_string() {
        let e = JmapError::invalid_result_reference();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"invalidResultReference\""));
    }

    #[test]
    fn cannot_calculate_changes_type_string() {
        let e = JmapError::cannot_calculate_changes();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"cannotCalculateChanges\""));
    }

    #[test]
    fn state_mismatch_type_string() {
        let e = JmapError::state_mismatch();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"stateMismatch\""));
    }

    #[test]
    fn too_large_type_string() {
        let e = JmapError::too_large();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"tooLarge\""));
    }

    #[test]
    fn request_too_large_includes_description() {
        let e = JmapError::request_too_large("body exceeds 10MB");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"requestTooLarge\""));
        assert!(json.contains("body exceeds 10MB"));
    }

    #[test]
    fn unknown_capability_includes_description() {
        let e = JmapError::unknown_capability("urn:ietf:params:jmap:mail");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"unknownCapability\""));
        assert!(json.contains("urn:ietf:params:jmap:mail"));
    }

    #[test]
    fn over_quota_type_string() {
        let e = JmapError::over_quota();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"overQuota\""));
    }

    #[test]
    fn rate_limit_type_string() {
        let e = JmapError::rate_limit();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"rateLimit\""));
    }

    #[test]
    fn invalid_patch_type_string() {
        let e = JmapError::invalid_patch();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"invalidPatch\""));
    }

    #[test]
    fn will_destroy_type_string() {
        let e = JmapError::will_destroy();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"willDestroy\""));
    }

    #[test]
    fn invalid_properties_type_string() {
        let e = JmapError::invalid_properties();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"invalidProperties\""));
    }

    #[test]
    fn singleton_type_string() {
        let e = JmapError::singleton();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"singleton\""));
    }

    #[test]
    fn unsupported_filter_type_string() {
        let e = JmapError::unsupported_filter();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"unsupportedFilter\""));
    }

    #[test]
    fn anchor_not_found_type_string() {
        let e = JmapError::anchor_not_found();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"anchorNotFound\""));
    }

    #[test]
    fn already_exists_type_string() {
        let e = JmapError::already_exists();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"alreadyExists\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn from_account_not_found_type_string() {
        let e = JmapError::from_account_not_found();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"fromAccountNotFound\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn from_account_not_supported_by_method_type_string() {
        let e = JmapError::from_account_not_supported_by_method();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"fromAccountNotSupportedByMethod\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn unsupported_sort_type_string() {
        let e = JmapError::unsupported_sort();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"unsupportedSort\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn too_many_changes_type_string() {
        let e = JmapError::too_many_changes();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"tooManyChanges\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn round_trip_deserialize() {
        // Verify the "type" rename survives a JSON round-trip.
        let original = JmapError::invalid_arguments("test");
        let json = serde_json::to_string(&original).unwrap();
        let restored: JmapError = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.error_type, "invalidArguments");
        assert_eq!(restored.description.as_deref(), Some("test"));
    }

    // Oracle: RFC 8620 §3.4.1 fixture — methodResponses[3] is ["error", {"type":"unknownMethod"}, "c3"]
    #[test]
    fn fixture_response_contains_unknown_method_error() {
        let raw = include_str!("../tests/fixtures/rfc8620-response.json");
        let v: serde_json::Value = serde_json::from_str(raw).expect("parse fixture");
        let inv = &v["methodResponses"][3];
        assert_eq!(inv[0], "error");
        assert_eq!(inv[1]["type"], "unknownMethod");
        assert_eq!(inv[2], "c3");
    }
}
