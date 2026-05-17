//! Capability types for the JMAP Chat extension.
//!
//! Normative references:
//!   - draft-atwood-jmap-chat-00 §3       (`ChatCapability` fields)
//!   - draft-atwood-jmap-chat-push-00     (`ChatPushCapability` fields)
//!
//! The Chat extension defines two account-level capability objects plus a
//! marker session-level capability (`urn:ietf:params:jmap:chat:websocket`,
//! whose value is an empty object — see `draft-atwood-jmap-chat-wss-00`,
//! not modelled here).

use serde::{Deserialize, Serialize};

use crate::message::BodyType;
use crate::push::UrgencyLevel;

/// Capability URI for core JMAP Chat support (draft-atwood-jmap-chat-00 §3).
pub const JMAP_CHAT_URI: &str = "urn:ietf:params:jmap:chat";

/// Capability URI for the JMAP Chat Push extension (draft-atwood-jmap-chat-push-00).
pub const JMAP_CHAT_PUSH_URI: &str = "urn:ietf:params:jmap:chat:push";

/// Account-level capability object for `"urn:ietf:params:jmap:chat"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:chat"]`.
///
/// Spec: draft-atwood-jmap-chat-00 §3
///
/// # Strictness
///
/// The struct-level `#[serde(default)]` is deliberately NOT present:
/// `max_body_bytes`, `max_attachment_bytes`, `max_attachments_per_message`,
/// and `supports_threads` are spec-required fields (draft-atwood-jmap-chat-00
/// §3 lines 171-184). A server returning `{}` for this capability is
/// non-compliant, and consumers surface that via a deserialize error rather
/// than silently defaulting every field to `0` / `false` (which would cause
/// callers to refuse to send any message because `max_body_bytes == 0`).
///
/// `supported_body_types` carries field-level `#[serde(default)]` only —
/// see its rustdoc for the forward-compat tolerance rationale.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCapability {
    /// Maximum UTF-8 byte length of a Message body.
    pub max_body_bytes: u64,
    /// Maximum single attachment blob size in bytes.
    pub max_attachment_bytes: u64,
    /// Maximum number of attachments per message.
    pub max_attachments_per_message: u64,
    /// Whether the server supports the optional thread model.
    pub supports_threads: bool,
    /// The set of Message `bodyType` values this server understands
    /// (draft-atwood-jmap-chat-00 §3).
    ///
    /// Spec requirements for compliant servers:
    ///
    /// - MUST include [`BodyType::Plain`] (`"text/plain"`).
    /// - SHOULD include [`BodyType::Markdown`] (`"text/markdown"`,
    ///   RFC 7763 CommonMark).
    /// - SHOULD include [`BodyType::Rich`]
    ///   (`"application/jmap-chat-rich"`).
    /// - SHOULD include [`BodyType::Other`]`("application/mls-ciphertext".into())`
    ///   for E2EE deployments.
    /// - MAY include [`BodyType::Other`]`("application/mimi-content".into())`.
    ///
    /// An empty `Vec` is non-compliant per spec ([`BodyType::Plain`] is
    /// mandatory) but consumers tolerate it via `Default` — the consumer is
    /// responsible for enforcing the MUST and acting accordingly (e.g.
    /// refusing to send rich messages to a server that does not advertise
    /// the matching variant).
    ///
    /// Element type is [`BodyType`] rather than `String` so callers can
    /// match on the typed variants directly; canonical MIME-type wire
    /// strings deserialize to their typed variant and any unknown wire
    /// string lands in [`BodyType::Other`] per the `impl_string_enum!`
    /// round-trip contract.
    #[serde(default)]
    pub supported_body_types: Vec<BodyType>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// The five aggregate-count caps `maxGroupMembers`,
    /// `maxSpaceMembers`, `maxRolesPerSpace`, `maxChannelsPerSpace`, and
    /// `maxCategoriesPerSpace` are not advertised on this capability
    /// in the current draft-atwood-jmap-chat-00 §3 — they are
    /// implementation-defined and enforced via standard `overQuota`
    /// SetError (RFC 8620 §5.3) at `Chat/set` and `Space/set` time.
    /// Servers that still emit them will round-trip the values
    /// harmlessly through `extra`.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Account-level capability object for `"urn:ietf:params:jmap:chat:push"`.
