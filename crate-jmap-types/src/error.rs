//! RFC 8620 §3.6 JMAP method-level error type ([`JmapError`]).

use crate::Id;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// JMAP method-level error, serializable for inclusion in `methodResponses`.
///
/// See RFC 8620 §3.6.2 for the standard error type strings.
/// The JSON key is `"type"` (not `"error_type"`) per RFC 8620.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{error_type}")]
#[non_exhaustive]
pub struct JmapError {
    /// Error type string per RFC 8620 §3.6.2.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable description. Omitted from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The id of the existing record. Only set for `"alreadyExists"` (RFC 8620 §5.4 MUST).
    #[serde(rename = "existingId", skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<Id>,
    /// Maximum `maxChanges` value the server will accept. Only set for `"tooManyChanges"` (RFC 8620 §9.6.1 MUST).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

impl JmapError {
    /// RFC 8620 §3.6.2 — "invalidArguments"
    pub fn invalid_arguments(desc: impl Into<String>) -> Self {
        Self {
            error_type: "invalidArguments".into(),
            description: Some(desc.into()),
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.2 — "forbidden"
    pub fn forbidden() -> Self {
        Self {
            error_type: "forbidden".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "notFound"
    pub fn not_found() -> Self {
        Self {
            error_type: "notFound".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.1 — "accountNotFound"
    pub fn account_not_found() -> Self {
        Self {
            error_type: "accountNotFound".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.1 — "accountNotSupportedByMethod"
    pub fn account_not_supported_by_method() -> Self {
        Self {
            error_type: "accountNotSupportedByMethod".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.1 — "accountReadOnly"
    pub fn account_read_only() -> Self {
        Self {
            error_type: "accountReadOnly".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.2 — "serverUnavailable"
    pub fn server_unavailable() -> Self {
        Self {
            error_type: "serverUnavailable".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.2 — "serverFail"
    pub fn server_fail(desc: impl Into<String>) -> Self {
        Self {
            error_type: "serverFail".into(),
            description: Some(desc.into()),
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.2 — "serverPartialFail"
    pub fn server_partial_fail() -> Self {
        Self {
            error_type: "serverPartialFail".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.2 — "unknownMethod"
    pub fn unknown_method() -> Self {
        Self {
            error_type: "unknownMethod".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.2 — "invalidResultReference"
    pub fn invalid_result_reference() -> Self {
        Self {
            error_type: "invalidResultReference".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.2 and §5.6 — "cannotCalculateChanges"
    pub fn cannot_calculate_changes() -> Self {
        Self {
            error_type: "cannotCalculateChanges".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "stateMismatch"
    pub fn state_mismatch() -> Self {
        Self {
            error_type: "stateMismatch".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "tooLarge"
    pub fn too_large() -> Self {
        Self {
            error_type: "tooLarge".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.1 and §5.3 — "requestTooLarge"
    pub fn request_too_large() -> Self {
        Self {
            error_type: "requestTooLarge".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "overQuota"
    pub fn over_quota() -> Self {
        Self {
            error_type: "overQuota".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "rateLimit"
    pub fn rate_limit() -> Self {
        Self {
            error_type: "rateLimit".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "invalidPatch"
    pub fn invalid_patch() -> Self {
        Self {
            error_type: "invalidPatch".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "willDestroy"
    pub fn will_destroy() -> Self {
        Self {
            error_type: "willDestroy".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "invalidProperties"
    pub fn invalid_properties() -> Self {
        Self {
            error_type: "invalidProperties".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.3 — "singleton"
    pub fn singleton() -> Self {
        Self {
            error_type: "singleton".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.5 — "unsupportedFilter"
    pub fn unsupported_filter() -> Self {
        Self {
            error_type: "unsupportedFilter".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.5 — "anchorNotFound"
    pub fn anchor_not_found() -> Self {
        Self {
            error_type: "anchorNotFound".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.4 — "alreadyExists"
    ///
    /// `existing_id` is the id of the record that already exists in the target account.
    /// Per RFC 8620 §5.4, this field MUST be present on the SetError object.
    pub fn already_exists(existing_id: Id) -> Self {
        Self {
            error_type: "alreadyExists".into(),
            description: None,
            existing_id: Some(existing_id),
            limit: None,
        }
    }

    /// RFC 8620 §5.4 — "fromAccountNotFound"
    pub fn from_account_not_found() -> Self {
        Self {
            error_type: "fromAccountNotFound".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.4 — "fromAccountNotSupportedByMethod"
    pub fn from_account_not_supported_by_method() -> Self {
        Self {
            error_type: "fromAccountNotSupportedByMethod".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.5 — "unsupportedSort"
    pub fn unsupported_sort() -> Self {
        Self {
            error_type: "unsupportedSort".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §5.6 — "tooManyChanges"
    pub fn too_many_changes() -> Self {
        Self {
            error_type: "tooManyChanges".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// Returns a `tooManyChanges` error with the server's limit included.
    ///
    /// Per RFC 8620 §9.6.1, the `limit` field MUST be present so the client
    /// knows the maximum `maxChanges` value to use on retry.
    pub fn too_many_changes_with_limit(limit: u64) -> Self {
        Self {
            error_type: "tooManyChanges".into(),
            description: None,
            existing_id: None,
            limit: Some(limit),
        }
    }

    /// RFC 8620 §3.6.1 — "notJSON" (request-level error)
    ///
    /// The request body was not valid JSON or did not have `application/json` content type.
    pub fn not_json() -> Self {
        Self {
            error_type: "notJSON".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.1 — "notRequest" (request-level error)
    ///
    /// The request parsed as JSON but did not match the JMAP Request object shape.
    pub fn not_request() -> Self {
        Self {
            error_type: "notRequest".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.1 — "limit" (request-level error)
    ///
    /// The request was rejected because it would exceed a capability limit such as
    /// `maxCallsInRequest` or `maxSizeRequest`.
    ///
    /// `limit_name` is the name of the exceeded limit (e.g. `"maxCallsInRequest"`).
    /// The HTTP layer MUST forward this name as the `"limit"` property in the
    /// RFC 7807 Problem Details response body.  The name is stored in
    /// [`description`][JmapError::description] for that purpose.
    ///
    /// **Invariant**: always construct limit errors with this function, never by
    /// setting `error_type = "limit"` and `description` manually.  The HTTP
    /// response layer (`jmap-server::RequestError`) reads `description` to
    /// populate the RFC-required `"limit"` field; a missing description produces
    /// an invalid response.
    pub fn limit(limit_name: impl Into<String>) -> Self {
        Self {
            error_type: "limit".into(),
            description: Some(limit_name.into()),
            existing_id: None,
            limit: None,
        }
    }

    /// RFC 8620 §3.6.1 — "unknownCapability" (request-level error)
    ///
    /// The request used a capability URI not recognized by this server.
    ///
    /// Always prefer [`unknown_capability_with_detail`][Self::unknown_capability_with_detail],
    /// which includes the failing URI so clients can act on it.
    #[deprecated(note = "always use unknown_capability_with_detail to include the URI in the error")]
    pub fn unknown_capability() -> Self {
        Self {
            error_type: "unknownCapability".into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }

    /// "unknownCapability" with the failing URI surfaced to the client.
    ///
    /// Always use this instead of [`unknown_capability()`][Self::unknown_capability].
    ///
    /// The URI is stored in [`description`][JmapError::description].  The HTTP layer
    /// (`jmap-server::RequestError`) reads `description` to populate the `"detail"` field
    /// in the RFC 7807 problem-details response body — the same mechanism used by
    /// [`limit()`][Self::limit] for its limit-name payload.
    ///
    /// **Invariant**: always construct unknownCapability errors that carry a URI with this
    /// function, never by setting `description` manually.  A missing or incorrect description
    /// means the client never learns which capability it requested that the server does not
    /// support.
    pub fn unknown_capability_with_detail(uri: impl Into<String>) -> Self {
        Self {
            error_type: "unknownCapability".into(),
            description: Some(uri.into()),
            existing_id: None,
            limit: None,
        }
    }

    /// Create a `JmapError` with a custom or extension error type string.
    ///
    /// Use this when propagating a server error whose `type` value is not one of
    /// the RFC 8620 standard types, or in tests that need to construct an
    /// arbitrary `JmapError` value.
    pub fn custom(error_type: impl Into<String>) -> Self {
        Self {
            error_type: error_type.into(),
            description: None,
            existing_id: None,
            limit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent oracle: RFC 8620 §3.6.2 and §5.x specify these exact type strings.

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
    fn request_too_large_type_string() {
        let e = JmapError::request_too_large();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"requestTooLarge\""));
        assert!(!json.contains("\"description\""));
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

    // Oracle: RFC 8620 §5.4 — alreadyExists MUST include existingId of type Id.
    #[test]
    fn already_exists_includes_existing_id() {
        let e = JmapError::already_exists(Id::from("abc123"));
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"alreadyExists\""));
        assert!(json.contains("\"existingId\""));
        assert!(json.contains("\"abc123\""));
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
    fn too_many_changes_with_limit_serializes_limit_field() {
        let err = JmapError::too_many_changes_with_limit(100);
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["type"], "tooManyChanges");
        assert_eq!(v["limit"], 100u64);
        assert!(v.get("description").is_none());
    }

    #[test]
    fn too_many_changes_without_limit_has_no_limit_field() {
        let err = JmapError::too_many_changes();
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["type"], "tooManyChanges");
        assert!(v.get("limit").is_none());
    }

    #[test]
    fn not_json_type_string() {
        let e = JmapError::not_json();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"notJSON\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn not_request_type_string() {
        let e = JmapError::not_request();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"notRequest\""));
        assert!(!json.contains("\"description\""));
    }

    #[test]
    fn limit_includes_limit_name_in_description() {
        // Oracle: the limit name is stored in description so the HTTP layer
        // can forward it as the "limit" field in RFC 7807 Problem Details.
        let e = JmapError::limit("maxCallsInRequest");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"limit\""));
        assert!(json.contains("\"maxCallsInRequest\""));
    }

    #[allow(deprecated)]
    #[test]
    fn unknown_capability_type_string() {
        let e = JmapError::unknown_capability();
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"unknownCapability\""));
        assert!(!json.contains("\"description\""));
    }

    // Oracle: RFC 8620 §3.6.1 — unknownCapability with detail includes the URI.
    #[test]
    fn unknown_capability_with_detail_includes_uri() {
        let e = JmapError::unknown_capability_with_detail("urn:example:unknown");
        assert_eq!(e.error_type, "unknownCapability");
        assert_eq!(e.description.as_deref(), Some("urn:example:unknown"));
    }

    #[test]
    fn custom_error_type_round_trips() {
        let e = JmapError::custom("urn:example:customError");
        assert_eq!(e.error_type, "urn:example:customError");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"urn:example:customError\""));
        let restored: JmapError = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.error_type, "urn:example:customError");
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
