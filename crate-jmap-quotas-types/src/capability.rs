//! RFC 9425 §2.1 capability struct and URI constant.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Capability URI for JMAP Quotas (RFC 9425 §2.1).
///
/// "Servers supporting this specification MUST add a property called
/// "urn:ietf:params:jmap:quota" to the capabilities object." Its value is
/// a [`QuotaCapability`] in both the Session `capabilities` and an
/// account's `accountCapabilities`.
pub const JMAP_QUOTA_URI: &str = "urn:ietf:params:jmap:quota";

/// Value of `urn:ietf:params:jmap:quota` (RFC 9425 §2.1): "an empty object
/// in both the JMAP Session capabilities property and an account's
/// accountCapabilities property".
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuotaCapability {
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: RFC 9425 §2.1 "The value of this property is an empty object".
    #[test]
    fn capability_is_an_empty_object() {
        assert_eq!(JMAP_QUOTA_URI, "urn:ietf:params:jmap:quota");
        assert_eq!(
            serde_json::to_value(QuotaCapability::default()).unwrap(),
            json!({})
        );
    }
}
