//! Conditional VTC Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_vtc_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.14; docs/design/conditional.md §5.1, §6, §7).
//!
//! Oracles: conditional-02 §9.1 (a property the caller may not read is
//! `forbidden`, whatever its value), §4.2 and §4.3 (an atomic failure
//! changes nothing and sends nothing); draft-atwood-jmap-vtc-00
//! (`streamKey`/`streamUri` are blank for non-moderators; one participant
//! record per user per call, §Reconnecting; L1066 `maxParticipantsPerCall`
//! is `overQuota`; L1137 a moderator kicks by setting `leftAt`).

mod common;

use std::sync::Arc;

use common::{
    ACCOUNT, ALICE, BOB, EVE, backend_with, capability, media_capability, seed, seed_as,
    stored_call, stored_livestream,
};
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, State};
use jmap_vtc_server::memory::MemoryBackend;
use jmap_vtc_server::{register_vtc_conditional, register_vtc_handlers};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:vtc",
    "urn:ietf:params:jmap:conditional",
];

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<jmap_types::Id> {
    let mut d = Dispatcher::new();
    register_vtc_handlers(&mut d, Arc::clone(b));
    register_vtc_conditional(&mut d, Arc::clone(b));
    d
}

async fn call(
    d: &Dispatcher<jmap_types::Id>,
    caller: &str,
    method: &str,
    mut args: Value,
) -> (String, Value) {
    args["accountId"] = json!(ACCOUNT);
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(
        d.dispatch(req, jmap_types::Id::from(caller), State::from("s"))
            .await,
    )
    .unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// A backend holding active room call C1 (initiator BOB, so BOB is its
/// moderator) under `cap`.
fn room(cap: Value) -> Arc<MemoryBackend> {
    let b = backend_with(cap);
    seed(
        &b,
        stored_call("C1", "room", "active", "2026-06-05T14:30:00Z"),
    );
    Arc::new(b)
}

/// §9.1: a non-moderator reads `streamKey` blanked, so a precondition on
/// it is `forbidden` even with the true value; the moderator's is compared.
#[tokio::test]
async fn stream_key_precondition_for_a_non_moderator_is_forbidden() {
    let b = room(media_capability());
    seed_as(
        &b,
        "VTCLivestream",
        stored_livestream("L1", "live", json!("2026-06-05T14:31:00Z")),
    );
    let d = dispatcher(&b);
    let check = |key: &str| json!({"ifUnchangedBy": {"L1": {"streamKey": key}}});

    let (name, r) = call(
        &d,
        ALICE,
        "VTCLivestream/set",
        check("sk_live_abc123def456"),
    )
    .await;
    assert_eq!(
        (name.as_str(), &r["type"]),
        ("error", &json!("forbidden")),
        "{r}"
    );

    let (name, r) = call(&d, BOB, "VTCLivestream/set", check("sk_live_abc123def456")).await;
    assert_eq!(name, "VTCLivestream/set", "{r}");
    let (name, r) = call(&d, BOB, "VTCLivestream/set", check("sk_other")).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["L1"]}))
    );
}

