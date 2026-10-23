//! `#creationId` references in `CalendarEvent/set` `calendarIds`, through
//! the full dispatcher (bd:JMAP-1p88).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1983-1996): "Some records may hold
//! references to other records (foreign keys). That reference may be set
//! (via create or update) in the same request as the referenced record is
//! created. To do this, the client refers to the new record using its
//! creation id prefixed with a "#". [...] the server MUST keep a simple map
//! for the duration of the request of creation id to record id for each
//! newly created record, so it can substitute in the correct value if
//! necessary in later method calls." draft-ietf-jmap-calendars-33 §5:
//! `calendarIds` is an `Id[Boolean]` set of Calendar ids.
//!
//! Each assertion compares the `calendarIds` keys with the id the server
//! reported for the referenced Calendar, or with a hand-written SetError
//! shape.

mod common;

use std::sync::Arc;

use jmap_types::Id;

use common::MemoryBackend;
use jmap_calendars_server::register_calendars_handlers;
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

fn dispatcher() -> Dispatcher<()> {
    let mut dispatcher = Dispatcher::new();
    register_calendars_handlers(
        &mut dispatcher,
        Arc::new(MemoryBackend::new().with_account(&Id::from(ACCOUNT))),
    );
    dispatcher
}

async fn run(dispatcher: &Dispatcher<()>, method_calls: Value) -> JmapResponse {
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:calendars"],
        "methodCalls": method_calls
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse, i: usize) -> &Value {
    &resp.method_responses[i].arguments
}

fn created_id(resp: &JmapResponse, i: usize, key: &str) -> String {
    args(resp, i)["created"][key]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created {key}: {}", args(resp, i)))
        .to_owned()
}

fn calendar(name: &str) -> Value {
    json!({ "name": name, "isSubscribed": true, "includeInAvailability": "all" })
}

fn event(calendar_ids: Value) -> Value {
    json!({
        "@type": "Event",
        "uid": "u1",
        "title": "Standup",
        "start": "2026-01-05T09:00:00",
        "calendarIds": calendar_ids
    })
}

async fn calendar_ids(d: &Dispatcher<()>, event_id: &str) -> Value {
    let resp = run(
        d,
        json!([["CalendarEvent/get", { "accountId": ACCOUNT, "ids": [event_id],
            "properties": ["calendarIds"] }, "0"]]),
    )
    .await;
    args(&resp, 0)["list"][0]["calendarIds"].clone()
}

#[tokio::test]
async fn calendar_ids_key_resolves_on_create() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["Calendar/set", { "accountId": ACCOUNT, "create": { "k": calendar("Home") } }, "0"],
            ["CalendarEvent/set", { "accountId": ACCOUNT,
                "create": { "e": event(json!({ "#k": true })) } }, "1"]
        ]),
    )
    .await;
    let cal = created_id(&resp, 0, "k");
    let ev = created_id(&resp, 1, "e");
    assert_eq!(calendar_ids(&d, &ev).await, json!({ cal: true }));
}

#[tokio::test]
async fn calendar_ids_patch_path_resolves_on_update() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([
            ["Calendar/set", { "accountId": ACCOUNT, "create": { "k": calendar("Home") } }, "0"],
            ["CalendarEvent/set", { "accountId": ACCOUNT,
                "create": { "e": event(json!({ "#k": true })) } }, "1"]
        ]),
    )
    .await;
    let home = created_id(&resp, 0, "k");
    let ev = created_id(&resp, 1, "e");

    let resp = run(
        &d,
        json!([
            ["Calendar/set", { "accountId": ACCOUNT, "create": { "w": calendar("Work") } }, "0"],
            ["CalendarEvent/set", { "accountId": ACCOUNT,
                "update": { (ev.clone()): { "calendarIds/#w": true } } }, "1"]
        ]),
    )
    .await;
    let work = created_id(&resp, 0, "w");
    assert!(
        args(&resp, 1)["updated"].get(&ev).is_some(),
        "{}",
        args(&resp, 1)
    );
    assert_eq!(
        calendar_ids(&d, &ev).await,
        json!({ home: true, work: true })
    );
}

#[tokio::test]
async fn unknown_calendar_reference_is_invalid_properties() {
    let d = dispatcher();
    let resp = run(
        &d,
        json!([["CalendarEvent/set", { "accountId": ACCOUNT,
            "create": { "e": event(json!({ "#nope": true })) } }, "0"]]),
    )
    .await;
    let err = &args(&resp, 0)["notCreated"]["e"];
    assert_eq!(err["type"], "invalidProperties", "{}", args(&resp, 0));
    assert_eq!(err["properties"], json!(["calendarIds"]), "{err}");
}
