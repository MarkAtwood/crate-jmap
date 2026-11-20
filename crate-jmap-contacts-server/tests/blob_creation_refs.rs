//! `#creationId` references in a ContactCard Media `blobId`, through the
//! full dispatcher (bd:JMAP-tna1.9; docs/design/blobext.md §9).
//!
//! Oracles: RFC 9610 §2: "For any Media object in the card [...] *blobId*:
//! Id"; "when creating or updating a ContactCard, clients MAY send a
//! "blobId" instead of the "uri" property for a Media object". RFC 8620
//! §5.3 (rfc8620.txt:1983-1996): the server "MUST keep a simple map for the
//! duration of the request of creation id to record id for each newly
//! created record, so it can substitute in the correct value if necessary
//! in later method calls"; draft-ietf-jmap-blobext-03 §8: a blob created
//! by Blob/upload, Blob/set or Blob/convert can be named by `#creationId`
//! in later method calls of the request.
//!
//! The first call is a stand-in `Blob/upload` answering with a `created`
//! map, from which the dispatcher fills the request's creation id map as it
//! does for the real method. Each test checks the stored Media `blobId`.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_contacts_server::register_contacts_handlers;
use jmap_server::{CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput};
use jmap_types::{Id, JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";
const OTHER: &str = "acc2";
const UPLOADED: &str = "Gupload1";

fn dispatcher() -> Dispatcher<()> {
    let mut dispatcher = Dispatcher::new();
    let upload = ClosureHandler::new(Arc::new(()), |_: Arc<()>, _: CallContext<()>| {
        Box::pin(async move {
            Ok(HandlerOutput::new(json!({
                "accountId": ACCOUNT,
                "created": {"b1": {"id": UPLOADED, "size": 3}}
            })))
        }) as HandlerFuture
    });
    dispatcher.register("Blob/upload", Arc::new(upload));
    register_contacts_handlers(
        &mut dispatcher,
        Arc::new(
            MemoryBackend::<()>::default()
                .with_account(&Id::from(ACCOUNT))
                .with_account(&Id::from(OTHER)),
        ),
    );
    dispatcher
}

/// Dispatch the upload of `b1`, then `calls`.
async fn run(d: &Dispatcher<()>, calls: Value) -> JmapResponse {
    let mut method_calls = vec![json!(["Blob/upload", {"accountId": ACCOUNT}, "up"])];
    method_calls.extend(calls.as_array().expect("calls").iter().cloned());
    let request: JmapRequest = serde_json::from_value(json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:contacts",
            "urn:ietf:params:jmap:blob2"
        ],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    d.dispatch(request, (), State::from("s0")).await
}

/// The response arguments of call `i` (0 is the upload).
fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created_id(resp: &JmapResponse, i: usize, key: &str) -> String {
    args(resp, i)["created"][key]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created {key}: {}", args(resp, i)))
        .to_owned()
}

fn card(uid: &str, media: Value) -> Value {
    json!({
        "@type": "Card", "version": "1.0", "uid": uid,
        "addressBookIds": {"#ab": true},
        "name": {"@type": "Name", "full": "Ada"},
        "media": media
    })
}

fn photo(blob_id: &str) -> Value {
    json!({"@type": "Media", "kind": "photo", "blobId": blob_id, "mediaType": "image/png"})
}

/// The stored `media/p/blobId` of card `id` in `account`.
async fn photo_blob(d: &Dispatcher<()>, account: &str, id: &str) -> Value {
    let resp = run(
        d,
        json!([["ContactCard/get", {"accountId": account, "ids": [id],
            "properties": ["media"]}, "0"]]),
    )
    .await;
    args(&resp, 1)["list"][0]["media"]["p"]["blobId"].clone()
}

const BOOK: &str = r#"{"name": "Friends", "isSubscribed": true}"#;

fn book() -> Value {
    serde_json::from_str(BOOK).expect("book")
}

/// Create, an update by patch path, and an update replacing the Media all
/// store the uploaded blob's id.
#[tokio::test]
async fn card_set_media_blob_id_resolves_upload() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["AddressBook/set", {"accountId": ACCOUNT, "create": {"ab": book()}}, "0"],
            ["ContactCard/set", {"accountId": ACCOUNT, "create": {
                "c": card("c", json!({"p": photo("#b1")})),
                "d": card("d", json!({"p": photo("Bliteral")})),
                "e": card("e", json!({"p": photo("Bliteral")}))
            }}, "1"]
        ]),
    )
    .await;
    let (c, d_id, e) = (
        created_id(&resp, 2, "c"),
        created_id(&resp, 2, "d"),
        created_id(&resp, 2, "e"),
    );
    assert_eq!(photo_blob(&d, ACCOUNT, &c).await, UPLOADED);

    let resp = run(
        &d,
        json!([["ContactCard/set", {"accountId": ACCOUNT, "update": {
            d_id.as_str(): {"media/p/blobId": "#b1"},
            e.as_str(): {"media/p": photo("#b1")}
        }}, "0"]]),
    )
    .await;
    assert!(
        args(&resp, 1)["notUpdated"].is_null(),
        "{:?}",
        args(&resp, 1)
    );
    assert_eq!(photo_blob(&d, ACCOUNT, &d_id).await, UPLOADED);
    assert_eq!(photo_blob(&d, ACCOUNT, &e).await, UPLOADED);
}

