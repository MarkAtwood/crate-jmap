//! `#creationId` references in FileNode `blobId`, through the full
//! dispatcher (bd:JMAP-tna1.9; docs/design/blobext.md §9).
//!
//! Oracles: RFC 8620 §5.3 (rfc8620.txt:1983-1996): the server "MUST keep a
//! simple map for the duration of the request of creation id to record id
//! for each newly created record, so it can substitute in the correct value
//! if necessary in later method calls"; draft-ietf-jmap-blobext-03 §8: a
//! blob created by Blob/upload, Blob/set or Blob/convert can be named by
//! `#creationId` in later method calls of the request.
//!
//! The first call is a stand-in `Blob/upload` answering with a `created`
//! map, from which the dispatcher fills the request's creation id map as it
//! does for the real method. The memory backend treats every blob as
//! present, so each test checks the blobId the stored node ends up with.

mod common;

use std::sync::Arc;

use common::{MemoryBackend, owner};
use jmap_filenode_server::register_filenode_handlers;
use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput};
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";
const OTHER: &str = "acc2";
const UPLOADED: &str = "Gupload1";

fn dispatcher() -> Dispatcher<Id> {
    let mut dispatcher = Dispatcher::new();
    let upload = ClosureHandler::new(Arc::new(()), |_: Arc<()>, _: CallContext<Id>| {
        Box::pin(async move {
            Ok(HandlerOutput::new(json!({
                "accountId": ACCOUNT,
                "created": {"b1": {"id": UPLOADED, "size": 3}}
            })))
        }) as HandlerFuture
    });
    dispatcher.register("Blob/upload", Arc::new(upload));
    register_filenode_handlers(
        &mut dispatcher,
        Arc::new(
            MemoryBackend::new()
                .with_account_owner(&Id::from(ACCOUNT), &owner())
                .with_account_owner(&Id::from(OTHER), &owner()),
        ),
    );
    dispatcher
}

/// Dispatch the upload of `b1`, then `calls`.
async fn run(d: &Dispatcher<Id>, calls: Value) -> JmapResponse {
    let mut method_calls = vec![json!(["Blob/upload", {"accountId": ACCOUNT}, "up"])];
    method_calls.extend(calls.as_array().expect("calls").iter().cloned());
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:filenode",
            "urn:ietf:params:jmap:blob2"
        ],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    d.dispatch(request, owner(), State::from("s0")).await
}

/// The response arguments of call `i` (0 is the upload).
fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn file(name: &str, blob_id: &str) -> Value {
    json!({"@type": "File", "name": name, "parentId": null, "blobId": blob_id,
           "mediaType": "text/plain"})
}

async fn blob_of(d: &Dispatcher<Id>, account: &str, id: &str) -> Value {
    let resp = run(
        d,
        json!([["FileNode/get", {"accountId": account, "ids": [id],
            "properties": ["blobId"]}, "0"]]),
    )
    .await;
    args(&resp, 1)["list"][0]["blobId"].clone()
}

#[tokio::test]
async fn set_create_and_update_blob_id_resolve_upload() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["FileNode/set", {"accountId": ACCOUNT, "create": {
            "f": file("a.txt", "#b1"),
            "g": file("b.txt", "Bliteral")
        }}, "0"]]),
    )
    .await;
    let set = args(&resp, 1);
    let f = set["created"]["f"]["id"].as_str().expect("f").to_owned();
    let g = set["created"]["g"]["id"].as_str().expect("g").to_owned();
    assert_eq!(blob_of(&d, ACCOUNT, &f).await, UPLOADED);

    let resp = run(
        &d,
        json!([["FileNode/set", {"accountId": ACCOUNT, "update": {
            g.as_str(): {"blobId": "#b1"}
        }}, "0"]]),
    )
    .await;
    assert!(
        args(&resp, 1)["updated"].get(&g).is_some(),
        "{:?}",
        args(&resp, 1)
    );
    assert_eq!(blob_of(&d, ACCOUNT, &g).await, UPLOADED);
}

/// An unknown reference is not a valid Id, as any `#` blobId was before.
#[tokio::test]
async fn set_unknown_blob_ref_fails_as_before() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["FileNode/set", {"accountId": ACCOUNT, "create": {
            "f": file("a.txt", "#nope"),
            "g": file("b.txt", "Bliteral")
        }}, "0"]]),
    )
    .await;
    let set = args(&resp, 1);
    let err = &set["notCreated"]["f"];
    assert_eq!(err["type"], "invalidProperties", "{set:?}");
    assert_eq!(err["properties"], json!(["blobId"]), "{set:?}");
    let g = set["created"]["g"]["id"].as_str().expect("g").to_owned();

    let resp = run(
        &d,
        json!([["FileNode/set", {"accountId": ACCOUNT, "update": {
            g.as_str(): {"blobId": "#nope"}
        }}, "0"]]),
    )
    .await;
    let err = &args(&resp, 1)["notUpdated"][&g];
    assert_eq!(err["type"], "invalidProperties", "{err:?}");
    assert_eq!(err["properties"], json!(["blobId"]), "{err:?}");
    assert_eq!(blob_of(&d, ACCOUNT, &g).await, "Bliteral");
}

/// A FileNode/copy create may override `blobId` (draft-ietf-jmap-filenode
/// §3.2.4); the override may name the uploaded blob.
#[tokio::test]
async fn copy_blob_id_override_resolves_upload() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["FileNode/set", {"accountId": OTHER, "create": {
            "f": file("a.txt", "Bsource")
        }}, "0"]]),
    )
    .await;
    let source = args(&resp, 1)["created"]["f"]["id"]
        .as_str()
        .expect("source")
        .to_owned();
    let resp = run(
        &d,
        json!([["FileNode/copy", {"fromAccountId": OTHER, "accountId": ACCOUNT,
            "create": {"c": {"id": source, "parentId": null, "blobId": "#b1"}}
        }, "0"]]),
    )
    .await;
    let copy = args(&resp, 1);
    let id = copy["created"]["c"]["id"]
        .as_str()
        .expect("copy")
        .to_owned();
    assert_eq!(blob_of(&d, ACCOUNT, &id).await, UPLOADED, "{copy:?}");
}