/// §4.2/§4.3: an atomic failure changes nothing and delivers no event: the
/// moderator's unmute request (an `UnmuteRequest` event) rolls back with
/// the failing change.
#[tokio::test]
async fn atomic_failure_emits_no_event() {
    let b = room(capability());
    let d = dispatcher(&b);
    let alice = join(&d).await;
    let (_, before) = call(&d, BOB, "VTCParticipant/get", json!({"ids": [&alice]})).await;

    let (name, r) = call(
        &d,
        BOB,
        "VTCParticipant/set",
        json!({"atomic": true, "update": {&alice: {"unmuteRequested": true}, "nope": {"leftAt": null}}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert!(b.events_for_test().is_empty(), "{:?}", b.events_for_test());
    let (_, after) = call(&d, BOB, "VTCParticipant/get", json!({"ids": [&alice]})).await;
    assert_eq!(after, before);

    let (name, r) = call(
        &d,
        BOB,
        "VTCParticipant/set",
        json!({"atomic": true, "update": {&alice: {"unmuteRequested": true}}}),
    )
    .await;
    assert_eq!(name, "VTCParticipant/set", "{r}");
    assert_eq!(b.events_for_test().len(), 1, "delivered on commit");
}

/// A cap met only by the final state: with room for one, the moderator
/// kicks ALICE and joins in one call. Creates run before updates, so only
/// `atomic` lets the join see the room the kick makes.
#[tokio::test]
async fn participant_cap_checked_on_final_state() {
    let mut cap = capability();
    cap["maxParticipantsPerCall"] = json!(1);
    let join_and_kick = |alice: &str, atomic: bool| {
        json!({
            "atomic": atomic,
            "create": {"me": {"callId": "C1", "joinMethod": "webrtc"}},
            "update": {alice: {"leftAt": "2026-06-05T15:02:00Z"}}
        })
    };

    let b = room(cap.clone());
    let d = dispatcher(&b);
    let alice = join(&d).await;
    let (_, r) = call(&d, BOB, "VTCParticipant/set", join_and_kick(&alice, false)).await;
    assert_eq!(r["notCreated"]["me"]["type"], "overQuota", "{r}");

    let b = room(cap);
    let d = dispatcher(&b);
    let alice = join(&d).await;
    let (name, r) = call(&d, BOB, "VTCParticipant/set", join_and_kick(&alice, true)).await;
    assert_eq!(name, "VTCParticipant/set", "{r}");
    assert!(r["created"].get("me").is_some(), "{r}");

    // Over the cap in the final state: a third user's atomic join is
    // rejected at commit, with the cap's error.
    let (name, r) = call(
        &d,
        EVE,
        "VTCParticipant/set",
        json!({"atomic": true, "create": {"e": {"callId": "C1", "joinMethod": "webrtc"}}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notCreated"]["e"]["type"], "overQuota", "{r}");
}

/// §Reconnecting, one record per user per call, holds under `atomic` too:
/// the commit refuses a second record for ALICE in C1.
#[tokio::test]
async fn second_record_for_a_user_is_rejected_at_commit() {
    let b = room(capability());
    let d = dispatcher(&b);
    let alice = join(&d).await;
    let (name, r) = call(
        &d,
        ALICE,
        "VTCParticipant/set",
        json!({"atomic": true, "create": {"again": {"callId": "C1", "joinMethod": "webrtc"}}}),
    )
    .await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "atomicFailure", "notCreated": {
                "again": {"type": "alreadyExists", "existingId": alice}
            }})
        )
    );
}

/// ALICE joins C1; her participant id.
async fn join(d: &Dispatcher<jmap_types::Id>) -> String {
    let (_, r) = call(
        d,
        ALICE,
        "VTCParticipant/set",
        json!({"create": {"p": {"callId": "C1", "joinMethod": "webrtc"}}}),
    )
    .await;
    r["created"]["p"]["id"].as_str().expect("joined").to_owned()
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = room(capability());
    let mut d: Dispatcher<jmap_types::Id> = Dispatcher::new();
    register_vtc_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps().len(), 4, "{:?}", d.conditional_gaps());
    register_vtc_conditional(&mut d, b);
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());
}

