//! Message/set create keeps and validates its optional properties
//! (bd:JMAP-yt1w.1).
//!
//! Oracle for every test: draft-atwood-jmap-chat-00 §Creating a Message:
//! "Optional: `attachments` (Attachment[]), `mentions` (Mention[]),
//! `broadcastMentions` (BroadcastMention[]), `actions` (MessageAction[]),
//! `replyTo` (String), `threadRootId` (String), `topicId` (String),
//! `subject` (String), `senderExpiresAt` (UTCDate), `burnOnRead`
//! (Boolean), `expiresAfterReadSeconds` (UnsignedInt)." and §Input
//! Validation: "When a `Message/set` create or update fails one of these
//! checks and this document names no other error for that check, the
//! server MUST reject the entry with an `invalidProperties` SetError
//! ([RFC8620] §5.3) whose `properties` names the offending property."

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_set, handle_message_get, handle_message_set};
use serde_json::{json, Map, Value};

const SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

async fn create_group(backend: &MemoryBackend) -> String {
    let (resp, _) = handle_chat_set(
        backend,
        &(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
        }),
    )
    .await
    .expect("Chat/set");
    resp["created"]["g"]["id"]
        .as_str()
        .expect("created group id")
        .to_owned()
}

/// Send one Message/set create whose object is the required properties
/// plus `extra`, and return the whole response. `bodyType` defaults to
/// `text/plain` unless `extra` sets it.
async fn create(backend: &MemoryBackend, chat_id: &str, extra: Value) -> Value {
    let mut obj: Map<String, Value> = extra.as_object().expect("extra object").clone();
    obj.insert("chatId".into(), json!(chat_id));
    obj.insert("body".into(), json!("hi @bob"));
    obj.entry("bodyType").or_insert(json!("text/plain"));
    obj.insert("sentAt".into(), json!("2024-01-01T00:00:00Z"));
    let (resp, _) = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "m": obj } }),
    )
    .await
    .expect("Message/set");
    resp
}

fn attachment(filename: &str, content_type: &str, sha256: &str) -> Value {
    json!({
        "blobId": "b1",
        "filename": filename,
        "contentType": content_type,
        "size": 3,
        "sha256": sha256,
    })
}

/// Every optional property in the create list is stored and read back.
/// §MessageAction: "Servers MUST store and forward MessageAction records
/// without inspection or transformation." so an unknown action key
/// survives too.
#[tokio::test]
async fn create_stores_optional_properties() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let attachments = json!([attachment("a.txt", "text/plain; charset=utf-8", SHA256)]);
    let mentions = json!([{ "id": "contact-bob", "offset": 3, "length": 4 }]);
    let actions = json!([{
        "type": "urn:jmap:chat:cap:vtc",
        "uri": "https://example.com/room",
        "label": "Join call",
        "vendorKey": { "x": 1 },
    }]);
    let resp = create(
        &backend,
        &chat_id,
        json!({
            "attachments": attachments,
            "mentions": mentions,
            "actions": actions,
            "topicId": "release",
            "subject": "Release 1.0",
            "expiresAfterReadSeconds": 30,
        }),
    )
    .await;
    let id = resp["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("not created: {resp}"))
        .to_owned();

    let (got, _) = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id] }),
    )
    .await
    .expect("Message/get");
    let msg = &got["list"][0];
    assert_eq!(msg["attachments"], attachments, "{msg}");
    assert_eq!(msg["mentions"], mentions, "{msg}");
    assert_eq!(msg["actions"], actions, "{msg}");
    assert_eq!(msg["topicId"], "release", "{msg}");
    assert_eq!(msg["subject"], "Release 1.0", "{msg}");
    assert_eq!(msg["expiresAfterReadSeconds"], 30, "{msg}");
}

/// §Message `topicId`: "MUST be non-empty and MUST NOT exceed 255 bytes."
/// A 255-byte value is at the limit and accepted.
#[tokio::test]
async fn create_accepts_topic_id_of_255_bytes() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!({ "topicId": "t".repeat(255) })).await;
    assert!(resp["created"]["m"].is_object(), "{resp}");
}

