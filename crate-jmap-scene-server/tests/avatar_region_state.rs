//! `SceneAvatar.regionState` (draft-atwood-jmap-scene-00 at jmap-chat-spec
//! 93f2258, §SceneAvatar, §Entering a Region, §SetError Conditions and
//! §Avatar Permissions). Each test quotes the sentence it checks.
//! Fixtures are hand-written JSON.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{handle_scene_avatar_get, handle_scene_avatar_set};
use serde_json::{Value, json};

/// A public region owned by [`BOB`] (the game server).
const REGION: &str = "01J5ABC0000000000000000001";
/// A player who neither owns the region nor is an admin.
const CAROL: &str = "user:carol@example.com";

fn setup(region_state: bool) -> MemoryBackend {
    let mut cap = capability();
    if region_state {
        cap["supportsAvatarRegionState"] = json!(true);
    }
    let backend = backend_with(cap);
    let mut r = stored_region(REGION, "Arena", "2026-06-01T10:00:00Z");
    r["ownerId"] = json!(BOB);
    seed_as(&backend, "SceneRegion", r);
    backend
}

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(ids handle_scene_avatar_set, backend, caller, args)
}

async fn get(backend: &MemoryBackend, caller: &str, id: &str) -> Value {
    let resp = call!(
        handle_scene_avatar_get,
        backend,
        caller,
        json!({ "ids": [id] })
    );
    resp["list"][0].clone()
}

/// `caller` enters [`REGION`] with `extra` properties; returns the result
/// of the create.
async fn enter_with(backend: &MemoryBackend, caller: &str, extra: Value) -> Value {
    let mut create = json!({ "regionId": REGION });
    for (k, v) in extra.as_object().unwrap() {
        create[k] = v.clone();
    }
    set(backend, caller, json!({ "create": { "av0": create } })).await
}

/// `caller` enters [`REGION`]; returns the new avatar's id.
async fn enter(backend: &MemoryBackend, caller: &str) -> String {
    let resp = enter_with(backend, caller, json!({})).await;
    assert!(resp["notCreated"].is_null(), "{resp}");
    resp["created"]["av0"]["id"].as_str().unwrap().to_owned()
}

async fn update(backend: &MemoryBackend, caller: &str, id: &str, patch: Value) -> Value {
    set(backend, caller, json!({ "update": { (id): patch } })).await
}

fn refused<'a>(resp: &'a Value, id: &str) -> &'a Value {
    assert!(resp["updated"].is_null(), "update must fail: {resp}");
    &resp["notUpdated"][id]
}

/// "`regionState` (Object|null): Present only when the account capability
/// `supportsAvatarRegionState` is `true`": without it the property is
/// absent, and a client may not set it on create or update.
#[tokio::test]
async fn absent_and_refused_without_the_capability() {
    let backend = setup(false);
    let id = enter(&backend, CAROL).await;
    let avatar = get(&backend, CAROL, &id).await;
    assert!(avatar.get("regionState").is_none(), "{avatar}");

    let resp = enter_with(&backend, ALICE, json!({"regionState": null})).await;
    assert_eq!(resp["notCreated"]["av0"]["type"], "invalidProperties");
    assert_eq!(
        resp["notCreated"]["av0"]["properties"],
        json!(["regionState"])
    );
    let resp = update(&backend, BOB, &id, json!({"regionState": {"team": "red"}})).await;
    assert_eq!(refused(&resp, &id)["type"], "invalidProperties");
}

/// "Default: `null`": an avatar entered without one carries `null`.
#[tokio::test]
async fn defaults_to_null() {
    let backend = setup(true);
    let resp = enter_with(&backend, CAROL, json!({})).await;
    assert_eq!(resp["created"]["av0"]["regionState"], json!(null));
    let id = resp["created"]["av0"]["id"].as_str().unwrap();
    assert_eq!(get(&backend, CAROL, id).await["regionState"], json!(null));
}

/// Create Errors: "Caller supplies a non-null `regionState` and is neither
/// the region owner nor an administrator" is `forbidden`; a `null` one is
/// the default and accepted. "a caller who is the region owner or has
/// administrative privileges may also supply `regionState`".
#[tokio::test]
async fn create_by_a_player_is_forbidden_by_the_owner_allowed() {
    let backend = setup(true);
    let resp = enter_with(&backend, CAROL, json!({"regionState": {"team": "red"}})).await;
    assert_eq!(resp["notCreated"]["av0"]["type"], "forbidden");
    let resp = enter_with(&backend, CAROL, json!({"regionState": null})).await;
    assert!(resp["notCreated"].is_null(), "{resp}");

    let resp = enter_with(&backend, BOB, json!({"regionState": {"role": "referee"}})).await;
    assert_eq!(
        resp["created"]["av0"]["regionState"],
        json!({"role": "referee"})
    );
    let resp = enter_with(&backend, ADMIN, json!({"regionState": {"role": "caster"}})).await;
    assert!(resp["notCreated"].is_null(), "{resp}");
}

