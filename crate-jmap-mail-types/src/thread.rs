//! RFC 8621 §3 Thread object.
//!
//! Provides [`Thread`] — groups related [`crate::Email`] objects by
//! conversation thread.

use jmap_types::Id;
use serde::{Deserialize, Serialize};

/// A Thread object as defined in RFC 8621 §3.
///
/// Groups related Email objects by conversation thread. The `emailIds` field
/// lists member Email ids sorted oldest-first by `receivedAt`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// The id of the Thread (immutable; server-set).
    pub id: Id,
    /// The ids of the Emails in the Thread, sorted oldest-first by `receivedAt` (server-set).
    pub email_ids: Vec<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Thread {
    /// Construct a [`Thread`] from its two required fields.
    pub fn new(id: Id, email_ids: Vec<Id>) -> Self {
        Self {
            id,
            email_ids,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Extras-preservation policy tests (JMAP-lbdy.2) ───────────────────

    /// `Thread.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn thread_preserves_vendor_extras() {
        let raw = json!({
            "id": "t1",
            "emailIds": ["e1", "e2"],
            "acmeCorpConversationTag": "support"
        });
        let thr: Thread = serde_json::from_value(raw).unwrap();
        assert_eq!(
            thr.extra
                .get("acmeCorpConversationTag")
                .and_then(|v| v.as_str()),
            Some("support")
        );
        let back = serde_json::to_value(&thr).unwrap();
        assert_eq!(back["acmeCorpConversationTag"], "support");
    }
}
