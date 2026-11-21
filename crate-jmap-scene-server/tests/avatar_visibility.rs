//! Avatar Visibility (draft-atwood-jmap-scene-00 at jmap-chat-spec 93f2258,
//! §Avatar Visibility; draft-atwood-jmap-scene-wss-00 §Visibility
//! Filtering), through `SceneBackend::avatar_view` and the
//! `MemoryBackend` rules `hide_avatar`, `withhold_avatar` and
//! `reveal_avatar`. Each test quotes the sentence it checks; fixtures are
//! hand-written JSON.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    AvatarRedaction, SceneEphemeralSubscription, handle_scene_avatar_changes,
    handle_scene_avatar_get, handle_scene_avatar_query, handle_scene_avatar_set,
    handle_scene_region_get, should_deliver,
};
use jmap_scene_types::{SceneEvent, SceneStreamEnable};
use jmap_types::Id;
use serde_json::{Value, json};

/// A public region owned by [`ALICE`] whose spawn point is `[0, 0, 5]`.
const R1: &str = "01J5ABC0000000000000000001";
const CAROL: &str = "user:carol@example.com";
const DAVE: &str = "user:dave@example.com";
const BOB_AV: &str = "01J5AVT0000000000000000002";
const CAROL_AV: &str = "01J5AVT0000000000000000003";
const DAVE_AV: &str = "01J5AVT0000000000000000004";

fn avatar(id: &str, user: &str, position: Value) -> Value {
    json!({
        "id": id,
        "regionId": R1,
        "userId": user,
        "displayName": user,
        "position": position,
        "orientation": [0, 0, 0, 1],
        "visualRef": null,
        "visualType": null,
        "joinedAt": "2026-06-06T10:00:00Z",
        "leftAt": null,
        "worldState": {"emote": "idle", "loadout": "awp"},
        "regionState": {"team": "red", "money": 800}
    })
}

/// [`BOB`] at `[10, 0, 0]`, [`CAROL`] at `[-10, 0, 0]` and [`DAVE`] at
/// `[0, 0, -15]` in [`R1`]; the account advertises
/// `supportsAvatarRegionState`.
fn scene() -> MemoryBackend {
    let mut cap = capability();
    cap["supportsAvatarRegionState"] = json!(true);
    let backend = backend_with(cap);
    let mut region = stored_region(R1, "Arena", "2026-06-01T10:00:00Z");
    region["activeAvatarCount"] = json!(3);
    seed_as(&backend, "SceneRegion", region);
    seed_as(
        &backend,
        "SceneAvatar",
        avatar(BOB_AV, BOB, json!([10, 0, 0])),
    );
    seed_as(
        &backend,
        "SceneAvatar",
        avatar(CAROL_AV, CAROL, json!([-10, 0, 0])),
    );
    seed_as(
        &backend,
        "SceneAvatar",
        avatar(DAVE_AV, DAVE, json!([0, 0, -15])),
    );
    backend
}

fn account() -> Id {
    Id::from(ACCOUNT)
}

/// Dave's position (spawn point instead), his `loadout` and his `money`
/// withheld.
fn dave_redaction() -> AvatarRedaction {
    let mut r = AvatarRedaction::default();
    r.position = Some([0.0, 0.0, 5.0]);
    r.world_state_keys = vec!["loadout".to_owned()];
    r.region_state_keys = vec!["money".to_owned()];
    r
}

async fn get(backend: &MemoryBackend, caller: &str, ids: &[&str]) -> Value {
    call!(
        handle_scene_avatar_get,
        backend,
        caller,
        json!({ "ids": ids })
    )
}

