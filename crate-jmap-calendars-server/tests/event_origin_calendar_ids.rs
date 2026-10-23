//! CalendarEvent `isOrigin` is set by the server and `calendarIds` always
//! names at least one calendar.
//!
//! Oracle: draft-ietf-jmap-calendars-33 §5:
//! - "isOrigin: Boolean (server-set) [...] This is true if, and only if:
//!   the event's \"organizerCalendarAddress\" property is null; or the
//!   account will receive messages sent to the \"organizerCalendarAddress\"
//!   URI."
//! - "calendarIds: Id[Boolean] [...] An event MUST belong to one or more
//!   Calendars at all times (until it is destroyed). [...] The value for
//!   each key in the object MUST be true."
//!
//! The account's own addresses are its ParticipantIdentity
//! `calendarAddress` values.

mod common;

use common::MemoryBackend;
use jmap_calendars_server::{
    handle_calendar_event_copy, handle_calendar_event_get, handle_calendar_event_set,
};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

const OWN: &str = "mailto:alice@example.com";
const FOREIGN: &str = "mailto:bob@example.org";

fn backend() -> MemoryBackend {
    let backend = MemoryBackend::new()
        .with_account("acc1")
        .with_account("acc2");
    backend.seed_object(
        "acc1",
        "ParticipantIdentity",
        "pi1",
        json!({ "id": "pi1", "name": "Alice", "calendarAddress": OWN, "isDefault": true }),
    );
    backend
}

fn event(extra: Value) -> Value {
    let mut ev = json!({
        "@type": "Event",
        "uid": "u1",
        "title": "Standup",
        "start": "2026-01-05T09:00:00",
        "calendarIds": { "cal1": true }
    });
    let map = ev.as_object_mut().unwrap();
    for (k, v) in extra.as_object().unwrap() {
        if v.is_null() {
            map.remove(k);
        } else {
            map.insert(k.clone(), v.clone());
        }
    }
    ev
}

async fn set(backend: &MemoryBackend, args: Value) -> Value {
    handle_calendar_event_set(backend, &(), args)
        .await
        .expect("/set must not return a top-level error")
        .primary
}

async fn create(backend: &MemoryBackend, ev: Value) -> Value {
    set(
        backend,
        json!({ "accountId": "acc1", "create": { "c1": ev } }),
    )
    .await
}

async fn get(backend: &MemoryBackend, account: &str, id: &str) -> Value {
    let args = json!({ "accountId": account, "ids": [id] });
    let HandlerOutput { primary: resp, .. } = handle_calendar_event_get(backend, &(), args)
        .await
        .expect("/get must not return a top-level error");
    resp["list"][0].clone()
}

async fn created_id(backend: &MemoryBackend, ev: Value) -> String {
    let resp = create(backend, ev).await;
    resp["created"]["c1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create failed: {resp}"))
        .to_owned()
}

async fn update(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    set(
        backend,
        json!({ "accountId": "acc1", "update": { id: patch } }),
    )
    .await
}

#[tokio::test]
async fn create_sets_is_origin_from_organizer() {
    let backend = backend();
    for (organizer, want) in [(None, true), (Some(OWN), true), (Some(FOREIGN), false)] {
        let ev = event(json!({ "organizerCalendarAddress": organizer }));
        let id = created_id(&backend, ev).await;
        assert_eq!(
            get(&backend, "acc1", &id).await["isOrigin"],
            want,
            "{organizer:?}"
        );
    }
}

/// draft-ietf-jmap-calendars-33 §5: `isOrigin` is "(server-set)"; RFC 8620
/// §1.1: "The client MUST NOT send this property when creating", and RFC
/// 8620 §5.3 lists that under `invalidProperties`.
#[tokio::test]
async fn create_rejects_client_is_origin() {
    let backend = backend();
    let resp = create(&backend, event(json!({ "isOrigin": false }))).await;
    assert_eq!(
        resp["notCreated"]["c1"],
        json!({ "type": "invalidProperties", "properties": ["isOrigin"] }),
        "{resp}"
    );
}

#[tokio::test]
async fn create_without_a_calendar_is_invalid_properties() {
    let backend = backend();
    for calendar_ids in [Value::Null, json!({}), json!({ "cal1": false })] {
        let ev = event(json!({ "calendarIds": calendar_ids }));
        let resp = create(&backend, ev).await;
        let err = &resp["notCreated"]["c1"];
        assert_eq!(err["type"], "invalidProperties", "{calendar_ids}: {resp}");
        assert_eq!(err["properties"], json!(["calendarIds"]), "{resp}");
    }
}

