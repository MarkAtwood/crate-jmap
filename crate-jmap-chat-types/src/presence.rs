use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Presence state as defined by the spec.
///
/// `Unknown` preserves any future value for round-trip fidelity.
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
    Unknown(String),
}

impl Serialize for Presence {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            Presence::Online => "online",
            Presence::Away => "away",
            Presence::Busy => "busy",
            Presence::Invisible => "invisible",
            Presence::Offline => "offline",
            Presence::Unknown(v) => v.as_str(),
        })
    }
}

impl<'de> Deserialize<'de> for Presence {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "online" => Presence::Online,
            "away" => Presence::Away,
            "busy" => Presence::Busy,
            "invisible" => Presence::Invisible,
            "offline" => Presence::Offline,
            _ => Presence::Unknown(s),
        })
    }
}

/// A user's current presence state (spec: PresenceStatus object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceStatus {
    pub id: Id,
    pub presence: Presence,
    pub receipt_sharing: bool,
    pub updated_at: UTCDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
}

impl PresenceStatus {
    /// Construct a [`PresenceStatus`] from its required fields.
    ///
    /// All optional fields default to `None`.
    pub fn new(
        id: Id,
        presence: Presence,
        receipt_sharing: bool,
        updated_at: UTCDate,
    ) -> Self {
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
