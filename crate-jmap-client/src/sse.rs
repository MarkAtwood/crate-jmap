// SSE types and frame parser for JMAP push notifications.
// Spec: RFC 8620 §7.3 (Push via Server-Sent Events)
// Wire format: RFC 8895 (Server-Sent Events)

use std::collections::HashMap;

use crate::push;

/// A parsed SSE frame: the event and the `id:` line value (if any).
///
/// # `id` field semantics
///
/// RFC 8895 §9.2 distinguishes three id states:
/// - A frame with no `id:` field → last event ID is **unchanged**.
/// - A frame with a bare `id:` field (no value) → last event ID is **reset**.
/// - A frame with `id: <value>` → last event ID is **updated** to `<value>`.
///
/// This implementation conflates the first two cases: both produce `id: None`.
/// Callers implementing reconnect with `Last-Event-ID` should treat `None` as
/// "no change" and retain the previously-seen ID. The "reset" semantic is not
/// representable without a tri-state type; this simplification is intentional
/// for JMAP, where bare `id:` reset frames are rare in practice.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SseFrame {
    pub event: SseEvent,
    pub id: Option<String>,
}

/// A parsed SSE event from a JMAP event source (RFC 8620 §7.3).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SseEvent {
    /// A "state" event: maps accountId → (typeName → newState).
    ///
    /// Triggers a `/changes` call for each type listed. Wire format:
    /// `{"@type":"StateChange","changed":{"<accountId>":{"<TypeName>":"<state>"}}}`
    StateChange(push::StateChange),
    /// Unrecognized event type, keepalive, or parse failure.
    ///
    /// Callers should silently ignore this variant.
    Unknown,
}

/// Parse a single SSE block (the text between two blank lines) into an [`SseFrame`].
///
/// Returns an [`SseFrame`] with `event = SseEvent::Unknown` for empty blocks,
/// keepalives, or unrecognized event types. Never panics. Malformed `data:`
/// JSON is silently ignored and returns `Unknown` rather than propagating an
/// error.
///
/// `SseFrame::id` carries the value of the `id:` line, if present. Callers
/// should track this and send it as `Last-Event-ID` on reconnect per RFC 8620
/// §7.3.
pub fn parse_sse_block(block: &str) -> SseFrame {
    let mut event_type: Option<&str> = None;
    let mut data_lines: Vec<&str> = Vec::new();
    let mut id: Option<String> = None;

    for line in block.lines() {
        // RFC 8895 §9.1: if value starts with U+0020 SPACE, remove exactly that one space.
        if let Some(value) = line.strip_prefix("event:") {
            event_type = Some(value.strip_prefix(' ').unwrap_or(value));
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.strip_prefix(' ').unwrap_or(value));
        } else if let Some(value) = line.strip_prefix("id:") {
            let v = value.strip_prefix(' ').unwrap_or(value);
            id = if v.is_empty() { None } else { Some(v.to_owned()) };
        }
        // Comments (lines starting with ':') and unknown fields are silently ignored.
    }

    let event = match event_type {
        Some("state") => match data_lines.as_slice() {
            [] => SseEvent::Unknown, // no data: lines → nothing to parse
            [single] => parse_state_data(single),
            _ => parse_state_data(&data_lines.join("\n")),
        },
        _ => SseEvent::Unknown,
    };

    SseFrame { event, id }
}

