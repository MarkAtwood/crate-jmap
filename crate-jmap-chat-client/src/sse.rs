//! SSE types and frame parser for JMAP Chat push notifications.
//!
//! Wraps the base-client [`parse_sse_block`](jmap_base_client::parse_sse_block) and
//! interprets the chat-specific `"typing"` and `"presence"` event types that the base
//! client leaves as [`SseEvent::Unknown`](jmap_base_client::SseEvent::Unknown).
//!
//! Spec: draft-atwood-jmap-chat-push-00 §§ typing, presence
//! Wire format: RFC 8895 (Server-Sent Events)

use jmap_base_client::SseEvent;
use jmap_chat_types::Presence;
use jmap_types::Id;

/// A parsed SSE event from the JMAP Chat event source.
///
/// Extends the base-client [`SseEvent`](jmap_base_client::SseEvent) with
/// chat-specific variants for typing indicators and presence updates.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ChatSseEvent {
    /// A "state" event: maps accountId → (typeName → newState).
    ///
    /// Triggers a `/changes` call for each type listed.
    /// Wire: `{"@type":"StateChange","changed":{"<accountId>":{"<TypeName>":"<state>"}}}`
    StateChange(jmap_base_client::StateChange),

    /// A "typing" indicator event. Not persisted; no state token.
    ///
    /// Wire: `{"chatId":"<id>","senderId":"<id>","typing":<bool>}`
    Typing {
        /// The chat in which typing is occurring.
        chat_id: Id,
        /// The sender contact id.
        sender_id: Id,
        /// `true` = started typing, `false` = stopped.
        typing: bool,
    },

    /// A "presence" update event. Not persisted.
    ///
    /// Wire: `{"contactId":"<id>","presence":"<state>","lastActiveAt":"..."|null,...}`
    Presence {
        /// The contact whose presence changed.
        contact_id: Id,
        /// Presence state.
        presence: Presence,
        /// ISO 8601 timestamp of last activity, or `None` if absent/null.
        last_active_at: Option<String>,
        /// Free-text status message, or `None` if absent/null.
        status_text: Option<String>,
        /// Status emoji, or `None` if absent/null.
        status_emoji: Option<String>,
    },

    /// Unrecognized event type, keepalive, or parse failure.
    ///
    /// `event_type` carries the value of the SSE `event:` field for
    /// diagnostics — e.g. `"ping"` for a keepalive.  Callers should silently
    /// ignore this variant and log `event_type` when debugging.
    Unknown {
        /// The raw value of the SSE `event:` field; empty string if absent.
        event_type: String,
    },
}

/// A parsed JMAP Chat SSE frame: event plus the `id:` line value (if any).
///
/// # `id` field semantics
///
/// Mirrors [`jmap_base_client::SseFrame`]: `None` means the frame had no
/// `id:` line or a bare `id:` reset. Callers should retain the previously-seen
/// ID across reconnects and send it as `Last-Event-ID` per RFC 8620 §7.3.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ChatSseFrame {
    pub event: ChatSseEvent,
    pub id: Option<String>,
}

