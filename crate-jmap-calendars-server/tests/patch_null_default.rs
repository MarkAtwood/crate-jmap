//! `/set` update with a `null` PatchObject value resets the property to
//! its spec default.
//!
//! Oracle: RFC 8620 §5.3: "If null, set to the default value if specified
//! for this property; otherwise, remove the property from the patched
//! object." Each test quotes the draft-ietf-jmap-calendars-33 line that
//! specifies the default it expects.

mod common;

use common::MemoryBackend;
use jmap_calendars_server::{
    handle_calendar_get, handle_calendar_set, handle_participant_identity_get,
    handle_participant_identity_set,
};
use jmap_server::HandlerOutput;
use serde_json::{Value, json};

/// Seed `acc1` with Calendar `cal1` whose `sortOrder` and `isVisible`
/// hold non-default values.
fn backend_with_calendar() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "Calendar",
        "cal1",
        json!({
            "id": "cal1",
            "name": "Work",
            "description": null,
            "color": null,
            "sortOrder": 7,
            "isSubscribed": true,
            "isVisible": false,
            "isDefault": false,
            "includeInAvailability": "all",
            "defaultAlertsWithTime": null,
            "defaultAlertsWithoutTime": null,
            "timeZone": null,
            "shareWith": null,
            "myRights": {
                "mayReadFreeBusy": true,
                "mayReadItems": true,
                "mayWriteAll": true,
                "mayWriteOwn": true,
                "mayUpdatePrivate": true,
                "mayRSVP": true,
                "mayShare": true,
                "mayDelete": true,
                "mayAdmin": true
            }
        }),
    );
    backend
}

/// Seed `acc1` with ParticipantIdentity `pi1` named "Alice".
fn backend_with_identity() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "ParticipantIdentity",
        "pi1",
        json!({
            "id": "pi1",
            "name": "Alice",
            "calendarAddress": "mailto:alice@example.com",
            "isDefault": true
        }),
    );
    backend
}

async fn update_calendar(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "cal1": patch } });
    handle_calendar_set(backend, &(), args)
        .await
        .expect("/set must not return a top-level error")
        .primary
}

async fn get_calendar(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["cal1"] });
    let HandlerOutput { primary: resp, .. } = handle_calendar_get(backend, &(), args)
        .await
        .expect("/get must not return a top-level error");
    resp["list"][0].clone()
}

async fn update_identity(backend: &MemoryBackend, patch: Value) -> Value {
    let args = json!({ "accountId": "acc1", "update": { "pi1": patch } });
    handle_participant_identity_set(backend, &(), args)
        .await
        .expect("/set must not return a top-level error")
        .primary
}

async fn get_identity(backend: &MemoryBackend) -> Value {
    let args = json!({ "accountId": "acc1", "ids": ["pi1"] });
    let HandlerOutput { primary: resp, .. } = handle_participant_identity_get(backend, &(), args)
        .await
        .expect("/get must not return a top-level error");
    resp["list"][0].clone()
}

/// draft-ietf-jmap-calendars-33 line 774:
/// "*sortOrder*: UnsignedInt (default: 0)".
#[tokio::test]
async fn calendar_sort_order_null_resets_to_zero() {
    let backend = backend_with_calendar();
    assert_eq!(get_calendar(&backend).await["sortOrder"], 7);

    let resp = update_calendar(&backend, json!({ "sortOrder": null })).await;
    assert!(resp["updated"].get("cal1").is_some(), "{resp}");

    let cal = get_calendar(&backend).await;
    assert_eq!(cal["sortOrder"], 0, "{cal}");
}

/// draft-ietf-jmap-calendars-33 line 808:
/// "*isVisible*: Boolean (default: true)".
#[tokio::test]
async fn calendar_is_visible_null_resets_to_true() {
    let backend = backend_with_calendar();
    assert_eq!(get_calendar(&backend).await["isVisible"], false);

    let resp = update_calendar(&backend, json!({ "isVisible": null })).await;
    assert!(resp["updated"].get("cal1").is_some(), "{resp}");

    let cal = get_calendar(&backend).await;
    assert_eq!(cal["isVisible"], true, "{cal}");
}

/// draft-ietf-jmap-calendars-33 line 630:
/// "*name*: String (default: "")".
#[tokio::test]
async fn participant_identity_name_null_resets_to_empty() {
    let backend = backend_with_identity();
    assert_eq!(get_identity(&backend).await["name"], "Alice");

    let resp = update_identity(&backend, json!({ "name": null })).await;
    assert!(resp["updated"].get("pi1").is_some(), "{resp}");

    let identity = get_identity(&backend).await;
    assert_eq!(identity["name"], "", "{identity}");
}

/// RFC 8620 §5.3: "If null, set to the default value if specified for
/// this property; otherwise, remove the property from the patched
/// object." draft-ietf-jmap-calendars-33 line 750 gives "*name*: String"
/// with no default, so the patched Calendar is, per RFC 8620 §5.3
/// "invalidProperties", "invalid in some way". The rejected update must
/// leave the stored calendar readable and unchanged.
#[tokio::test]
async fn calendar_required_name_null_is_invalid_properties() {
    let backend = backend_with_calendar();
    let resp = update_calendar(&backend, json!({ "name": null })).await;
    assert_eq!(
        resp["notUpdated"]["cal1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(get_calendar(&backend).await["name"], "Work");
}

/// RFC 8620 §5.3: "If null, set to the default value if specified for
/// this property; otherwise, remove the property from the patched
/// object." draft-ietf-jmap-calendars-33 line 634 gives
/// "*calendarAddress*: String" with no default, so the update is
/// "invalidProperties" and the stored identity stays readable.
#[tokio::test]
async fn participant_identity_required_calendar_address_null_is_invalid_properties() {
    let backend = backend_with_identity();
    let resp = update_identity(&backend, json!({ "calendarAddress": null })).await;
    assert_eq!(
        resp["notUpdated"]["pi1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        get_identity(&backend).await["calendarAddress"],
        "mailto:alice@example.com"
    );
}