/// Each case breaks one §Input Validation rule and must be rejected with
/// `invalidProperties` naming that property.
#[tokio::test]
async fn create_rejects_invalid_optional_properties() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let cases = [
        // "`topicId`: reject an empty value or a value exceeding 255 bytes."
        ("topicId", json!({ "topicId": "" })),
        ("topicId", json!({ "topicId": "t".repeat(256) })),
        // "`subject`: validate UTF-8; enforce an implementation-defined
        // maximum byte length; reject when `topicId` is absent."
        ("subject", json!({ "subject": "orphan" })),
        (
            "subject",
            json!({ "topicId": "t", "subject": "s".repeat(1_000_000) }),
        ),
        // "`mentions`: reject any entry where `offset + length` exceeds
        // body byte length." (body is "hi @bob", 7 bytes)
        (
            "mentions",
            json!({ "mentions": [{ "id": "contact-bob", "offset": 3, "length": 5 }] }),
        ),
        // "`filename`: reject values containing `/`, `\`, or null bytes."
        (
            "attachments",
            json!({ "attachments": [attachment("a/b", "text/plain", SHA256)] }),
        ),
        (
            "attachments",
            json!({ "attachments": [attachment("a\\b", "text/plain", SHA256)] }),
        ),
        (
            "attachments",
            json!({ "attachments": [attachment("a\0b", "text/plain", SHA256)] }),
        ),
        // "`contentType`: reject syntactically invalid MIME values."
        (
            "attachments",
            json!({ "attachments": [attachment("a", "text", SHA256)] }),
        ),
        (
            "attachments",
            json!({ "attachments": [attachment("a", "text/pl ain", SHA256)] }),
        ),
        // §Attachment `sha256`: "Lowercase hex SHA-256 of blob content."
        (
            "attachments",
            json!({ "attachments": [attachment("a", "text/plain", &SHA256.to_uppercase())] }),
        ),
        // §Creating a Message: "`expiresAfterReadSeconds` (UnsignedInt)".
        (
            "expiresAfterReadSeconds",
            json!({ "expiresAfterReadSeconds": -1 }),
        ),
        (
            "expiresAfterReadSeconds",
            json!({ "expiresAfterReadSeconds": "30" }),
        ),
    ];
    for (property, extra) in cases {
        let resp = create(&backend, &chat_id, extra.clone()).await;
        assert_eq!(
            resp["notCreated"]["m"],
            json!({ "type": "invalidProperties", "properties": [property] }),
            "{extra} -> {resp}"
        );
    }
}

/// §Fields Inside the Ciphertext: "When `bodyType` indicates an encrypted
/// type, in any topology, the Message fields `mentions`,
/// `broadcastMentions`, `attachments`, `actions`, `replyTo`,
/// `threadRootId`, `topicId`, and `subject` MUST be empty or absent" and
/// "Servers MUST reject a `Message/set` create or update, or a
/// `Peer/deliver` payload, that has an encrypted `bodyType` and any of
/// these fields non-empty, with `invalidArguments`."
#[tokio::test]
async fn create_rejects_routing_fields_with_encrypted_body_type() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let cases = [
        json!({ "mentions": [{ "id": "contact-bob", "offset": 3, "length": 4 }] }),
        json!({ "broadcastMentions": [{ "scope": "everyone", "offset": 0, "length": 2 }] }),
        json!({ "attachments": [attachment("a.txt", "text/plain", SHA256)] }),
        json!({ "actions": [{ "type": "urn:jmap:chat:cap:vtc", "uri": "https://example.com/r" }] }),
        json!({ "replyTo": "m0" }),
        json!({ "threadRootId": "m0" }),
        json!({ "topicId": "release" }),
        json!({ "topicId": "release", "subject": "Release 1.0" }),
    ];
    for mut extra in cases {
        extra["bodyType"] = json!("application/mls-ciphertext");
        let resp = create(&backend, &chat_id, extra.clone()).await;
        assert_eq!(
            resp["notCreated"]["m"]["type"], "invalidArguments",
            "{extra} -> {resp}"
        );
    }
}

/// The same rule allows these fields when "empty or absent", and
/// `expiresAfterReadSeconds` is not one of them.
#[tokio::test]
async fn create_accepts_empty_routing_fields_with_encrypted_body_type() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(
        &backend,
        &chat_id,
        json!({
            "bodyType": "application/mls-ciphertext",
            "mentions": [],
            "broadcastMentions": [],
            "attachments": [],
            "actions": [],
            "expiresAfterReadSeconds": 30,
        }),
    )
    .await;
    assert!(resp["created"]["m"].is_object(), "{resp}");
}

/// §Rich Body Format: "When `bodyType` is `\"application/jmap-chat-rich\"`,
/// ... Both the `mentions` and `broadcastMentions` arrays on the Message
/// MUST be empty". The rejection matches the server's existing
/// `invalidArguments` for a non-empty `broadcastMentions` on a rich body.
#[tokio::test]
async fn create_rejects_mentions_with_rich_body_type() {
    let backend = MemoryBackend::new();
    let chat_id = create_group(&backend).await;
    let resp = create(
        &backend,
        &chat_id,
        json!({
            "bodyType": "application/jmap-chat-rich",
            "mentions": [{ "id": "contact-bob", "offset": 3, "length": 4 }],
        }),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["m"]["type"], "invalidArguments",
        "{resp}"
    );
}