/// A ContactCard/copy create may override `media`; the override's blobId
/// may name the uploaded blob.
#[tokio::test]
async fn card_copy_media_override_resolves_upload() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["AddressBook/set", {"accountId": OTHER, "create": {"ab": book()}}, "0"],
            ["ContactCard/set", {"accountId": OTHER, "create": {
                "c": card("c", json!({"p": photo("Bsource")}))
            }}, "1"],
            ["AddressBook/set", {"accountId": ACCOUNT, "create": {"dst": book()}}, "2"]
        ]),
    )
    .await;
    let source = created_id(&resp, 2, "c");
    let book = created_id(&resp, 3, "dst");
    let resp = run(
        &d,
        json!([["ContactCard/copy", {"fromAccountId": OTHER, "accountId": ACCOUNT,
            "create": {"k": {"id": source, "addressBookIds": {book.as_str(): true},
                "media": {"p": photo("#b1")}}}
        }, "0"]]),
    )
    .await;
    let copy = created_id(&resp, 1, "k");
    assert_eq!(photo_blob(&d, ACCOUNT, &copy).await, UPLOADED);
}

/// An unknown reference is not resolved and is not an Id, so the create,
/// an update and a copy override fail `invalidProperties` naming `media`
/// (RFC 9610 §2 "*blobId*: Id"; bd:JMAP-tna1.15).
#[tokio::test]
async fn unknown_blob_ref_is_invalid_properties() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["AddressBook/set", {"accountId": ACCOUNT, "create": {"ab": book()}}, "0"],
            ["ContactCard/set", {"accountId": ACCOUNT, "create": {
                "bad": card("c", json!({"p": photo("#nope")})),
                "ok": card("d", json!({"p": photo("Bliteral")}))
            }}, "1"]
        ]),
    )
    .await;
    let invalid = json!({"type": "invalidProperties", "properties": ["media"],
                         "description": "a blobId is not an Id (RFC 8620 §1.2)"});
    assert_eq!(args(&resp, 2)["notCreated"]["bad"], invalid);
    let ok = created_id(&resp, 2, "ok");

    let resp = run(
        &d,
        json!([["ContactCard/set", {"accountId": ACCOUNT, "update": {
            ok.as_str(): {"media/p/blobId": "#nope"}
        }}, "0"]]),
    )
    .await;
    assert_eq!(args(&resp, 1)["notUpdated"][&ok], invalid);
    assert_eq!(photo_blob(&d, ACCOUNT, &ok).await, "Bliteral");
}

/// A ContactCard/copy override's Media `blobId` must be an Id too.
#[tokio::test]
async fn copy_override_with_unknown_blob_ref_is_invalid_properties() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["AddressBook/set", {"accountId": OTHER, "create": {"ab": book()}}, "0"],
            ["ContactCard/set", {"accountId": OTHER, "create": {
                "c": card("c", json!({"p": photo("Bsource")}))
            }}, "1"],
            ["AddressBook/set", {"accountId": ACCOUNT, "create": {"dst": book()}}, "2"]
        ]),
    )
    .await;
    let source = created_id(&resp, 2, "c");
    let book = created_id(&resp, 3, "dst");
    let resp = run(
        &d,
        json!([["ContactCard/copy", {"fromAccountId": OTHER, "accountId": ACCOUNT,
            "create": {"k": {"id": source, "addressBookIds": {book.as_str(): true},
                "media": {"p": photo("#nope")}}}
        }, "0"]]),
    )
    .await;
    let err = &args(&resp, 1)["notCreated"]["k"];
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["media"]), "{err}");
}
