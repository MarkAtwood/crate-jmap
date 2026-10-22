//! Push notification payload types for chat message delivery.

use crate::chat::ChatKind;
use crate::message::BroadcastScope;
use jmap_types::{impl_string_enum, Id, State, UTCDate, Urgency};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A client-supplied Web Push urgency in [`ChatPushConfig`]
/// (draft-atwood-jmap-chat-push-00 §ChatPushConfig, RFC 8030 Section 5.3).
///
/// `Other` keeps a value outside RFC 8030 so a server can name it in an
/// `invalidProperties` SetError ("If the client supplies a value not in
/// `supportedUrgencyValues`, the server MUST return an `invalidProperties`
/// SetError"). Everything the server advertises or sends uses the closed
/// [`Urgency`]; [`to_urgency`](Self::to_urgency) converts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UrgencyLevel {
    /// Lowest priority (e.g. ad-hoc background sync hints).
    VeryLow,
    /// Low priority.
    Low,
    /// Default priority (the spec floor — servers MUST support this).
    Normal,
    /// High priority (e.g. mention-triggered notifications).
    High,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(UrgencyLevel, "a web-push urgency string",
    "very-low" => VeryLow,
    "low" => Low,
    "normal" => Normal,
    "high" => High,
);

impl UrgencyLevel {
    /// The RFC 8030 [`Urgency`] for this value, or `None` for `Other`.
    pub fn to_urgency(&self) -> Option<Urgency> {
        match self {
            Self::VeryLow => Some(Urgency::VeryLow),
            Self::Low => Some(Urgency::Low),
            Self::Normal => Some(Urgency::Normal),
            Self::High => Some(Urgency::High),
            Self::Other(_) => None,
        }
    }
}

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
    ///
    /// Typed against [`UrgencyLevel`]; unknown wire values are
    /// preserved verbatim via `UrgencyLevel::Other(_)` for
    /// forward-compat with future spec revisions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency: Option<UrgencyLevel>,
    /// The `mentionUrgency` property (draft-atwood-jmap-chat-push-00 §4.1).
    ///
    /// Same typing and forward-compat contract as
    /// [`urgency`](Self::urgency).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention_urgency: Option<UrgencyLevel>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Summary of a single message delivered via push notification.
///
/// Every field is optional on the wire. draft-atwood-jmap-chat-push-00
/// §ChatPushConfig: "The `properties` list is authoritative: the server
/// returns exactly the requested fields." A field the subscription did not
/// request is `None` and is not serialized.
///
/// Per spec, `body_snippet`, `has_mention` and `mention_scopes` MUST be
/// `None` when `encrypted` is `Some(true)`. The type does not enforce this;
/// [`ChatMessageEntry::encrypted`] does.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageEntry {
    /// The `messageId` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<Id>,
    /// The `chatId` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Id>,
    /// The `chatKind` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_kind: Option<ChatKind>,
    /// The `senderId` property (draft-atwood-jmap-chat-push-00 §5.2).
    ///
    /// The accountId for owner-composed messages, otherwise the
    /// sender's `ChatContact.id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<Id>,
    /// The `sentAt` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<UTCDate>,
    /// The `hasMention` property (draft-atwood-jmap-chat-push-00 §5.2).
    ///
    /// Per spec, this field MUST be `None` when `encrypted` is `Some(true)`:
    /// mentions are inside the ciphertext, so the server cannot know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_mention: Option<bool>,
    /// The `mentionScopes` property (draft-atwood-jmap-chat-push-00 §5.2).
    ///
    /// Broadcast scopes by which the account owner was targeted. Per
    /// spec, this field MUST be `None` when `encrypted` is `Some(true)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention_scopes: Option<Vec<BroadcastScope>>,
    /// The `encrypted` property (draft-atwood-jmap-chat-push-00 §5.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,

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
    ///
    /// For channel chats, derived from the sender's `SpaceMember.nick` in
    /// the containing Space if present, otherwise the sender ChatContact's
    /// `displayName`, otherwise its `login`, otherwise its `identity`. For
    /// direct and group chats, the same chain without the `nick` step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_display_name: Option<String>,
    /// The `bodySnippet` property (draft-atwood-jmap-chat-push-00 §5.2).
    ///
    /// Per spec, this field MUST be `None` when `encrypted` is `Some(true)`.
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
    /// The `unreadCount` property (draft-atwood-jmap-chat-push-00 §5.1).
    ///
    /// The account owner's total unread message count across all non-muted
    /// Chats when the push was generated. When `None`, the client derives
    /// the badge count via `Chat/get`. It belongs to the push as a whole,
    /// not to any entry, and `ChatPushConfig.properties` does not govern it.
    pub unread_count: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unread_count: Option<u64>,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    extra: serde_json::Map<String, serde_json::Value>,
}

