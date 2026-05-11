//! Presence state and PresenceStatus object.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Presence state as defined by the spec.
///
/// `Other` preserves any future value for round-trip fidelity.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Presence {
    /// Actively connected and available.
    Online,
    /// Connected but not actively attending.
    Away,
    /// Connected but occupied and not to be interrupted.
    Busy,
    /// Connected but appearing offline to others.
    Invisible,
    /// Not connected.
    Offline,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(Presence, "a presence state string",
    "online" => Online,
    "away" => Away,
    "busy" => Busy,
    "invisible" => Invisible,
    "offline" => Offline,
);

/// A user's current presence state (spec: PresenceStatus object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceStatus {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.21).
    pub id: Id,
    /// The `presence` property (draft-atwood-jmap-chat-00 §4.21).
    pub presence: Presence,
    /// The `receiptSharing` property (draft-atwood-jmap-chat-00 §4.21).
    pub receipt_sharing: bool,
    /// The `updatedAt` property (draft-atwood-jmap-chat-00 §4.21).
    pub updated_at: UTCDate,
    /// The `statusText` property (draft-atwood-jmap-chat-00 §4.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    /// The `statusEmoji` property (draft-atwood-jmap-chat-00 §4.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_emoji: Option<String>,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
}

impl PresenceStatus {
    /// Construct a [`PresenceStatus`] from its required fields.
    ///
    /// All optional fields default to `None`.
    pub fn new(id: Id, presence: Presence, receipt_sharing: bool, updated_at: UTCDate) -> Self {
        Self {
            id,
            presence,
            receipt_sharing,
            updated_at,
            status_text: None,
            status_emoji: None,
            expires_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: minimal JSON — only required fields; status_text must be None.
    #[test]
    fn presence_status_deser() {
        let json = r#"{"id":"ps1","presence":"online","receiptSharing":true,"updatedAt":"2026-01-01T00:00:00Z"}"#;
        let ps: PresenceStatus = serde_json::from_str(json).expect("deserialize PresenceStatus");
        assert_eq!(ps.id.as_ref(), "ps1");
        assert_eq!(ps.presence, Presence::Online);
        assert!(ps.receipt_sharing);
        assert!(ps.status_text.is_none());
    }

    // Oracle: full JSON round-trip — all fields survive serialize → deserialize.
    #[test]
    fn presence_status_roundtrip() {
        let json = r#"{
            "id": "ps2",
            "presence": "away",
            "receiptSharing": false,
            "updatedAt": "2026-04-01T08:00:00Z",
            "statusText": "Out for lunch",
            "statusEmoji": "🍔",
            "expiresAt": "2026-04-01T09:00:00Z"
        }"#;
        let ps: PresenceStatus = serde_json::from_str(json).expect("deserialize");
        assert_eq!(ps.presence, Presence::Away);
        assert_eq!(ps.status_text.as_deref(), Some("Out for lunch"));
        assert_eq!(
            ps.expires_at.as_ref().map(|d| d.as_ref()),
            Some("2026-04-01T09:00:00Z")
        );
        let serialized = serde_json::to_string(&ps).expect("serialize");
        let ps2: PresenceStatus = serde_json::from_str(&serialized).expect("re-deserialize");
        assert_eq!(ps, ps2);
    }
}
