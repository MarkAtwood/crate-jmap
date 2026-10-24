//! Message/set create resolves an Attachment that carries `filenodeId`
//! through `ChatBackend::filenode_attachment` (bd:JMAP-v9py.16.6).
//!
//! Oracles, at jmap-chat-spec 0cd3af0:
//!
//! - draft-atwood-jmap-chat-00.md ("chat Lnnn") L277-290: the Attachment
//!   fields `blobId`, `filename`, `contentType`, `size` and `sha256`, none
//!   marked optional; L1676: "Enforce `maxAttachmentsPerMessage` at
//!   creation and update time."
//! - draft-atwood-jmap-chat-filenode-00.md ("filenode Lnnn") L167: "The
//!   `Attachment` type defined in {{JMAP-CHAT}} is extended with the
//!   following optional fields when the `urn:ietf:params:jmap:chat:filenode`
//!   capability is active." L172: "When `filenodeId` is present on an
//!   Attachment, the fields `blobId`, `filename`, `contentType`, `size`, and
//!   `sha256` become OPTIONAL for that Attachment entry; the server MUST
//!   populate any absent fields from the referenced FileNode's properties
//!   before storage." L174: "If validation fails, the server MUST return a
//!   SetError of type `invalidProperties` naming `filenodeId`."
//!
//! The resolved Attachment is hand-written JSON; SHA-256("abc") is the
//! FIPS 180-2 Appendix B.1 / RFC 6234 §8.5 TEST1 vector.

mod common;

use common::{ACCOUNT_ID, TrackingBackend};
use jmap_chat_server::message::{handle_message_get, handle_message_set};
use jmap_chat_server::{ChatBackend, ChatLimits, handle_chat_set};
use jmap_server::HandlerOutput;
use jmap_server::{SetError, SetErrorType};
use serde_json::{Value, json};

/// SHA-256("abc"), FIPS 180-2 Appendix B.1 / RFC 6234 §8.5 TEST1.
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// The Attachment a FileNode-aware backend fills in for `filenodeId` F1.
fn filled() -> Value {
    json!({
        "blobId": "B1",
        "filename": "report.txt",
        "contentType": "text/plain",
        "size": 3,
        "sha256": SHA256_ABC,
        "filenodeId": "F1",
    })
}

async fn create_group<B: ChatBackend<CallerCtx = ()>>(backend: &B) -> String {
    let HandlerOutput { primary: resp, .. } = handle_chat_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "g": { "kind": "group", "name": "G", "memberIds": ["contact-bob"] } }
        }),
        &[],
    )
    .await
    .expect("Chat/set");
    resp["created"]["g"]["id"]
        .as_str()
        .expect("created group id")
        .to_owned()
}

/// One Message/set create with `attachments`; returns the response.
async fn create<B: ChatBackend<CallerCtx = ()>>(
    backend: &B,
    chat_id: &str,
    attachments: Value,
) -> Value {
    let HandlerOutput { primary: resp, .. } = handle_message_set(
        backend,
        &(),
        &std::collections::BTreeMap::new(),
        json!({
            "accountId": ACCOUNT_ID,
            "create": { "m": {
                "chatId": chat_id,
                "body": "see file",
                "sentAt": "2024-01-01T00:00:00Z",
                "attachments": attachments,
            } }
        }),
        &[],
    )
    .await
    .expect("Message/set");
    resp
}

/// filenode L167 extends Attachment only "when the
/// `urn:ietf:params:jmap:chat:filenode` capability is active". A backend
/// that does not resolve FileNodes (the default hook) leaves the chat
/// L277-290 fields required, so a `filenodeId`-only entry is malformed.
#[tokio::test]
async fn filenode_id_only_without_the_extension_is_invalid() {
    let backend = common::account_backend();
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!([{ "filenodeId": "F1" }])).await;
    assert_eq!(
        resp["notCreated"]["m"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["m"]["properties"],
        json!(["attachments"])
    );
}

/// filenode L172: with `filenodeId` the five fields "become OPTIONAL" and
/// "the server MUST populate any absent fields ... before storage". The
/// stored Message carries the Attachment the backend filled in.
#[tokio::test]
async fn filenode_id_only_entry_is_stored_as_resolved() {
    let backend = TrackingBackend::with_filenode_attachment(Ok(
        serde_json::from_value(filled()).expect("hand-written Attachment")
    ));
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!([{ "filenodeId": "F1" }])).await;
    let id = resp["created"]["m"]["id"].clone();
    assert!(id.is_string(), "not created: {resp}");
    assert_eq!(backend.filenode_attachment_call_count(), 1);

    let HandlerOutput { primary: got, .. } = handle_message_get(
        &backend,
        &(),
        json!({ "accountId": ACCOUNT_ID, "ids": [id], "properties": ["attachments"] }),
    )
    .await
    .expect("Message/get");
    assert_eq!(got["list"][0]["attachments"], json!([filled()]), "{got}");
}

/// filenode L174: a failed FileNode check is "a SetError of type
/// `invalidProperties` naming `filenodeId`"; the handler returns the
/// backend's SetError unchanged.
#[tokio::test]
async fn filenode_rejection_is_returned_as_is() {
    let backend = TrackingBackend::with_filenode_attachment(Err(SetError::new(
        SetErrorType::InvalidProperties,
    )
    .with_properties(["filenodeId"])));
    let chat_id = create_group(&backend).await;
    let resp = create(&backend, &chat_id, json!([{ "filenodeId": "F9" }])).await;
    assert_eq!(
        resp["notCreated"]["m"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notCreated"]["m"]["properties"], json!(["filenodeId"]));
}

/// chat L1676: "Enforce `maxAttachmentsPerMessage` at creation". The cap
/// is applied to the request before any FileNode is resolved.
#[tokio::test]
async fn attachment_cap_applies_before_resolution() {
    let backend = TrackingBackend::with_filenode_attachment(Ok(
        serde_json::from_value(filled()).expect("hand-written Attachment")
    ));
    backend.inner().set_limits_for_test(Some(
        ChatLimits::default().with_max_attachments_per_message(1),
    ));
    let chat_id = create_group(&backend).await;
    let two = json!([{ "filenodeId": "F1" }, { "filenodeId": "F1" }]);
    let resp = create(&backend, &chat_id, two).await;
    assert_eq!(resp["notCreated"]["m"]["type"], "tooLarge", "{resp}");
    assert_eq!(backend.filenode_attachment_call_count(), 0);
}

/// filenode L172: "If `filenodeId` is absent, these fields retain their
/// normal required status"; such an entry is not a FileNode link, so the
/// hook is not consulted.
#[tokio::test]
async fn entry_without_filenode_id_skips_resolution() {
    let backend =
        TrackingBackend::with_filenode_attachment(Err(SetError::new(SetErrorType::Forbidden)));
    let chat_id = create_group(&backend).await;
    let mut plain = filled();
    plain.as_object_mut().expect("object").remove("filenodeId");
    let resp = create(&backend, &chat_id, json!([plain])).await;
    assert!(
        resp["created"]["m"]["id"].is_string(),
        "not created: {resp}"
    );
    assert_eq!(backend.filenode_attachment_call_count(), 0);
}
