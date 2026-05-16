//! Push notification payload types for chat message delivery.

use crate::chat::ChatKind;
use crate::message::SenderId;
use jmap_types::{Id, State, UTCDate};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Client-supplied filter controlling which push notifications are delivered.
///
/// All fields are optional; omitting a field uses the server default.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatPushConfig {
    /// The `kinds` property (draft-atwood-jmap-chat-push-00 §4.1).
    ///
    /// Typed against [`ChatKind`] so consumers can dispatch on the
    /// known kinds without re-parsing wire strings. Unknown wire
    /// values land in `ChatKind::Other(_)` for forward-compat and
    /// round-trip verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<ChatKind>>,
    /// The `chatIds` property (draft-atwood-jmap-chat-push-00 §4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_ids: Option<Vec<Id>>,
    /// The `properties` property (draft-atwood-jmap-chat-push-00 §4.1).
    ///
    /// Left as `Vec<String>` rather than retyped to one of the
    /// per-object `*Property` enums in `backend.rs`. Three reasons:
    /// (1) the same field admits both `Chat`-level and
    /// `Message`-level property names in the same list, so it has
    /// no single typed home; (2) the property-selector enums lack
    /// an `Other(String)` forward-compat arm, so retyping would
    /// turn unknown property names into a deserialize failure
    /// rather than the current preserve-and-forward behaviour;
    /// (3) the field is consumed by the server as a projection
    /// hint, not as authorisation-grade input, so primitive
    /// obsession is bounded here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Vec<String>>,
    /// The `urgency` property (draft-atwood-jmap-chat-push-00 §4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency: Option<String>,
    /// The `mentionUrgency` property (draft-atwood-jmap-chat-push-00 §4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention_urgency: Option<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Summary of a single message delivered via push notification.
///
/// Per spec, `body_snippet` MUST be `None` when `encrypted` is `true`.
/// The type does not enforce this — it is a caller invariant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageEntry {
    /// The `messageId` property (draft-atwood-jmap-chat-push-00 §5.2).
    pub message_id: Id,
    /// The `chatId` property (draft-atwood-jmap-chat-push-00 §5.2).
    pub chat_id: Id,
    /// The `chatKind` property (draft-atwood-jmap-chat-push-00 §5.2).
    pub chat_kind: ChatKind,
    /// The `senderId` property (draft-atwood-jmap-chat-push-00 §5.2).
    ///
    /// Typed identically to [`crate::Message::sender_id`] and
    /// [`crate::Reaction::sender_id`]: `SenderId::Owner` for messages
    /// the account owner composed, `SenderId::Contact(<id>)`
    /// otherwise. See [`SenderId`] for the wire `"self"` sentinel
    /// and its collision caveat.
    pub sender_id: SenderId,
    /// The `sentAt` property (draft-atwood-jmap-chat-push-00 §5.2).
    pub sent_at: UTCDate,
    /// The `hasMention` property (draft-atwood-jmap-chat-push-00 §5.2).
    pub has_mention: bool,
    /// The `hasMentionAll` property (draft-atwood-jmap-chat-push-00 §5.2).
    pub has_mention_all: bool,
    /// The `encrypted` property (draft-atwood-jmap-chat-push-00 §5.2).
    pub encrypted: bool,

    /// The `chatName` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_name: Option<String>,
    /// The `spaceId` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
    /// The `spaceName` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_name: Option<String>,
    /// The `senderDisplayName` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_display_name: Option<String>,
    /// The `bodySnippet` property (draft-atwood-jmap-chat-push-00 §5.2).
    ///
    /// Per spec, this field MUST be `None` when `encrypted` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_snippet: Option<String>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Push payload carrying one or more new message summaries for an account.
///
/// The wire format includes `"@type": "ChatMessagePush"` as a discriminant.
/// `state` is a JMAP state token ([`State`]) — an opaque, comparable string token.
///
/// # Serialize / Deserialize
///
/// The `@type` discriminant is emitted on serialize and consumed on
/// deserialize via a hand-rolled (De)Serialize pair rather than
/// `#[serde(tag = "...")]`. The derive-based approach interacts
/// pathologically with `#[serde(flatten)]` on the
/// [`extra`](Self::extra) catch-all: the tag string ends up in
/// both the tag slot AND the flatten map, producing duplicate
/// `@type` keys on re-serialise. The hand-rolled impl strips
/// `@type` from the deserialised map before populating `extra` so
/// the catch-all does not capture the discriminant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessagePush {
    /// The `accountId` property (draft-atwood-jmap-chat-push-00 §5.1).
    pub account_id: Id,
    /// The `state` property (draft-atwood-jmap-chat-push-00 §5.1).
    pub state: State,
    /// The `messages` property (draft-atwood-jmap-chat-push-00 §5.1).
    pub messages: Vec<ChatMessageEntry>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md). The `@type` discriminant is
    /// stripped before population so callers can rely on
    /// `extras.get("@type").is_none()`.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Wire-tag value for [`ChatMessagePush`]. Used by both
