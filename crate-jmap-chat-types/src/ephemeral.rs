//! WebSocket ephemeral message types for real-time events.

use crate::clearable::{some_clearable, Clearable};
use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Client→server: subscribe to ephemeral events for selected data types.
///
/// `chat_ids: None` means all chats; `contact_ids: None` means all contacts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamEnable {
    /// Data-type tags to stream (e.g. `"typing"`, `"presence"`).
    pub data_types: Vec<String>,
    /// Chats to filter on; `None` (or JSON `null`) means all chats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_ids: Option<Vec<Id>>,
    /// Contacts to filter on; `None` (or JSON `null`) means all contacts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_ids: Option<Vec<Id>>,
}

/// Client→server: unsubscribe from ephemeral events.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatStreamDisable {}

/// Server→client: a contact is typing (or stopped typing) in a chat.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTypingEvent {
    /// The chat in which typing is occurring.
    pub chat_id: Id,
    /// The `ChatContact.id` of the sender (not necessarily a JMAP `Id`).
    pub sender_id: String,
    /// `true` if the contact started typing; `false` if they stopped.
    pub typing: bool,
}

/// Server→client: a contact's presence state has changed.
///
/// For `status_text` and `status_emoji`:
/// - `None` = field absent → no change
/// - `Some(Clearable::Clear)` = JSON `null` → clear the value
/// - `Some(Clearable::Set(v))` = JSON string → set to `v`
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPresenceEvent {
    /// The contact whose presence changed.
    pub contact_id: Id,
    /// Presence state string (e.g. `"online"`, `"away"`, `"busy"`).
    pub presence: String,
    /// When the contact was last active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<UTCDate>,
    /// Free-text status message; `null` clears it, absent leaves it unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub status_text: Option<Clearable<String>>,
    /// Status emoji; `null` clears it, absent leaves it unchanged.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_clearable"
    )]
    pub status_emoji: Option<Clearable<String>>,
}

/// Wrapper enum for all WebSocket ephemeral messages.
///
/// The `@type` JSON field acts as the discriminant.
///
/// Unknown `@type` values deserialize to `EphemeralMessage::Unknown` rather
/// than producing an error, allowing forward-compatible clients.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "@type")]
pub enum EphemeralMessage {
    #[serde(rename = "ChatStreamEnable")]
    Enable(ChatStreamEnable),
    #[serde(rename = "ChatStreamDisable")]
    Disable(ChatStreamDisable),
    #[serde(rename = "ChatTypingEvent")]
    Typing(ChatTypingEvent),
    #[serde(rename = "ChatPresenceEvent")]
    Presence(ChatPresenceEvent),
    /// Any `@type` not recognized by this version of the library.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_event_roundtrip() {
        let json = r#"{"@type":"ChatTypingEvent","chatId":"c1","senderId":"alice","typing":true}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Typing(e) => {
                assert_eq!(e.chat_id, Id::from("c1"));
                assert_eq!(e.sender_id, "alice");
                assert!(e.typing);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn presence_event_null_status_text() {
        // null → Some(Clearable::Clear)
        let json =
            r#"{"@type":"ChatPresenceEvent","contactId":"u1","presence":"away","statusText":null}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(e.status_text, Some(Clearable::Clear));
                assert_eq!(e.status_emoji, None); // absent
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn presence_event_set_status_text() {
        // value → Some(Clearable::Set(...))
        let json = r#"{"@type":"ChatPresenceEvent","contactId":"u1","presence":"online","statusText":"In a meeting"}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(
                    e.status_text,
                    Some(Clearable::Set("In a meeting".to_owned()))
                );
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn presence_event_absent_status() {
        // field absent → None
        let json = r#"{"@type":"ChatPresenceEvent","contactId":"u1","presence":"busy"}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Presence(e) => {
                assert_eq!(e.status_text, None);
                assert_eq!(e.status_emoji, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn stream_enable_null_chat_ids() {
        let json = r#"{"@type":"ChatStreamEnable","dataTypes":["typing"],"chatIds":null,"contactIds":null}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        match msg {
            EphemeralMessage::Enable(e) => {
                assert_eq!(e.data_types, vec!["typing"]);
                assert_eq!(e.chat_ids, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn stream_disable_roundtrip() {
        let json = r#"{"@type":"ChatStreamDisable"}"#;
        let msg: EphemeralMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, EphemeralMessage::Disable(_)));
        let out = serde_json::to_string(&msg).unwrap();
        assert_eq!(out, json);
    }
}
