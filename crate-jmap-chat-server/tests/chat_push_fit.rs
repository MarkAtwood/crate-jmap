//! Fitting a `ChatMessagePush` to a platform size limit (bd:JMAP-0x0q.15).
//!
//! Oracle: draft-atwood-jmap-chat-push-00.md §Delivery at jmap-chat-spec
//! commit 6bcd9d8, lines 232 and 234. Each test quotes the sentence it
//! enforces. Payloads and every expected result are hand-written JSON; byte
//! limits are the lengths of those hand-written results.

use jmap_chat_server::fit_chat_message_push;
use jmap_chat_types::ChatMessagePush;
use serde_json::{Value, json};

/// One full channel entry with every optional field present.
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

/// [`full_entry`] without `bodySnippet`, every other field intact.
fn no_snippet_entry(n: u32) -> Value {
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

/// Fit `input` to `limit`, asserting each serialized payload is within it.
fn fit(input: &Value, limit: usize) -> Vec<Option<Value>> {
    fit_chat_message_push(push(input), limit)
        .into_iter()
        .map(|p| {
            p.map(|p| {
                let bytes = serde_json::to_vec(&p).expect("serializable");
                assert!(bytes.len() <= limit, "{} > {limit}", bytes.len());
                serde_json::from_slice(&bytes).expect("valid JSON")
            })
        })
        .collect()
}

/// "Servers keep each `ChatMessagePush` within the push platform's payload
/// size limit": a payload within the limit is delivered unchanged.
#[test]
fn leaves_a_fitting_payload_unchanged() {
    let full = payload(vec![full_entry(1), full_entry(2)]);
    assert_eq!(fit(&full, len(&full)), vec![Some(full.clone())]);
    assert_eq!(fit(&full, 3992), vec![Some(full)]);
}

/// "If a single `ChatMessageEntry` does not fit, the server SHOULD shorten
/// or omit that entry's `bodySnippet`": only `bodySnippet` goes, and
/// `sentAt`, `spaceId`, `chatName` and the rest stay.
#[test]
fn omits_only_body_snippet_from_an_entry_that_does_not_fit() {
    let full = payload(vec![full_entry(1)]);
    let trimmed = payload(vec![no_snippet_entry(1)]);
    assert_eq!(fit(&full, len(&full) - 1), vec![Some(trimmed.clone())]);
    assert_eq!(fit(&full, len(&trimmed)), vec![Some(trimmed)]);
}

/// "if the entry still does not fit, the server delivers a `StateChange`
/// for the `Message` data type in place of a `ChatMessagePush` for that
/// message": no other field is stripped to make it fit.
#[test]
fn falls_back_when_the_entry_without_snippet_does_not_fit() {
    let full = payload(vec![full_entry(1)]);
    let trimmed = payload(vec![no_snippet_entry(1)]);
    assert_eq!(fit(&full, len(&trimmed) - 1), vec![None]);
    assert_eq!(fit(&full, 0), vec![None]);
}

/// The fallback applies to an encrypted entry too, which has no
/// `bodySnippet` to omit; its `chatName`, `senderDisplayName` and `sentAt`
/// are never removed.
#[test]
fn falls_back_for_an_encrypted_entry_that_does_not_fit() {
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
    assert_eq!(fit(&input, len(&input)), vec![Some(input.clone())]);
    assert_eq!(fit(&input, len(&input) - 1), vec![None]);
}

/// "if more matching messages are pending than the limit allows, the
/// server MUST deliver additional `ChatMessagePush` payloads to cover the
/// remainder": a batch over the size limit is split into one payload per
/// message instead of losing entries.
#[test]
fn splits_an_oversized_batch_without_losing_entries() {
    let full = payload(vec![full_entry(1), full_entry(2), full_entry(3)]);
    let one = |n| payload(vec![full_entry(n)]);
    assert_eq!(
        fit(&full, len(&one(1))),
        vec![Some(one(1)), Some(one(2)), Some(one(3))]
    );
}

/// The split and the per-entry rule combine: each message's payload drops
/// only its own `bodySnippet` when it alone does not fit.
#[test]
fn splits_then_omits_snippets_per_entry() {
    let short = json!({
        "messageId": "msg-2",
        "chatId": "chat-1",
        "chatKind": "group",
        "senderId": "contact-bob",
        "encrypted": false,
        "bodySnippet": "ok",
    });
    let full = payload(vec![full_entry(1), short.clone()]);
    let trimmed = payload(vec![no_snippet_entry(1)]);
    assert_eq!(
        fit(&full, len(&trimmed)),
        vec![Some(trimmed), Some(payload(vec![short]))]
    );
}
