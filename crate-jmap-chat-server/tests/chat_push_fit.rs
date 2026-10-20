//! Fitting a `ChatMessagePush` to a platform size limit (bd:JMAP-nbk3.9).
//!
//! Oracle: jmap-chat-push-platform-guide.md §Payload truncation at
//! jmap-chat-spec commit 3227c0f. Each test quotes the sentence it enforces.
//! Payloads and every expected truncation stage are hand-written JSON; byte
//! limits are the lengths of those hand-written stages.

use jmap_chat_server::fit_chat_message_push;
use jmap_chat_types::ChatMessagePush;
use serde_json::{json, Value};

/// One full channel entry with every droppable field present.
fn full_entry(n: u32) -> Value {
    json!({
        "messageId": format!("msg-{n}"),
        "chatId": "chat-1",
        "chatKind": "channel",
        "chatName": "general",
        "spaceId": "space-1",
        "spaceName": "Acme Engineering",
        "senderId": "contact-bob",
        "senderDisplayName": "Bob Example",
        "sentAt": "2026-10-19T12:00:00Z",
        "hasMention": true,
        "mentionScopes": ["everyone"],
        "encrypted": false,
        "bodySnippet": "The release candidate is tagged; please test it before Friday.",
    })
}

/// [`full_entry`] after every droppable field is gone.
fn minimal_entry(n: u32) -> Value {
    json!({
        "messageId": format!("msg-{n}"),
        "chatId": "chat-1",
        "chatKind": "channel",
        "senderId": "contact-bob",
        "hasMention": true,
        "mentionScopes": ["everyone"],
        "encrypted": false,
    })
}

fn payload(messages: Vec<Value>) -> Value {
    json!({
        "@type": "ChatMessagePush",
        "accountId": "acct-owner",
        "state": "s42",
        "messages": messages,
    })
}

fn push(v: &Value) -> ChatMessagePush {
    serde_json::from_value(v.clone()).expect("valid ChatMessagePush")
}

fn len(v: &Value) -> usize {
    serde_json::to_vec(v).expect("serializable").len()
}

/// Fit `input` to `limit`, asserting the serialized result is within it.
fn fit(input: &Value, limit: usize) -> Option<Value> {
    fit_chat_message_push(push(input), limit).map(|p| {
        let bytes = serde_json::to_vec(&p).expect("serializable");
        assert!(bytes.len() <= limit, "{} > {limit}", bytes.len());
        serde_json::from_slice(&bytes).expect("valid JSON")
    })
}

/// Remove `fields` from every entry of the hand-written payload `v`.
fn without(v: &Value, fields: &[&str]) -> Value {
    let mut v = v.clone();
    for entry in v["messages"].as_array_mut().expect("messages array") {
        let entry = entry.as_object_mut().expect("entry object");
        for f in fields {
            entry.remove(*f);
        }
    }
    v
}

/// "servers SHOULD drop fields from `ChatMessageEntry` objects in this
/// order until the payload fits: 1. `bodySnippet` ... 2. `spaceId` ...
/// 3. `sentAt` ... 4. `spaceName` 5. `chatName` 6. `senderDisplayName`"
#[test]
fn drops_fields_in_guide_order_until_it_fits() {
    let full = payload(vec![full_entry(1), full_entry(2)]);
    let order = [
        "bodySnippet",
        "spaceId",
        "sentAt",
        "spaceName",
        "chatName",
        "senderDisplayName",
    ];
    let stages: Vec<Value> = (0..=order.len())
        .map(|k| without(&full, &order[..k]))
        .collect();
    assert_eq!(
        stages[order.len()],
        payload(vec![minimal_entry(1), minimal_entry(2)])
    );
    for (k, stage) in stages.iter().enumerate() {
        assert_eq!(fit(&full, len(stage)).as_ref(), Some(stage), "stage {k}");
        if let Some(next) = stages.get(k + 1) {
            assert_eq!(fit(&full, len(stage) - 1).as_ref(), Some(next), "below {k}");
        }
    }
}

/// "If the payload still exceeds the limit after all droppable fields are
/// removed from every entry (typically due to a large batch), servers
/// SHOULD drop entries from the end of the `messages` array, keeping the
/// first entry."
#[test]
fn drops_entries_from_the_end_keeping_the_first() {
    let full = payload(vec![full_entry(1), full_entry(2), full_entry(3)]);
    let two = payload(vec![minimal_entry(1), minimal_entry(2)]);
    let one = payload(vec![minimal_entry(1)]);
    assert_eq!(fit(&full, len(&two)), Some(two.clone()));
    assert_eq!(fit(&full, len(&two) - 1), Some(one.clone()));
    assert_eq!(fit(&full, len(&one)), Some(one));
}

/// "If a single minimal entry still exceeds the platform limit, servers
/// MUST fall back to a plain `StateChange` for that subscription rather
/// than sending an invalid or oversized payload."
#[test]
fn returns_none_when_one_minimal_entry_is_too_large() {
    let full = payload(vec![full_entry(1), full_entry(2)]);
    let one = payload(vec![minimal_entry(1)]);
    assert_eq!(fit(&full, len(&one) - 1), None);
    assert_eq!(fit(&full, 0), None);
}

/// "Servers MUST NOT drop: `@type`, `accountId`, `state` (on the outer
/// object), `messageId`, `chatId`, `chatKind`, `senderId`, `hasMention`,
/// `mentionScopes`, `encrypted`. (For an encrypted message `hasMention`
/// and `mentionScopes` are absent from the start ...)"
#[test]
fn keeps_must_keep_fields_for_encrypted_entry() {
    let input = payload(vec![json!({
        "messageId": "msg-1",
        "chatId": "chat-1",
        "chatKind": "group",
        "chatName": "Team",
        "senderId": "contact-bob",
        "senderDisplayName": "Bob Example",
        "sentAt": "2026-10-19T12:00:00Z",
        "encrypted": true,
    })]);
    let minimal = payload(vec![json!({
        "messageId": "msg-1",
        "chatId": "chat-1",
        "chatKind": "group",
        "senderId": "contact-bob",
        "encrypted": true,
    })]);
    assert_eq!(fit(&input, len(&minimal)), Some(minimal.clone()));
    assert_eq!(fit(&input, len(&minimal) - 1), None);
}

/// "When a `ChatMessagePush` payload exceeds the platform size limit ...
/// servers SHOULD drop fields": a payload within the limit is unchanged.
#[test]
fn leaves_a_fitting_payload_unchanged() {
    let full = payload(vec![full_entry(1)]);
    assert_eq!(fit(&full, len(&full)), Some(full.clone()));
    assert_eq!(fit(&full, 3992), Some(full));
}
