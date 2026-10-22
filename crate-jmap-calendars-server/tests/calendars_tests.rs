//! Integration tests for `jmap-calendars-server` using `MemoryBackend`.
//!
//! All expected values are derived from the spec
//! (draft-ietf-jmap-calendars-33) and RFC 8620, not from the code under
//! test. Wire-shape literals are hand-written from the draft's prose.
//!
//! These tests exercise the cookie-cut canonical shape established by
//! JMAP-hwdv.1 (jmap-mail-server) and JMAP-hwdv.3 (jmap-chat-server).
//! Bead: JMAP-hwdv.5.

mod common;

use common::MemoryBackend;
use jmap_calendars_server::{
    handle_calendar_changes, handle_calendar_event_changes, handle_calendar_event_get,
    handle_calendar_event_notification_query, handle_calendar_event_query,
    handle_calendar_event_set, handle_calendar_get, handle_calendar_set,
    handle_participant_identity_get, handle_participant_identity_set,
};
use jmap_server::HandlerOutput;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Calendar fixture helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid Calendar JSON value with the given id and name.
///
/// Fields below are the §4 mandatory shape; null/default values follow the
/// draft. `myRights` is server-set and required to deserialize as a
/// `Calendar` (no `Default`-only constructor exists outside the defining
/// crate per `#[non_exhaustive]`).
fn calendar_fixture(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "description": null,
        "color": null,
        "sortOrder": 0,
        "isSubscribed": true,
        "isVisible": true,
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
    })
}

