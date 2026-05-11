//! Chat message, attachments, reactions, and delivery state types.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

/// Delivery state of a [`Message`] as defined by the spec.
///
/// `Other` preserves any future value for round-trip fidelity.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeliveryState {
    /// Enqueued but not yet acknowledged by any recipient.
    Pending,
    /// Acknowledged by the recipient's server.
    Delivered,
    /// Delivery failed permanently.
    Failed,
    /// Received on the recipient's device.
    Received,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(DeliveryState, "a delivery state string",
    "pending" => Pending,
    "delivered" => Delivered,
    "failed" => Failed,
    "received" => Received,
);

/// Why a recipient acknowledged a message (RFC-JMAP-Chat §ReadDisposition).
///
/// `Other` preserves any unrecognized value for round-trip fidelity.
/// Servers MUST NOT reject messages carrying unknown values.
///
/// # MAINTENANCE
/// When adding a new variant: (1) add it to the enum below, (2) add the
/// corresponding `"wire-name" => Variant` arm in `impl_string_enum!` below.
/// Both must stay in sync — a variant absent from the macro falls through to
/// `Other(String)` on deserialize and serializes incorrectly.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReadDisposition {
    /// Message content was presented to the user's attention (default).
    Displayed,
    /// Message was removed without being displayed.
    Deleted,
    /// Message was handled by an automated process.
    Processed,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(ReadDisposition, "a read disposition string",
    "displayed" => Displayed,
    "deleted"   => Deleted,
    "processed" => Processed,
);

/// Identifies who sent a [`Message`] or placed a [`Reaction`].
///
/// The account owner is represented as the wire sentinel `"self"`.
/// All other participants carry their `ChatContact.id` string.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SenderId {
    /// The message or reaction was sent by the account owner.
    Owner,
    /// Another participant, identified by their `ChatContact.id`.
    Contact(String),
}

impl Serialize for SenderId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            SenderId::Owner => "self",
            SenderId::Contact(id) => id.as_str(),
        })
    }
}

impl<'de> Deserialize<'de> for SenderId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(if s == "self" {
            SenderId::Owner
        } else {
            SenderId::Contact(s)
        })
    }
}

impl std::fmt::Display for SenderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SenderId::Owner => "self",
            SenderId::Contact(id) => id.as_str(),
        })
    }
}

