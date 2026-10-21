//! Who may see the avatars of a region bound to an E2EE call.
//!
//! Oracle: draft-atwood-jmap-scene-00 at jmap-chat-spec 6bcd9d8, L2654:
//! "When a SceneRegion's `activeCallId` references a VTCCall with
//! `e2eeEnabled` set to `true`, the server SHOULD NOT expose the region's
//! SceneAvatar records to users who are not participants in the bound
//! VTCCall"; draft-atwood-jmap-vtc-00 at 6bcd9d8, L1644 defines the
//! non-participants as "users who do not have a VTCParticipant record in
//! it". Participant records are hand-written from the vtc field list.

mod common;

use common::*;
use jmap_scene_server::JmapBackend;
use jmap_scene_vtc_server::may_see_region_avatars;
use jmap_types::Id;
use jmap_vtc_types::VtcCall;
use serde_json::{json, Value};

const MEMBER: &str = "user:member@example.com";
const DEPARTED: &str = "user:departed@example.com";
const OUTSIDER: &str = "user:outsider@example.com";

/// Create a room call with `e2eeEnabled` set to `e2ee` and return it as
/// stored.
async fn room_call(backend: &Bound, e2ee: bool) -> VtcCall {
    let resp = vtc_set(
        backend,
        json!({ "create": { "c0": {
            "callType": "room", "mediaTypes": ["audio"], "e2eeEnabled": e2ee
        } } }),
    )
    .await;
    let id = resp["created"]["c0"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create failed: {resp}"));
    let (mut found, _) = backend
        .vtc
        .get_objects::<VtcCall>(
            &OWNER.to_owned(),
            &Id::from(ACCOUNT),
            Some(&[Id::from(id)]),
            None,
        )
        .await
        .expect("VTCCall read");
    found.pop().expect("stored call")
}

/// Store a VTCParticipant `id` of `user` in `call_id`, with `leftAt`.
fn seed_participant(backend: &Bound, id: &str, call_id: &Id, user: &str, left_at: Value) {
    backend.vtc.insert_object_for_test(
        "VTCParticipant",
        &Id::from(ACCOUNT),
        id,
        json!({
            "id": id,
            "callId": call_id,
            "userId": user,
            "displayName": user,
            "role": "participant",
            "joinedAt": "2026-01-01T00:00:00Z",
            "leftAt": left_at,
            "callResponse": null,
            "joinMethod": "webrtc",
            "gatewayData": null,
            "lobbyState": null,
            "mediaState": { "audio": false, "video": false, "screen": false, "raisedHand": false },
            "speakerTimeMs": 0,
            "kickedBy": null,
            "e2eeFingerprint": null,
            "unmuteRequested": false,
            "breakoutCallId": null
        }),
    );
}

async fn sees(backend: &Bound, call: &VtcCall, user: &str) -> bool {
    may_see_region_avatars(
        &backend.vtc,
        &user.to_owned(),
        &Id::from(ACCOUNT),
        call,
        user,
    )
    .await
    .expect("VTCParticipant read")
}

/// A user with a VTCParticipant record in the E2EE call sees the avatars,
/// also after leaving (the record still exists); a user with none, or with
/// a record only in another call, does not.
#[tokio::test]
async fn e2ee_call_shows_avatars_only_to_its_participants() {
    let backend = Bound::new();
    let call = room_call(&backend, true).await;
    let other = room_call(&backend, true).await;
    seed_participant(&backend, "p1", &call.id, MEMBER, json!(null));
    seed_participant(
        &backend,
        "p2",
        &call.id,
        DEPARTED,
        json!("2026-01-01T01:00:00Z"),
    );
    seed_participant(&backend, "p3", &other.id, OUTSIDER, json!(null));

    assert!(sees(&backend, &call, MEMBER).await);
    assert!(sees(&backend, &call, DEPARTED).await);
    assert!(!sees(&backend, &call, OUTSIDER).await);
}

/// The rule is conditioned on "`e2eeEnabled` set to `true`": a call
/// without E2EE hides nothing, even from a user with no record in it.
#[tokio::test]
async fn call_without_e2ee_hides_nothing() {
    let backend = Bound::new();
    let call = room_call(&backend, false).await;
    assert!(sees(&backend, &call, OUTSIDER).await);
}
