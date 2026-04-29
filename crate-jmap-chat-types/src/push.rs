use crate::chat::ChatKind;
use jmap_types::{Id, State, UTCDate};
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
    pub chat_kind: ChatKind,
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
/// `state` is a JMAP state token ([`State`]) — an opaque, comparable string token.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "@type", rename = "ChatMessagePush")]
#[serde(rename_all = "camelCase")]
pub struct ChatMessagePush {
    pub account_id: Id,
    pub state: State,
    pub messages: Vec<ChatMessageEntry>,
}

impl ChatMessagePush {
    /// Construct a [`ChatMessagePush`] from its required fields.
    pub fn new(account_id: Id, state: impl Into<State>, messages: Vec<ChatMessageEntry>) -> Self {
        Self {
            account_id,
            state: state.into(),
            messages,
        }
    }
}

impl ChatMessageEntry {
    /// Construct a plaintext [`ChatMessageEntry`] for push delivery.
    ///
    /// Sets `encrypted = false`. Pass `body_snippet = None` when the user has
    /// disabled message previews; pass `Some(snippet)` to include a preview.
    ///
    /// Use [`ChatMessageEntry::encrypted`] for encrypted messages — that
    /// constructor enforces `body_snippet = None` so plaintext cannot leak.
    #[allow(clippy::too_many_arguments)]
    pub fn plaintext(
        message_id: Id,
        chat_id: Id,
        chat_kind: ChatKind,
        sender_id: impl Into<String>,
        sent_at: UTCDate,
        has_mention: bool,
        has_mention_all: bool,
        body_snippet: Option<impl Into<String>>,
    ) -> Self {
        Self {
            message_id,
            chat_id,
            chat_kind,
            sender_id: sender_id.into(),
            sent_at,
            has_mention,
            has_mention_all,
            encrypted: false,
            body_snippet: body_snippet.map(Into::into),
            chat_name: None,
            space_id: None,
            space_name: None,
            sender_display_name: None,
        }
    }

    /// Construct an encrypted [`ChatMessageEntry`] for push delivery.
    ///
    /// Sets `encrypted = true` and `body_snippet = None`. The spec forbids
    /// body content in encrypted push entries; this constructor makes the
    /// invariant impossible to violate.
    pub fn encrypted(
        message_id: Id,
        chat_id: Id,
        chat_kind: ChatKind,
        sender_id: impl Into<String>,
        sent_at: UTCDate,
        has_mention: bool,
        has_mention_all: bool,
    ) -> Self {
        Self {
            message_id,
            chat_id,
            chat_kind,
            sender_id: sender_id.into(),
            sent_at,
            has_mention,
            has_mention_all,
            encrypted: true,
            body_snippet: None,
            chat_name: None,
            space_id: None,
            space_name: None,
            sender_display_name: None,
        }
    }
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
        assert_eq!(push.state.as_ref(), "d35ecb040aab");
        assert_eq!(push.messages.len(), 1);
        let entry = &push.messages[0];
        assert_eq!(entry.message_id.as_ref(), "msg1");
        assert_eq!(entry.chat_kind, ChatKind::Channel);
        assert!(entry.has_mention);
    }

    // Oracle: serde tag contract — serialized ChatMessagePush must carry "@type":"ChatMessagePush".
    #[test]
    fn push_type_tag() {
        let push = ChatMessagePush {
            account_id: Id::from("u1"),
            state: State::from("abc123"),
            messages: vec![],
        };
        let json = serde_json::to_string(&push).expect("serialize ChatMessagePush");
        assert!(
            json.contains(r#""@type":"ChatMessagePush""#),
            "expected @type tag in output, got: {json}"
        );
    }

    // Oracle: encrypted() constructor must enforce encrypted=true and body_snippet=None.
    // Verifies the invariant: spec forbids body content in encrypted push entries.
    #[test]
    fn encrypted_entry_has_no_snippet() {
        let entry = ChatMessageEntry::encrypted(
            Id::from("m1"),
            Id::from("c1"),
            ChatKind::Channel,
            "alice",
            UTCDate::from("2026-04-29T00:00:00Z"),
            false,
            false,
        );
        assert!(entry.encrypted);
        assert!(
            entry.body_snippet.is_none(),
            "encrypted entry must have no body_snippet"
        );
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(
            !json.contains("bodySnippet"),
            "bodySnippet must be absent in encrypted entry, got: {json}"
        );
    }

    // Oracle: plaintext() sets encrypted=false and passes body_snippet through.
    #[test]
    fn plaintext_entry_carries_snippet() {
        let entry = ChatMessageEntry::plaintext(
            Id::from("m2"),
            Id::from("c1"),
            ChatKind::Direct,
            "bob",
            UTCDate::from("2026-04-29T00:00:00Z"),
            false,
            false,
            Some("Hello!"),
        );
        assert!(!entry.encrypted);
        assert_eq!(entry.body_snippet.as_deref(), Some("Hello!"));
    }

    // Oracle: plaintext() with body_snippet=None is valid (user has previews disabled).
    #[test]
    fn plaintext_entry_no_snippet() {
        let entry = ChatMessageEntry::plaintext(
            Id::from("m3"),
            Id::from("c1"),
            ChatKind::Direct,
            "carol",
            UTCDate::from("2026-04-29T00:00:00Z"),
            false,
            false,
            None::<String>,
        );
        assert!(!entry.encrypted);
        assert!(entry.body_snippet.is_none());
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
        assert_eq!(entry.chat_kind, ChatKind::Direct);
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