/// A file attached to a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// The `blobId` property (draft-atwood-jmap-chat-00 §4.1).
    pub blob_id: Id,
    /// The `filename` property (draft-atwood-jmap-chat-00 §4.1).
    pub filename: String,
    /// The `contentType` property (draft-atwood-jmap-chat-00 §4.1).
    pub content_type: String,
    /// The `size` property (draft-atwood-jmap-chat-00 §4.1).
    pub size: u64,
    /// SHA-256 digest of the attachment content, hex-encoded: exactly 64 lowercase hex characters.
    ///
    /// Kept as `String` rather than a validated newtype because this crate is wire-format only —
    /// it does not validate field values, consistent with how `Id`, `UTCDate`, and `body_type`
    /// are handled. A newtype with `TryFrom` validation would be inconsistent with that boundary.
    /// A newtype without validation would add type-tagging overhead for a field that already has
    /// a distinct name; mixing it with `blob_id` is not a realistic mistake.
    pub sha256: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An `@mention` within a [`Message`] body.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mention {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.4).
    pub id: Id,
    /// The `offset` property (draft-atwood-jmap-chat-00 §4.4).
    pub offset: u64,
    /// The `length` property (draft-atwood-jmap-chat-00 §4.4).
    pub length: u64,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An interactive action button attached to a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageAction {
    /// The `type` property (draft-atwood-jmap-chat-00 §4.3).
    ///
    /// Wire name is `"type"` — Rust keyword, so renamed explicitly.
    #[serde(rename = "type")]
    pub action_type: String,
    /// The `uri` property (draft-atwood-jmap-chat-00 §4.3).
    pub uri: String,
    /// The `label` property (draft-atwood-jmap-chat-00 §4.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
    /// The `metadata` property (draft-atwood-jmap-chat-00 §4.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single emoji reaction placed on a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    /// The `emoji` property (draft-atwood-jmap-chat-00 §4.6).
    pub emoji: String,
    /// The `customEmojiId` property (draft-atwood-jmap-chat-00 §4.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_emoji_id: Option<Id>,
    /// The `senderId` property (draft-atwood-jmap-chat-00 §4.6).
    pub sender_id: SenderId,
    /// The `sentAt` property (draft-atwood-jmap-chat-00 §4.6).
    pub sent_at: UTCDate,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A prior revision of a [`Message`] body, stored in edit history.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRevision {
    /// The `body` property (draft-atwood-jmap-chat-00 §4.5).
    pub body: String,
    /// The `bodyType` property (draft-atwood-jmap-chat-00 §4.5).
    pub body_type: String,
    /// The `editedAt` property (draft-atwood-jmap-chat-00 §4.5).
    pub edited_at: UTCDate,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Per-recipient delivery receipt for a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    /// The `deliveredAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<UTCDate>,
    /// The `deviceDeliveredAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_delivered_at: Option<UTCDate>,
    /// The `readAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<UTCDate>,
    /// The `readDisposition` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_disposition: Option<ReadDisposition>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single chat message as defined by the JMAP Chat extension.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.11).
    pub id: Id,
    /// The `senderMsgId` property (draft-atwood-jmap-chat-00 §4.11).
    pub sender_msg_id: Id,
    /// The `senderId` property (draft-atwood-jmap-chat-00 §4.11).
    pub sender_id: SenderId,
    /// The `chatId` property (draft-atwood-jmap-chat-00 §4.11).
    pub chat_id: Id,
    /// The `body` property (draft-atwood-jmap-chat-00 §4.11).
    pub body: String,
    /// The `bodyType` property (draft-atwood-jmap-chat-00 §4.11).
    pub body_type: String,
    /// The `attachments` property (draft-atwood-jmap-chat-00 §4.11).
    pub attachments: Vec<Attachment>,
    /// The `mentions` property (draft-atwood-jmap-chat-00 §4.11).
    pub mentions: Vec<Mention>,
    /// The `actions` property (draft-atwood-jmap-chat-00 §4.11).
    pub actions: Vec<MessageAction>,
    /// The `reactions` property (draft-atwood-jmap-chat-00 §4.11).
    pub reactions: HashMap<String, Reaction>,
    /// The `sentAt` property (draft-atwood-jmap-chat-00 §4.11).
    pub sent_at: UTCDate,
    /// The `receivedAt` property (draft-atwood-jmap-chat-00 §4.11).
    pub received_at: UTCDate,
    /// The `deliveryState` property (draft-atwood-jmap-chat-00 §4.11).
    pub delivery_state: DeliveryState,

    /// The `replyTo` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Id>,
    /// The `threadRootId` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_root_id: Option<Id>,
    /// The `replyCount` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_count: Option<u64>,
    /// The `unreadReplyCount` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread_reply_count: Option<u64>,
    /// The `senderExpiresAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_expires_at: Option<UTCDate>,
    /// The `burnOnRead` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burn_on_read: Option<bool>,
    /// The `deliveryReceipts` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_receipts: Option<HashMap<String, DeliveryReceipt>>,
    /// The `deliveredAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<UTCDate>,
    /// The `readAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<UTCDate>,
    /// The `readDisposition` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_disposition: Option<ReadDisposition>,
    /// The `editedAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<UTCDate>,
    /// The `editHistory` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_history: Option<Vec<MessageRevision>>,
    /// The `deletedAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<UTCDate>,
    /// The `deletedForAll` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_for_all: Option<bool>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Message {
    /// Construct a [`Message`] from its required fields.
    ///
    /// All optional fields default to `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        sender_msg_id: Id,
        sender_id: SenderId,
        chat_id: Id,
        body: impl Into<String>,
        body_type: impl Into<String>,
        attachments: Vec<Attachment>,
        mentions: Vec<Mention>,
        actions: Vec<MessageAction>,
        reactions: HashMap<String, Reaction>,
        sent_at: UTCDate,
        received_at: UTCDate,
        delivery_state: DeliveryState,
    ) -> Self {
        Self {
            id,
            sender_msg_id,
            sender_id,
            chat_id,
            body: body.into(),
            body_type: body_type.into(),
            attachments,
            mentions,
            actions,
            reactions,
            sent_at,
            received_at,
            delivery_state,
            reply_to: None,
            thread_root_id: None,
            reply_count: None,
            unread_reply_count: None,
            sender_expires_at: None,
            burn_on_read: None,
            delivery_receipts: None,
            delivered_at: None,
            read_at: None,
            read_disposition: None,
            edited_at: None,
            edit_history: None,
            deleted_at: None,
            deleted_for_all: None,
            extra: serde_json::Map::new(),
        }
    }
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
        assert_eq!(msg.sender_id, SenderId::Owner);
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
        assert_eq!(reaction.sender_id, SenderId::Contact("u99".to_owned()));
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
            extra: serde_json::Map::new(),
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
                "readAt": "2026-01-04T08:05:00Z",
                "readDisposition": "displayed"
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
        assert_eq!(u1.read_disposition, Some(ReadDisposition::Displayed));
        assert!(u1.device_delivered_at.is_none());
        let u2 = map.get("u2").expect("u2");
        assert!(u2.delivered_at.is_none());
        assert!(u2.read_disposition.is_none());

        let roundtrip = serde_json::to_string(&map).expect("serialize");
        let map2: HashMap<String, DeliveryReceipt> =
            serde_json::from_str(&roundtrip).expect("re-deserialize");
        assert_eq!(map, map2);
    }

    // Oracle: spec §ReadDisposition wire values (hand-crafted; verified against spec text).
    #[test]
    fn read_disposition_roundtrip() {
        let cases = [
            ("\"displayed\"", ReadDisposition::Displayed),
            ("\"deleted\"", ReadDisposition::Deleted),
            ("\"processed\"", ReadDisposition::Processed),
            (
                "\"voice-listened\"",
                ReadDisposition::Other("voice-listened".to_owned()),
            ),
        ];
        for (json_str, expected) in cases {
            let got: ReadDisposition =
                serde_json::from_str(json_str).expect("deserialize ReadDisposition");
            assert_eq!(got, expected, "deser {json_str}");
            let back = serde_json::to_string(&got).expect("serialize");
            assert_eq!(back, json_str, "reser {json_str}");
        }
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `Attachment.extra` captures vendor fields and preserves them.
    #[test]
    fn attachment_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "blobId": "b1",
            "filename": "a.png",
            "contentType": "image/png",
            "size": 100,
            "sha256": "0".repeat(64),
            "acmeCorpScanResult": "clean"
        });
        let a: Attachment = serde_json::from_value(raw).unwrap();
        assert_eq!(
            a.extra.get("acmeCorpScanResult").and_then(|v| v.as_str()),
            Some("clean")
        );
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["acmeCorpScanResult"], "clean");
    }

    /// `Mention.extra` captures vendor fields and preserves them.
    #[test]
    fn mention_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "u1",
            "offset": 0,
            "length": 5,
            "acmeCorpHighlight": "soft"
        });
        let m: Mention = serde_json::from_value(raw).unwrap();
        assert_eq!(
            m.extra.get("acmeCorpHighlight").and_then(|v| v.as_str()),
            Some("soft")
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["acmeCorpHighlight"], "soft");
    }

    /// `MessageAction.extra` captures vendor fields and preserves them.
    #[test]
    fn message_action_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "type": "button",
            "uri": "https://example.com",
            "acmeCorpDisplayPriority": 5
        });
        let a: MessageAction = serde_json::from_value(raw).unwrap();
        assert_eq!(
            a.extra
                .get("acmeCorpDisplayPriority")
                .and_then(|v| v.as_u64()),
            Some(5)
        );
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["acmeCorpDisplayPriority"], 5);
    }

    /// `Reaction.extra` captures vendor fields and preserves them.
    #[test]
    fn reaction_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "emoji": "👍",
            "senderId": "self",
            "sentAt": "2026-01-02T10:00:00Z",
            "acmeCorpClientUuid": "device-7"
        });
        let r: Reaction = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpClientUuid").and_then(|v| v.as_str()),
            Some("device-7")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpClientUuid"], "device-7");
    }

    /// `MessageRevision.extra` captures vendor fields and preserves them.
    #[test]
    fn message_revision_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "body": "v1",
            "bodyType": "text/plain",
            "editedAt": "2026-01-05T12:00:00Z",
            "acmeCorpEditReason": "typo"
        });
        let r: MessageRevision = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpEditReason").and_then(|v| v.as_str()),
            Some("typo")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpEditReason"], "typo");
    }

    /// `DeliveryReceipt.extra` captures vendor fields and preserves them.
    #[test]
    fn delivery_receipt_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "deliveredAt": "2026-01-04T08:00:00Z",
            "acmeCorpReceiptId": "rcpt-9"
        });
        let r: DeliveryReceipt = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpReceiptId").and_then(|v| v.as_str()),
            Some("rcpt-9")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpReceiptId"], "rcpt-9");
    }

    /// `Message.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn message_preserves_vendor_extras() {
        let raw = serde_json::json!({
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
            "deliveryState": "delivered",
            "acmeCorpRoutedVia": "edge-3"
        });
        let m: Message = serde_json::from_value(raw).unwrap();
        assert_eq!(
            m.extra.get("acmeCorpRoutedVia").and_then(|v| v.as_str()),
            Some("edge-3")
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["acmeCorpRoutedVia"], "edge-3");
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
