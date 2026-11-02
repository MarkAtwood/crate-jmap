//! Server-set values and empty patches in `Calendar/set`,
//! `ParticipantIdentity/set` and `CalendarEvent/set` updates
//! (bd:JMAP-t8ej.25.4).
//!
//! Oracle, RFC 8620 §5.3 (update): "Any server-set properties MAY be
//! included in the patch if their value is identical to the current server
//! value (before applying the patches to the object).  Otherwise, the
//! update MUST be rejected with an "invalidProperties" SetError."
//!
//! draft-ietf-jmap-calendars: Calendar `id` is "(immutable; server-set)",
//! `isDefault` and `myRights` "(server-set)"; ParticipantIdentity `id` is
//! "(immutable; server-set)", `isDefault` "(server-set)"; CalendarEvent
//! `id` is "(immutable; server-set)".
//!
//! An empty patch `{}`, literal or left empty once the identical
//! server-set values are dropped, succeeds as a no-op: `updated` maps the
//! id to null, nothing is written, and the state is unchanged (decision
//! recorded on JMAP-t8ej.25).

mod common;

use std::collections::BTreeMap;

use common::MemoryBackend;
use jmap_calendars_server::{
    handle_calendar_event_get, handle_calendar_event_set, handle_calendar_get, handle_calendar_set,
    handle_participant_identity_get, handle_participant_identity_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

fn my_rights() -> Value {
    json!({
        "mayReadFreeBusy": true,
        "mayReadItems": true,
        "mayWriteAll": true,
        "mayWriteOwn": true,
        "mayUpdatePrivate": true,
        "mayRSVP": true,
        "mayShare": true,
        "mayDelete": true,
        "mayAdmin": true
    })
}

/// A backend with Calendar `cal1`, ParticipantIdentity `pi1` and
/// CalendarEvent `ev1` in it.
fn seeded_backend() -> MemoryBackend {
    let acc = Id::from("acc1");
    let backend = MemoryBackend::new().with_account(&acc);
    backend.seed_object(
        &acc,
        "Calendar",
        &Id::from("cal1"),
        json!({
            "id": "cal1",
            "name": "Work",
            "sortOrder": 0,
            "isSubscribed": true,
            "isVisible": true,
            "isDefault": true,
            "includeInAvailability": "all",
            "myRights": my_rights()
        }),
    );
    backend.seed_object(
        &acc,
        "ParticipantIdentity",
        &Id::from("pi1"),
        json!({
            "id": "pi1",
            "name": "Alice",
            "calendarAddress": "mailto:alice@example.com",
            "isDefault": true
        }),
    );
    backend.seed_object(
        &acc,
        "CalendarEvent",
        &Id::from("ev1"),
        json!({
            "id": "ev1",
            "@type": "Event",
            "uid": "u1",
            "title": "Standup",
            "start": "2026-01-05T09:00:00",
            "calendarIds": { "cal1": true }
        }),
    );
    backend
}

/// Assert `resp` reports `id` as a no-op update: `updated[id]` is null,
/// nothing failed, and the state did not move.
fn assert_noop(resp: &Value, id: &str) {
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let updated = resp["updated"]
        .as_object()
        .unwrap_or_else(|| panic!("{resp}"));
    assert_eq!(updated.get(id), Some(&Value::Null), "{resp}");
    assert_eq!(resp["oldState"], resp["newState"], "{resp}");
}

/// Assert `resp` rejects `id` with `invalidProperties` naming `props`.
fn assert_invalid(resp: &Value, id: &str, props: Value) {
    assert_eq!(
        resp["notUpdated"][id]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notUpdated"][id]["properties"], props, "{resp}");
}

async fn calendar_set(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "cal1": patch } });
    handle_calendar_set(backend, &(), args, &[])
        .await
        .expect("Calendar/set")
        .primary
}

async fn calendar_get(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["cal1"] });
    handle_calendar_get(backend, &(), args, &[])
        .await
        .expect("Calendar/get")
        .primary["list"][0]
        .clone()
}

async fn identity_set(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "pi1": patch } });
    handle_participant_identity_set(backend, &(), args)
        .await
        .expect("ParticipantIdentity/set")
        .primary
}

async fn identity_get(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["pi1"] });
    handle_participant_identity_get(backend, &(), args)
        .await
        .expect("ParticipantIdentity/get")
        .primary["list"][0]
        .clone()
}

async fn event_set(backend: &MemoryBackend, update: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": update });
    handle_calendar_event_set(backend, &(), &BTreeMap::new(), args, &[])
        .await
        .expect("CalendarEvent/set")
        .primary
}

async fn event_get(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["ev1"] });
    handle_calendar_event_get(backend, &(), args, &[])
        .await
        .expect("CalendarEvent/get")
        .primary["list"][0]
        .clone()
}

