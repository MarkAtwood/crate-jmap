//! Scene extensions to the RFC 8620 §5.3 `SetError`
//! (draft-atwood-jmap-scene-00 §Content Moderation for User-Created
//! Objects).
//!
//! The draft adds the `contentPolicy` type and a `retryAfter` property on
//! `rateLimit`. [`jmap_types::SetError`] already carries any type string,
//! and keeps extension properties in its `extra` map, so this module only
//! names them.

use jmap_types::SetError;

/// The `SetError` type a server uses for content that fails its content
/// policy: "the server SHOULD destroy the object and SHOULD notify the
/// creator via a `SetError` of type `contentPolicy`", and "Servers MAY
/// reject `SceneRegion/set` or `SceneObject/set` create or update requests
/// that violate text content policies with a `SetError` of type
/// `contentPolicy`."
pub const CONTENT_POLICY: &str = "contentPolicy";

/// The `SetError` property that carries the retry delay of a `rateLimit`
/// error.
pub const RETRY_AFTER: &str = "retryAfter";

/// Why [`retry_after`] could not read the property.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryAfterError {
    /// `retryAfter` is present but not a non-negative integer number of
    /// seconds. The raw JSON value is kept for diagnostics.
    Malformed(serde_json::Value),
}

impl std::fmt::Display for RetryAfterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetryAfterError::Malformed(raw) => write!(f, "retryAfter present but malformed: {raw}"),
        }
    }
}

impl std::error::Error for RetryAfterError {}

/// Read the `retryAfter` property of `err`: "When a rate limit is
/// exceeded, the server SHOULD return a `SetError` of type `rateLimit` and
/// SHOULD include a `retryAfter` property indicating the number of seconds
/// before the client may retry."
///
/// - `Ok(None)`: the property is absent; the caller picks its own backoff.
/// - `Ok(Some(seconds))`: wait `seconds` before retrying.
/// - `Err(RetryAfterError::Malformed(raw))`: the property is not a
///   non-negative integer. The fault is the server's; the caller should
///   still back off.
///
/// The property is read whatever `err.error_type` is.
pub fn retry_after(err: &SetError) -> Result<Option<u64>, RetryAfterError> {
    match err.extra.get(RETRY_AFTER) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| RetryAfterError::Malformed(v.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: serde_json::Value) -> SetError {
        serde_json::from_value(v).expect("a SetError")
    }

    #[test]
    fn content_policy_is_the_draft_type_name() {
        // draft-atwood-jmap-scene-00 L2443 "Servers MAY reject
        // `SceneRegion/set` or `SceneObject/set` create or update requests
        // that violate text content policies with a `SetError` of type
        // `contentPolicy`."
        let err = parse(json!({ "type": "contentPolicy" }));
        assert_eq!(err.error_type, CONTENT_POLICY);
    }

    #[test]
    fn retry_after_is_seconds() {
        // L2445 "the server SHOULD return a `SetError` of type `rateLimit`
        // and SHOULD include a `retryAfter` property indicating the number
        // of seconds before the client may retry."
        let err = parse(json!({ "type": "rateLimit", "retryAfter": 30 }));
        assert_eq!(err.error_type, "rateLimit");
        assert_eq!(retry_after(&err), Ok(Some(30)));
    }

    #[test]
    fn absent_retry_after_is_none() {
        // L2445 "SHOULD include": a server may leave it out.
        let err = parse(json!({ "type": "rateLimit" }));
        assert_eq!(retry_after(&err), Ok(None));
    }

    #[test]
    fn non_seconds_retry_after_is_malformed() {
        // L2445 "the number of seconds": neither a string, a negative
        // count nor a fraction is one.
        for raw in [json!("30"), json!(-1), json!(1.5), json!(null)] {
            let err = parse(json!({ "type": "rateLimit", "retryAfter": raw.clone() }));
            assert_eq!(retry_after(&err), Err(RetryAfterError::Malformed(raw)));
        }
    }
}
