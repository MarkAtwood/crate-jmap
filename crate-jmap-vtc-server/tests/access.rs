//! Per-call access control and E2EE rules (draft-atwood-jmap-vtc-00
//! §Access Control, §End-to-End Encrypted Calls). Line numbers and quotes
//! cite draft-atwood-jmap-vtc-00.md at jmap-chat-spec 6bcd9d8; each
//! assertion's oracle is the quoted draft text or a hand-written record.

#[macro_use]
mod common;

use common::*;
use jmap_types::Id;
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{
    handle_vtc_call_get, handle_vtc_call_query, handle_vtc_call_set, handle_vtc_livestream_get,
    handle_vtc_participant_changes, handle_vtc_participant_get, handle_vtc_participant_query,
    handle_vtc_participant_set, handle_vtc_recording_get,
};
use serde_json::{Value, json};

/// A backend under [`media_capability`] holding room call `C1` (active,
/// initiator [`BOB`]) with `e2eeEnabled` set to `e2ee`.
fn room(e2ee: bool) -> MemoryBackend {
    let backend = backend_with(media_capability());
    let mut call = stored_call("C1", "room", "active", "2026-06-05T14:30:00Z");
    call["e2eeEnabled"] = json!(e2ee);
    seed(&backend, call);
    backend
}

/// Join `C1` as `caller` and return the new record's id.
async fn join(backend: &MemoryBackend, caller: &str) -> String {
    let resp = call!(
        handle_vtc_participant_set,
        backend,
        caller,
        json!({ "create": { "p0": { "callId": "C1", "joinMethod": "webrtc" } } }),
    );
    resp["created"]["p0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("join failed: {resp}"))
        .to_owned()
}

/// `true` if `/get` response `resp` lists `id` in `notFound` and not in
/// `list`.
fn hidden(resp: &Value, id: &str) -> bool {
    resp["notFound"] == json!([id]) && resp["list"] == json!([])
}

/// L457: "a moderator MAY change it from `false` to `true` via update ...
/// The server SHOULD reject an update from `true` to `false` with
/// `invalidProperties` naming `e2eeEnabled`, so that participants never
/// have to detect a downgrade."
#[tokio::test]
async fn e2ee_downgrade_is_invalid_properties() {
    let backend = room(true);
    // BOB is the initiator, a moderator ("The call initiator receives
    // `"moderator"` by default").
    let err = update(&backend, BOB, "C1", json!({ "e2eeEnabled": false }))
        .await
        .expect_err("downgrade must fail");
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["e2eeEnabled"]), "{err}");
    assert_eq!(get(&backend, "C1").await["e2eeEnabled"], true);

    // RFC 8620 §5.3: a PatchObject value of `null` means "reset to the
    // default value", and L457 says "Default: `false`", so `null` is the
    // same downgrade.
    let err = update(&backend, BOB, "C1", json!({ "e2eeEnabled": null }))
        .await
        .expect_err("null reset must fail");
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(err["properties"], json!(["e2eeEnabled"]), "{err}");
    assert_eq!(get(&backend, "C1").await["e2eeEnabled"], true);

    // Keeping it `true` is no downgrade.
    update(&backend, BOB, "C1", json!({ "e2eeEnabled": true }))
        .await
        .expect("e2eeEnabled true to true");
}

/// L1671 "Non-participants receive `notFound` for calls they cannot
/// access"; L1660 "Non-members SHOULD receive `notFound` for `VTCCall/get`
/// and `VTCParticipant/get` requests on the call"; L814 "The query SHOULD
/// return only calls the authenticated user has access to".
#[tokio::test]
async fn inaccessible_call_is_not_found_and_not_queried() {
    let backend = room(false);
    let pid = join(&backend, EVE).await;
    backend.deny_call_access(ALICE, "C1");

    let resp = call!(
        handle_vtc_call_get,
        &backend,
        ALICE,
        json!({ "ids": ["C1"] })
    );
    assert!(hidden(&resp, "C1"), "{resp}");
    let resp = call!(handle_vtc_call_get, &backend, ALICE, json!({ "ids": null }));
    assert_eq!(resp["list"], json!([]), "{resp}");
    let resp = call!(handle_vtc_call_query, &backend, ALICE, json!({}));
    assert_eq!(resp["ids"], json!([]), "{resp}");

    let resp = call!(
        handle_vtc_participant_get,
        &backend,
        ALICE,
        json!({ "ids": [pid] })
    );
    assert!(hidden(&resp, &pid), "{resp}");

    // The same reads as a caller with access find both records.
    let resp = call!(handle_vtc_call_query, &backend, BOB, json!({}));
    assert_eq!(resp["ids"], json!(["C1"]), "{resp}");
    let resp = call!(
        handle_vtc_participant_get,
        &backend,
        BOB,
        json!({ "ids": [pid] })
    );
    assert_eq!(resp["list"][0]["id"], json!(pid), "{resp}");
}