impl Serialize for ChatMessagePush {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let twin = ChatMessagePushFields {
            account_id: self.account_id.clone(),
            state: self.state.clone(),
            messages: self.messages.clone(),
            unread_count: self.unread_count,
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
            unread_count: twin.unread_count,
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
            unread_count: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl ChatMessageEntry {
    /// Construct a plaintext [`ChatMessageEntry`] for push delivery.
    ///
    /// Sets every base field and `encrypted = Some(false)`. Pass `body_snippet = None` when the user has
    /// disabled message previews; pass `Some(snippet)` to include a preview.
    ///
    /// Use [`ChatMessageEntry::encrypted`] for encrypted messages — that
    /// constructor enforces `body_snippet = None` so plaintext cannot leak.
    #[allow(clippy::too_many_arguments)]
    pub fn plaintext(
        message_id: Id,
        chat_id: Id,
        chat_kind: ChatKind,
        sender_id: Id,
        sent_at: UTCDate,
        has_mention: bool,
        mention_scopes: Vec<BroadcastScope>,
        body_snippet: Option<impl Into<String>>,
    ) -> Self {
        Self {
            message_id: Some(message_id),
            chat_id: Some(chat_id),
            chat_kind: Some(chat_kind),
            sender_id: Some(sender_id),
            sent_at: Some(sent_at),
            has_mention: Some(has_mention),
            mention_scopes: Some(mention_scopes),
            encrypted: Some(false),
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
    /// Sets every base field and `encrypted = Some(true)`, and leaves `body_snippet`, `has_mention`, and
    /// `mention_scopes` as `None`. The spec forbids body content and mention
    /// information in encrypted push entries; this constructor makes the
    /// invariant impossible to violate.
    pub fn encrypted(
        message_id: Id,
        chat_id: Id,
        chat_kind: ChatKind,
        sender_id: Id,
        sent_at: UTCDate,
    ) -> Self {
        Self {
            message_id: Some(message_id),
            chat_id: Some(chat_id),
            chat_kind: Some(chat_kind),
            sender_id: Some(sender_id),
            sent_at: Some(sent_at),
            has_mention: None,
            mention_scopes: None,
            encrypted: Some(true),
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
        let json = r#"{"@type":"ChatMessagePush","accountId":"u1","state":"d35ecb040aab","messages":[{"messageId":"msg1","chatId":"c1","chatKind":"channel","chatName":"general","spaceId":"sp1","spaceName":"ACME","senderId":"alice","senderDisplayName":"Alice","sentAt":"2026-04-26T14:32:00Z","hasMention":true,"mentionScopes":[],"encrypted":false,"bodySnippet":"Hello"}]}"#;
        let push: ChatMessagePush =
            serde_json::from_str(json).expect("deserialize ChatMessagePush");
        assert_eq!(push.account_id.as_ref(), "u1");
        assert_eq!(push.state.as_ref(), "d35ecb040aab");
        assert_eq!(push.messages.len(), 1);
        let entry = &push.messages[0];
        assert_eq!(entry.message_id, Some(Id::from("msg1")));
        assert_eq!(entry.chat_kind, Some(ChatKind::Channel));
        assert_eq!(entry.has_mention, Some(true));
    }

    // Oracle: the Example Payload in draft-atwood-jmap-chat-push-00, verbatim.
    #[test]
    fn push_deser_spec_example() {
        let json = r#"{
          "@type": "ChatMessagePush",
          "accountId": "u1",
          "state": "d35ecb040aab",
          "messages": [
            {
              "messageId": "01J3YKZQP5MWVT8PPBEHTJ3HX",
              "chatId": "01J3XKZQN4MWVT8PPBEHTJ3HX",
              "chatKind": "channel",
              "chatName": "general",
              "spaceId": "01J2WKZQM3LVST7OOBDHSI2GW",
              "spaceName": "ACME Corp",
              "senderId": "c8xq2m",
              "senderDisplayName": "Alice",
              "sentAt": "2026-04-26T14:32:00Z",
              "hasMention": true,
              "mentionScopes": [],
              "encrypted": false,
              "bodySnippet": "Hey @bob, the deploy is ready for review"
            }
          ]
        }"#;
        let push: ChatMessagePush = serde_json::from_str(json).expect("deserialize spec example");
        let entry = &push.messages[0];
        assert_eq!(entry.has_mention, Some(true));
        assert_eq!(entry.mention_scopes.as_deref(), Some(&[][..]));
        assert!(entry.extra.is_empty(), "no spec field may land in extra");
    }

    // Oracle: draft-atwood-jmap-chat-push-00 End-to-End Encrypted Deployments:
    // the server MUST omit hasMention and mentionScopes when encrypted is true.
    #[test]
    fn encrypted_entry_without_mention_fields_deserializes() {
        let json = r#"{
            "messageId": "m5",
            "chatId": "c1",
            "chatKind": "group",
            "senderId": "alice",
            "sentAt": "2026-04-29T00:00:00Z",
            "encrypted": true
        }"#;
        let entry: ChatMessageEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(entry.has_mention, None);
        assert_eq!(entry.mention_scopes, None);
    }

    // Oracle: draft-atwood-jmap-chat-push-00 §ChatMessagePush mentionScopes:
    // "Each element is one of "everyone", "here", or "admins"". Hand-written
    // JSON; an unknown element is kept as `Other` and serialized back.
    #[test]
    fn mention_scopes_decode_typed_and_keep_unknown() {
        let json = r#"{"mentionScopes":["here","admins","later"]}"#;
        let entry: ChatMessageEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            entry.mention_scopes,
            Some(vec![
                BroadcastScope::Here,
                BroadcastScope::Admins,
                BroadcastScope::Other("later".into()),
            ])
        );
        assert_eq!(serde_json::to_string(&entry).expect("serialize"), json);
    }

    // Oracle: serde tag contract — serialized ChatMessagePush must carry "@type":"ChatMessagePush".
    #[test]
    fn push_type_tag() {
        let push = ChatMessagePush {
            account_id: Id::from("u1"),
            state: State::from("abc123"),
            messages: vec![],
            unread_count: None,
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
            Id::from("alice"),
            UTCDate::from("2026-04-29T00:00:00Z"),
        );
        assert_eq!(entry.encrypted, Some(true));
        assert!(
            entry.body_snippet.is_none(),
            "encrypted entry must have no body_snippet"
        );
        let json = serde_json::to_string(&entry).expect("serialize");
        for key in ["bodySnippet", "hasMention", "mentionScopes"] {
            assert!(
                !json.contains(key),
                "{key} must be absent in encrypted entry, got: {json}"
            );
        }
    }

    // Oracle: plaintext() sets encrypted=false and passes body_snippet through.
    #[test]
    fn plaintext_entry_carries_snippet() {
        let entry = ChatMessageEntry::plaintext(
            Id::from("m2"),
            Id::from("c1"),
            ChatKind::Direct,
            Id::from("bob"),
            UTCDate::from("2026-04-29T00:00:00Z"),
            false,
            vec![],
            Some("Hello!"),
        );
        assert_eq!(entry.encrypted, Some(false));
        assert_eq!(entry.body_snippet.as_deref(), Some("Hello!"));
    }

    // Oracle: plaintext() with body_snippet=None is valid (user has previews disabled).
    #[test]
    fn plaintext_entry_no_snippet() {
        let entry = ChatMessageEntry::plaintext(
            Id::from("m3"),
            Id::from("c1"),
            ChatKind::Direct,
            Id::from("carol"),
            UTCDate::from("2026-04-29T00:00:00Z"),
            false,
            vec![],
            None::<String>,
        );
        assert_eq!(entry.encrypted, Some(false));
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
            "mentionScopes": [],
            "encrypted": false
        }"#;
        let entry: ChatMessageEntry =
            serde_json::from_str(json).expect("deserialize ChatMessageEntry");
        assert_eq!(entry.chat_kind, Some(ChatKind::Direct));
        assert!(entry.chat_name.is_none());
    }

