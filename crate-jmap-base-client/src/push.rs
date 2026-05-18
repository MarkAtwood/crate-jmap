//! Canonical push notification types shared by SSE and WebSocket transports.
//! Spec: RFC 8620 §7.1 (Push Subscriptions)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use jmap_types::{Id, State};

/// A state change push notification (RFC 8620 §7.1).
///
/// Sent over both SSE (as a push event) and WebSocket (as a frame type).
///
/// # `extra` equality is feature-flag-dependent (bd:JMAP-6r7c.43)
///
/// The derived `PartialEq` / `Eq` impl's behaviour on the `extra` field
/// depends on the global `serde_json/preserve_order` feature flag — see
/// the [crate-level note](crate#extra-field-equality-and-the-serde_jsonpreserve_order-feature-bdjmap-6r7c43)
/// for the canonical statement.
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

    /// bd:JMAP-6r7c.43 — regression-guard for the workspace's
    /// serde_json/preserve_order posture. Two StateChange values with the
    /// same `extra` keys inserted in different orders MUST compare equal
    /// under the workspace's default configuration (BTreeMap-backed
    /// serde_json::Map). If a future Cargo.lock or workspace-level feature
    /// change accidentally enables `preserve_order`, this assertion fails
    /// loudly and surfaces the SemVer-policy break before downstream
    /// consumers hit silently-different equality semantics.
    ///
    /// Both values are deserialized from JSON (the wire path) so the test
    /// exercises the same construction code path consumers use.
    #[test]
    fn extra_equality_is_order_insensitive_under_workspace_flags() {
        // Two equivalent JSON payloads with different key-insertion orders
        // for the vendor extras. Under BTreeMap-backed Map the keys are
        // re-sorted lexicographically on deserialize, so the resulting
        // structures compare equal.
        let raw_a = json!({
            "changed": {"acc1": {"Email": "s1"}},
            "vendorA": 1,
            "vendorB": 2
        });
        let raw_b = json!({
            "changed": {"acc1": {"Email": "s1"}},
            "vendorB": 2,
            "vendorA": 1
        });

        let a: StateChange = serde_json::from_value(raw_a).expect("a must deserialize");
        let b: StateChange = serde_json::from_value(raw_b).expect("b must deserialize");

        assert_eq!(
            a, b,
            "extra-map equality is order-insensitive under the workspace's \
             default serde_json::Map (BTreeMap-backed); if this fails, \
             check whether preserve_order has been enabled in the dep graph"
        );
    }
}