#[tokio::test]
async fn update_is_origin_is_invalid_properties() {
    let backend = backend();
    let id = created_id(&backend, event(json!({}))).await;
    let resp = update(&backend, &id, json!({ "isOrigin": false })).await;
    assert_eq!(
        resp["notUpdated"][&id]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notUpdated"][&id]["properties"], json!(["isOrigin"]));
    assert_eq!(get(&backend, "acc1", &id).await["isOrigin"], true);
}

#[tokio::test]
async fn update_organizer_recomputes_is_origin() {
    let backend = backend();
    let id = created_id(&backend, event(json!({}))).await;

    let resp = update(
        &backend,
        &id,
        json!({ "organizerCalendarAddress": FOREIGN }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(get(&backend, "acc1", &id).await["isOrigin"], false);

    let resp = update(&backend, &id, json!({ "organizerCalendarAddress": null })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(get(&backend, "acc1", &id).await["isOrigin"], true);
}

#[tokio::test]
async fn update_leaving_no_calendar_is_invalid_properties() {
    let backend = backend();
    let id = created_id(&backend, event(json!({}))).await;
    for patch in [
        json!({ "calendarIds": {} }),
        json!({ "calendarIds": null }),
        json!({ "calendarIds/cal1": null }),
        json!({ "calendarIds/cal2": false }),
    ] {
        let resp = update(&backend, &id, patch.clone()).await;
        let err = &resp["notUpdated"][&id];
        assert_eq!(err["type"], "invalidProperties", "{patch}: {resp}");
        assert_eq!(err["properties"], json!(["calendarIds"]), "{resp}");
    }
    assert_eq!(
        get(&backend, "acc1", &id).await["calendarIds"],
        json!({ "cal1": true })
    );
}

#[tokio::test]
async fn update_moving_to_another_calendar_succeeds() {
    let backend = backend();
    let id = created_id(&backend, event(json!({}))).await;
    let patch = json!({ "calendarIds/cal1": null, "calendarIds/cal2": true });
    let resp = update(&backend, &id, patch).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(
        get(&backend, "acc1", &id).await["calendarIds"],
        json!({ "cal2": true })
    );
}

#[tokio::test]
async fn copy_recomputes_is_origin_in_target_account() {
    // acc2 has no ParticipantIdentity, so it receives nothing sent to OWN.
    let backend = backend();
    let id = created_id(&backend, event(json!({ "organizerCalendarAddress": OWN }))).await;
    assert_eq!(get(&backend, "acc1", &id).await["isOrigin"], true);

    let args = json!({
        "fromAccountId": "acc1",
        "accountId": "acc2",
        "create": { "k1": { "id": id } }
    });
    let HandlerOutput { primary: resp, .. } = handle_calendar_event_copy(&backend, &(), args, "c0")
        .await
        .expect("/copy must not return a top-level error");
    let new_id = resp["created"]["k1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("copy failed: {resp}"));
    assert_eq!(get(&backend, "acc2", new_id).await["isOrigin"], false);
}

/// draft-ietf-jmap-calendars-33 §5.9: "If omitted on create, the server
/// MUST set the following properties: * @type MUST be set to "Event" *
/// version MUST be set to the JSCalendar version the event conforms to,
/// e.g., "2.0"". A `/copy` creates the event in the target account
/// (§5.10, RFC 8620 §5.4), so a stored source that lacks both gets them.
#[tokio::test]
async fn copy_fills_omitted_type_and_version() {
    let backend = backend();
    backend.seed_object(
        "acc1",
        "CalendarEvent",
        "src",
        json!({
            "id": "src",
            "uid": "u-bare",
            "title": "Standup",
            "start": "2026-01-05T09:00:00",
            "calendarIds": { "cal1": true }
        }),
    );
    let args = json!({
        "fromAccountId": "acc1",
        "accountId": "acc2",
        "create": { "k1": { "id": "src" } }
    });
    let HandlerOutput { primary: resp, .. } = handle_calendar_event_copy(&backend, &(), args, "c0")
        .await
        .expect("/copy must not return a top-level error");
    let new_id = resp["created"]["k1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("copy failed: {resp}"));
    let copied = get(&backend, "acc2", new_id).await;
    assert_eq!(copied["@type"], "Event", "{copied}");
    assert_eq!(copied["version"], "2.0", "{copied}");
}
