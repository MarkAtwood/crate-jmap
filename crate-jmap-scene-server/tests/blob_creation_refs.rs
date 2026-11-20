//! `#creationId` references in SceneAsset `blobId`, through the full
//! dispatcher (bd:JMAP-tna1.9; docs/design/blobext.md §9).
//!
//! Oracles: RFC 8620 §5.3 (rfc8620.txt:1983-1996): the server "MUST keep a
//! simple map for the duration of the request of creation id to record id
//! for each newly created record, so it can substitute in the correct value
//! if necessary in later method calls"; draft-ietf-jmap-blobext-03 §8: a
//! blob created by Blob/upload, Blob/set or Blob/convert can be named by
//! `#creationId` in later method calls of the request.
//!
//! The first call is a stand-in `Blob/upload` reporting the fixture blob
//! [`BLOB`] as created `b1`; the dispatcher fills the request's creation id
//! map from it as it does for the real method.

#[macro_use]
mod common;

use std::sync::Arc;

use common::*;
use jmap_scene_server::register_scene_handlers;
use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput};
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

async fn run(asset: Value) -> JmapResponse {
    let mut dispatcher = Dispatcher::new();
    let upload = ClosureHandler::new(
        Arc::new(()),
        |_: Arc<()>, _: CallContext<jmap_types::Id>| {
            Box::pin(async move {
                Ok(HandlerOutput::new(json!({
                    "accountId": ACCOUNT,
                    "created": {"b1": {"id": BLOB, "size": BLOB_SIZE}}
                })))
            }) as HandlerFuture
        },
    );
    dispatcher.register("Blob/upload", Arc::new(upload));
    register_scene_handlers(&mut dispatcher, Arc::new(backend()));
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:scene",
            "urn:ietf:params:jmap:blob2"
        ],
        "methodCalls": [
            ["Blob/upload", {"accountId": ACCOUNT}, "up"],
            ["SceneAsset/set", {"accountId": ACCOUNT, "create": {"a": asset}}, "0"]
        ]
    }))
    .expect("request JSON must deserialize");
    dispatcher
        .dispatch(request, jmap_types::Id::from(ALICE), State::from("s0"))
        .await
}

fn asset(blob_id: &str) -> Value {
    json!({"blobId": blob_id, "mediaType": "model/gltf-binary"})
}

/// The asset's `size` is the uploaded blob's, so the create looked it up.
#[tokio::test]
async fn asset_create_blob_id_resolves_upload() {
    let resp = run(asset("#b1")).await;
    let set = &resp.method_responses[1].arguments;
    assert_eq!(set["created"]["a"]["size"], json!(BLOB_SIZE), "{set:?}");
}

/// An unknown reference is not a valid Id, as any `#` blobId was before.
#[tokio::test]
async fn asset_unknown_blob_ref_fails_as_before() {
    let resp = run(asset("#nope")).await;
    let set = &resp.method_responses[1].arguments;
    let err = &set["notCreated"]["a"];
    assert_eq!(err["type"], "invalidProperties", "{set:?}");
    assert_eq!(err["properties"], json!(["blobId"]), "{set:?}");
}
