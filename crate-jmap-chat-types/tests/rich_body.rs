//! Wire shape of the `application/jmap-chat-rich` body (bd:JMAP-ccy6.1).
//!
//! Oracle: draft-atwood-jmap-chat-00 at jmap-chat-spec 91672c9,
//! §Rich Body Format (L1445-1500).

mod common;

use jmap_chat_types::{BroadcastScope, RichBody, SpanType};
use serde_json::json;

/// §Example (L1486-1497), copied verbatim from the draft.
fn draft_example() -> String {
    common::fixture("draft_example.json")
}

/// The draft's example parses into the spans it lists, in order, with
/// each span's type-specific field from the L1470-1479 table.
#[test]
fn draft_example_parses_field_for_field() {
    let rich: RichBody = serde_json::from_str(&draft_example()).expect("draft example");
    let s = &rich.spans;
    assert_eq!(s.len(), 8);
    assert_eq!(
        (s[0].kind.clone(), s[0].text.as_str()),
        (SpanType::Text, "Hello ")
    );
    assert_eq!(
        (s[1].kind.clone(), s[1].text.as_str()),
        (SpanType::Mention, "@alice")
    );
    assert_eq!(s[1].user_id.as_deref(), Some("contact-alice"));
    assert_eq!(
        (s[3].kind.clone(), s[3].text.as_str()),
        (SpanType::Broadcast, "@here")
    );
    assert_eq!(s[3].scope, Some(BroadcastScope::Here));
    assert_eq!(
        (s[5].kind.clone(), s[5].text.as_str()),
        (SpanType::Code, "foo()")
    );
    assert_eq!(
        (s[7].kind.clone(), s[7].text.as_str()),
        (SpanType::Codeblock, "fn main() {}")
    );
    assert_eq!(s[7].lang.as_deref(), Some("rust"));
    assert!(s.iter().all(|span| span.extra.is_empty()));
}

/// The draft's example serializes back to the same JSON value.
#[test]
fn draft_example_round_trips() {
    let wire: serde_json::Value = serde_json::from_str(&draft_example()).expect("json");
    let rich: RichBody = serde_json::from_value(wire.clone()).expect("draft example");
    assert_eq!(serde_json::to_value(&rich).expect("serialize"), wire);
}

/// L1470-1479: every defined span type string maps to its typed variant
/// and serializes back unchanged; `"link"` carries `uri`.
#[test]
fn every_defined_span_type_round_trips() {
    let wire = json!({ "spans": [
        {"type": "text", "text": "a"},
        {"type": "bold", "text": "b"},
        {"type": "italic", "text": "c"},
        {"type": "bold-italic", "text": "d"},
        {"type": "code", "text": "e"},
        {"type": "codeblock", "text": "f"},
        {"type": "blockquote", "text": "g"},
        {"type": "mention", "text": "@h", "userId": "u1"},
        {"type": "broadcast", "text": "@everyone", "scope": "everyone"},
        {"type": "link", "text": "site", "uri": "https://example.com/"}
    ]});
    let rich: RichBody = serde_json::from_value(wire.clone()).expect("defined types");
    let kinds: Vec<SpanType> = rich.spans.iter().map(|s| s.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            SpanType::Text,
            SpanType::Bold,
            SpanType::Italic,
            SpanType::BoldItalic,
            SpanType::Code,
            SpanType::Codeblock,
            SpanType::Blockquote,
            SpanType::Mention,
            SpanType::Broadcast,
            SpanType::Link,
        ]
    );
    assert_eq!(rich.spans[9].uri.as_deref(), Some("https://example.com/"));
    assert_eq!(serde_json::to_value(&rich).expect("serialize"), wire);
}

/// L1481: "Servers MUST NOT reject messages solely because they contain
/// unrecognized span types; this allows protocol evolution." An unknown
/// span type, with its unknown fields, parses and round-trips unchanged.
#[test]
fn unknown_span_type_round_trips() {
    let wire = json!({ "spans": [
        {"type": "strikethrough", "text": "old", "style": {"color": "red"}, "depth": 2},
        {"type": "text", "text": " new"}
    ]});
    let rich: RichBody = serde_json::from_value(wire.clone()).expect("unknown type accepted");
    assert_eq!(
        rich.spans[0].kind,
        SpanType::Other("strikethrough".to_owned())
    );
    assert_eq!(rich.spans[0].text, "old");
    assert_eq!(serde_json::to_value(&rich).expect("serialize"), wire);
}

/// L1464: `text` "MUST be present on all span types."
#[test]
fn span_without_text_is_rejected() {
    let wire = json!({ "spans": [{"type": "bold"}] });
    assert!(serde_json::from_value::<RichBody>(wire).is_err());
}
