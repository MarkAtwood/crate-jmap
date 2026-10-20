//! `CalendarEvent/set` update applies PatchObjects per RFC 8620 §5.3.
//!
//! Oracle: RFC 8620 §5.3 (`update` argument). Each test quotes the
//! sentence it checks; expected values are hand-written from that text,
//! using real JSCalendar properties of `CalendarEvent`.

mod common;

use common::MemoryBackend;
use jmap_calendars_server::{handle_calendar_event_get, handle_calendar_event_set};
use jmap_server::HandlerOutput;
use serde_json::{json, Value};

/// Seed `acc1` with event `ev1` holding one participant and one alert,
/// and no `locations`.
fn seeded_backend() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "CalendarEvent",
        "ev1",
        json!({
            "id": "ev1",
            "@type": "Event",
            "uid": "u1",
            "title": "Standup",
            "start": "2026-01-05T09:00:00",
            "calendarIds": { "cal1": true },
            "participants": {
                "p1": {
                    "@type": "Participant",
                    "name": "Alice",
                    "participationStatus": "needs-action"
                }
            },
            "alerts": {
                "a1": {
                    "@type": "Alert",
                    "trigger": { "@type": "OffsetTrigger", "offset": "-PT15M" },
                    "action": "display"
                }
            }
        }),
    );
    backend
}

/// Send one `update` for `ev1` and return the `/set` response.
async fn update_ev1(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "ev1": patch } });
    handle_calendar_event_set(backend, &(), args)
        .await
        .expect("/set must not return a top-level error")
        .primary
}

/// Fetch `ev1` from the store via `CalendarEvent/get`.
async fn get_ev1(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["ev1"] });
    let HandlerOutput { primary: resp, .. } = handle_calendar_event_get(backend, &(), args)
        .await
        .expect("/get must not return a top-level error");
    resp["list"][0].clone()
}

/// RFC 8620 §5.3: "The keys are a path in JSON Pointer format [RFC6901],
/// with an implicit leading "/"". So `participants/p1/participationStatus`
/// sets the nested member of participant `p1` (shared-property route),
/// leaving its siblings alone and creating no literal top-level key.
#[tokio::test]
async fn path_key_updates_nested_shared_member() {
    let backend = seeded_backend();
    let resp = update_ev1(
        &backend,
        json!({ "participants/p1/participationStatus": "accepted" }),
    )
    .await;
    assert!(
        resp["updated"].get("ev1").is_some(),
        "update must succeed: {resp}"
    );

    let ev = get_ev1(&backend).await;
    assert_eq!(
        ev["participants"]["p1"]["participationStatus"], "accepted",
        "{ev}"
    );
    assert_eq!(ev["participants"]["p1"]["name"], "Alice", "{ev}");
    assert!(
        ev.get("participants/p1/participationStatus").is_none(),
        "{ev}"
    );
}

/// RFC 8620 §5.3: "The keys are a path in JSON Pointer format [RFC6901],
/// with an implicit leading "/"". `alerts` is per-user, so this patch
/// takes the per-user route and must still be applied as a path.
#[tokio::test]
async fn path_key_updates_nested_per_user_member() {
    let backend = seeded_backend();
    let resp = update_ev1(&backend, json!({ "alerts/a1/action": "email" })).await;
    assert!(
        resp["updated"].get("ev1").is_some(),
        "update must succeed: {resp}"
    );

    let ev = get_ev1(&backend).await;
    assert_eq!(ev["alerts"]["a1"]["action"], "email", "{ev}");
    assert_eq!(ev["alerts"]["a1"]["trigger"]["offset"], "-PT15M", "{ev}");
    assert!(ev.get("alerts/a1/action").is_none(), "{ev}");
}

/// RFC 8620 §5.3: "Anything else: The value to set for this property
/// (this may be a replacement or addition to the object being patched)."
/// A top-level `participants` value replaces the whole map; members of
/// the old `p1` absent from the new value (`name`) are gone, not merged.
#[tokio::test]
async fn top_level_object_value_replaces_wholesale() {
    let backend = seeded_backend();
    let resp = update_ev1(
        &backend,
        json!({
            "participants": {
                "p1": { "@type": "Participant", "participationStatus": "declined" }
            }
        }),
    )
    .await;
    assert!(
        resp["updated"].get("ev1").is_some(),
        "update must succeed: {resp}"
    );

    let ev = get_ev1(&backend).await;
    assert_eq!(
        ev["participants"]["p1"]["participationStatus"], "declined",
        "{ev}"
    );
    assert!(ev["participants"]["p1"].get("name").is_none(), "{ev}");
}

/// RFC 8620 §5.3: "All parts prior to the last (i.e., the value after the
/// final slash) MUST already exist on the object being patched." and "if
/// there is any violation, the update MUST be rejected with an
/// "invalidPatch" error". `ev1` has no `locations`.
#[tokio::test]
async fn path_through_absent_parent_is_invalid_patch() {
    let backend = seeded_backend();
    let before = get_ev1(&backend).await;
    let resp = update_ev1(
        &backend,
        json!({ "title": "Changed", "locations/l1/name": "Room 1" }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["ev1"]["type"], "invalidPatch", "{resp}");
    assert_eq!(
        get_ev1(&backend).await,
        before,
        "stored event must be unchanged"
    );
}

/// RFC 8620 §5.3: "There MUST NOT be two patches in the PatchObject where
/// the pointer of one is the prefix of the pointer of the other, e.g.,
/// "alerts/1/offset" and "alerts"." and "if there is any violation, the
/// update MUST be rejected with an "invalidPatch" error".
#[tokio::test]
async fn prefix_conflict_is_invalid_patch() {
    let backend = seeded_backend();
    let before = get_ev1(&backend).await;
    let resp = update_ev1(
        &backend,
        json!({ "participants": {}, "participants/p1/participationStatus": "accepted" }),
    )
    .await;
    assert_eq!(resp["notUpdated"]["ev1"]["type"], "invalidPatch", "{resp}");
    assert_eq!(
        get_ev1(&backend).await,
        before,
        "stored event must be unchanged"
    );
}

/// RFC 8620 §5.3 "invalidProperties": "It contains properties that are
/// invalid according to the type specification of this record type."
/// draft-ietf-calext-jscalendarbis-16 line 1415: "sequence: UnsignedInt
/// (optional; default: 0)", so a string is rejected and the stored event
/// stays readable and unchanged.
#[tokio::test]
async fn wrong_typed_sequence_is_invalid_properties() {
    let backend = seeded_backend();
    let before = get_ev1(&backend).await;
    let resp = update_ev1(&backend, json!({ "sequence": "one" })).await;
    assert_eq!(
        resp["notUpdated"]["ev1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(get_ev1(&backend).await, before, "ev1 must be unchanged");
}
