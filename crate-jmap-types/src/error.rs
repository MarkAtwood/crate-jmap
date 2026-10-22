//! RFC 8620 §3.6 JMAP method-level error type ([`JmapError`],
//! [`MethodErrorType`]).

use crate::Id;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The machine-readable `type` of a [`JmapError`].
///
/// Known variants are the RFC 8620 request-level error types (§3.6.1,
/// whose wire form in an RFC 7807 problem-details body is prefixed with
/// `urn:ietf:params:jmap:error:`), the §3.6.2 method-level error types,
/// and the method-specific errors of §5.1 to §5.6. Every other wire
/// string, including extension errors such as `"rateLimited"`, is
/// [`Self::Other`].
///
/// `From<&str>` and `Deserialize` share one mapping: a known wire string
/// always builds its typed variant, never `Other`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MethodErrorType {
    /// RFC 8620 §3.6.1 — the request used an unsupported capability.
    UnknownCapability,
    /// RFC 8620 §3.6.1 — the content type was not `application/json` or
    /// the request did not parse as I-JSON.
    NotJson,
    /// RFC 8620 §3.6.1 — the request parsed as JSON but not as a Request
    /// object.
    NotRequest,
    /// RFC 8620 §3.6.1 — the request exceeded a server limit; the limit's
    /// name is [`JmapError::limit_name`].
    Limit,
    /// RFC 8620 §3.6.2 — an internal server resource was temporarily
    /// unavailable.
    ServerUnavailable,
    /// RFC 8620 §3.6.2 — an unexpected error occurred.
    ServerFail,
    /// RFC 8620 §3.6.2 — some, but not all, expected changes were made.
    ServerPartialFail,
    /// RFC 8620 §3.6.2 — the server does not recognise this method name.
    UnknownMethod,
    /// RFC 8620 §3.6.2 — an argument is of the wrong type or invalid.
    InvalidArguments,
    /// RFC 8620 §3.6.2 — a result reference failed to resolve.
    InvalidResultReference,
    /// RFC 8620 §3.6.2 — the method or arguments are forbidden.
    Forbidden,
    /// RFC 8620 §3.6.2 — the `accountId` does not correspond to a valid
    /// account.
    AccountNotFound,
    /// RFC 8620 §3.6.2 — the account does not support this method.
    AccountNotSupportedByMethod,
    /// RFC 8620 §3.6.2 — the account is read-only.
    AccountReadOnly,
    /// RFC 8620 §5.1 and §5.3 — too many ids or objects in one call.
    RequestTooLarge,
    /// RFC 8620 §5.2 and §5.6 — the server cannot calculate the changes
    /// from the given state.
    CannotCalculateChanges,
    /// RFC 8620 §5.3 and §5.4 — `ifInState` did not match the current
    /// state.
    StateMismatch,
    /// RFC 8620 §5.4 — the `fromAccountId` does not correspond to a valid
    /// account.
    FromAccountNotFound,
    /// RFC 8620 §5.4 — the `fromAccountId` account does not support this
    /// method.
    FromAccountNotSupportedByMethod,
    /// RFC 8620 §5.5 — the anchor was not found in the results.
    AnchorNotFound,
    /// RFC 8620 §5.5 — the sort is syntactically valid but unsupported.
    UnsupportedSort,
    /// RFC 8620 §5.5 — the filter is syntactically valid but unsupported.
    UnsupportedFilter,
    /// RFC 8620 §5.6 — there are more changes than `maxChanges`.
    TooManyChanges,
    /// Any other error type, such as an extension's.
    Other(String),
}

crate::impl_string_enum!(
    MethodErrorType,
    "a JMAP method error type string",
    "unknownCapability" => UnknownCapability,
    "notJSON" => NotJson,
    "notRequest" => NotRequest,
    "limit" => Limit,
    "serverUnavailable" => ServerUnavailable,
    "serverFail" => ServerFail,
    "serverPartialFail" => ServerPartialFail,
    "unknownMethod" => UnknownMethod,
    "invalidArguments" => InvalidArguments,
    "invalidResultReference" => InvalidResultReference,
    "forbidden" => Forbidden,
    "accountNotFound" => AccountNotFound,
    "accountNotSupportedByMethod" => AccountNotSupportedByMethod,
    "accountReadOnly" => AccountReadOnly,
    "requestTooLarge" => RequestTooLarge,
    "cannotCalculateChanges" => CannotCalculateChanges,
    "stateMismatch" => StateMismatch,
    "fromAccountNotFound" => FromAccountNotFound,
    "fromAccountNotSupportedByMethod" => FromAccountNotSupportedByMethod,
    "anchorNotFound" => AnchorNotFound,
    "unsupportedSort" => UnsupportedSort,
    "unsupportedFilter" => UnsupportedFilter,
    "tooManyChanges" => TooManyChanges,
);

