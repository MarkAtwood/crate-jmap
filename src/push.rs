use jmap_types::{Id, UTCDate};
use serde::{Deserialize, Serialize};

/// Client-supplied filter controlling which push notifications are delivered.
///
/// All fields are optional; omitting a field uses the server default.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatPushConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_ids: Option<Vec<Id>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention_urgency: Option<String>,
}

/// Summary of a single message delivered via push notification.
///
/// Per spec, `body_snippet` MUST be `None` when `encrypted` is `true`.
/// The type does not enforce this — it is a caller invariant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageEntry {
    pub message_id: Id,
    pub chat_id: Id,
    pub chat_kind: String,
    pub sender_id: String,
    pub sent_at: UTCDate,
    pub has_mention: bool,
    pub has_mention_all: bool,
    pub encrypted: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_display_name: Option<String>,
    /// Per spec, this field MUST be `None` when `encrypted` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_snippet: Option<String>,
}

/// Push payload carrying one or more new message summaries for an account.
///
/// The wire format includes `"@type": "ChatMessagePush"` as a discriminant.
/// `state` is a JMAP state token (opaque string), not an [`Id`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "@type", rename = "ChatMessagePush")]
#[serde(rename_all = "camelCase")]
pub struct ChatMessagePush {
    pub account_id: Id,
    pub state: String,
    pub messages: Vec<ChatMessageEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: hand-crafted JSON matching the spec full example.
    // Asserts field mapping (camelCase → snake_case) and nested entry fields.
    #[test]
    fn push_deser_full() {
        let json = r#"{"@type":"ChatMessagePush","accountId":"u1","state":"d35ecb040aab","messages":[{"messageId":"msg1","chatId":"c1","chatKind":"channel","chatName":"general","spaceId":"sp1","spaceName":"ACME","senderId":"alice","senderDisplayName":"Alice","sentAt":"2026-04-26T14:32:00Z","hasMention":true,"hasMentionAll":false,"encrypted":false,"bodySnippet":"Hello"}]}"#;
        let push: ChatMessagePush =
            serde_json::from_str(json).expect("deserialize ChatMessagePush");
        assert_eq!(push.account_id.as_ref(), "u1");
        assert_eq!(push.state, "d35ecb040aab");
        assert_eq!(push.messages.len(), 1);
        let entry = &push.messages[0];
        assert_eq!(entry.message_id.as_ref(), "msg1");
        assert_eq!(entry.chat_kind, "channel");
        assert!(entry.has_mention);
    }

    // Oracle: serde tag contract — serialized ChatMessagePush must carry "@type":"ChatMessagePush".
    #[test]
    fn push_type_tag() {
        let push = ChatMessagePush {
            account_id: Id::from("u1"),
            state: "abc123".to_string(),
            messages: vec![],
        };
        let json = serde_json::to_string(&push).expect("serialize ChatMessagePush");
        assert!(
            json.contains(r#""@type":"ChatMessagePush""#),
            "expected @type tag in output, got: {json}"
        );
    }

    // Oracle: hand-crafted minimal direct-chat entry with no chat_name;
    // verifies that chat_name deserializes as None when absent.
    #[test]
    fn push_entry_direct_no_name() {
        let json = r#"{
            "messageId": "m2",
            "chatId": "c2",
            "chatKind": "direct",
            "senderId": "bob",
            "sentAt": "2026-04-27T10:00:00Z",
            "hasMention": false,
            "hasMentionAll": false,
            "encrypted": false
        }"#;
        let entry: ChatMessageEntry =
            serde_json::from_str(json).expect("deserialize ChatMessageEntry");
        assert_eq!(entry.chat_kind, "direct");
        assert!(entry.chat_name.is_none());
    }

    // Oracle: ChatPushConfig with all fields None must serialize to `{}`.
    // Verifies skip_serializing_if = "Option::is_none" on every field.
    #[test]
    fn push_config_all_none() {
        let cfg = ChatPushConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize ChatPushConfig");
        assert_eq!(
            json, "{}",
            "all-None ChatPushConfig must serialize to {{}}; got: {json}"
        );
    }
}
