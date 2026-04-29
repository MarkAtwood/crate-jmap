use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Tracks how far a user has read within a Chat (spec: ReadPosition object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPosition {
    pub id: Id,
    pub chat_id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_message_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<UTCDate>,
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