/// L926: "The server SHOULD return `notFound` if the `callId` does not
/// exist or the caller does not have access to it." The update and
/// destroy paths of `VTCCall/set` answer as for a missing call (RFC 8620
/// §5.3 `notFound`), so the response does not reveal the call.
#[tokio::test]
async fn set_on_inaccessible_call_is_not_found() {
    let backend = room(false);
    backend.deny_call_access(ALICE, "C1");

    let resp = call!(
        handle_vtc_participant_set,
        &backend,
        ALICE,
        json!({ "create": { "p0": { "callId": "C1", "joinMethod": "webrtc" } } }),
    );
    assert_eq!(resp["notCreated"]["p0"]["type"], "notFound", "{resp}");

    let resp = call!(
        handle_vtc_call_set,
        &backend,
        ALICE,
        json!({ "update": { "C1": { "subject": "x" } }, "destroy": ["C1"] }),
    );
    assert_eq!(resp["notUpdated"]["C1"]["type"], "notFound", "{resp}");
    assert_eq!(resp["notDestroyed"]["C1"]["type"], "notFound", "{resp}");
}

/// L1108 and L1206: "The server SHOULD return `notFound` for recording
/// ids belonging to calls the authenticated user has no access to", and
/// likewise for livestream ids.
#[tokio::test]
async fn media_of_inaccessible_call_is_not_found() {
    let backend = room(false);
    let account = Id::from(ACCOUNT);
    backend.insert_object_for_test(
        "VTCRecording",
        &account,
        "R1",
        stored_recording("R1", "recording", "2026-06-05T14:31:00Z"),
    );
    backend.insert_object_for_test(
        "VTCLivestream",
        &account,
        "L1",
        stored_livestream("L1", "live", json!("2026-06-05T14:31:00Z")),
    );
    backend.deny_call_access(ALICE, "C1");

    let resp = call!(
        handle_vtc_recording_get,
        &backend,
        ALICE,
        json!({ "ids": ["R1"] })
    );
    assert!(hidden(&resp, "R1"), "{resp}");
    let resp = call!(
        handle_vtc_livestream_get,
        &backend,
        ALICE,
        json!({ "ids": ["L1"] })
    );
    assert!(hidden(&resp, "L1"), "{resp}");

    let resp = call!(
        handle_vtc_recording_get,
        &backend,
        BOB,
        json!({ "ids": ["R1"] })
    );
    assert_eq!(resp["list"][0]["id"], "R1", "{resp}");
    let resp = call!(
        handle_vtc_livestream_get,
        &backend,
        BOB,
        json!({ "ids": ["L1"] })
    );
    assert_eq!(resp["list"][0]["id"], "L1", "{resp}");
}

/// L1644: "The server SHOULD NOT expose VTCParticipant records ... for the
/// call to users who do not have a VTCParticipant record in it, even where
/// {{access-control}} would otherwise make the call visible to them ...
/// For such users the server SHOULD return `notFound` for
/// `VTCParticipant/get` and exclude the records from
/// `VTCParticipant/query`. `VTCParticipant/changes` keeps reporting
/// changes for the account as a whole".
#[tokio::test]
async fn e2ee_participants_hidden_from_users_without_a_record() {
    let backend = room(true);
    let pid = join(&backend, ALICE).await;

    let resp = call!(
        handle_vtc_participant_get,
        &backend,
        EVE,
        json!({ "ids": [pid] })
    );
    assert!(hidden(&resp, &pid), "{resp}");
    let resp = call!(
        handle_vtc_participant_query,
        &backend,
        EVE,
        json!({ "filter": { "callId": "C1" } })
    );
    assert_eq!(resp["ids"], json!([]), "{resp}");
    let resp = call!(
        handle_vtc_participant_changes,
        &backend,
        EVE,
        json!({ "sinceState": "0" })
    );
    assert_eq!(resp["created"], json!([pid]), "{resp}");

    // The call itself stays visible to EVE; only its participants hide.
    let resp = call!(handle_vtc_call_get, &backend, EVE, json!({ "ids": ["C1"] }));
    assert_eq!(resp["list"][0]["id"], "C1", "{resp}");

    // A user with a record in the call sees it.
    let resp = call!(
        handle_vtc_participant_get,
        &backend,
        ALICE,
        json!({ "ids": [pid] })
    );
    assert_eq!(resp["list"][0]["id"], json!(pid), "{resp}");
    let resp = call!(
        handle_vtc_participant_query,
        &backend,
        ALICE,
        json!({ "filter": { "callId": "C1" } })
    );
    assert_eq!(resp["ids"], json!([pid]), "{resp}");
}

/// The L1644 restriction applies only to calls with `e2eeEnabled: true`
/// (L1619 "This section applies to calls with `e2eeEnabled: true`").
#[tokio::test]
async fn non_e2ee_participants_visible_to_call_viewers() {
    let backend = room(false);
    let pid = join(&backend, ALICE).await;

    let resp = call!(
        handle_vtc_participant_get,
        &backend,
        EVE,
        json!({ "ids": [pid] })
    );
    assert_eq!(resp["list"][0]["id"], json!(pid), "{resp}");
}
