//! `#creationId` references in chat blobId-typed properties, through the
//! full dispatcher (bd:JMAP-tna1.9; docs/design/blobext.md §9).
//!
//! Oracles: RFC 8620 §5.3 (rfc8620.txt:1983-1996): the server "MUST keep a
//! simple map for the duration of the request of creation id to record id
//! for each newly created record, so it can substitute in the correct value
//! if necessary in later method calls"; draft-ietf-jmap-blobext-03 §8: a
//! blob created by Blob/upload, Blob/set or Blob/convert can be named by
//! `#creationId` in later method calls of the request. The blob properties
//! are draft-atwood-jmap-chat-00's Chat `avatarBlobId`, Space `iconBlobId`,
//! Attachment `blobId` and `thumbnailBlobId`, and CustomEmoji `blobId`.
//!
//! The first call is a stand-in `Blob/upload` answering with a `created`
//! map, from which the dispatcher fills the request's creation id map as it
//! does for the real method. The reference backend does not verify
//! attachment blobs it was not told about, so each test checks the stored
//! value.

mod common;

use std::sync::Arc;

use common::{ACCOUNT_ID, MemoryBackend};
use jmap_chat_server::register_chat_handlers;
use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput};
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const UPLOADED: &str = "Gupload1";
const SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Dispatch the upload of `b1`, then `calls`, on a fresh backend.
async fn run(calls: Value) -> JmapResponse {
    let backend = MemoryBackend::<()>::default();
    backend.register_account(&Id::from(ACCOUNT_ID));
    let mut dispatcher = Dispatcher::new();
    let upload = ClosureHandler::new(Arc::new(()), |_: Arc<()>, _: CallContext<()>| {
        Box::pin(async move {
            Ok(HandlerOutput::new(json!({
                "accountId": ACCOUNT_ID,
                "created": {"b1": {"id": UPLOADED, "size": 3}}
            })))
        }) as HandlerFuture
    });
    dispatcher.register("Blob/upload", Arc::new(upload));
    register_chat_handlers(&mut dispatcher, Arc::new(backend));
    let mut method_calls = vec![json!(["Blob/upload", {"accountId": ACCOUNT_ID}, "up"])];
    method_calls.extend(calls.as_array().expect("calls").iter().cloned());
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:chat",
            "urn:ietf:params:jmap:blob2"
        ],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

/// The response arguments of call `i` (0 is the upload).
fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn attachment(blob_id: &str) -> Value {
    json!({"blobId": blob_id, "thumbnailBlobId": blob_id, "filename": "a.txt",
           "contentType": "text/plain", "size": 3, "sha256": SHA256})
}

#[tokio::test]
async fn chat_space_message_and_emoji_blob_ids_resolve_upload() {
    let resp = run(json!([
        ["Chat/set", {"accountId": ACCOUNT_ID, "create": {
            "g": {"kind": "group", "name": "G", "avatarBlobId": "#b1"}
        }}, "0"],
        ["Space/set", {"accountId": ACCOUNT_ID, "create": {
            "s": {"name": "S", "iconBlobId": "#b1"}
        }}, "1"],
        ["Message/set", {"accountId": ACCOUNT_ID, "create": {
            "m": {"chatId": "#g", "body": "x", "bodyType": "text/plain",
                  "sentAt": "2024-01-01T00:00:00Z", "attachments": [attachment("#b1")]}
        }}, "2"],
        ["CustomEmoji/set", {"accountId": ACCOUNT_ID, "create": {
            "e": {"name": "party", "blobId": "#b1"}
        }}, "3"],
        ["Chat/get", {"accountId": ACCOUNT_ID, "ids": null,
            "properties": ["avatarBlobId"]}, "4"],
        ["Space/get", {"accountId": ACCOUNT_ID, "ids": null,
            "properties": ["iconBlobId"]}, "5"],
        ["Message/get", {"accountId": ACCOUNT_ID, "ids": null,
            "properties": ["attachments"]}, "6"],
        ["CustomEmoji/get", {"accountId": ACCOUNT_ID, "ids": null,
            "properties": ["blobId"]}, "7"]
    ]))
    .await;
    for i in 1..=4 {
        assert!(
            args(&resp, i)["notCreated"].is_null(),
            "{:?}",
            args(&resp, i)
        );
    }
    assert_eq!(args(&resp, 5)["list"][0]["avatarBlobId"], UPLOADED);
    assert_eq!(args(&resp, 6)["list"][0]["iconBlobId"], UPLOADED);
    let stored = &args(&resp, 7)["list"][0]["attachments"][0];
    assert_eq!(stored["blobId"], UPLOADED, "{stored}");
    assert_eq!(stored["thumbnailBlobId"], UPLOADED, "{stored}");
    assert_eq!(args(&resp, 8)["list"][0]["blobId"], UPLOADED);
}

/// An unknown reference meets the checks any `#` value met before: the
/// Id-typed attachment `blobId` and CustomEmoji `blobId` reject it.
#[tokio::test]
async fn unknown_blob_ref_fails_as_before() {
    let resp = run(json!([
        ["Chat/set", {"accountId": ACCOUNT_ID, "create": {
            "g": {"kind": "group", "name": "G"}
        }}, "0"],
        ["Message/set", {"accountId": ACCOUNT_ID, "create": {
            "m": {"chatId": "#g", "body": "x", "bodyType": "text/plain",
                  "sentAt": "2024-01-01T00:00:00Z", "attachments": [attachment("#nope")]}
        }}, "1"],
        ["CustomEmoji/set", {"accountId": ACCOUNT_ID, "create": {
            "e": {"name": "party", "blobId": "#nope"}
        }}, "2"]
    ]))
    .await;
    let err = &args(&resp, 2)["notCreated"]["m"];
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["attachments"]), "{err}");
    let err = &args(&resp, 3)["notCreated"]["e"];
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["blobId"]), "{err}");
}
