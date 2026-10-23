//! `VTCCall/query` (draft-atwood-jmap-vtc-00 §VTCCall/query, L721-759) and
//! handler registration. Line numbers cite draft-atwood-jmap-vtc-00.md at
//! jmap-chat-spec 3227c0f.

mod common;

use std::sync::Arc;

use common::*;
use jmap_server::{Dispatcher, Invocation, JmapRequest, State};
use jmap_types::Id;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::register_vtc_handlers;
use serde_json::{Value, json};

/// Three calls with hand-picked timestamps, plus Alice's participant record
/// on C2.
fn seeded() -> MemoryBackend {
    let backend = backend();
    let mut c1 = stored_call("C1", "room", "ended", "2026-06-01T10:00:00Z");
    c1["endedAt"] = json!("2026-06-01T11:00:00Z");
    c1["endReason"] = json!("completed");
    c1["chatId"] = json!("CHAT1");
    let mut c2 = stored_call("C2", "ring", "ended", "2026-06-02T10:00:00Z");
    c2["endedAt"] = json!("2026-06-02T10:05:00Z");
    c2["endReason"] = json!("missed");
    c2["initiatorId"] = json!(ALICE);
    let mut c3 = stored_call("C3", "scheduled", "pending", "2026-06-03T10:00:00Z");
    c3["spaceId"] = json!("SPACE1");
    for c in [c1, c2, c3] {
        seed(&backend, c);
    }
    backend.insert_object_for_test(
        "VTCParticipant",
        &Id::from(ACCOUNT),
        "P1",
        json!({
            "id": "P1",
            "callId": "C2",
            "userId": EVE,
            "displayName": "Eve",
            "role": "participant",
            "joinedAt": "2026-06-02T10:01:00Z",
            "leftAt": "2026-06-02T10:04:00Z",
            "callResponse": "accepted",
            "joinMethod": "webrtc",
            "gatewayData": null,
            "lobbyState": null,
            "mediaState": {"audio": true, "video": false, "screen": false, "raisedHand": false},
            "speakerTimeMs": 0,
            "kickedBy": null,
            "e2eeFingerprint": null,
            "unmuteRequested": false
        }),
    );
    backend
}

async fn ids(backend: &MemoryBackend, args: Value) -> Vec<String> {
    let resp = query(backend, args).await.expect("VTCCall/query");
    serde_json::from_value(resp["ids"].clone()).expect("ids")
}

#[tokio::test]
async fn default_sort_is_created_at_descending() {
    let backend = seeded();
    // L759 "Default sort: `createdAt` descending."
    assert_eq!(ids(&backend, json!({})).await, ["C3", "C2", "C1"]);
    assert_eq!(
        ids(&backend, json!({ "sort": [] })).await,
        ["C3", "C2", "C1"]
    );
}

#[tokio::test]
async fn sorts_by_created_at_and_ended_at() {
    let backend = seeded();
    // L759 "Sort properties: `createdAt`, `endedAt`."; RFC 8620 §5.5
    // `isAscending` "(optional; default: true)".
    assert_eq!(
        ids(&backend, json!({ "sort": [{ "property": "createdAt" }] })).await,
        ["C1", "C2", "C3"]
    );
    let ended = ids(
        &backend,
        json!({ "filter": { "state": "ended" }, "sort": [{ "property": "endedAt", "isAscending": false }] }),
    )
    .await;
    assert_eq!(ended, ["C2", "C1"]);
}

