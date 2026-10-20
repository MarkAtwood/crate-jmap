//! Deriving `mentions` and `broadcastMentions` from rich-body spans.
//!
//! draft-atwood-jmap-chat-00 §Rich Body Format: "A client composing a rich
//! body MUST list each user it marks with a `"mention"` span in `mentions`
//! ({{mention}}) and each scope it marks with a `"broadcast"` span in
//! `broadcastMentions` ({{broadcast-mention}}); only those arrays take
//! effect."

use jmap_chat_client::{broadcast_mentions_from_spans, mentions_from_spans};
use jmap_chat_types::RichBody;
use serde_json::json;

/// The §Rich Body Format example body, copied verbatim from the draft.
const DRAFT_EXAMPLE: &str = r#"{
  "spans": [
    {"type": "text", "text": "Hello "},
    {"type": "mention", "text": "@alice", "userId": "contact-alice"},
    {"type": "text", "text": " and "},
    {"type": "broadcast", "text": "@here", "scope": "here"},
    {"type": "text", "text": ", see this code: "},
    {"type": "code", "text": "foo()"},
    {"type": "text", "text": ". Full example:"},
    {"type": "codeblock", "text": "fn main() {}", "lang": "rust"}
  ]
}"#;

fn parse(body: &str) -> RichBody {
    serde_json::from_str(body).expect("valid rich body")
}

/// §Rich Body Format, after the example: "A Message carrying this body
/// lists `{"id": "contact-alice"}` in `mentions` and `{"scope": "here"}` in
/// `broadcastMentions`."
#[test]
fn draft_example_yields_stated_arrays() {
    let body = parse(DRAFT_EXAMPLE);
    assert_eq!(
        serde_json::to_value(mentions_from_spans(&body)).expect("serialize"),
        json!([{"id": "contact-alice"}])
    );
    assert_eq!(
        serde_json::to_value(broadcast_mentions_from_spans(&body)).expect("serialize"),
        json!([{"scope": "here"}])
    );
}

/// "list each user it marks" / "each scope it marks": a user or scope marked
/// by several spans is listed once.
#[test]
fn repeated_marks_are_listed_once() {
    let body = parse(
        r#"{"spans": [
            {"type": "mention", "text": "@alice", "userId": "contact-alice"},
            {"type": "broadcast", "text": "@here", "scope": "here"},
            {"type": "mention", "text": "@bob", "userId": "contact-bob"},
            {"type": "mention", "text": "@alice", "userId": "contact-alice"},
            {"type": "broadcast", "text": "@here", "scope": "here"}
        ]}"#,
    );
    assert_eq!(
        serde_json::to_value(mentions_from_spans(&body)).expect("serialize"),
        json!([{"id": "contact-alice"}, {"id": "contact-bob"}])
    );
    assert_eq!(
        serde_json::to_value(broadcast_mentions_from_spans(&body)).expect("serialize"),
        json!([{"scope": "here"}])
    );
}

/// Only `"mention"` and `"broadcast"` spans mark users and scopes. A span of
/// any other type, including an unknown one, which "Clients MUST silently
/// ignore", contributes nothing even if it carries `userId` or `scope`.
#[test]
fn other_span_types_mark_nothing() {
    let body = parse(
        r#"{"spans": [
            {"type": "bold", "text": "x", "userId": "contact-alice"},
            {"type": "future-span", "text": "y", "scope": "everyone"}
        ]}"#,
    );
    assert!(mentions_from_spans(&body).is_empty());
    assert!(broadcast_mentions_from_spans(&body).is_empty());
}