/// JMAP method-level error, serializable for inclusion in `methodResponses`.
///
/// See RFC 8620 §3.6.2 for the standard error types.
/// The JSON key is `"type"` (not `"error_type"`) per RFC 8620.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{error_type}")]
#[non_exhaustive]
pub struct JmapError {
    /// Error type per RFC 8620 §3.6.
    #[serde(rename = "type")]
    pub error_type: MethodErrorType,
    /// Human-readable description. Omitted from JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The id of the existing record. Only set for `"alreadyExists"` (RFC 8620 §5.4 MUST).
    #[serde(rename = "existingId", skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<Id>,
    /// Maximum `maxChanges` value the server will accept. Only set for `"tooManyChanges"` (RFC 8620 §9.6.1 MUST).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// Name of the exceeded limit for a request-level `limit` error
    /// (RFC 8620 §3.6.1: "the "limit" property MUST contain the name of
    /// the limit being applied"), e.g. `"maxCallsInRequest"`.
    ///
    /// Request-level errors are sent as an RFC 7807 problem-details body,
    /// where `jmap-server` writes this as the `"limit"` property. It never
    /// appears in a `methodResponses` error, so it is not serialized here
    /// (the `limit` key there is the `tooManyChanges` count).
    #[serde(skip)]
    pub limit_name: Option<String>,
}