#[tokio::test]
async fn other_sort_property_is_unsupported() {
    let backend = seeded();
    // RFC 8620 §5.5 "unsupportedSort: The sort is syntactically valid, but
    // includes a property the server does not support sorting on"; L759
    // names only `createdAt` and `endedAt`.
    let err = query(&backend, json!({ "sort": [{ "property": "subject" }] }))
        .await
        .expect_err("unsupportedSort");
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

#[tokio::test]
async fn filters_by_state_call_type_and_initiator() {
    let backend = seeded();
    // L725-734: `state`, `callType`, `initiatorId` ("Filter to calls
    // initiated by this userId").
    assert_eq!(
        ids(&backend, json!({ "filter": { "state": "pending" } })).await,
        ["C3"]
    );
    assert_eq!(
        ids(&backend, json!({ "filter": { "callType": "ring" } })).await,
        ["C2"]
    );
    assert_eq!(
        ids(&backend, json!({ "filter": { "initiatorId": BOB } })).await,
        ["C3", "C1"]
    );
}

#[tokio::test]
async fn filter_properties_combine_with_and() {
    let backend = seeded();
    // L757 "All filter properties are combined with logical AND."
    assert_eq!(
        ids(
            &backend,
            json!({ "filter": { "state": "ended", "initiatorId": BOB } })
        )
        .await,
        ["C1"]
    );
    assert!(
        ids(
            &backend,
            json!({ "filter": { "state": "pending", "callType": "room" } })
        )
        .await
        .is_empty()
    );
}

#[tokio::test]
async fn created_bounds_are_at_or_after_and_before() {
    let backend = seeded();
    // L746 `createdAfter`: "Calls created at or after this time."
    assert_eq!(
        ids(
            &backend,
            json!({ "filter": { "createdAfter": "2026-06-02T10:00:00Z" } })
        )
        .await,
        ["C3", "C2"]
    );
    // L749 `createdBefore`: "Calls created before this time."
    assert_eq!(
        ids(
            &backend,
            json!({ "filter": { "createdBefore": "2026-06-02T10:00:00Z" } })
        )
        .await,
        ["C1"]
    );
}

#[tokio::test]
async fn ended_bounds_skip_calls_that_have_not_ended() {
    let backend = seeded();
    // L752 `endedAfter`: "Calls ended at or after this time."; C3 has not
    // ended (L358 `endedAt` "`null` while the call is active or pending").
    assert_eq!(
        ids(
            &backend,
            json!({ "filter": { "endedAfter": "2026-06-02T10:05:00Z" } })
        )
        .await,
        ["C2"]
    );
    // L755 `endedBefore`: "Calls ended before this time."
    assert_eq!(
        ids(
            &backend,
            json!({ "filter": { "endedBefore": "2026-06-02T10:05:00Z" } })
        )
        .await,
        ["C1"]
    );
}

#[tokio::test]
async fn has_participant_includes_former_participants() {
    let backend = seeded();
    // L743 `hasParticipant`: "Filter to calls in which this userId is or
    // was a participant." Eve joined C2 and left it.
    assert_eq!(
        ids(&backend, json!({ "filter": { "hasParticipant": EVE } })).await,
        ["C2"]
    );
    assert!(
        ids(&backend, json!({ "filter": { "hasParticipant": ALICE } }))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn chat_filters_need_the_chat_capability() {
    let backend = seeded();
    // L737 `chatId` and L740 `spaceId`: "Only valid when
    // `urn:ietf:params:jmap:chat` is present; servers MUST return
    // `unsupportedFilter` otherwise."
    for filter in [
        json!({ "chatId": "CHAT1" }),
        json!({ "spaceId": "SPACE1" }),
        json!({ "operator": "NOT", "conditions": [{ "chatId": "CHAT1" }] }),
    ] {
        let err = query(&backend, json!({ "filter": filter }))
            .await
            .expect_err("unsupportedFilter");
        assert_eq!(err.error_type.to_string(), "unsupportedFilter", "{filter}");
    }

    backend.advertise(&Id::from(ACCOUNT), CHAT_URI);
    assert_eq!(
        ids(&backend, json!({ "filter": { "chatId": "CHAT1" } })).await,
        ["C1"]
    );
    assert_eq!(
        ids(&backend, json!({ "filter": { "spaceId": "SPACE1" } })).await,
        ["C3"]
    );
}

#[tokio::test]
async fn unknown_filter_property_is_unsupported() {
    let backend = seeded();
    // RFC 8620 §5.5 "unsupportedFilter: The filter is syntactically valid,
    // but the server cannot process it."; L725-755 define no `subject`
    // filter.
    let err = query(&backend, json!({ "filter": { "subject": "Standup" } }))
        .await
        .expect_err("unsupportedFilter");
    assert_eq!(err.error_type.to_string(), "unsupportedFilter");
}

#[tokio::test]
async fn registers_the_five_vtc_call_methods() {
    let backend = Arc::new(seeded());
    let mut dispatcher: Dispatcher<String> = Dispatcher::new();
    register_vtc_handlers(&mut dispatcher, backend);

    // L523-761: `VTCCall/get`, `/changes`, `/set`, `/query`,
    // `/queryChanges`.
    for (method, args) in [
        (
            "VTCCall/get",
            json!({ "accountId": ACCOUNT, "ids": ["C1"] }),
        ),
        (
            "VTCCall/changes",
            json!({ "accountId": ACCOUNT, "sinceState": "0" }),
        ),
        ("VTCCall/set", json!({ "accountId": ACCOUNT })),
        ("VTCCall/query", json!({ "accountId": ACCOUNT })),
        (
            "VTCCall/queryChanges",
            json!({ "accountId": ACCOUNT, "sinceQueryState": "0" }),
        ),
    ] {
        let req = JmapRequest::new(
            vec![
                "urn:ietf:params:jmap:core".into(),
                "urn:ietf:params:jmap:vtc".into(),
            ],
            vec![Invocation::new(method, args, "c0")],
            None,
        );
        let resp = dispatcher
            .dispatch(req, BOB.to_owned(), State::from("s0"))
            .await;
        let out = &resp.method_responses[0].arguments;
        assert_ne!(out["type"], "unknownMethod", "{method}: {out}");
    }
}

#[tokio::test]
async fn unknown_account_is_account_not_found() {
    let backend = seeded();
    // RFC 8620 §3.6.2 "accountNotFound: The accountId does not correspond
    // to a valid account", checked before the L737 chat-filter rule.
    let caller = BOB.to_owned();
    let err = jmap_vtc_server::handle_vtc_call_query(
        &backend,
        &caller,
        json!({ "accountId": "NOPE", "filter": { "chatId": "CHAT1" } }),
    )
    .await
    .expect_err("accountNotFound");
    assert_eq!(err.error_type.to_string(), "accountNotFound");
    let err = jmap_vtc_server::handle_vtc_call_set(
        &backend,
        &caller,
        json!({ "accountId": "NOPE", "create": { "c0": { "callType": "room", "mediaTypes": ["audio"] } } }),
    )
    .await
    .expect_err("accountNotFound");
    assert_eq!(err.error_type.to_string(), "accountNotFound");
}
