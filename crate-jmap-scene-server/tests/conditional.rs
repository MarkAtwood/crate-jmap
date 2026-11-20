//! Conditional Scene Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_scene_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.15; docs/design/conditional.md §5.1, §7).
//!
//! Oracles: conditional-02 §9.1 (a property the caller may not read is
//! `forbidden`, whatever its value) and §4.2; draft-atwood-jmap-scene-00
//! L457 (`invitedUserIds` is returned "only to callers who may update the
//! region; for every other caller, the server returns `[]`"), §Entering a
//! Region (one SceneAvatar record per user per region, `alreadyExists`;
//! `maxAvatarsPerRegion`, `overQuota`) and §Leaving (records are not
//! destroyed).

mod common;

use std::sync::Arc;

use common::{ACCOUNT, ADMIN, ALICE, BOB, backend_with, capability, seed_as, stored_region};
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{register_scene_conditional, register_scene_handlers};
use jmap_server::Dispatcher;
use jmap_types::{JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:scene",
    "urn:ietf:params:jmap:conditional",
];
const REGION: &str = "R1";

/// Region R1, owned by ALICE, under `cap`; `edit` adjusts the stored
/// region.
fn backend(cap: Value, edit: impl FnOnce(&mut Value)) -> Arc<MemoryBackend> {
    let b = backend_with(cap);
    let mut r = stored_region(REGION, "Plaza", "2026-06-06T10:00:00Z");
    edit(&mut r);
    seed_as(&b, "SceneRegion", r);
    Arc::new(b)
}

fn dispatcher(b: &Arc<MemoryBackend>) -> Dispatcher<jmap_types::Id> {
    let mut d = Dispatcher::new();
    register_scene_handlers(&mut d, Arc::clone(b));
    register_scene_conditional(&mut d, Arc::clone(b));
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

/// §9.1: BOB, invited but not allowed to update R1, reads
/// `invitedUserIds` blanked; a precondition on it is `forbidden` even with
/// the true value. ALICE, the owner, has hers compared.
#[tokio::test]
async fn invited_user_ids_precondition_for_a_non_updater_is_forbidden() {
    let b = backend(capability(), |r| {
        r["accessPolicy"] = json!("invite");
        r["invitedUserIds"] = json!([BOB]);
    });
    let d = dispatcher(&b);
    let check = |ids: Value| json!({"ifUnchangedBy": {REGION: {"invitedUserIds": ids}}});

    let (name, r) = call(&d, BOB, "SceneRegion/set", check(json!([BOB]))).await;
    assert_eq!(
        (name.as_str(), &r["type"]),
        ("error", &json!("forbidden")),
        "{r}"
    );

    let (name, r) = call(&d, ALICE, "SceneRegion/set", check(json!([BOB]))).await;
    assert_eq!(name, "SceneRegion/set", "{r}");
    let (name, r) = call(&d, ALICE, "SceneRegion/set", check(json!([]))).await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "stateMismatch", "failed": [REGION]})
        )
    );
}

/// Even an empty list is hidden from a non-updater, so the answer cannot
/// tell them it is empty.
#[tokio::test]
async fn an_empty_invited_list_is_hidden_too() {
    let b = backend(capability(), |_| {});
    let d = dispatcher(&b);
    let (name, r) = call(
        &d,
        BOB,
        "SceneRegion/set",
        json!({"ifUnchangedBy": {REGION: {"invitedUserIds": []}}}),
    )
    .await;
    assert_eq!(
        (name.as_str(), &r["type"]),
        ("error", &json!("forbidden")),
        "{r}"
    );
}

#[tokio::test]
async fn atomic_failure_rolls_back() {
    let b = backend(capability(), |_| {});
    let d = dispatcher(&b);
    let (_, before) = call(&d, ALICE, "SceneRegion/get", json!({"ids": [REGION]})).await;
    let (name, r) = call(
        &d,
        ALICE,
        "SceneRegion/set",
        json!({"atomic": true, "update": {REGION: {"name": "Square"}, "nope": {"name": "x"}}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    let (_, after) = call(&d, ALICE, "SceneRegion/get", json!({"ids": [REGION]})).await;
    assert_eq!(after, before);
}

/// `caller` enters R1; their avatar id.
async fn enter(d: &Dispatcher<jmap_types::Id>, caller: &str) -> String {
    let (_, r) = call(
        d,
        caller,
        "SceneAvatar/set",
        json!({"create": {"a": {"regionId": REGION}}}),
    )
    .await;
    r["created"]["a"]["id"]
        .as_str()
        .expect("entered")
        .to_owned()
}

/// A cap met only by the final state: R1 holds one avatar; ADMIN ejects
/// ALICE and enters in one call. Creates run before updates, so only
/// `atomic` lets the entry see the room the ejection makes.
#[tokio::test]
async fn avatar_cap_checked_on_final_state() {
    let mut cap = capability();
    cap["maxAvatarsPerRegion"] = json!(1);
    let eject_and_enter = |alice: &str, atomic: bool| {
        json!({
            "atomic": atomic,
            "create": {"me": {"regionId": REGION}},
            "update": {alice: {"leftAt": "2026-06-06T11:45:00Z"}}
        })
    };

    let b = backend(cap.clone(), |_| {});
    let d = dispatcher(&b);
    let alice = enter(&d, ALICE).await;
    let (_, r) = call(&d, ADMIN, "SceneAvatar/set", eject_and_enter(&alice, false)).await;
    assert_eq!(r["notCreated"]["me"]["type"], "overQuota", "{r}");

    let b = backend(cap, |_| {});
    let d = dispatcher(&b);
    let alice = enter(&d, ALICE).await;
    let (name, r) = call(&d, ADMIN, "SceneAvatar/set", eject_and_enter(&alice, true)).await;
    assert_eq!(name, "SceneAvatar/set", "{r}");
    assert!(r["created"].get("me").is_some(), "{r}");

    // Over the cap in the final state: rejected at commit.
    let (name, r) = call(
        &d,
        BOB,
        "SceneAvatar/set",
        json!({"atomic": true, "create": {"b": {"regionId": REGION}}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["notCreated"]["b"]["type"], "overQuota", "{r}");
}

/// One record per user per region holds under `atomic` too (records are
/// never destroyed, so the check stays with the create).
#[tokio::test]
async fn second_record_for_a_user_is_rejected_at_commit() {
    let b = backend(capability(), |_| {});
    let d = dispatcher(&b);
    let alice = enter(&d, ALICE).await;
    let (name, r) = call(
        &d,
        ALICE,
        "SceneAvatar/set",
        json!({"atomic": true, "create": {"again": {"regionId": REGION}}}),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notCreated"]["again"]["type"], "alreadyExists", "{r}");
    assert_eq!(r["notCreated"]["again"]["existingId"], json!(alice), "{r}");
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = backend(capability(), |_| {});
    let mut d: Dispatcher<jmap_types::Id> = Dispatcher::new();
    register_scene_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps().len(), 4, "{:?}", d.conditional_gaps());
    register_scene_conditional(&mut d, b);
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());
}
