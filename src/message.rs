use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A file attached to a [`Message`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub blob_id: Id,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
    pub sha256: String,
}

/// An `@mention` within a [`Message`] body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct Mention {
    pub id: Id,
    pub offset: u64,
    pub length: u64,
}

/// An interactive action button attached to a [`Message`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct MessageAction {
    /// Wire name is `"type"` — Rust keyword, so renamed explicitly.
    #[serde(rename = "type")]
    pub action_type: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A single emoji reaction placed on a [`Message`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub emoji: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_emoji_id: Option<Id>,
    /// `"self"` or a `ChatContact.id` — not a JMAP `Id`, may hold sentinel strings.
    pub sender_id: String,
    pub sent_at: UTCDate,
}

/// A prior revision of a [`Message`] body, stored in edit history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct MessageRevision {
    pub body: String,
    pub body_type: String,
    pub edited_at: UTCDate,
}

/// Per-recipient delivery receipt for a [`Message`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_delivered_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<UTCDate>,
}

/// A single chat message as defined by the JMAP Chat extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: Id,
    pub sender_msg_id: Id,
    /// `"self"` or a `ChatContact.id`.
    pub sender_id: String,
    pub chat_id: Id,
    pub body: String,
    pub body_type: String,
    pub attachments: Vec<Attachment>,
    pub mentions: Vec<Mention>,
    pub actions: Vec<MessageAction>,
    pub reactions: HashMap<String, Reaction>,
    pub sent_at: UTCDate,
    pub received_at: UTCDate,
    pub delivery_state: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_root_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread_reply_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_expires_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burn_on_read: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_receipts: Option<HashMap<String, DeliveryReceipt>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_history: Option<Vec<MessageRevision>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<UTCDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_for_all: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: hand-crafted minimal JSON matching the spec's required field set.
    // Vec fields deserialize from `[]` — they must be empty, not None.
    // reactions deserializes from `{}` — must be an empty map.
    #[test]
    fn message_deser_minimal() {
        let json = r#"{
            "id": "m1",
            "senderMsgId": "smid1",
            "senderId": "self",
            "chatId": "c1",
            "body": "hi",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize minimal Message");
        assert_eq!(msg.id.as_ref(), "m1");
        assert_eq!(msg.sender_id, "self");
        assert!(msg.attachments.is_empty());
        assert!(msg.mentions.is_empty());
        assert!(msg.actions.is_empty());
        assert!(msg.reactions.is_empty());
        assert!(msg.reply_to.is_none());
        assert!(msg.edit_history.is_none());
        assert!(msg.deleted_at.is_none());
    }

    // Oracle: hand-crafted JSON with one reaction entry; verify key and emoji field.
    #[test]
    fn message_deser_with_reactions() {
        let json = r#"{
            "id": "m2",
            "senderMsgId": "smid2",
            "senderId": "u42",
            "chatId": "c1",
            "body": "hello",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {
                "r1": {
                    "emoji": "👍",
                    "senderId": "u99",
                    "sentAt": "2026-01-02T10:00:00Z"
                }
            },
            "sentAt": "2026-01-02T09:00:00Z",
            "receivedAt": "2026-01-02T09:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize Message with reactions");
        assert_eq!(msg.reactions.len(), 1);
        let reaction = msg.reactions.get("r1").expect("reaction key r1");
        assert_eq!(reaction.emoji, "👍");
        assert_eq!(reaction.sender_id, "u99");
    }

    // Oracle: serde rename contract — the wire key for action_type must be "type".
    // Verified by serializing and checking the JSON string directly.
    #[test]
    fn message_action_type_wire_name() {
        let action = MessageAction {
            action_type: "button".to_string(),
            uri: "https://example.com".to_string(),
            label: None,
            expires_at: None,
            metadata: None,
        };
        let json = serde_json::to_string(&action).expect("serialize MessageAction");
        assert!(
            json.contains(r#""type":"button""#),
            "expected wire key \"type\", got: {json}"
        );
        assert!(
            !json.contains("actionType"),
            "wire key must not be actionType, got: {json}"
        );
    }

    // Oracle: skip_serializing_if = "Option::is_none" contract — absent optional
    // fields must not appear in serialized output.
    #[test]
    fn message_ser_omits_none() {
        let json_in = r#"{
            "id": "m3",
            "senderMsgId": "smid3",
            "senderId": "self",
            "chatId": "c1",
            "body": "test",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-03T00:00:00Z",
            "receivedAt": "2026-01-03T00:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json_in).expect("deserialize");
        let json_out = serde_json::to_string(&msg).expect("serialize");
        assert!(
            !json_out.contains("replyTo"),
            "replyTo must be absent when None, got: {json_out}"
        );
        assert!(
            !json_out.contains("editHistory"),
            "editHistory must be absent when None, got: {json_out}"
        );
        assert!(
            !json_out.contains("deletedAt"),
            "deletedAt must be absent when None, got: {json_out}"
        );
    }

    // Oracle: hand-crafted DeliveryReceipt JSON; roundtrip must preserve all fields.
    #[test]
    fn delivery_receipt_roundtrip() {
        let json = r#"{
            "u1": {
                "deliveredAt": "2026-01-04T08:00:00Z",
                "readAt": "2026-01-04T08:05:00Z"
            },
            "u2": {}
        }"#;
        let map: HashMap<String, DeliveryReceipt> =
            serde_json::from_str(json).expect("deserialize DeliveryReceipt map");
        assert_eq!(map.len(), 2);
        let u1 = map.get("u1").expect("u1");
        assert_eq!(
            u1.delivered_at.as_ref().map(|d| d.as_ref()),
            Some("2026-01-04T08:00:00Z")
        );
        assert_eq!(
            u1.read_at.as_ref().map(|d| d.as_ref()),
            Some("2026-01-04T08:05:00Z")
        );
        assert!(u1.device_delivered_at.is_none());
        let u2 = map.get("u2").expect("u2");
        assert!(u2.delivered_at.is_none());

        let roundtrip = serde_json::to_string(&map).expect("serialize");
        let map2: HashMap<String, DeliveryReceipt> =
            serde_json::from_str(&roundtrip).expect("re-deserialize");
        assert_eq!(map, map2);
    }

    // Oracle: hand-crafted MessageRevision JSON; roundtrip must preserve all fields.
    #[test]
    fn message_revision_roundtrip() {
        let json = r#"{
            "body": "original text",
            "bodyType": "text/plain",
            "editedAt": "2026-01-05T12:00:00Z"
        }"#;
        let rev: MessageRevision = serde_json::from_str(json).expect("deserialize MessageRevision");
        assert_eq!(rev.body, "original text");
        assert_eq!(rev.body_type, "text/plain");
        assert_eq!(rev.edited_at.as_ref(), "2026-01-05T12:00:00Z");

        let roundtrip = serde_json::to_string(&rev).expect("serialize");
        let rev2: MessageRevision =
            serde_json::from_str(&roundtrip).expect("re-deserialize MessageRevision");
        assert_eq!(rev, rev2);
    }
}
