//! ReadPosition object tracking a user's read cursor in a Chat.

use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Tracks how far a user has read within a Chat (spec: ReadPosition object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPosition {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.20).
    pub id: Id,
    /// The `chatId` property (draft-atwood-jmap-chat-00 §4.20).
    pub chat_id: Id,
    /// The `lastReadMessageId` property (draft-atwood-jmap-chat-00 §4.20).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<Id>,
    /// The `lastReadAt` property (draft-atwood-jmap-chat-00 §4.20).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<UTCDate>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ReadPosition {
    /// Construct a [`ReadPosition`] from its required fields.
    ///
    /// Both optional fields default to `None`.
    pub fn new(id: Id, chat_id: Id) -> Self {
        Self {
            id,
            chat_id,
            last_read_message_id: None,
            last_read_at: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: minimal JSON — only required fields; both optional fields must be None.
    #[test]
    fn read_position_no_reads() {
        let json = r#"{"id":"rp1","chatId":"c1"}"#;
        let rp: ReadPosition = serde_json::from_str(json).expect("deserialize ReadPosition");
        assert_eq!(rp.id.as_ref(), "rp1");
        assert_eq!(rp.chat_id.as_ref(), "c1");
        assert!(rp.last_read_message_id.is_none());
        assert!(rp.last_read_at.is_none());
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `ReadPosition.extra` captures vendor fields and preserves them.
    #[test]
    fn read_position_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "rp1",
            "chatId": "c1",
            "acmeCorpClient": "mobile-ios"
        });
        let rp: ReadPosition = serde_json::from_value(raw).unwrap();
        assert_eq!(
            rp.extra.get("acmeCorpClient").and_then(|v| v.as_str()),
            Some("mobile-ios")
        );
        let back = serde_json::to_value(&rp).unwrap();
        assert_eq!(back["acmeCorpClient"], "mobile-ios");
    }

    // Oracle: full JSON round-trip — all fields survive serialize → deserialize.
    #[test]
    fn read_position_roundtrip() {
        let json = r#"{
            "id": "rp2",
            "chatId": "c2",
            "lastReadMessageId": "msg99",
            "lastReadAt": "2026-04-01T12:00:00Z"
        }"#;
        let rp: ReadPosition = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            rp.last_read_message_id.as_ref().map(|id| id.as_ref()),
            Some("msg99")
        );
        assert_eq!(
            rp.last_read_at.as_ref().map(|d| d.as_ref()),
            Some("2026-04-01T12:00:00Z")
        );
        let serialized = serde_json::to_string(&rp).expect("serialize");
        let rp2: ReadPosition = serde_json::from_str(&serialized).expect("re-deserialize");
        assert_eq!(rp, rp2);
    }
}
