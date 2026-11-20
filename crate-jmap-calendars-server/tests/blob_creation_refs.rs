//! `#creationId` references in calendar blobId-typed values, through the
//! full dispatcher (bd:JMAP-tna1.9; docs/design/blobext.md §9).
//!
//! Oracles: draft-ietf-jmap-calendars §5.3: "Instead of mandating an
//! "href" property, clients may set a "blobId" property instead to
//! reference a blob of binary data in the account" on a Link; §5.13
//! CalendarEvent/parse `blobIds: Id[]`. RFC 8620 §5.3
//! (rfc8620.txt:1983-1996): the server "MUST keep a simple map for the
//! duration of the request of creation id to record id for each newly
//! created record, so it can substitute in the correct value if necessary
//! in later method calls"; draft-ietf-jmap-blobext-03 §8: a blob created
//! by Blob/upload, Blob/set or Blob/convert can be named by `#creationId`
//! in later method calls of the request.
//!
//! The first call is a stand-in `Blob/upload` answering with a `created`
//! map, from which the dispatcher fills the request's creation id map as it
//! does for the real method.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_calendars_server::register_calendars_handlers;
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
    register_calendars_handlers(
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
            "urn:ietf:params:jmap:calendars",
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

fn calendar() -> Value {
    json!({"name": "Work", "isSubscribed": true, "includeInAvailability": "all"})
}

fn link(blob_id: &str) -> Value {
    json!({"@type": "Link", "blobId": blob_id, "rel": "enclosure",
           "contentType": "application/pdf"})
}

/// An event with an attachment and a Location carrying a Link: a top-level
/// and a nested Link.
fn event(uid: &str, calendar: &str, blob_id: &str) -> Value {
    json!({
        "@type": "Event", "uid": uid, "title": "Review",
        "start": "2026-01-05T09:00:00", "calendarIds": {calendar: true},
        "links": {"a": link(blob_id)},
        "locations": {"room": {"@type": "Location", "name": "Room 1",
            "links": {"map": link(blob_id)}}}
    })
}

/// The stored `links/a/blobId` and `locations/room/links/map/blobId` of
/// event `id` in `account`.
async fn blobs(d: &Dispatcher<()>, account: &str, id: &str) -> (Value, Value) {
    let resp = run(
        d,
        json!([["CalendarEvent/get", {"accountId": account, "ids": [id],
            "properties": ["links", "locations"]}, "0"]]),
    )
    .await;
    let e = &args(&resp, 1)["list"][0];
    (
        e["links"]["a"]["blobId"].clone(),
        e["locations"]["room"]["links"]["map"]["blobId"].clone(),
    )
}

#[tokio::test]
async fn event_set_link_blob_ids_resolve_upload() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["Calendar/set", {"accountId": ACCOUNT, "create": {"cal": calendar()}}, "0"],
            ["CalendarEvent/set", {"accountId": ACCOUNT, "create": {
                "e": event("e", "#cal", "#b1"),
                "f": event("f", "#cal", "Bliteral")
            }}, "1"]
        ]),
    )
    .await;
    let (e, f) = (created_id(&resp, 2, "e"), created_id(&resp, 2, "f"));
    let up = json!(UPLOADED);
    assert_eq!(blobs(&d, ACCOUNT, &e).await, (up.clone(), up.clone()));

    let resp = run(
        &d,
        json!([["CalendarEvent/set", {"accountId": ACCOUNT, "update": {
            f.as_str(): {
                "links/a/blobId": "#b1",
                "locations/room/links/map": link("#b1")
            }
        }}, "0"]]),
    )
    .await;
    assert!(
        args(&resp, 1)["notUpdated"].is_null(),
        "{:?}",
        args(&resp, 1)
    );
    assert_eq!(blobs(&d, ACCOUNT, &f).await, (up.clone(), up));
}