/// (De)Serialize impls; the single source of truth for the
/// `@type` discriminant.
const CHAT_MESSAGE_PUSH_TYPE: &str = "ChatMessagePush";

/// Helper: derive-shaped twin of [`ChatMessagePush`] without the
/// `@type` machinery. Used as an internal serde target so the
/// hand-rolled (De)Serialize impls can defer to derived
/// (de)serialise for the typed fields and only handle the
/// `@type` injection / stripping themselves.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatMessagePushFields {
    account_id: Id,
    state: State,
    messages: Vec<ChatMessageEntry>,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    extra: serde_json::Map<String, serde_json::Value>,
}

impl Serialize for ChatMessagePush {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let twin = ChatMessagePushFields {
            account_id: self.account_id.clone(),
            state: self.state.clone(),
            messages: self.messages.clone(),
            extra: self.extra.clone(),
        };
        let mut value = serde_json::to_value(&twin).map_err(serde::ser::Error::custom)?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert(
                "@type".to_owned(),
                serde_json::Value::String(CHAT_MESSAGE_PUSH_TYPE.to_owned()),
            );
        }
        value.serialize(s)
    }
}

impl<'de> Deserialize<'de> for ChatMessagePush {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
        // Strip and verify @type before deserialising the typed
        // fields. If @type is present it MUST equal "ChatMessagePush";
        // a mismatch is a wire-protocol error.
        match map.remove("@type") {
            Some(serde_json::Value::String(s)) if s == CHAT_MESSAGE_PUSH_TYPE => {}
            Some(serde_json::Value::String(s)) => {
                return Err(D::Error::custom(format!(
                    "@type must be \"{CHAT_MESSAGE_PUSH_TYPE}\", found \"{s}\""
                )));
            }
            Some(other) => {
                return Err(D::Error::custom(format!(
                    "@type must be a string, found {other}"
                )));
            }
            None => {
                // @type absent — accept for backwards-compat with
                // deserialisers that already routed the value
                // here via outer dispatch. The derive-shaped
                // serialise always emits it on the way out.
            }
        }
        let twin: ChatMessagePushFields =
            serde_json::from_value(serde_json::Value::Object(map)).map_err(D::Error::custom)?;
        Ok(ChatMessagePush {
            account_id: twin.account_id,
            state: twin.state,
            messages: twin.messages,
            extra: twin.extra,
        })
    }
}