/// "The avatar's user cannot change `regionState` unless that user is
/// also the region owner or an administrator; the server returns
/// `forbidden`." `worldState` stays the user's.
#[tokio::test]
async fn the_player_cannot_change_their_own() {
    let backend = setup(true);
    let id = enter(&backend, CAROL).await;
    let resp = update(
        &backend,
        CAROL,
        &id,
        json!({"regionState": {"health": 999}}),
    )
    .await;
    assert_eq!(refused(&resp, &id)["type"], "forbidden");
    let resp = update(&backend, CAROL, &id, json!({"regionState/health": 999})).await;
    assert_eq!(refused(&resp, &id)["type"], "forbidden");
    let resp = update(
        &backend,
        CAROL,
        &id,
        json!({"worldState": {"emote": "wave"}}),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(get(&backend, CAROL, &id).await["regionState"], json!(null));
}

/// "region owners and administrators MAY also change `regionState` on any
/// SceneAvatar in the region"; "the avatar's user can read all of it".
/// A PatchObject path into it is a change to it.
#[tokio::test]
async fn the_owner_and_admins_change_it_the_player_reads_it() {
    let backend = setup(true);
    let id = enter(&backend, CAROL).await;
    let resp = update(
        &backend,
        BOB,
        &id,
        json!({"regionState": {"team": "blue", "health": 100, "money": 800}}),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let resp = update(&backend, ADMIN, &id, json!({"regionState/health": 75})).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(
        get(&backend, CAROL, &id).await["regionState"],
        json!({"team": "blue", "health": 75, "money": 800})
    );
}

/// "An update by a region owner or administrator to another user's avatar
/// may combine an eject with a change to `regionState`, and patches no
/// other property."
#[tokio::test]
async fn the_owner_may_combine_with_an_eject_and_nothing_else() {
    let backend = setup(true);
    let id = enter(&backend, CAROL).await;
    let resp = update(
        &backend,
        BOB,
        &id,
        json!({"regionState": {"team": "red"}, "worldState": {"x": 1}}),
    )
    .await;
    assert_eq!(refused(&resp, &id)["type"], "forbidden");
    let resp = update(
        &backend,
        BOB,
        &id,
        json!({"regionState": {"team": "red"}, "displayName": "Renamed"}),
    )
    .await;
    assert_eq!(refused(&resp, &id)["type"], "forbidden");

    let resp = update(
        &backend,
        BOB,
        &id,
        json!({"leftAt": "2026-06-06T12:00:00Z", "regionState": {"eliminated": true}}),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let avatar = get(&backend, CAROL, &id).await;
    assert!(!avatar["leftAt"].is_null());
    assert_eq!(avatar["regionState"], json!({"eliminated": true}));
}

/// Another player is neither the owner nor an admin: `forbidden`, as for
/// any other update of another user's avatar.
#[tokio::test]
async fn another_player_cannot_change_it() {
    let backend = setup(true);
    let id = enter(&backend, CAROL).await;
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({"regionState": {"team": "red"}}),
    )
    .await;
    assert_eq!(refused(&resp, &id)["type"], "forbidden");
}

/// "It keeps its value when the user leaves and re-enters."
#[tokio::test]
async fn kept_across_leave_and_reentry() {
    let backend = setup(true);
    let id = enter(&backend, CAROL).await;
    update(&backend, BOB, &id, json!({"regionState": {"score": 3}})).await;
    let resp = update(
        &backend,
        CAROL,
        &id,
        json!({"leftAt": "2026-06-06T12:00:00Z"}),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let resp = update(&backend, CAROL, &id, json!({"leftAt": null})).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(
        get(&backend, CAROL, &id).await["regionState"],
        json!({"score": 3})
    );
}

/// `regionState` is "(Object|null)": any other JSON type is
/// `invalidProperties`.
#[tokio::test]
async fn wrong_type_is_invalid_properties() {
    let backend = setup(true);
    let resp = enter_with(&backend, BOB, json!({"regionState": [1]})).await;
    assert_eq!(resp["notCreated"]["av0"]["type"], "invalidProperties");
    let id = enter(&backend, CAROL).await;
    let resp = update(&backend, BOB, &id, json!({"regionState": "red"})).await;
    assert_eq!(refused(&resp, &id)["type"], "invalidProperties");
}