/// A CalendarEvent/copy create may override `links`; the override's
/// blobId may name the uploaded blob.
#[tokio::test]
async fn event_copy_link_override_resolves_upload() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["Calendar/set", {"accountId": OTHER, "create": {"cal": calendar()}}, "0"],
            ["CalendarEvent/set", {"accountId": OTHER, "create": {
                "e": event("e", "#cal", "Bsource")
            }}, "1"],
            ["Calendar/set", {"accountId": ACCOUNT, "create": {"dst": calendar()}}, "2"]
        ]),
    )
    .await;
    let source = created_id(&resp, 2, "e");
    let dst = created_id(&resp, 3, "dst");
    let resp = run(
        &d,
        json!([["CalendarEvent/copy", {"fromAccountId": OTHER, "accountId": ACCOUNT,
            "create": {"k": {"id": source, "calendarIds": {dst.as_str(): true},
                "links": {"a": link("#b1")}}}
        }, "0"]]),
    )
    .await;
    let copy = created_id(&resp, 1, "k");
    assert_eq!(blobs(&d, ACCOUNT, &copy).await.0, UPLOADED);
}

/// The reference backend has no iCalendar parser, so every blob is
/// `notParsable`; the entry is named as sent.
#[tokio::test]
async fn event_parse_blob_ids_resolve_upload() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["CalendarEvent/parse", {"accountId": ACCOUNT, "blobIds": ["#b1"]}, "0"]]),
    )
    .await;
    assert_eq!(
        args(&resp, 1)["notParsable"],
        json!(["#b1"]),
        "{:?}",
        args(&resp, 1)
    );

    let resp = run(
        &d,
        json!([["CalendarEvent/parse", {"accountId": ACCOUNT, "blobIds": ["#nope"]}, "0"]]),
    )
    .await;
    assert_eq!(resp.method_responses[1].name, "error");
    assert_eq!(args(&resp, 1)["type"], "invalidArguments");
}

/// An unknown reference is not resolved and is not an Id, so a create, an
/// update and a copy override naming it fail `invalidProperties` naming the
/// property that holds the Link (draft-ietf-jmap-calendars §5.3 Link
/// `blobId`; RFC 8620 §1.2; bd:JMAP-tna1.15).
#[tokio::test]
async fn unknown_link_blob_ref_is_invalid_properties() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["Calendar/set", {"accountId": ACCOUNT, "create": {"cal": calendar()}}, "0"],
            ["CalendarEvent/set", {"accountId": ACCOUNT, "create": {
                "bad": event("e", "#cal", "#nope"),
                "ok": event("f", "#cal", "Bliteral")
            }}, "1"]
        ]),
    )
    .await;
    let err = &args(&resp, 2)["notCreated"]["bad"];
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["links"]), "{err}");
    let ok = created_id(&resp, 2, "ok");

    let resp = run(
        &d,
        json!([["CalendarEvent/set", {"accountId": ACCOUNT, "update": {
            ok.as_str(): {"locations/room/links/map/blobId": "#nope"}
        }}, "0"]]),
    )
    .await;
    let err = &args(&resp, 1)["notUpdated"][&ok];
    assert_eq!(err["properties"], json!(["locations"]), "{err}");
    let lit = json!("Bliteral");
    assert_eq!(blobs(&d, ACCOUNT, &ok).await, (lit.clone(), lit));
}

#[tokio::test]
async fn copy_override_with_unknown_link_blob_ref_is_invalid_properties() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["Calendar/set", {"accountId": OTHER, "create": {"cal": calendar()}}, "0"],
            ["CalendarEvent/set", {"accountId": OTHER, "create": {
                "e": event("e", "#cal", "Bsource")
            }}, "1"],
            ["Calendar/set", {"accountId": ACCOUNT, "create": {"dst": calendar()}}, "2"]
        ]),
    )
    .await;
    let source = created_id(&resp, 2, "e");
    let dst = created_id(&resp, 3, "dst");
    let resp = run(
        &d,
        json!([["CalendarEvent/copy", {"fromAccountId": OTHER, "accountId": ACCOUNT,
            "create": {"k": {"id": source, "calendarIds": {dst.as_str(): true},
                "links": {"a": link("#nope")}}}
        }, "0"]]),
    )
    .await;
    let err = &args(&resp, 1)["notCreated"]["k"];
    assert_eq!(err["properties"], json!(["links"]), "{err}");
}