    /// Oracle: typed UrgencyLevel round-trips against the four
    /// spec-defined wire values plus an unknown forward-compat
    /// value preserved via Other(_). Wire form is byte-equivalent
    /// to what a JMAP client would emit before this retype.
    #[test]
    fn push_config_urgency_typed_round_trip() {
        let cases = [
            ("very-low", UrgencyLevel::VeryLow),
            ("low", UrgencyLevel::Low),
            ("normal", UrgencyLevel::Normal),
            ("high", UrgencyLevel::High),
        ];
        for (wire, expected) in cases {
            let json = format!(r#"{{"urgency":"{wire}"}}"#);
            let cfg: ChatPushConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(cfg.urgency, Some(expected.clone()), "wire {wire:?}");
            let back = serde_json::to_string(&cfg).unwrap();
            assert_eq!(back, json, "round-trip wire {wire:?}");
        }

        // Unknown wire value preserved via Other(_).
        let json = r#"{"urgency":"insanely-high"}"#;
        let cfg: ChatPushConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            cfg.urgency,
            Some(UrgencyLevel::Other("insanely-high".to_owned()))
        );
        let back = serde_json::to_string(&cfg).unwrap();
        assert_eq!(back, json);
    }

    /// Oracle: RFC 8030 §5.3 `urgency-option` strings. Each wire value
    /// parsed as a `ChatPushConfig.urgency` maps to the `Urgency` whose
    /// header value is the same string; a value outside the grammar has
    /// no `Urgency`.
    #[test]
    fn urgency_level_to_urgency_matches_header_value() {
        for wire in ["very-low", "low", "normal", "high"] {
            let level: UrgencyLevel = serde_json::from_value(serde_json::json!(wire)).unwrap();
            assert_eq!(level.to_urgency().map(Urgency::as_str), Some(wire));
        }
        let other: UrgencyLevel = serde_json::from_value(serde_json::json!("urgent")).unwrap();
        assert_eq!(other.to_urgency(), None);
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
            "mentionScopes": [],
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

    /// draft-atwood-jmap-chat-push-00 L149-150 (§ChatMessagePush):
    /// "`unreadCount` (UnsignedInt|null, optional): ... It is a property
    /// of the push as a whole, not of any one entry". A top-level value is
    /// typed, not left in `extra`; an absent one stays absent on the wire.
    #[test]
    fn chat_message_push_unread_count_typed_and_optional() {
        let raw = serde_json::json!({
            "@type": "ChatMessagePush",
            "accountId": "u1",
            "state": "s1",
            "messages": [{ "messageId": "m1" }],
            "unreadCount": 7
        });
        let push: ChatMessagePush = serde_json::from_value(raw).unwrap();
        assert_eq!(push.unread_count, Some(7));
        assert!(push.extra.is_empty());
        assert_eq!(serde_json::to_value(&push).unwrap()["unreadCount"], 7);

        let raw = serde_json::json!({
            "@type": "ChatMessagePush",
            "accountId": "u1",
            "state": "s1",
            "messages": [{ "messageId": "m1" }]
        });
        let push: ChatMessagePush = serde_json::from_value(raw).unwrap();
        assert_eq!(push.unread_count, None);
        assert!(serde_json::to_value(&push)
            .unwrap()
            .get("unreadCount")
            .is_none());
    }

    /// draft-atwood-jmap-chat-push-00 L153-155: `unreadCount` is not in
    /// the §ChatMessageEntry field list, so on an entry it is an unknown
    /// field and is preserved in `extra`.
    #[test]
    fn chat_message_entry_unread_count_is_extra() {
        let raw = serde_json::json!({ "messageId": "m1", "unreadCount": 7 });
        let entry: ChatMessageEntry = serde_json::from_value(raw).unwrap();
        assert_eq!(entry.extra.get("unreadCount"), Some(&serde_json::json!(7)));
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
