//! Message/set create enforces the ChatLimits body and attachment limits
//! and checks attachments against their blob (bd:JMAP-y3f2.5).
//!
//! Oracles: draft-atwood-jmap-chat-00 (jmap-chat-spec 91672c9)
//! §Account-Level Capability Object, §Attachment, §Input Validation and
//! §Denial of Service, quoted per test; SHA-256 digests are the published
//! FIPS 180-2 / RFC 6234 test vectors.

mod common;

use common::{MemoryBackend, ACCOUNT_ID};
use jmap_chat_server::{handle_chat_set, handle_message_set, AttachmentBlob, ChatLimits};
use jmap_server::HandlerOutput;
use serde_json::{json, Map, Value};

/// SHA-256("abc"), FIPS 180-2 Appendix B.1 / RFC 6234 §8.5 TEST1.
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
/// SHA-256 of empty content, quoted in draft-atwood-jmap-cid-00 L376-377.
const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn digest(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex vector");
    }
    out
}

async fn create_group(backend: &MemoryBackend) -> String {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
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

/// One Message/set create of `body` plus `extra`; returns the response.
async fn create(backend: &MemoryBackend, chat_id: &str, body: &str, extra: Value) -> Value {
    let mut obj: Map<String, Value> = extra.as_object().expect("extra object").clone();
    obj.insert("chatId".into(), json!(chat_id));
    obj.insert("body".into(), json!(body));
    obj.insert("sentAt".into(), json!("2024-01-01T00:00:00Z"));
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "create": { "m": obj } }),
    )
    .await
    .expect("Message/set");
    resp
}

fn attachment(blob_id: &str, size: u64, sha256: &str) -> Value {
    json!({
        "blobId": blob_id,
        "filename": "a.txt",
        "contentType": "text/plain",
        "size": size,
        "sha256": sha256,
    })
}

fn assert_created(resp: &Value) {
    assert!(
        resp["created"]["m"]["id"].is_string(),
        "not created: {resp}"
    );
}

fn assert_too_large(resp: &Value) {
    assert_eq!(resp["notCreated"]["m"]["type"], "tooLarge", "{resp}");
}

fn assert_attachments_invalid(resp: &Value) {
    assert_eq!(
        resp["notCreated"]["m"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["m"]["properties"],
        json!(["attachments"]),
        "{resp}"
    );
}

/// L235-236 `maxBodyBytes`: "Maximum UTF-8 byte length of a Message
/// `body`. Servers MUST reject messages exceeding this limit; in
/// `Message/set` the rejection is a `tooLarge` SetError." The limit is the
/// account's configured value, and it counts bytes: "é" is two.
#[tokio::test]
async fn body_limit_is_the_configured_max_body_bytes() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(ChatLimits::default().with_max_body_bytes(4)));
    let chat_id = create_group(&backend).await;

    assert_created(&create(&backend, &chat_id, "abcd", json!({})).await);
    assert_created(&create(&backend, &chat_id, "\u{e9}\u{e9}", json!({})).await);
    assert_too_large(&create(&backend, &chat_id, "abcde", json!({})).await);
    assert_too_large(&create(&backend, &chat_id, "abc\u{e9}", json!({})).await);
}

/// L241-242 `maxAttachmentsPerMessage`: "Maximum number of attachments
/// per message." L1676: "Enforce `maxAttachmentsPerMessage` at creation
/// and update time."
#[tokio::test]
async fn attachment_count_over_limit_is_too_large() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        ChatLimits::default().with_max_attachments_per_message(2),
    ));
    let chat_id = create_group(&backend).await;
    let a = attachment("b1", 3, SHA256_ABC);

    let two = json!({ "attachments": [a, a] });
    assert_created(&create(&backend, &chat_id, "hi", two).await);
    let three = json!({ "attachments": [a, a, a] });
    assert_too_large(&create(&backend, &chat_id, "hi", three).await);
}

/// L241 types `maxAttachmentsPerMessage` as `UnsignedInt`, which RFC 8620
/// §1.3 defines as "an Int where the value MUST be in the range
/// 0 <= value <= 2^53-1". A cap of 2^32 is valid and does not reject a
/// message with three attachments.
#[tokio::test]
async fn attachment_count_limit_accepts_unsigned_int_above_u32() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(
        ChatLimits::default().with_max_attachments_per_message(4_294_967_296),
    ));
    let chat_id = create_group(&backend).await;
    let a = attachment("b1", 3, SHA256_ABC);

    let three = json!({ "attachments": [a, a, a] });
    assert_created(&create(&backend, &chat_id, "hi", three).await);
}

/// L238-239 `maxAttachmentBytes`: "Maximum size in bytes of a single
/// attachment blob."
#[tokio::test]
async fn attachment_size_over_limit_is_too_large() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(ChatLimits::default().with_max_attachment_bytes(3)));
    let chat_id = create_group(&backend).await;

    let at = json!({ "attachments": [attachment("b1", 3, SHA256_ABC)] });
    assert_created(&create(&backend, &chat_id, "hi", at).await);
    let over = json!({ "attachments": [attachment("b2", 4, SHA256_ABC)] });
    assert_too_large(&create(&backend, &chat_id, "hi", over).await);
}

/// L1676: "Enforce `maxBodyBytes` and `maxAttachmentBytes` at parse time,
/// before any fetch or storage." A declared size over the limit is
/// `tooLarge` even though it also mismatches the stored blob.
#[tokio::test]
async fn attachment_size_limit_precedes_blob_check() {
    let backend = MemoryBackend::new();
    backend.set_limits_for_test(Some(ChatLimits::default().with_max_attachment_bytes(3)));
    backend.set_attachment_blob_for_test(
        ACCOUNT_ID,
        "b1",
        Some(AttachmentBlob::new(3, digest(SHA256_ABC))),
    );
    let chat_id = create_group(&backend).await;

    let over = json!({ "attachments": [attachment("b1", 4, SHA256_ABC)] });
    assert_too_large(&create(&backend, &chat_id, "hi", over).await);
}

/// L286-290 §Attachment: `size` "Blob size in bytes. Servers MUST verify
/// against actual content." `sha256` "Lowercase hex SHA-256 of blob
/// content. Servers SHOULD verify." L1663-1664 / L1667: a failed check
/// for which "this document names no other error" is "an
/// `invalidProperties` SetError ... whose `properties` names the
/// offending property". The fixture blob is the three bytes "abc".
#[tokio::test]
async fn attachment_size_and_sha256_are_checked_against_the_blob() {
    let backend = MemoryBackend::new();
    backend.set_attachment_blob_for_test(
        ACCOUNT_ID,
        "abc",
        Some(AttachmentBlob::new(3, digest(SHA256_ABC))),
    );
    let chat_id = create_group(&backend).await;
    let with = |size, sha256| json!({ "attachments": [attachment("abc", size, sha256)] });

    assert_created(&create(&backend, &chat_id, "hi", with(3, SHA256_ABC)).await);
    assert_attachments_invalid(&create(&backend, &chat_id, "hi", with(4, SHA256_ABC)).await);
    assert_attachments_invalid(&create(&backend, &chat_id, "hi", with(3, SHA256_EMPTY)).await);
    // Differs from SHA256_ABC in the final hex digit only.
    let last_digit = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ac";
    assert_attachments_invalid(&create(&backend, &chat_id, "hi", with(3, last_digit)).await);
}