/// §5.3: the Calendar server-set values sent as seeded are allowed; with
/// nothing else, the update is a no-op.
#[tokio::test]
async fn calendar_identical_server_set_values_are_a_noop() {
    let backend = seeded_backend();
    let before = calendar_get(&backend).await;
    let patch = json!({ "id": "cal1", "isDefault": true, "myRights": my_rights() });
    let resp = calendar_set(&backend, patch).await;
    assert_noop(&resp, "cal1");
    assert_eq!(calendar_get(&backend).await, before);
}

/// §5.3: an identical server-set value next to a client change is dropped
/// and the change applies.
#[tokio::test]
async fn calendar_identical_server_set_value_with_change_applies() {
    let backend = seeded_backend();
    let resp = calendar_set(&backend, json!({ "isDefault": true, "name": "Home" })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert!(resp["updated"].get("cal1").is_some(), "{resp}");
    assert_eq!(calendar_get(&backend).await["name"], "Home");
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn calendar_empty_patch_is_a_noop() {
    let backend = seeded_backend();
    let before = calendar_get(&backend).await;
    let resp = calendar_set(&backend, json!({})).await;
    assert_noop(&resp, "cal1");
    assert_eq!(calendar_get(&backend).await, before);
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn calendar_changed_server_set_value_is_invalid_properties() {
    let backend = seeded_backend();
    let before = calendar_get(&backend).await;
    let resp = calendar_set(&backend, json!({ "isDefault": false, "name": "Home" })).await;
    assert_invalid(&resp, "cal1", json!(["isDefault"]));
    assert_eq!(calendar_get(&backend).await, before);
}

/// §5.3: the ParticipantIdentity server-set values sent as seeded are
/// allowed; with nothing else, the update is a no-op.
#[tokio::test]
async fn identity_identical_server_set_values_are_a_noop() {
    let backend = seeded_backend();
    let before = identity_get(&backend).await;
    let resp = identity_set(&backend, json!({ "id": "pi1", "isDefault": true })).await;
    assert_noop(&resp, "pi1");
    assert_eq!(identity_get(&backend).await, before);
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn identity_empty_patch_is_a_noop() {
    let backend = seeded_backend();
    let before = identity_get(&backend).await;
    let resp = identity_set(&backend, json!({})).await;
    assert_noop(&resp, "pi1");
    assert_eq!(identity_get(&backend).await, before);
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn identity_changed_server_set_value_is_invalid_properties() {
    let backend = seeded_backend();
    let before = identity_get(&backend).await;
    let resp = identity_set(&backend, json!({ "isDefault": false, "name": "Bob" })).await;
    assert_invalid(&resp, "pi1", json!(["isDefault"]));
    assert_eq!(identity_get(&backend).await, before);
}

/// §5.3: CalendarEvent `id` sent with its stored value is allowed; with
/// nothing else, the update is a no-op.
#[tokio::test]
async fn event_identical_server_set_value_is_a_noop() {
    let backend = seeded_backend();
    let before = event_get(&backend).await;
    let resp = event_set(&backend, json!({ "ev1": { "id": "ev1" } })).await;
    assert_noop(&resp, "ev1");
    assert_eq!(event_get(&backend).await, before);
}

/// §5.3: an identical `id` next to a client change is dropped and the
/// change applies.
#[tokio::test]
async fn event_identical_server_set_value_with_change_applies() {
    let backend = seeded_backend();
    let resp = event_set(
        &backend,
        json!({ "ev1": { "id": "ev1", "title": "Retro" } }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert!(resp["updated"].get("ev1").is_some(), "{resp}");
    assert_eq!(event_get(&backend).await["title"], "Retro");
}

/// Decision on JMAP-t8ej.25: a literal `{}` is a no-op success.
#[tokio::test]
async fn event_empty_patch_is_a_noop() {
    let backend = seeded_backend();
    let before = event_get(&backend).await;
    let resp = event_set(&backend, json!({ "ev1": {} })).await;
    assert_noop(&resp, "ev1");
    assert_eq!(event_get(&backend).await, before);
}

/// §5.3 "Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError."
#[tokio::test]
async fn event_changed_server_set_value_is_invalid_properties() {
    let backend = seeded_backend();
    let before = event_get(&backend).await;
    let resp = event_set(
        &backend,
        json!({ "ev1": { "id": "other", "title": "Retro" } }),
    )
    .await;
    assert_invalid(&resp, "ev1", json!(["id"]));
    assert_eq!(event_get(&backend).await, before);
}

/// Decision on JMAP-t8ej.25: an empty patch of a missing id is
/// `notFound`.
#[tokio::test]
async fn event_empty_patch_of_missing_id_is_not_found() {
    let backend = seeded_backend();
    let resp = event_set(&backend, json!({ "nope": {} })).await;
    assert_eq!(resp["notUpdated"]["nope"]["type"], "notFound", "{resp}");
}