/// §Livestream States: the per-account cap on livestreams in `"starting"`
/// or `"live"` is judged on the final state under `atomic`: stopping one
/// and starting another in one call stays within a cap of one, and two
/// creates over it fail the whole call with `overQuota`.
#[tokio::test]
async fn livestream_cap_checked_on_final_state() {
    let b = room(media_capability());
    b.set_max_active_livestreams(Some(1));
    seed_as(
        &b,
        "VTCLivestream",
        stored_livestream("L1", "live", json!("2026-06-05T14:41:00Z")),
    );
    let d = dispatcher(&b);
    let start = json!({
        "callId": "C1",
        "streamUri": "rtmp://live.example.com/ingest",
        "streamKey": "sk_live_abc123def456"
    });

    let (_, r) = call(
        &d,
        BOB,
        "VTCLivestream/set",
        json!({"create": {"n": start.clone()}}),
    )
    .await;
    assert_eq!(r["notCreated"]["n"]["type"], "overQuota", "{r}");

    let (name, r) = call(
        &d,
        BOB,
        "VTCLivestream/set",
        json!({"atomic": true, "create": {"n": start.clone()}, "update": {"L1": {"state": "stopped"}}}),
    )
    .await;
    assert_eq!(name, "VTCLivestream/set", "{r}");
    assert!(r["created"].get("n").is_some(), "{r}");

    let (name, r) = call(
        &d,
        BOB,
        "VTCLivestream/set",
        json!({"atomic": true, "create": {"a": start.clone(), "b": start}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    let failed: Vec<&String> = r["notCreated"]
        .as_object()
        .expect("notCreated")
        .keys()
        .collect();
    assert!(!failed.is_empty(), "{r}");
    assert!(
        failed
            .iter()
            .all(|k| r["notCreated"][k.as_str()]["type"] == "overQuota"),
        "{r}"
    );
}

/// jmap-chat-spec 03a4b76 L783: "A create that would give the account
/// more than `maxConcurrentCalls` calls in `"creating"`, `"ringing"`,
/// `"active"` or `"pending"` state: `overQuota`." Under `atomic` the
/// account is judged on the state the call commits (conditional-02 §4.2):
/// ending one call and creating another stays within a limit of one, and
/// two creates over it fail the whole call with `overQuota`.
#[tokio::test]
async fn max_concurrent_calls_checked_on_final_state() {
    let mut cap = capability();
    cap["maxConcurrentCalls"] = json!(1);
    let b = room(cap);
    let d = dispatcher(&b);
    let new_room = json!({"callType": "room", "mediaTypes": ["audio"]});

    let (_, r) = call(
        &d,
        BOB,
        "VTCCall/set",
        json!({"create": {"n": new_room.clone()}}),
    )
    .await;
    assert_eq!(r["notCreated"]["n"]["type"], "overQuota", "{r}");

    // RFC 8620 §5.3 runs creates before updates, so only the final-state
    // check lets this create through.
    let (name, r) = call(
        &d,
        BOB,
        "VTCCall/set",
        json!({"atomic": true, "create": {"n": new_room.clone()}, "update": {"C1": {"state": "ended"}}}),
    )
    .await;
    assert_eq!(name, "VTCCall/set", "{r}");
    assert!(r["created"].get("n").is_some(), "{r}");

    let (name, r) = call(
        &d,
        BOB,
        "VTCCall/set",
        json!({"atomic": true, "create": {"a": new_room.clone(), "b": new_room}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    let failed = r["notCreated"].as_object().expect("notCreated");
    assert!(!failed.is_empty(), "{r}");
    assert!(failed.values().all(|e| e["type"] == "overQuota"), "{r}");
}

/// jmap-chat-spec 03a4b76 L784: "A create with a `parentCallId` whose call
/// already has `maxBreakoutRooms` breakout rooms: `overQuota`", judged on
/// the final state under `atomic`.
#[tokio::test]
async fn max_breakout_rooms_checked_on_final_state() {
    let mut cap = capability();
    cap["maxBreakoutRooms"] = json!(1);
    let b = room(cap);
    let d = dispatcher(&b);
    let breakout = json!({"callType": "room", "mediaTypes": ["audio"], "parentCallId": "C1"});

    let (name, r) = call(
        &d,
        BOB,
        "VTCCall/set",
        json!({"atomic": true, "create": {"a": breakout.clone(), "b": breakout}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    let failed = r["notCreated"].as_object().expect("notCreated");
    assert!(!failed.is_empty(), "{r}");
    assert!(failed.values().all(|e| e["type"] == "overQuota"), "{r}");
    // conditional-02 §4.3: the failed call changed nothing.
    let (_, r) = call(&d, BOB, "VTCCall/get", json!({"ids": ["C1"]})).await;
    assert_eq!(r["list"][0]["breakoutRoomIds"], json!([]), "{r}");
}