impl ChatMessagePush {
    /// Construct a [`ChatMessagePush`] from its required fields.
    pub fn new(account_id: Id, state: impl Into<State>, messages: Vec<ChatMessageEntry>) -> Self {
        Self {
            account_id,
            state: state.into(),
            messages,
            extra: serde_json::Map::new(),
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
        sender_id: SenderId,
        sent_at: UTCDate,
        has_mention: bool,
        has_mention_all: bool,
        body_snippet: Option<impl Into<String>>,
    ) -> Self {
        Self {
            message_id,
            chat_id,
            chat_kind,
            sender_id,
            sent_at,
            has_mention,
            has_mention_all,
            encrypted: false,
            body_snippet: body_snippet.map(Into::into),
            chat_name: None,
            space_id: None,
            space_name: None,
            sender_display_name: None,
            extra: serde_json::Map::new(),
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
        sender_id: SenderId,
        sent_at: UTCDate,
        has_mention: bool,
        has_mention_all: bool,
    ) -> Self {
        Self {
            message_id,
            chat_id,
            chat_kind,
            sender_id,
            sent_at,
            has_mention,
            has_mention_all,
            encrypted: true,
            body_snippet: None,
            chat_name: None,
            space_id: None,
            space_name: None,
            sender_display_name: None,
            extra: serde_json::Map::new(),
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
            extra: serde_json::Map::new(),
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
            SenderId::Contact("alice".to_owned()),
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
            SenderId::Contact("bob".to_owned()),
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
            SenderId::Contact("carol".to_owned()),
            UTCDate::from("2026-04-29T00:00:00Z"),
            false,
            false,
            None::<String>,
        );
        assert!(!entry.encrypted);
        assert!(entry.body_snippet.is_none());
    }

    /// Oracle: the parallel SenderId routing test for push entries.
    /// A push entry whose wire `senderId` is `"self"` decodes to
    /// `SenderId::Owner`, in parity with Message, Reaction, and
    /// ChatTypingEvent. Push consumers can then echo-suppress the
    /// account owner's own messages without raw-string comparison.
    #[test]
    fn push_entry_self_sentinel_routes_to_owner() {
        let json = r#"{
            "messageId": "m4",
            "chatId": "c1",
            "chatKind": "direct",
            "senderId": "self",
            "sentAt": "2026-04-29T00:00:00Z",
            "hasMention": false,
            "hasMentionAll": false,
            "encrypted": false
        }"#;
        let entry: ChatMessageEntry =
            serde_json::from_str(json).expect("deserialize ChatMessageEntry");
        assert_eq!(entry.sender_id, SenderId::Owner);
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

    /// Oracle: a ChatPushConfig with typed `kinds` round-trips
    /// byte-equivalently against the wire form a JMAP client would
    /// emit before this retype. Asserts that wire-format compatibility
    /// is preserved when the field shape moves from `Vec<String>` to
    /// `Vec<ChatKind>`.
    #[test]
    fn push_config_kinds_typed_round_trip() {
        let wire = r#"{"kinds":["direct","channel","custom-vendor-kind"]}"#;
        let cfg: ChatPushConfig = serde_json::from_str(wire).expect("deserialize ChatPushConfig");
        assert_eq!(
            cfg.kinds,
            Some(vec![
                ChatKind::Direct,
                ChatKind::Channel,
                ChatKind::Other("custom-vendor-kind".to_owned()),
            ])
        );
        let back = serde_json::to_string(&cfg).expect("serialize ChatPushConfig");
        assert_eq!(back, wire);
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

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `ChatPushConfig.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_push_config_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "kinds": ["channel"],
            "acmeCorpQuietHours": "22-08"
        });
        let cfg: ChatPushConfig = serde_json::from_value(raw).unwrap();
        assert_eq!(
            cfg.extra.get("acmeCorpQuietHours").and_then(|v| v.as_str()),
            Some("22-08")
        );
        let back = serde_json::to_value(&cfg).unwrap();
        assert_eq!(back["acmeCorpQuietHours"], "22-08");
    }

    /// `ChatMessageEntry.extra` captures vendor fields and preserves them.
    #[test]
    fn chat_message_entry_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "messageId": "m1",
            "chatId": "c1",
            "chatKind": "channel",
            "senderId": "alice",
            "sentAt": "2026-04-26T14:32:00Z",
            "hasMention": false,
            "hasMentionAll": false,
            "encrypted": false,
            "acmeCorpThreadDepth": 3
        });
        let entry: ChatMessageEntry = serde_json::from_value(raw).unwrap();
        assert_eq!(
            entry
                .extra
                .get("acmeCorpThreadDepth")
                .and_then(|v| v.as_u64()),
            Some(3)
        );
        let back = serde_json::to_value(&entry).unwrap();
        assert_eq!(back["acmeCorpThreadDepth"], 3);
    }

    /// `ChatMessagePush.extra` captures vendor fields and preserves them.
    /// The `@type` tag is consumed by the struct's `#[serde(tag)]`
    /// directive; the test below ASSERTS @type is NOT captured by
    /// extra (which would otherwise cause duplicate keys on
    /// re-serialise). Without that negative assertion, a future
    /// serde regression on the tag-vs-flatten interaction would
    /// pass the old "vendor field round-trips" check but corrupt
    /// the wire payload.
    #[test]
    fn chat_message_push_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "@type": "ChatMessagePush",
            "accountId": "u1",
            "state": "abc",
            "messages": [],
            "acmeCorpBatchId": "batch-7"
        });
        let push: ChatMessagePush = serde_json::from_value(raw).unwrap();

        // Positive: vendor field is captured.
        assert_eq!(
            push.extra.get("acmeCorpBatchId").and_then(|v| v.as_str()),
            Some("batch-7")
        );

        // Negative: the tag-vs-flatten interaction excludes @type
        // from extras. A regression here would emit duplicate @type
        // keys on re-serialise.
        assert!(
            push.extra.get("@type").is_none(),
            "@type must not leak into extras; got: {:?}",
            push.extra
        );

        // Round-trip preserves the vendor field AND emits @type
        // exactly once at the top level.
        let back = serde_json::to_value(&push).unwrap();
        assert_eq!(back["acmeCorpBatchId"], "batch-7");
        assert_eq!(back["@type"], "ChatMessagePush");

        // Serialise to a string and verify @type appears exactly
        // once. JSON forbids duplicate keys at the parser level,
        // but serializers MAY emit duplicates; this asserts the
        // emitted bytes are well-formed.
        let json = serde_json::to_string(&push).unwrap();
        let occurrences = json.matches(r#""@type":"#).count();
        assert_eq!(
            occurrences, 1,
            "@type must appear exactly once in serialised output; got {occurrences} in {json}"
        );
    }
}
