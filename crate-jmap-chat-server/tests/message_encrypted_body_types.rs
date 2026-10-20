//! `ChatLimits::encrypted_body_types` decides which `bodyType` values the
//! §Fields Inside the Ciphertext empty-field rule covers (bd:JMAP-5mz6.3).
//!
//! Oracles (jmap-chat-spec 91672c9):
//! - draft-atwood-jmap-chat-00 L1732: "When `bodyType` indicates an
//!   encrypted type, in any topology, the Message fields `mentions`,
//!   `broadcastMentions`, `attachments`, `actions`, `replyTo`,
//!   `threadRootId`, `topicId`, and `subject` MUST be empty or absent [...]
//!   Servers MUST reject a `Message/set` create or update that has an
//!   encrypted `bodyType` and any of these fields non-empty with an
//!   `invalidProperties` SetError whose `properties` lists each such field".
//! - draft-atwood-jmap-chat-mimi-00 L420: "In a Chat with a `roomId`, every
//!   Message has `bodyType` `"application/mimi-fanout"`. It is an encrypted
//!   body type for the purposes of {{JMAP-CHAT}}, including its rules on
//!   fields inside the ciphertext."
//!
//! `application/mimi-fanout` is configured here only as an encrypted type,
//! not as a server-reserved one, so that `Message/set` create reaches the
//! empty-field rule.

mod common;

use common::{seed_group_chat, MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_message_set, ChatLimits};
use jmap_server::HandlerOutput;
use serde_json::{json, Value};

const CIPHER: &str = "application/mls-ciphertext";
const FANOUT: &str = "application/mimi-fanout";

const SUPPORTED: &[&str] = &["text/plain", CIPHER, FANOUT];

fn backend(encrypted: &'static [&'static str]) -> MemoryBackend {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        ChatLimits::default()
            .with_supported_body_types(SUPPORTED)
            .with_encrypted_body_types(encrypted),
    ));
    seed_group_chat(&backend, "c1");
    backend
}

async fn set(backend: &MemoryBackend, args: Value) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(backend, &(), args)
        .await
        .expect("Message/set");
    resp
}

async fn create(backend: &MemoryBackend, extra: &Value) -> Value {
    let mut obj = json!({ "chatId": "c1", "body": "x", "sentAt": "2024-01-01T00:00:00Z" });
    obj.as_object_mut()
        .expect("object")
        .extend(extra.as_object().expect("extra object").clone());
    set(
        backend,
        json!({ "accountId": ACCOUNT_ID, "create": { "m0": obj } }),
    )
    .await
}

/// L1732 create half: with `application/mimi-fanout` configured as
/// encrypted (mimi L420), each non-empty listed field is rejected with the
/// same `invalidProperties` SetError, naming the same fields, as for
/// `application/mls-ciphertext`.
#[tokio::test]
async fn configured_type_rejects_fields_like_mls_ciphertext_on_create() {
    let backend = backend(&[CIPHER, FANOUT]);
    let cases = [
        (json!({ "mentions": [{ "id": "u1" }] }), json!(["mentions"])),
        (
            json!({ "actions": [{ "type": "urn:ietf:params:jmap:chat:cap:vtc", "uri": "https://example.com/r" }] }),
            json!(["actions"]),
        ),
        (json!({ "replyTo": "m9" }), json!(["replyTo"])),
        (json!({ "threadRootId": "m9" }), json!(["threadRootId"])),
        (
            json!({ "topicId": "t1", "subject": "s" }),
            json!(["topicId", "subject"]),
        ),
    ];
    for (extra, properties) in cases {
        for body_type in [CIPHER, FANOUT] {
            let mut extra = extra.clone();
            extra["bodyType"] = json!(body_type);
            let resp = create(&backend, &extra).await;
            assert_eq!(
                resp["notCreated"]["m0"],
                json!({
                    "type": "invalidProperties",
                    "properties": properties,
                    "description": format!("must be empty or absent when bodyType is {body_type}"),
                }),
                "{extra} -> {resp}"
            );
        }
    }
}

/// L1732 create half for `broadcastMentions` and `attachments`: each,
/// non-empty under an encrypted `bodyType`, is rejected naming itself, and
/// both together are named together.
#[tokio::test]
async fn encrypted_type_rejects_broadcast_mentions_and_attachments_on_create() {
    let backend = backend(&[CIPHER, FANOUT]);
    let broadcast = json!([{ "scope": "everyone" }]);
    let attachments = json!([{
        "blobId": "b1",
        "filename": "a.txt",
        "contentType": "text/plain",
        "size": 3,
        // SHA-256("abc"), FIPS 180-2 Appendix B.1.
        "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    }]);
    let cases = [
        (
            json!({ "broadcastMentions": broadcast }),
            json!(["broadcastMentions"]),
        ),
        (
            json!({ "attachments": attachments }),
            json!(["attachments"]),
        ),
        (
            json!({ "broadcastMentions": broadcast, "attachments": attachments }),
            json!(["broadcastMentions", "attachments"]),
        ),
    ];
    for (extra, properties) in cases {
        for body_type in [CIPHER, FANOUT] {
            let mut extra = extra.clone();
            extra["bodyType"] = json!(body_type);
            let resp = create(&backend, &extra).await;
            assert_eq!(
                resp["notCreated"]["m0"]["type"], "invalidProperties",
                "{extra} -> {resp}"
            );
            let mut got: Vec<String> = resp["notCreated"]["m0"]["properties"]
                .as_array()
                .unwrap_or_else(|| panic!("no properties: {resp}"))
                .iter()
                .map(|p| p.as_str().expect("string").to_owned())
                .collect();
            got.sort();
            let mut want: Vec<String> = properties
                .as_array()
                .expect("array")
                .iter()
                .map(|p| p.as_str().expect("string").to_owned())
                .collect();
            want.sort();
            assert_eq!(got, want, "{extra} -> {resp}");
        }
    }
}

/// L1732 update half: a patch switching `bodyType` to the configured type
/// over a stored `topicId` and `subject` is rejected naming both.
#[tokio::test]
async fn configured_type_rejects_stored_fields_on_update() {
    let backend = backend(&[CIPHER, FANOUT]);
    let resp = create(&backend, &json!({ "topicId": "t1", "subject": "s" })).await;
    let id = resp["created"]["m0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("not created: {resp}"))
        .to_owned();

    let resp = set(
        &backend,
        json!({ "accountId": ACCOUNT_ID, "update": { &id: { "bodyType": FANOUT } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"][&id]["properties"],
        json!(["topicId", "subject"]),
        "{resp}"
    );
}

/// The rule follows the configured list: a type that is supported but not
/// listed as encrypted does not "indicate an encrypted type", so a
/// `topicId` and `subject` on it are stored.
#[tokio::test]
async fn unlisted_type_is_not_treated_as_encrypted() {
    let backend = backend(&[CIPHER]);
    let resp = create(
        &backend,
        &json!({ "bodyType": FANOUT, "topicId": "t1", "subject": "s" }),
    )
    .await;
    assert!(resp["created"]["m0"].is_object(), "{resp}");
}

/// The default list is `application/mls-ciphertext`, the type L612 names:
/// "When `bodyType` indicates an end-to-end encrypted payload (e.g.,
/// `"application/mls-ciphertext"`)".
#[test]
fn default_encrypted_types_are_mls_ciphertext() {
    assert_eq!(ChatLimits::default().encrypted_body_types, &[CIPHER]);
}