impl JmapError {
    /// RFC 8620 §3.6.2 — "invalidArguments"
    pub fn invalid_arguments(desc: impl Into<String>) -> Self {
        Self {
            error_type: MethodErrorType::InvalidArguments,
            description: Some(desc.into()),
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.2 — "forbidden"
    pub fn forbidden() -> Self {
        Self {
            error_type: MethodErrorType::Forbidden,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "notFound"
    pub fn not_found() -> Self {
        Self {
            error_type: MethodErrorType::from("notFound"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.1 — "accountNotFound"
    pub fn account_not_found() -> Self {
        Self {
            error_type: MethodErrorType::AccountNotFound,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.1 — "accountNotSupportedByMethod"
    pub fn account_not_supported_by_method() -> Self {
        Self {
            error_type: MethodErrorType::AccountNotSupportedByMethod,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.1 — "accountReadOnly"
    pub fn account_read_only() -> Self {
        Self {
            error_type: MethodErrorType::AccountReadOnly,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.2 — "serverUnavailable"
    pub fn server_unavailable() -> Self {
        Self {
            error_type: MethodErrorType::ServerUnavailable,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.2 — "serverFail"
    pub fn server_fail(desc: impl Into<String>) -> Self {
        Self {
            error_type: MethodErrorType::ServerFail,
            description: Some(desc.into()),
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.2 — "serverPartialFail"
    pub fn server_partial_fail() -> Self {
        Self {
            error_type: MethodErrorType::ServerPartialFail,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.2 — "unknownMethod"
    pub fn unknown_method() -> Self {
        Self {
            error_type: MethodErrorType::UnknownMethod,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.2 — "invalidResultReference"
    pub fn invalid_result_reference() -> Self {
        Self {
            error_type: MethodErrorType::InvalidResultReference,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.2 and §5.6 — "cannotCalculateChanges"
    pub fn cannot_calculate_changes() -> Self {
        Self {
            error_type: MethodErrorType::CannotCalculateChanges,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "stateMismatch"
    pub fn state_mismatch() -> Self {
        Self {
            error_type: MethodErrorType::StateMismatch,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "tooLarge"
    pub fn too_large() -> Self {
        Self {
            error_type: MethodErrorType::from("tooLarge"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.1 and §5.3 — "requestTooLarge"
    pub fn request_too_large() -> Self {
        Self {
            error_type: MethodErrorType::RequestTooLarge,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "overQuota"
    pub fn over_quota() -> Self {
        Self {
            error_type: MethodErrorType::from("overQuota"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "rateLimit"
    pub fn rate_limit() -> Self {
        Self {
            error_type: MethodErrorType::from("rateLimit"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "invalidPatch"
    pub fn invalid_patch() -> Self {
        Self {
            error_type: MethodErrorType::from("invalidPatch"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "willDestroy"
    pub fn will_destroy() -> Self {
        Self {
            error_type: MethodErrorType::from("willDestroy"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "invalidProperties"
    pub fn invalid_properties() -> Self {
        Self {
            error_type: MethodErrorType::from("invalidProperties"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.3 — "singleton"
    pub fn singleton() -> Self {
        Self {
            error_type: MethodErrorType::from("singleton"),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.5 — "unsupportedFilter"
    pub fn unsupported_filter() -> Self {
        Self {
            error_type: MethodErrorType::UnsupportedFilter,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.5 — "anchorNotFound"
    pub fn anchor_not_found() -> Self {
        Self {
            error_type: MethodErrorType::AnchorNotFound,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.4 — "alreadyExists"
    ///
    /// `existing_id` is the id of the record that already exists in the target account.
    /// Per RFC 8620 §5.4, this field MUST be present on the SetError object.
    pub fn already_exists(existing_id: Id) -> Self {
        Self {
            error_type: MethodErrorType::from("alreadyExists"),
            description: None,
            existing_id: Some(existing_id),
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.4 — "fromAccountNotFound"
    pub fn from_account_not_found() -> Self {
        Self {
            error_type: MethodErrorType::FromAccountNotFound,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.4 — "fromAccountNotSupportedByMethod"
    pub fn from_account_not_supported_by_method() -> Self {
        Self {
            error_type: MethodErrorType::FromAccountNotSupportedByMethod,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §5.5 — "unsupportedSort"
    pub fn unsupported_sort() -> Self {
        Self {
            error_type: MethodErrorType::UnsupportedSort,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// Returns a `tooManyChanges` error with the server's limit included.
    ///
    /// Per RFC 8620 §9.6.1, the `limit` field MUST be present so the client
    /// knows the maximum `maxChanges` value to use on retry.
    pub fn too_many_changes_with_limit(limit: u64) -> Self {
        Self {
            error_type: MethodErrorType::TooManyChanges,
            description: None,
            existing_id: None,
            limit: Some(limit),
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.1 — "notJSON" (request-level error)
    ///
    /// The request body was not valid JSON or did not have `application/json` content type.
    pub fn not_json() -> Self {
        Self {
            error_type: MethodErrorType::NotJson,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.1 — "notRequest" (request-level error)
    ///
    /// The request parsed as JSON but did not match the JMAP Request object shape.
    pub fn not_request() -> Self {
        Self {
            error_type: MethodErrorType::NotRequest,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// RFC 8620 §3.6.1 — "limit" (request-level error)
    ///
    /// The request was rejected because it would exceed a capability limit such as
    /// `maxCallsInRequest` or `maxSizeRequest`.
    ///
    /// `limit_name` is the name of the exceeded limit (e.g. `"maxCallsInRequest"`),
    /// stored in [`limit_name`][JmapError::limit_name]. The HTTP layer
    /// (`jmap-server::RequestError`) writes it as the RFC-required `"limit"`
    /// property of the RFC 7807 problem-details body.
    pub fn limit(limit_name: impl Into<String>) -> Self {
        Self {
            error_type: MethodErrorType::Limit,
            description: None,
            existing_id: None,
            limit: None,
            limit_name: Some(limit_name.into()),
        }
    }

    /// "unknownCapability" with the failing URI surfaced to the client.
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
            error_type: MethodErrorType::UnknownCapability,
            description: Some(uri.into()),
            existing_id: None,
            limit: None,
            limit_name: None,
        }
    }

    /// Create a `JmapError` with an extension error type such as
    /// `"rateLimited"`.
    ///
    /// A string naming a known type builds that variant
    /// (`JmapError::custom("forbidden")` equals [`JmapError::forbidden`]).
    pub fn custom(error_type: impl Into<MethodErrorType>) -> Self {
        Self {
            error_type: error_type.into(),
            description: None,
            existing_id: None,
            limit: None,
            limit_name: None,
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
    fn too_many_changes_with_limit_serializes_limit_field() {
        let err = JmapError::too_many_changes_with_limit(100);
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["type"], "tooManyChanges");
        assert_eq!(v["limit"], 100u64);
        assert!(v.get("description").is_none());
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
    fn limit_carries_limit_name_in_its_own_field() {
        // Oracle: RFC 8620 §3.6.1 — "the "limit" property MUST contain the
        // name of the limit being applied". The name has its own field, not
        // the human-readable description.
        let e = JmapError::limit("maxCallsInRequest");
        assert_eq!(e.error_type, MethodErrorType::Limit);
        assert_eq!(e.limit_name.as_deref(), Some("maxCallsInRequest"));
        assert_eq!(e.description, None);
        // The name only travels in the problem-details body; the
        // methodResponses form is just the type.
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"type": "limit"})
        );
    }

    // Oracle: RFC 8620 §3.6.1 — unknownCapability with detail includes the URI.
    #[test]
    fn unknown_capability_with_detail_includes_uri() {
        let e = JmapError::unknown_capability_with_detail("urn:example:unknown");
        assert_eq!(e.error_type, MethodErrorType::UnknownCapability);
        assert_eq!(e.description.as_deref(), Some("urn:example:unknown"));
    }

    #[test]
    fn custom_error_type_round_trips() {
        let e = JmapError::custom("urn:example:customError");
        assert_eq!(
            e.error_type,
            MethodErrorType::Other("urn:example:customError".to_owned())
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"urn:example:customError\""));
        let restored: JmapError = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, e);
    }

    #[test]
    fn round_trip_deserialize() {
        // Verify the "type" rename survives a JSON round-trip.
        let original = JmapError::invalid_arguments("test");
        let json = serde_json::to_string(&original).unwrap();
        let restored: JmapError = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.error_type, MethodErrorType::InvalidArguments);
        assert_eq!(restored.description.as_deref(), Some("test"));
    }

    // Oracle: the error type strings of RFC 8620 §3.6.1, §3.6.2 and
    // §5.1-§5.6, hand-copied. Each must serialize to that string and decode
    // back to its typed variant, never `Other`.
    #[test]
    fn method_error_type_all_known_variants_round_trip() {
        let cases: &[(MethodErrorType, &str)] = &[
            (MethodErrorType::UnknownCapability, "unknownCapability"),
            (MethodErrorType::NotJson, "notJSON"),
            (MethodErrorType::NotRequest, "notRequest"),
            (MethodErrorType::Limit, "limit"),
            (MethodErrorType::ServerUnavailable, "serverUnavailable"),
            (MethodErrorType::ServerFail, "serverFail"),
            (MethodErrorType::ServerPartialFail, "serverPartialFail"),
            (MethodErrorType::UnknownMethod, "unknownMethod"),
            (MethodErrorType::InvalidArguments, "invalidArguments"),
            (
                MethodErrorType::InvalidResultReference,
                "invalidResultReference",
            ),
            (MethodErrorType::Forbidden, "forbidden"),
            (MethodErrorType::AccountNotFound, "accountNotFound"),
            (
                MethodErrorType::AccountNotSupportedByMethod,
                "accountNotSupportedByMethod",
            ),
            (MethodErrorType::AccountReadOnly, "accountReadOnly"),
            (MethodErrorType::RequestTooLarge, "requestTooLarge"),
            (
                MethodErrorType::CannotCalculateChanges,
                "cannotCalculateChanges",
            ),
            (MethodErrorType::StateMismatch, "stateMismatch"),
            (MethodErrorType::FromAccountNotFound, "fromAccountNotFound"),
            (
                MethodErrorType::FromAccountNotSupportedByMethod,
                "fromAccountNotSupportedByMethod",
            ),
            (MethodErrorType::AnchorNotFound, "anchorNotFound"),
            (MethodErrorType::UnsupportedSort, "unsupportedSort"),
            (MethodErrorType::UnsupportedFilter, "unsupportedFilter"),
            (MethodErrorType::TooManyChanges, "tooManyChanges"),
        ];
        for (variant, wire) in cases {
            assert_eq!(
                serde_json::to_value(variant).unwrap(),
                serde_json::Value::String((*wire).to_owned())
            );
            let decoded: MethodErrorType =
                serde_json::from_value(serde_json::Value::String((*wire).to_owned())).unwrap();
            assert_eq!(&decoded, variant, "{wire} must decode to its variant");
            assert_eq!(&MethodErrorType::from(*wire), variant);
        }
        // An extension type stays Other and round-trips verbatim.
        let decoded: MethodErrorType = serde_json::from_str(r#""rateLimited""#).unwrap();
        assert_eq!(decoded, MethodErrorType::Other("rateLimited".to_owned()));
        assert_eq!(serde_json::to_string(&decoded).unwrap(), r#""rateLimited""#);
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