///
/// Found at `accounts[id].accountCapabilities["urn:ietf:params:jmap:chat:push"]`.
///
/// Spec: draft-atwood-jmap-chat-push-00
///
/// # Strictness
///
/// The struct-level `#[serde(default)]` is deliberately NOT present:
/// `max_snippet_bytes` and `supported_urgency_values` are spec-required
/// (draft-atwood-jmap-chat-push-00 lines 90-94). `max_messages_per_push`
/// is the only optional field (line 96) and is already `Option<u64>`.
/// A server returning `{}` for this capability is non-compliant; consumers
/// surface that via a deserialize error rather than silently defaulting
/// `max_snippet_bytes` to `0` (which would force every `bodySnippet` to be
/// truncated to nothing).
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPushCapability {
    /// Maximum byte length of a `bodySnippet` in `ChatMessagePush`.
    /// Truncation occurs on a UTF-8 boundary.
    pub max_snippet_bytes: u64,
    /// Supported Web Push urgency values.
    /// MUST include at least [`UrgencyLevel::Normal`] and
    /// [`UrgencyLevel::High`].
    ///
    /// Element type is [`UrgencyLevel`] rather than `String` so callers can
    /// match on the typed variants directly. Canonical wire strings
    /// (`"very-low"`, `"low"`, `"normal"`, `"high"`) deserialize to typed
    /// variants; unknown wire strings land in [`UrgencyLevel::Other`] per
    /// the `impl_string_enum!` round-trip contract.
    pub supported_urgency_values: Vec<UrgencyLevel>,
    /// Maximum number of `ChatMessageEntry` objects per push payload.
    /// `None` means the server does not impose a bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_messages_per_push: Option<u64>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: hand-built JSON from draft-atwood-jmap-chat-00 §3 field
    /// names. `supportedBodyTypes` on the wire deserialises into typed
    /// `BodyType` variants preserving order.
    #[test]
    fn chat_capability_round_trip() {
        let raw = json!({
            "maxBodyBytes": 4096_u64,
            "maxAttachmentBytes": 10_000_000_u64,
            "maxAttachmentsPerMessage": 8_u64,
            "supportsThreads": true,
            "supportedBodyTypes": [
                "text/plain",
                "text/markdown",
                "application/jmap-chat-rich",
                "application/x-acme"
            ]
        });
        let cap: ChatCapability =
            serde_json::from_value(raw).expect("ChatCapability must deserialize");
        assert_eq!(cap.max_body_bytes, 4096);
        assert_eq!(cap.max_attachment_bytes, 10_000_000);
        assert_eq!(cap.max_attachments_per_message, 8);
        assert!(cap.supports_threads);
        assert_eq!(
            cap.supported_body_types,
            vec![
                BodyType::Plain,
                BodyType::Markdown,
                BodyType::Rich,
                BodyType::Other("application/x-acme".to_owned()),
            ]
        );
        assert!(cap.extra.is_empty());
    }

    /// Oracle: an unknown wire field is captured by `extra` and round-trips.
    #[test]
    fn chat_capability_extras_round_trip() {
        let raw = json!({
            "maxBodyBytes": 1024_u64,
            "maxAttachmentBytes": 1024_u64,
            "maxAttachmentsPerMessage": 1_u64,
            "supportsThreads": false,
            "acmeCorpQuota": "gold"
        });
        let cap: ChatCapability = serde_json::from_value(raw).expect("must deserialize");
        assert_eq!(
            cap.extra.get("acmeCorpQuota").and_then(|v| v.as_str()),
            Some("gold")
        );
        let back = serde_json::to_value(&cap).expect("serialize");
        assert_eq!(back["acmeCorpQuota"], "gold");
    }

    /// Oracle: hand-built JSON from draft-atwood-jmap-chat-push-00 field
    /// names. Optional field absent → `None`.
    #[test]
    fn chat_push_capability_round_trip_no_optional() {
        let raw = json!({
            "maxSnippetBytes": 256_u64,
            "supportedUrgencyValues": ["normal", "high"]
        });
        let cap: ChatPushCapability =
            serde_json::from_value(raw).expect("ChatPushCapability must deserialize");
        assert_eq!(cap.max_snippet_bytes, 256);
        assert_eq!(
            cap.supported_urgency_values,
            vec![UrgencyLevel::Normal, UrgencyLevel::High]
        );
        assert!(cap.max_messages_per_push.is_none());
        assert!(cap.extra.is_empty());
    }

    /// Oracle: optional field present serialises with the optional key.
    #[test]
    fn chat_push_capability_with_optional() {
        let raw = json!({
            "maxSnippetBytes": 512_u64,
            "supportedUrgencyValues": ["low", "normal", "high"],
            "maxMessagesPerPush": 16_u64
        });
        let cap: ChatPushCapability = serde_json::from_value(raw).expect("must deserialize");
        assert_eq!(cap.max_messages_per_push, Some(16));
        let back = serde_json::to_value(&cap).expect("serialize");
        assert_eq!(back["maxMessagesPerPush"], 16);
    }

    /// Oracle: URI constants match the draft-atwood-jmap-chat IANA-pending
    /// registrations verbatim.
    #[test]
    fn capability_uri_constants_match_draft() {
        assert_eq!(JMAP_CHAT_URI, "urn:ietf:params:jmap:chat");
        assert_eq!(JMAP_CHAT_PUSH_URI, "urn:ietf:params:jmap:chat:push");
    }
}
