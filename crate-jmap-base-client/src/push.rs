//! Canonical push notification types shared by SSE and WebSocket transports.
//! Spec: RFC 8620 §7.1 (Push Subscriptions)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use jmap_types::{Id, State};

/// A state change push notification (RFC 8620 §7.1).
///
/// Sent over both SSE (as a push event) and WebSocket (as a frame type).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateChange {
    /// For each account that changed: maps data-type name to the new [`State`] token.
    ///
    /// Outer key: account [`Id`].  Inner key: JMAP data-type name (e.g. `"Email"`).
    /// Inner value: new opaque state string; pass to `Email/changes` etc. as `sinceState`.
    pub changed: HashMap<Id, HashMap<String, State>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `StateChange.extra` captures unknown fields on deserialize and
    /// flattens them on serialize (round-trip).
    #[test]
    fn state_change_preserves_vendor_extras() {
        let raw = json!({
            "changed": {
                "acc1": { "Email": "s42" }
            },
            "acmeCorpSequence": 17
        });
        let obj: StateChange = serde_json::from_value(raw).expect("StateChange must deserialize");
        assert_eq!(
            obj.extra.get("acmeCorpSequence").and_then(|v| v.as_u64()),
            Some(17)
        );

        // Round-trip: serializing back must reproduce the vendor field.
        let v = serde_json::to_value(&obj).expect("StateChange must serialize");
        assert_eq!(v["acmeCorpSequence"], json!(17));
    }
}
