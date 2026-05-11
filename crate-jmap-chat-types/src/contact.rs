//! ChatContact and Endpoint objects for remote users.

use crate::presence::Presence;
use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// A reachable endpoint for a contact (e.g. XMPP, SIP, email).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    /// Wire name is "type" — camelCase expansion would give "endpointType".
    #[serde(rename = "type")]
    pub endpoint_type: String,
    /// URI identifying the endpoint.
    pub uri: String,
    /// Human-readable label for this endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Arbitrary provider-defined metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A remote user known to this server (spec: ChatContact object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContact {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.8).
    pub id: Id,
    /// The `login` property (draft-atwood-jmap-chat-00 §4.8).
    pub login: String,
    /// The `firstSeenAt` property (draft-atwood-jmap-chat-00 §4.8).
    pub first_seen_at: UTCDate,
    /// The `lastSeenAt` property (draft-atwood-jmap-chat-00 §4.8).
    pub last_seen_at: UTCDate,
    /// The `blocked` property (draft-atwood-jmap-chat-00 §4.8).
    pub blocked: bool,
    /// The `displayName` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The `presence` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
    /// The `lastActiveAt` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<UTCDate>,
    /// The `statusText` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_text: Option<String>,
    /// The `statusEmoji` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_emoji: Option<String>,
    /// The `endpoints` property (draft-atwood-jmap-chat-00 §4.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<Vec<Endpoint>>,
}

impl ChatContact {
    /// Construct a [`ChatContact`] from its required fields.
    ///
    /// All optional fields default to `None`.
    pub fn new(
        id: Id,
        login: impl Into<String>,
        first_seen_at: UTCDate,
        last_seen_at: UTCDate,
        blocked: bool,
    ) -> Self {
        Self {
            id,
            login: login.into(),
            first_seen_at,
            last_seen_at,
            blocked,
            display_name: None,
            presence: None,
            last_active_at: None,
            status_text: None,
            status_emoji: None,
            endpoints: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: minimal JSON — only required fields present; all optional fields absent.
    #[test]
    fn contact_deser_minimal() {
        let json = r#"{
            "id": "u1",
            "login": "alice@example.com",
            "firstSeenAt": "2026-01-01T00:00:00Z",
            "lastSeenAt": "2026-04-01T00:00:00Z",
            "blocked": false
        }"#;
        let c: ChatContact = serde_json::from_str(json).expect("deserialize ChatContact");
        assert_eq!(c.id.as_ref(), "u1");
        assert_eq!(c.login, "alice@example.com");
        assert!(!c.blocked);
        assert!(c.display_name.is_none());
        assert!(c.endpoints.is_none());
    }

    // Oracle: the wire field name for endpoint_type must be "type", not "endpointType".
    #[test]
    fn endpoint_type_wire_name() {
        let ep = Endpoint {
            endpoint_type: "xmpp".to_owned(),
            uri: "xmpp:alice@example.com".to_owned(),
            label: None,
            metadata: None,
        };
        let json = serde_json::to_string(&ep).expect("serialize Endpoint");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse JSON");
        assert!(v.get("type").is_some(), "expected key \"type\" in JSON");
        assert!(
            v.get("endpointType").is_none(),
            "unexpected key \"endpointType\" in JSON"
        );
    }

    // Oracle: when blocked=true, the "blocked" key must appear in serialized output.
    #[test]
    fn contact_blocked_present() {
        let c = ChatContact {
            id: Id::from("u2"),
            login: "bob@example.com".to_owned(),
            first_seen_at: UTCDate::from("2026-01-01T00:00:00Z"),
            last_seen_at: UTCDate::from("2026-04-01T00:00:00Z"),
            blocked: true,
            display_name: None,
            presence: None,
            last_active_at: None,
            status_text: None,
            status_emoji: None,
            endpoints: None,
        };
        let json = serde_json::to_string(&c).expect("serialize ChatContact");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse JSON");
        assert_eq!(v["blocked"], serde_json::json!(true));
    }
}