/// Parse the data payload of a "state" event.
///
/// Accepts both the bare `{"changed":{...}}` shape and the shape with
/// `"@type":"StateChange"` per RFC 8620 §7.3 (StateChange object definition).
/// The `@type` field is stripped before deserialization; only `changed` is used.
fn parse_state_data(data: &str) -> SseEvent {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data) else {
        return SseEvent::Unknown;
    };
    let Some(obj) = v.as_object_mut() else {
        return SseEvent::Unknown;
    };
    let Some(changed_val) = obj.remove("changed") else {
        return SseEvent::Unknown;
    };
    let Ok(changed) =
        serde_json::from_value::<HashMap<String, HashMap<String, String>>>(changed_val)
    else {
        return SseEvent::Unknown;
    };
    SseEvent::StateChange(push::StateChange { changed })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: spec §7 "state" event format.
    #[test]
    fn parse_state_event() {
        let block = "event: state\ndata: {\"changed\":{\"acc1\":{\"Message\":\"s42\"}}}";
        let SseFrame { event, .. } = parse_sse_block(block);
        match event {
            SseEvent::StateChange(sc) => {
                assert_eq!(
                    sc.changed
                        .get("acc1")
                        .and_then(|m| m.get("Message"))
                        .map(String::as_str),
                    Some("s42"),
                    "changed[acc1][Message] must equal s42"
                );
            }
            other => panic!("expected StateChange, got {other:?}"),
        }
    }

    /// Oracle: spec §7 "state" event format — @type field is present.
    /// The @type field must be accepted and ignored; only "changed" matters.
    #[test]
    fn parse_state_event_with_type_field() {
        let block = "event: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"acc1\":{\"Message\":\"s42\"}}}";
        let SseFrame { event, .. } = parse_sse_block(block);
        match event {
            SseEvent::StateChange(sc) => {
                assert_eq!(
                    sc.changed
                        .get("acc1")
                        .and_then(|m| m.get("Message"))
                        .map(String::as_str),
                    Some("s42"),
                    "changed[acc1][Message] must equal s42"
                );
            }
            other => panic!("expected StateChange, got {other:?}"),
        }
    }

    /// Oracle: RFC 8895 §9 — unrecognized event type must yield Unknown.
    #[test]
    fn parse_unknown_event() {
        let block = "event: ping\ndata: {}";
        let SseFrame { event, .. } = parse_sse_block(block);
        assert!(
            matches!(event, SseEvent::Unknown),
            "unrecognized event type must yield Unknown"
        );
    }

    /// Oracle: RFC 8895 §9 — empty block (keepalive) must yield Unknown.
    #[test]
    fn parse_empty_block() {
        let SseFrame { event, id } = parse_sse_block("");
        assert!(
            matches!(event, SseEvent::Unknown),
            "empty block must yield Unknown"
        );
        assert!(id.is_none(), "empty block must have no id");
    }

    /// Oracle: security requirement §G — malformed JSON in data must yield
    /// Unknown, never panic or propagate an error.
    #[test]
    fn parse_malformed_data_json() {
        let block = "event: state\ndata: not-json";
        let SseFrame { event, .. } = parse_sse_block(block);
        assert!(
            matches!(event, SseEvent::Unknown),
            "malformed JSON must yield Unknown, not panic or error"
        );
    }

    /// Oracle: RFC 8895 §9 — `id:` line value must be returned in `SseFrame::id`.
    #[test]
    fn parse_id_line() {
        let block = "id: evt-42\nevent: state\ndata: {\"changed\":{}}";
        let SseFrame { event, id } = parse_sse_block(block);
        assert_eq!(id.as_deref(), Some("evt-42"), "id must be evt-42");
        assert!(
            matches!(event, SseEvent::StateChange(_)),
            "must still parse as StateChange"
        );
    }

    /// Oracle: RFC 8895 §9 — multiple `data:` lines must be joined with `\n`.
    ///
    /// Two data: lines are collected and joined. If only the first line were
    /// used, a complete single-line state JSON would parse as StateChange. Because
    /// the second data: line is appended (joined with '\n'), the combined
    /// string is invalid JSON, so the result must be Unknown — proving both
    /// lines are captured.
    #[test]
    fn parse_multiline_data() {
        // First data: line alone is a complete, valid state JSON object.
        // Second data: line appends "extra", making the joined string invalid JSON.
        // Result must be Unknown (not StateChange), proving both lines are joined.
        let block = concat!(
            "event: state\n",
            "data: {\"changed\":{\"acc1\":{\"Message\":\"s1\"}}}\n",
            "data: extra"
        );
        let SseFrame { event, .. } = parse_sse_block(block);
        assert!(
            matches!(event, SseEvent::Unknown),
            "both data: lines must be joined: first-line-valid JSON + second line = Unknown"
        );
    }

    /// Verify SseEvent does not contain Typing or Presence variants.
    /// This match will fail to compile if either variant is ever reintroduced.
    #[test]
    fn sse_event_no_typing_or_presence() {
        let e = SseEvent::Unknown;
        match e {
            SseEvent::StateChange(_) => {}
            SseEvent::Unknown => {}
        }
    }
}