// ---------------------------------------------------------------------------
// Test 1: Calendar/get against an empty account → empty list, notFound=[]
// Oracle: RFC 8620 §5.1 — /get on an empty store returns list:[] and notFound
// must be a (possibly empty) array, never null.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_get_empty_account_returns_empty_list() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "ids": null
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_get(&backend, &(), args)
        .await
        .expect("/get must not return top-level error");

    assert_eq!(
        resp["accountId"], "acc1",
        "response must echo accountId: {resp}"
    );
    assert!(
        resp["list"].is_array() && resp["list"].as_array().unwrap().is_empty(),
        "list must be an empty array: {resp}"
    );
    assert!(
        resp["notFound"].is_array(),
        "notFound MUST be an array (never null) per RFC 8620 §5.1: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Calendar/get with explicit ids returns seeded calendar + notFound
// Oracle: RFC 8620 §5.1 — found objects in list, unknown ids in notFound.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_get_seeded_calendar_and_unknown_id() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "Calendar", "cal1", calendar_fixture("cal1", "Work"));

    let args = json!({
        "accountId": "acc1",
        "ids": ["cal1", "does-not-exist"]
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_get(&backend, &(), args)
        .await
        .expect("/get must not return top-level error");

    let list = resp["list"].as_array().expect("list must be array");
    assert_eq!(list.len(), 1, "exactly one calendar must be found: {resp}");
    assert_eq!(list[0]["id"], "cal1", "must return cal1: {resp}");
    assert_eq!(list[0]["name"], "Work", "must echo name: {resp}");

    let not_found = resp["notFound"].as_array().expect("notFound must be array");
    assert_eq!(not_found.len(), 1, "one id must be not-found: {resp}");
    assert_eq!(
        not_found[0], "does-not-exist",
        "must echo unknown id: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Calendar/changes on an empty store returns empty result with
// stable state
// Oracle: RFC 8620 §5.2 — /changes since the current state returns no
// changes; oldState == newState.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_changes_no_changes_since_current_state() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "sinceState": "0"
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_changes(&backend, &(), args)
        .await
        .expect("/changes must not return top-level error");

    assert_eq!(
        resp["oldState"], "0",
        "oldState must echo sinceState: {resp}"
    );
    assert_eq!(
        resp["newState"], "0",
        "newState must equal oldState for empty store: {resp}"
    );
    assert!(
        resp["created"].as_array().unwrap().is_empty(),
        "created must be empty: {resp}"
    );
    assert!(
        resp["updated"].as_array().unwrap().is_empty(),
        "updated must be empty: {resp}"
    );
    assert!(
        resp["destroyed"].as_array().unwrap().is_empty(),
        "destroyed must be empty: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Calendar/set destroy with no events succeeds
// Oracle: draft-ietf-jmap-calendars-33 §4.4 — destroy proceeds when the
// calendar has no events; oldState bumps to newState.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_set_destroy_empty_calendar_succeeds() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "Calendar", "cal1", calendar_fixture("cal1", "Work"));

    let args = json!({
        "accountId": "acc1",
        "destroy": ["cal1"]
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
        .await
        .expect("/set must not return top-level error");

    let destroyed = resp["destroyed"]
        .as_array()
        .expect("destroyed must be an array");
    assert_eq!(destroyed.len(), 1, "exactly one id destroyed: {resp}");
    assert_eq!(destroyed[0], "cal1", "must destroy cal1: {resp}");

    assert_ne!(
        resp["oldState"], resp["newState"],
        "state must bump after destroy: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Calendar/set destroy with events → calendarHasEvent error
// Oracle: draft-ietf-jmap-calendars-33 §4.4, §10.8.1 — destroy of a
// Calendar that has events MUST fail with the spec-registered name
// `calendarHasEvent` (singular).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_set_destroy_with_events_returns_calendar_has_event() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "Calendar", "cal1", calendar_fixture("cal1", "Work"));
    // Seed an event referencing cal1 so calendar_has_events returns true.
    backend.seed_object(
        "acc1",
        "CalendarEvent",
        "evt1",
        json!({
            "id": "evt1",
            "@type": "Event",
            "uid": "evt1-uid",
            "title": "Standup",
            "calendarIds": { "cal1": true }
        }),
    );

    let args = json!({
        "accountId": "acc1",
        "destroy": ["cal1"]
        // onDestroyRemoveEvents defaults to false
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
        .await
        .expect("/set must not return top-level error");

    assert!(
        resp["notDestroyed"].is_object(),
        "notDestroyed must be present: {resp}"
    );
    assert_eq!(
        resp["notDestroyed"]["cal1"]["type"], "calendarHasEvent",
        "draft §10.8.1 registers the singular form `calendarHasEvent`: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Calendar/set create rejects client-supplied id
// Oracle: RFC 8620 §5.3 — "The id property MUST NOT be set in the
// create object." Servers MUST respond with invalidProperties citing
// `properties: ["id"]`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_set_create_with_client_id_invalid_properties() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": { "id": "client-chose-this", "name": "My Calendar" }
        }
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
        .await
        .expect("/set must not return top-level error");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "must reject client-supplied id: {resp}"
    );
    assert_eq!(
        resp["notCreated"]["c1"]["properties"][0], "id",
        "must cite 'id' in properties: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Calendar/set on unknown accountId → method-level accountNotFound
// Oracle: RFC 8620 §3.6.2 — accountId not recognised must produce
// method-level `accountNotFound`, not a silent no-op envelope.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_set_unknown_account_returns_account_not_found() {
    let backend = MemoryBackend::new(); // no accounts registered

    let args = json!({
        "accountId": "nobody",
        "create": { "c1": { "name": "x" } }
    });

    let err = handle_calendar_set(&backend, &(), args)
        .await
        .expect_err("unknown accountId must yield a method-level JmapError");

    let err_str = format!("{err:?}");
    assert!(
        err_str.contains("accountNotFound") || err_str.contains("AccountNotFound"),
        "method-level error must be accountNotFound: {err_str}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: CalendarEvent/get on empty store returns empty list
// Oracle: RFC 8620 §5.1.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_event_get_empty_account_returns_empty_list() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "ids": null
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_event_get(&backend, &(), args)
        .await
        .expect("/get must not return top-level error");

    assert!(
        resp["list"].as_array().unwrap().is_empty(),
        "list must be empty: {resp}"
    );
    assert!(
        resp["notFound"].is_array(),
        "notFound must be an array per RFC 8620 §5.1: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 9: CalendarEvent/query against an empty store returns ids=[] total=0
// Oracle: RFC 8620 §5.5.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_event_query_empty_store_returns_no_ids() {
    let backend = MemoryBackend::new().with_account("acc1");

    // `calculateTotal: true` so `total` is present in the response per
    // RFC 8620 §5.5 (otherwise the server MAY omit `total`).
    let args = json!({
        "accountId": "acc1",
        "calculateTotal": true
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_event_query(&backend, &(), args)
        .await
        .expect("/query must not return top-level error");

    assert!(
        resp["ids"].as_array().unwrap().is_empty(),
        "ids must be an empty array: {resp}"
    );
    assert_eq!(
        resp["total"], 0,
        "total must be 0 when calculateTotal=true: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 10: CalendarEvent/changes on an empty store
// Oracle: RFC 8620 §5.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_event_changes_empty_store_empty_result() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({
        "accountId": "acc1",
        "sinceState": "0"
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_event_changes(&backend, &(), args)
        .await
        .expect("/changes must not return top-level error");

    assert!(resp["created"].as_array().unwrap().is_empty());
    assert!(resp["updated"].as_array().unwrap().is_empty());
    assert!(resp["destroyed"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Test 11: ParticipantIdentity/get and /set baseline
// Oracle: RFC 8620 §5.1, §5.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn participant_identity_get_empty_and_set_create_rejects_id() {
    let backend = MemoryBackend::new().with_account("acc1");

    let args = json!({ "accountId": "acc1", "ids": null });
    let HandlerOutput { primary: resp, .. } = handle_participant_identity_get(&backend, &(), args)
        .await
        .expect("/get must not return top-level error");
    assert!(resp["list"].as_array().unwrap().is_empty());
    assert!(resp["notFound"].is_array());

    // RFC 8620 §5.3 — client-supplied id on create must be invalidProperties.
    let args = json!({
        "accountId": "acc1",
        "create": {
            "p1": { "id": "client-chose-id", "name": "Alice", "sendFrom": "alice@example.com" }
        }
    });
    let HandlerOutput { primary: resp, .. } = handle_participant_identity_set(&backend, &(), args)
        .await
        .expect("/set must not return top-level error");
    assert_eq!(
        resp["notCreated"]["p1"]["type"], "invalidProperties",
        "client-supplied id must be rejected: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 12: state bumps monotonically across create + destroy
// Oracle: RFC 8620 §5.2 — each mutating /set call advances the state
// counter; oldState == previous newState.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_set_state_bumps_after_each_mutation() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "Calendar", "cal1", calendar_fixture("cal1", "Work"));

    let args = json!({ "accountId": "acc1", "destroy": ["cal1"] });
    let HandlerOutput { primary: resp, .. } = handle_calendar_set(&backend, &(), args)
        .await
        .expect("first /set must succeed");
    let state_after_destroy = resp["newState"].clone();
    assert_ne!(resp["oldState"], state_after_destroy, "state must bump");

    // A subsequent /changes call from sinceState=oldState should report
    // exactly one destroyed id.
    let args = json!({
        "accountId": "acc1",
        "sinceState": resp["oldState"]
    });
    let HandlerOutput { primary: resp2, .. } = handle_calendar_changes(&backend, &(), args)
        .await
        .expect("/changes must succeed");

    let destroyed = resp2["destroyed"]
        .as_array()
        .expect("destroyed must be array");
    assert_eq!(destroyed.len(), 1, "exactly one destroy event: {resp2}");
    assert_eq!(destroyed[0], "cal1");
    assert_eq!(resp2["newState"], state_after_destroy);
}

// ---------------------------------------------------------------------------
// Test 13: CalendarEvent/set create rejects client-supplied id
// Oracle: RFC 8620 §5.3.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_event_set_create_with_client_id_invalid_properties() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object("acc1", "Calendar", "cal1", calendar_fixture("cal1", "Work"));

    let args = json!({
        "accountId": "acc1",
        "create": {
            "c1": {
                "id": "client-id",
                "@type": "Event",
                "uid": "u1",
                "title": "Test",
                "calendarIds": { "cal1": true }
            }
        }
    });
    let HandlerOutput { primary: resp, .. } = handle_calendar_event_set(&backend, &(), args)
        .await
        .expect("/set must not return top-level error");

    assert_eq!(
        resp["notCreated"]["c1"]["type"], "invalidProperties",
        "client-supplied id must be rejected: {resp}"
    );
}

// ---------------------------------------------------------------------------
// Test 12: CalendarEvent/query honours anchor / anchorOffset
// Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt):
// "If found, the "anchorOffset" is then added to its index.  If the
// resulting index is now negative, it is clamped to 0. ... If the anchor
// is not found, the call is rejected with an "anchorNotFound" error.  If an
// "anchor" is specified, any position argument supplied by the client MUST
// be ignored."  Bead: JMAP-yt1w.26.
// ---------------------------------------------------------------------------

/// `acc1` holding CalendarEvents `e1`..`e4`; `MemoryBackend` returns
/// query ids in lexical id order, so `e2` is at index 1.
fn backend_with_four_events() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account("acc1");
    for id in ["e1", "e2", "e3", "e4"] {
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            id,
            json!({ "id": id, "calendarIds": { "cal1": true } }),
        );
    }
    backend
}

#[tokio::test]
async fn calendar_event_query_anchor_selects_page_and_ignores_position() {
    let backend = backend_with_four_events();
    let args = json!({
        "accountId": "acc1",
        "position": 0,
        "anchor": "e2",
        "anchorOffset": 1,
        "limit": 2
    });

    let HandlerOutput { primary: resp, .. } = handle_calendar_event_query(&backend, &(), args)
        .await
        .expect("/query must not return top-level error");

    assert_eq!(resp["ids"], json!(["e3", "e4"]), "anchor e2 + 1: {resp}");
    assert_eq!(resp["position"], 2, "{resp}");
}

#[tokio::test]
async fn calendar_event_query_negative_anchor_offset_clamps_to_zero() {
    let backend = backend_with_four_events();
    let args = json!({ "accountId": "acc1", "anchor": "e2", "anchorOffset": -5 });

    let HandlerOutput { primary: resp, .. } = handle_calendar_event_query(&backend, &(), args)
        .await
        .expect("/query must not return top-level error");

    assert_eq!(resp["ids"], json!(["e1", "e2", "e3", "e4"]), "{resp}");
    assert_eq!(resp["position"], 0, "{resp}");
}

#[tokio::test]
async fn calendar_event_query_unknown_anchor_is_anchor_not_found() {
    let backend = backend_with_four_events();
    let args = json!({ "accountId": "acc1", "anchor": "missing" });

    let err = handle_calendar_event_query(&backend, &(), args)
        .await
        .expect_err("an anchor absent from the results must be rejected");

    assert_eq!(err.error_type.to_string(), "anchorNotFound");
}

/// RFC 8620 §5.5: "anchor: "Id" (default: null)"; any other JSON type is
/// `invalidArguments`, which RFC 8620 §3.6.2 defines as "One of the
/// arguments is of the wrong type or otherwise invalid".
#[tokio::test]
async fn calendar_event_query_non_id_anchor_is_invalid_arguments() {
    let backend = backend_with_four_events();
    let args = json!({ "accountId": "acc1", "anchor": 7 });

    let err = handle_calendar_event_query(&backend, &(), args)
        .await
        .expect_err("a non-Id anchor must be rejected");

    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

/// draft-ietf-jmap-calendars-33 L3224 (IANA registration):
/// "Capability Name:  urn:ietf:params:jmap:calendars". The constant is
/// declared in jmap-calendars-types and re-exported at this crate's root.
#[test]
fn capability_uri_calendars_matches_draft() {
    assert_eq!(
        jmap_calendars_server::JMAP_CALENDARS_URI,
        "urn:ietf:params:jmap:calendars"
    );
}

// ---------------------------------------------------------------------------
// CalendarEvent/query evaluates an RFC 8620 §5.5 filter tree
// Oracle: RFC 8620 §5.5 types `filter` as "FilterOperator|FilterCondition|null";
// "AND": all of the conditions must match, "OR": at least one must match,
// "NOT": none of the conditions must match. draft-ietf-jmap-calendars-33
// §5.11 `inCalendar`: the event must be in this calendar. Expected ids are
// read off the hand-written fixture: e1 is in cal1, e2 in cal2, e3 in both.
// Bead: JMAP-skk9.2.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_event_query_evaluates_operator_tree() {
    let backend = MemoryBackend::new().with_account("acc1");
    for (id, calendar_ids) in [
        ("e1", json!({ "cal1": true })),
        ("e2", json!({ "cal2": true })),
        ("e3", json!({ "cal1": true, "cal2": true })),
    ] {
        backend.seed_object(
            "acc1",
            "CalendarEvent",
            id,
            json!({ "id": id, "calendarIds": calendar_ids }),
        );
    }
    let cal = |id: &str| json!({ "inCalendar": id });
    for (filter, want) in [
        (
            json!({ "operator": "OR", "conditions": [cal("cal1"), cal("cal2")] }),
            json!(["e1", "e2", "e3"]),
        ),
        (
            json!({ "operator": "AND", "conditions": [
                cal("cal1"),
                { "operator": "NOT", "conditions": [cal("cal2")] },
            ] }),
            json!(["e1"]),
        ),
        (
            json!({ "operator": "NOT", "conditions": [cal("cal1")] }),
            json!(["e2"]),
        ),
    ] {
        let args = json!({ "accountId": "acc1", "filter": filter });
        let HandlerOutput { primary: resp, .. } = handle_calendar_event_query(&backend, &(), args)
            .await
            .expect("/query must not return top-level error");
        assert_eq!(resp["ids"], want, "filter {filter}: {resp}");
    }
}

// ---------------------------------------------------------------------------
// The reference backend rejects a filter it cannot evaluate
// Oracle: RFC 8620 §5.5 "A server MUST reject the call with an
// unsupportedFilter error if it cannot process the given filter." The
// reference MemoryBackend evaluates only `inCalendar` on CalendarEvent and
// nothing on CalendarEventNotification, so each hand-written filter below
// names something it cannot evaluate. Counting the unevaluated condition as
// a match would make the NOT case return an empty list instead.
// Bead: JMAP-skk9.19.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn calendar_query_rejects_filter_backend_cannot_evaluate() {
    let backend = MemoryBackend::new().with_account("acc1");
    backend.seed_object(
        "acc1",
        "CalendarEvent",
        "e1",
        json!({ "id": "e1", "calendarIds": { "cal1": true } }),
    );
    for filter in [
        json!({ "operator": "NOT", "conditions": [{ "after": "2026-01-01T00:00:00" }] }),
        json!({ "operator": "AND", "conditions": [
            { "inCalendar": "cal1" },
            { "title": "standup" },
        ] }),
        json!({ "inCalendar": "cal1", "text": "standup" }),
    ] {
        let args = json!({ "accountId": "acc1", "filter": filter });
        let err = handle_calendar_event_query(&backend, &(), args)
            .await
            .expect_err("an unevaluable CalendarEvent filter must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedFilter", "{filter}");
    }

    for filter in [
        json!({ "operator": "NOT", "conditions": [{ "calendarEventIds": ["e1"] }] }),
        json!({ "type": "created" }),
    ] {
        let args = json!({ "accountId": "acc1", "filter": filter });
        let err = handle_calendar_event_notification_query(&backend, &(), args)
            .await
            .expect_err("an unevaluable CalendarEventNotification filter must be rejected");
        assert_eq!(err.error_type.to_string(), "unsupportedFilter", "{filter}");
    }
}