async fn query(backend: &MemoryBackend, caller: &str, filter: Value) -> Vec<String> {
    let mut f = filter;
    f["regionId"] = json!(R1);
    let resp = call!(
        handle_scene_avatar_query,
        backend,
        caller,
        json!({ "filter": f })
    );
    let mut ids: Vec<String> = resp["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids
}

async fn changes(backend: &MemoryBackend, caller: &str, since: &str, max: Option<u64>) -> Value {
    let mut args = json!({ "sinceState": since });
    if let Some(max) = max {
        args["maxChanges"] = json!(max);
    }
    call!(handle_scene_avatar_changes, backend, caller, args)
}

async fn state(backend: &MemoryBackend) -> String {
    get(backend, ALICE, &[]).await["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Without rules every avatar is visible in full: "Defaults keep current
/// behaviour".
#[tokio::test]
async fn default_shows_everything() {
    let backend = scene();
    let resp = get(&backend, CAROL, &[BOB_AV, DAVE_AV]).await;
    assert_eq!(resp["notFound"], json!([]));
    assert_eq!(resp["list"].as_array().unwrap().len(), 2);
    assert_eq!(
        query(&backend, CAROL, json!({})).await,
        [BOB_AV, CAROL_AV, DAVE_AV]
    );
}

/// "A hidden avatar is treated as if it did not exist: `SceneAvatar/get`
/// reports it in `notFound`, `SceneAvatar/query` does not return it, and a
/// `SceneAvatar/set` update of it returns `notFound`." "A caller always
/// sees their own avatar in full", and the owner (Alice) is not hidden
/// from. "`activeAvatarCount` on SceneRegion counts hidden avatars".
#[tokio::test]
async fn hidden_avatar_does_not_exist_for_the_caller() {
    let backend = scene();
    backend.hide_avatar(&account(), CAROL, BOB_AV);

    let resp = get(&backend, CAROL, &[BOB_AV, DAVE_AV]).await;
    assert_eq!(resp["notFound"], json!([BOB_AV]));
    assert_eq!(resp["list"].as_array().unwrap().len(), 1);
    assert_eq!(query(&backend, CAROL, json!({})).await, [CAROL_AV, DAVE_AV]);

    let resp = call!(
        ids handle_scene_avatar_set,
        &backend,
        CAROL,
        json!({ "update": { (BOB_AV): { "displayName": "x" } } })
    );
    assert_eq!(resp["notUpdated"][BOB_AV], json!({"type": "notFound"}));

    assert_eq!(get(&backend, BOB, &[BOB_AV]).await["notFound"], json!([]));
    assert_eq!(get(&backend, ALICE, &[BOB_AV]).await["notFound"], json!([]));
    assert_eq!(get(&backend, DAVE, &[BOB_AV]).await["notFound"], json!([]));
    let region = call!(
        handle_scene_region_get,
        &backend,
        CAROL,
        json!({ "ids": [R1] })
    );
    assert_eq!(region["list"][0]["activeAvatarCount"], 3);
}

/// "For `position` and `orientation`, the server returns a value the
/// deployment chooses ... such as the region's `spawnPosition`"; "For
/// `worldState` and `regionState`, the server leaves out the withheld
/// top-level keys."
#[tokio::test]
async fn withheld_properties_are_replaced_or_left_out() {
    let backend = scene();
    backend.withhold_avatar(&account(), CAROL, DAVE_AV, dave_redaction());
    let dave = get(&backend, CAROL, &[DAVE_AV]).await["list"][0].clone();
    assert_eq!(dave["position"], json!([0.0, 0.0, 5.0]));
    assert_eq!(dave["orientation"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(dave["worldState"], json!({"emote": "idle"}));
    assert_eq!(dave["regionState"], json!({"team": "red"}));

    // Dave himself and the region owner see it all.
    for caller in [DAVE, ALICE] {
        let dave = get(&backend, caller, &[DAVE_AV]).await["list"][0].clone();
        assert_eq!(dave["position"], json!([0.0, 0.0, -15.0]), "{caller}");
        assert_eq!(dave["regionState"]["money"], 800, "{caller}");
    }
}

/// "`SceneAvatar/query` evaluates its filters and sorts against the values
/// returned to the caller, so a spatial query cannot locate an avatar
/// whose position is withheld."
#[tokio::test]
async fn spatial_filters_see_only_returned_positions() {
    let backend = scene();
    let at_dave = json!({"withinRadius": {"center": [0, 0, -15], "radius": 1}});
    let at_spawn = json!({"withinRadius": {"center": [0, 0, 5], "radius": 1}});
    assert_eq!(query(&backend, CAROL, at_dave.clone()).await, [DAVE_AV]);
    backend.withhold_avatar(&account(), CAROL, DAVE_AV, dave_redaction());
    assert!(query(&backend, CAROL, at_dave.clone()).await.is_empty());
    assert_eq!(query(&backend, CAROL, at_spawn).await, [DAVE_AV]);
    assert_eq!(query(&backend, BOB, at_dave).await, [DAVE_AV]);
}

/// "`SceneAvatar/changes` lists an avatar in `created` when it becomes
/// visible to the caller, in `destroyed` when it becomes hidden, and in
/// `updated` only when a value returned to the caller changes"; "The state
/// string remains one per account, so a `/changes` call after a change the
/// caller cannot see returns a new state with no ids"; "Whenever the
/// caller's view changes, the server advances the SceneAvatar state
/// string".
#[tokio::test]
async fn changes_follow_the_callers_view() {
    let backend = scene();
    let s0 = state(&backend).await;
    backend.hide_avatar(&account(), CAROL, BOB_AV);
    let s1 = state(&backend).await;
    assert_ne!(s0, s1);

    let c = changes(&backend, CAROL, &s0, None).await;
    assert_eq!(c["destroyed"], json!([BOB_AV]));
    assert_eq!(c["created"], json!([]));
    assert_eq!(c["newState"], json!(s1));
    // Bob's own view did not change: a new state with no ids.
    let c = changes(&backend, BOB, &s0, None).await;
    assert_eq!(
        (&c["created"], &c["updated"], &c["destroyed"]),
        (&json!([]), &json!([]), &json!([]))
    );
    assert_eq!(c["newState"], json!(s1));

    // Bob moves: hidden from Carol, so she sees a new state and no ids.
    let moved = call!(
        ids handle_scene_avatar_set,
        &backend,
        BOB,
        json!({ "update": { (BOB_AV): { "position": [11, 0, 0] } } })
    );
    assert!(moved["notUpdated"].is_null(), "{moved}");
    let s2 = state(&backend).await;
    let c = changes(&backend, CAROL, &s1, None).await;
    assert_eq!(c["updated"], json!([]));
    assert_eq!(c["newState"], json!(s2));
    assert_eq!(
        changes(&backend, DAVE, &s1, None).await["updated"],
        json!([BOB_AV])
    );

    // Dave's position is withheld: his moving is no change for Carol, a
    // change to a key she sees is.
    backend.withhold_avatar(&account(), CAROL, DAVE_AV, dave_redaction());
    let s3 = state(&backend).await;
    assert_eq!(
        changes(&backend, CAROL, &s2, None).await["updated"],
        json!([DAVE_AV])
    );
    call!(
        ids handle_scene_avatar_set,
        &backend,
        DAVE,
        json!({ "update": { (DAVE_AV): { "position": [0, 0, -20] } } })
    );
    let s4 = state(&backend).await;
    assert_eq!(
        changes(&backend, CAROL, &s3, None).await["updated"],
        json!([])
    );
    call!(
        ids handle_scene_avatar_set,
        &backend,
        DAVE,
        json!({ "update": { (DAVE_AV): { "worldState/emote": "wave" } } })
    );
    assert_eq!(
        changes(&backend, CAROL, &s4, None).await["updated"],
        json!([DAVE_AV])
    );

    // Revealed again: `created` for Carol.
    let s5 = state(&backend).await;
    backend.reveal_avatar(&account(), CAROL, BOB_AV);
    assert_eq!(
        changes(&backend, CAROL, &s5, None).await["created"],
        json!([BOB_AV])
    );
}

/// RFC 8620 §5.2 `maxChanges` over the per-caller view: an intermediate
/// state when the changes do not fit, `cannotCalculateChanges` for a state
/// from before the first rule.
#[tokio::test]
async fn changes_paginate_through_intermediate_states() {
    let backend = scene();
    let s0 = state(&backend).await;
    backend.hide_avatar(&account(), CAROL, BOB_AV);
    let s1 = state(&backend).await;
    backend.hide_avatar(&account(), CAROL, DAVE_AV);
    let c = changes(&backend, CAROL, &s0, Some(1)).await;
    assert_eq!(c["destroyed"], json!([BOB_AV]));
    assert_eq!(c["hasMoreChanges"], json!(true));
    assert_eq!(c["newState"], json!(s1));
    let c = changes(&backend, CAROL, &s1, Some(1)).await;
    assert_eq!(c["destroyed"], json!([DAVE_AV]));
    assert_eq!(c["hasMoreChanges"], json!(false));

    let fresh = scene();
    call!(
        ids handle_scene_avatar_set,
        &fresh,
        BOB,
        json!({ "update": { (BOB_AV): { "displayName": "Bobby" } } })
    );
    fresh.hide_avatar(&account(), CAROL, DAVE_AV);
    let err = handle_scene_avatar_changes(
        &fresh,
        &Id::from(CAROL),
        json!({ "accountId": ACCOUNT, "sinceState": "0" }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.as_str(), "cannotCalculateChanges");
}

/// wss §Visibility Filtering: "a `SceneAvatarEvent` about an avatar hidden
/// from the recipient ... is not delivered to that recipient."
#[tokio::test]
async fn avatar_events_about_hidden_avatars_are_not_delivered() {
    let backend = scene();
    backend.hide_avatar(&account(), CAROL, BOB_AV);
    let mut sub = SceneEphemeralSubscription::default();
    sub.replace_with(&SceneStreamEnable::default()).unwrap();
    let event = |avatar: &str, user: &str| -> SceneEvent {
        serde_json::from_value(json!({
            "@type": "SceneAvatarEvent",
            "accountId": ACCOUNT,
            "regionId": R1,
            "avatarId": avatar,
            "event": "entered",
            "userId": user,
            "displayName": user
        }))
        .unwrap()
    };
    let delivers = |recipient: &str, e: SceneEvent| {
        let backend = &backend;
        let sub = &sub;
        let recipient = Id::from(recipient);
        async move {
            should_deliver(backend, &recipient, &account(), &account(), sub, &e)
                .await
                .unwrap()
        }
    };
    assert!(!delivers(CAROL, event(BOB_AV, BOB)).await);
    assert!(delivers(CAROL, event(DAVE_AV, DAVE)).await);
    assert!(delivers(DAVE, event(BOB_AV, BOB)).await);
}