/// Parse a single SSE block (text between two blank lines) into a [`ChatSseFrame`].
///
/// Delegates to [`jmap_base_client::parse_sse_block`] for low-level SSE framing,
/// then interprets chat-specific event types:
///
/// - `"state"` → [`ChatSseEvent::StateChange`]
/// - `"typing"` → [`ChatSseEvent::Typing`] (or `Unknown` on JSON parse failure)
/// - `"presence"` → [`ChatSseEvent::Presence`] (or `Unknown` on JSON parse failure)
/// - everything else → [`ChatSseEvent::Unknown`]
///
/// Never panics. Malformed JSON is silently ignored and produces `Unknown`.
pub fn parse_chat_sse_block(block: &str) -> ChatSseFrame {
    let frame = jmap_base_client::parse_sse_block(block);
    let event = match frame.event {
        SseEvent::StateChange(sc) => ChatSseEvent::StateChange(sc),
        SseEvent::Unknown { event_type, data } => match event_type.as_str() {
            "typing" => parse_typing_data(&data).unwrap_or(ChatSseEvent::Unknown { event_type }),
            "presence" => {
                parse_presence_data(&data).unwrap_or(ChatSseEvent::Unknown { event_type })
            }
            _ => ChatSseEvent::Unknown { event_type },
        },
        // SseEvent is #[non_exhaustive]: forward any future base-client variants as Unknown.
        _ => ChatSseEvent::Unknown {
            event_type: String::new(),
        },
    };
    ChatSseFrame {
        event,
        id: frame.id,
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Wire shape of the "typing" event data field.
#[derive(serde::Deserialize)]
struct TypingPayload {
    #[serde(rename = "chatId")]
    chat_id: Id,
    #[serde(rename = "senderId")]
    sender_id: Id,
    typing: bool,
}

/// Wire shape of the "presence" event data field.
///
/// `lastActiveAt`, `statusText`, and `statusEmoji` are JSON strings or `null`.
/// A `null` value is treated the same as absence: both yield `None`.
#[derive(serde::Deserialize)]
struct PresencePayload {
    #[serde(rename = "contactId")]
    contact_id: Id,
    presence: Presence,
    #[serde(rename = "lastActiveAt")]
    last_active_at: Option<String>,
    #[serde(rename = "statusText")]
    status_text: Option<String>,
    #[serde(rename = "statusEmoji")]
    status_emoji: Option<String>,
}

fn parse_typing_data(data: &str) -> Option<ChatSseEvent> {
    let p: TypingPayload = serde_json::from_str(data).ok()?;
    Some(ChatSseEvent::Typing {
        chat_id: p.chat_id,
        sender_id: p.sender_id,
        typing: p.typing,
    })
}

fn parse_presence_data(data: &str) -> Option<ChatSseEvent> {
    let p: PresencePayload = serde_json::from_str(data).ok()?;
    Some(ChatSseEvent::Presence {
        contact_id: p.contact_id,
        presence: p.presence,
        last_active_at: p.last_active_at,
        status_text: p.status_text,
        status_emoji: p.status_emoji,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: "state" SSE event is promoted to ChatSseEvent::StateChange.
    /// Wire format from RFC 8620 §7.3 state change example.
    #[test]
    fn parse_state_event_promotes_to_state_change() {
        let block = "event: state\ndata: {\"changed\":{\"acc1\":{\"Message\":\"s42\"}}}";
        let ChatSseFrame { event, .. } = parse_chat_sse_block(block);
        match event {
            ChatSseEvent::StateChange(sc) => {
                assert_eq!(
                    sc.changed
                        .get("acc1")
                        .and_then(|m| m.get("Message"))
                        .map(|s| s.as_ref()),
                    Some("s42"),
                    "changed[acc1][Message] must equal s42"
                );
            }
            other => panic!("expected StateChange, got {other:?}"),
        }
    }

    /// Oracle: "typing" SSE event with valid JSON produces Typing variant.
    /// Wire format from draft-atwood-jmap-chat-push-00.
    #[test]
    fn parse_typing_event_valid_json() {
        let block = "event: typing\ndata: {\"chatId\":\"c1\",\"senderId\":\"u1\",\"typing\":true}";
        let ChatSseFrame { event, .. } = parse_chat_sse_block(block);
        match event {
            ChatSseEvent::Typing {
                chat_id,
                sender_id,
                typing,
            } => {
                assert_eq!(chat_id.as_ref(), "c1");
                assert_eq!(sender_id.as_ref(), "u1");
                assert!(typing, "typing must be true");
            }
            other => panic!("expected Typing, got {other:?}"),
        }
    }

    /// Oracle: "presence" SSE event with all fields present.
    /// Wire format from draft-atwood-jmap-chat-push-00.
    #[test]
    fn parse_presence_event_all_fields() {
        let block = concat!(
            "event: presence\n",
            "data: {\"contactId\":\"ct1\",\"presence\":\"online\",",
            "\"lastActiveAt\":\"2024-01-01T00:00:00Z\",",
            "\"statusText\":\"in a meeting\",\"statusEmoji\":\"📅\"}"
        );
        let ChatSseFrame { event, .. } = parse_chat_sse_block(block);
        match event {
            ChatSseEvent::Presence {
                contact_id,
                presence,
                last_active_at,
                status_text,
                status_emoji,
            } => {
                assert_eq!(contact_id.as_ref(), "ct1");
                assert_eq!(presence, Presence::Online);
                assert_eq!(last_active_at.as_deref(), Some("2024-01-01T00:00:00Z"));
                assert_eq!(status_text.as_deref(), Some("in a meeting"));
                assert_eq!(status_emoji.as_deref(), Some("📅"));
            }
            other => panic!("expected Presence, got {other:?}"),
        }
    }

    /// Oracle: "typing" event with malformed JSON degrades to Unknown.
    /// Security requirement: never panic on bad server data.
    #[test]
    fn parse_typing_malformed_json_degrades_to_unknown() {
        let block = "event: typing\ndata: not-json";
        let ChatSseFrame { event, .. } = parse_chat_sse_block(block);
        match event {
            ChatSseEvent::Unknown { event_type } => {
                assert_eq!(
                    event_type, "typing",
                    "Unknown must carry original event_type"
                );
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    /// Oracle: "presence" event with malformed JSON degrades to Unknown.
    /// Security requirement: never panic on bad server data.
    #[test]
    fn parse_presence_malformed_json_degrades_to_unknown() {
        let block = "event: presence\ndata: {\"not\":\"valid-presence\"}";
        let ChatSseFrame { event, .. } = parse_chat_sse_block(block);
        // Presence is missing required `contactId` field — must degrade to Unknown.
        assert!(
            matches!(event, ChatSseEvent::Unknown { .. }),
            "invalid presence JSON must yield Unknown"
        );
    }

    /// Oracle: unrecognized event type produces Unknown with the original event_type.
    /// RFC 8895 §9 forward-compatibility requirement.
    #[test]
    fn parse_unknown_event_type_preserved() {
        let block = "event: ping\ndata: {}";
        let ChatSseFrame { event, .. } = parse_chat_sse_block(block);
        match event {
            ChatSseEvent::Unknown { event_type } => {
                assert_eq!(event_type, "ping");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    /// Oracle: `id:` line value is threaded through to ChatSseFrame::id.
    /// RFC 8895 §9 / RFC 8620 §7.3: callers use this for Last-Event-ID on reconnect.
    #[test]
    fn id_line_propagated_through_frame() {
        let block = "id: evt-99\nevent: state\ndata: {\"changed\":{}}";
        let ChatSseFrame { id, .. } = parse_chat_sse_block(block);
        assert_eq!(id.as_deref(), Some("evt-99"));
    }
}
