//! `SceneAvatar/set`, `/get` and `/changes` (draft-atwood-jmap-scene-00
//! §SceneAvatar Methods, L1899-2226, and §Step 4 L3244-3289, at
//! jmap-chat-spec fda85cb). Each assertion quotes the draft text it
//! checks; the request bodies are the draft's examples where it has one.
//! Expected `activeAvatarCount` values are counted by hand from each
//! scenario.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    handle_scene_avatar_changes, handle_scene_avatar_get, handle_scene_avatar_set,
    handle_scene_region_changes, handle_scene_region_get,
};
use serde_json::{json, Value};

/// The region of the draft's examples, owned by [`BOB`].
const REGION: &str = "01J5ABC0000000000000000001";
/// A second region, owned by [`BOB`].
const OTHER_REGION: &str = "01J5ABC0000000000000000002";
/// A user who neither owns the regions nor is an admin.
const CAROL: &str = "user:carol@example.com";

/// A region owned by [`BOB`] whose `spawnPosition` is the `[0, 0, 10]`
/// the L1950 response reports for an avatar entering it.
fn region(id: &str, access_policy: &str) -> Value {
    let mut r = stored_region(id, "Gallery", "2026-06-01T10:00:00Z");
    r["ownerId"] = json!(BOB);
    r["spawnPosition"] = json!([0, 0, 10]);
    r["accessPolicy"] = json!(access_policy);
    r
}

fn backend_with_regions(capability: Value) -> MemoryBackend {
    let backend = backend_with(capability);
    for id in [REGION, OTHER_REGION] {
        seed_as(&backend, "SceneRegion", region(id, "public"));
    }
    backend
}

fn setup() -> MemoryBackend {
    backend_with_regions(capability())
}

/// The L1970-1975 create, verbatim.
fn alice_enters() -> Value {
    json!({
        "regionId": "01J5ABC0000000000000000001",
        "displayName": "Alice Chen",
        "visualRef": "blob-avatar-alice-001",
        "visualType": "model/gltf-binary"
    })
}

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_scene_avatar_set, backend, caller, args)
}

async fn get(backend: &MemoryBackend, id: &str) -> Value {
    let resp = call!(
        handle_scene_avatar_get,
        backend,
        ALICE,
        json!({ "ids": [id] })
    );
    resp["list"][0].clone()
}

async fn active_avatar_count(backend: &MemoryBackend, region: &str) -> Value {
    let resp = call!(
        handle_scene_region_get,
        backend,
        ALICE,
        json!({ "ids": [region] })
    );
    resp["list"][0]["activeAvatarCount"].clone()
}

/// `caller` enters `region`; returns the `created` entry, which must exist.
async fn enter(backend: &MemoryBackend, caller: &str, region: &str) -> Value {
    let resp = set(
        backend,
        caller,
        json!({ "create": { "av0": { "regionId": region } } }),
    )
    .await;
    assert!(resp["notCreated"].is_null(), "enter failed: {resp}");
    resp["created"]["av0"].clone()
}

/// The `notCreated` entry for one create by `caller`, which must fail.
async fn create_error(backend: &MemoryBackend, caller: &str, create: Value) -> Value {
    let resp = set(backend, caller, json!({ "create": { "av0": create } })).await;
    assert!(resp["created"].is_null(), "create must fail: {resp}");
    resp["notCreated"]["av0"].clone()
}

async fn update(backend: &MemoryBackend, caller: &str, id: &str, patch: Value) -> Value {
    set(backend, caller, json!({ "update": { (id): patch } })).await
}

/// The `notUpdated` entry of an update by `caller`, which must fail.
async fn update_error(backend: &MemoryBackend, caller: &str, id: &str, patch: Value) -> Value {
    let resp = update(backend, caller, id, patch).await;
    assert!(resp["updated"].is_null(), "update must fail: {resp}");
    resp["notUpdated"][id].clone()
}

fn assert_invalid(err: &Value, expected: &[&str]) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    let mut p: Vec<&str> = err["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .map(|v| v.as_str().expect("string"))
        .collect();
    p.sort_unstable();
    assert_eq!(p, expected, "{err}");
}

/// The L2089 and L2107 leave patch.
fn leave() -> Value {
    json!({ "leftAt": "2026-06-06T11:45:00Z" })
}

#[tokio::test]
async fn spec_enter_example() {
    // L1924 "The server sets `id`, `userId`, `position` (to the region's
    // `spawnPosition`), `orientation` (to the region's `spawnOrientation`),
    // `joinedAt`, and `leftAt` (to `null`)." The L1991-2004 response
    // carries the whole record.
    let backend = setup();
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "av0": alice_enters() } }),
    )
    .await;
    let created = &resp["created"]["av0"];
    assert!(created["id"].is_string(), "{resp}");
    assert_eq!(created["regionId"], REGION);
    assert_eq!(created["userId"], ALICE);
    assert_eq!(created["displayName"], "Alice Chen");
    assert_eq!(created["position"], json!([0.0, 0.0, 10.0]));
    assert_eq!(created["orientation"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(created["visualRef"], "blob-avatar-alice-001");
    assert_eq!(created["visualType"], "model/gltf-binary");
    assert!(created["joinedAt"].is_string());
    assert_eq!(created["leftAt"], Value::Null);
    assert_eq!(created["worldState"], Value::Null);
    assert_ne!(resp["oldState"], resp["newState"]);

    // L293 `activeAvatarCount`: "Number of avatars currently present in
    // the region." One avatar has entered.
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);
    assert_eq!(
        get(&backend, created["id"].as_str().unwrap()).await,
        *created
    );
}

#[tokio::test]
async fn entering_changes_the_region() {
    // RFC 8620 §5.2: `/changes` lists the ids "for records that have been
    // updated"; `activeAvatarCount` is a SceneRegion property (L293).
    let backend = setup();
    enter(&backend, ALICE, REGION).await;
    let resp = call!(
        handle_scene_region_changes,
        &backend,
        ALICE,
        json!({ "sinceState": "0" })
    );
    assert_eq!(resp["updated"], json!([REGION]), "{resp}");
}

#[tokio::test]
async fn missing_display_name_is_server_set() {
    // L1924 "If `displayName` is not supplied by the client, the server
    // sets it"; L559 types it `String` (not nullable).
    let backend = setup();
    let created = enter(&backend, ALICE, REGION).await;
    assert!(created["displayName"].is_string(), "{created}");
}

#[tokio::test]
async fn duplicate_enter_is_already_exists() {
    // L2016 "When the user already has an active SceneAvatar (with
    // `leftAt: null`) in the same region, the server MUST NOT create a
    // duplicate and MUST return an `alreadyExists` SetError ... in
    // `notCreated` with `existingId` set to the id of the current avatar
    // record"; L2197 repeats it.
    let backend = setup();
    let first = enter(&backend, ALICE, REGION).await;
    let err = create_error(&backend, ALICE, alice_enters()).await;
    assert_eq!(err["type"], "alreadyExists", "{err}");
    assert_eq!(err["existingId"], first["id"]);
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);
}

#[tokio::test]
async fn entering_another_region_ejects_the_previous_avatar() {
    // L2199 "When the user has an active avatar in a different region, the
    // server MUST set `leftAt` on the previous avatar (auto-eject) before
    // creating the new one ... This is not a SetError".
    let backend = setup();
    let first = enter(&backend, ALICE, REGION).await;
    let second = enter(&backend, ALICE, OTHER_REGION).await;
    assert_ne!(first["id"], second["id"]);
    let previous = get(&backend, first["id"].as_str().unwrap()).await;
    assert!(previous["leftAt"].is_string(), "{previous}");
    assert_eq!(active_avatar_count(&backend, REGION).await, 0);
    assert_eq!(active_avatar_count(&backend, OTHER_REGION).await, 1);
}

/// The 6bcd9d8 L2366-2372 re-entry patch.
fn reenter() -> Value {
    json!({ "leftAt": null })
}

/// [`ALICE`] enters `region` and leaves; returns her avatar id.
async fn departed(backend: &MemoryBackend, region: &str) -> String {
    let id = enter(backend, ALICE, region).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(update(backend, ALICE, &id, leave()).await["notUpdated"].is_null());
    id
}

#[tokio::test]
async fn reentering_updates_the_departed_record() {
    // jmap-chat-spec 6bcd9d8 L2363 "A user who has left (has a non-null
    // `leftAt`) re-enters the same region by updating their existing
    // SceneAvatar record to set `leftAt` to `null`. The server sets
    // `joinedAt` to the current time, `position` to the region's
    // `spawnPosition`, and `orientation` to the region's
    // `spawnOrientation`. ... `SceneAvatar/changes` reports the record as
    // updated."
    let backend = setup();
    let first = enter(&backend, ALICE, REGION).await;
    let id = first["id"].as_str().unwrap().to_owned();
    // The 6bcd9d8 L2269-2270 teleport moves her off the spawn point.
    let teleport = json!({
        "position": [25.0, 0, -12.3],
        "orientation": [0, 0.924, 0, 0.383]
    });
    assert!(update(&backend, ALICE, &id, teleport).await["notUpdated"].is_null());
    assert!(update(&backend, ALICE, &id, leave()).await["notUpdated"].is_null());
    assert_eq!(active_avatar_count(&backend, REGION).await, 0);

    let state = call!(
        handle_scene_avatar_get,
        &backend,
        ALICE,
        json!({ "ids": [] })
    )["state"]
        .clone();
    let resp = update(&backend, ALICE, &id, reenter()).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    // RFC 8620 §5.3: the echo carries "any properties that changed in a
    // way not explicitly requested by the PatchObject".
    let echo = &resp["updated"][&id];
    assert_eq!(echo["position"], json!([0.0, 0.0, 10.0]), "{resp}");
    assert_eq!(echo["orientation"], json!([0.0, 0.0, 0.0, 1.0]), "{resp}");
    assert!(echo["joinedAt"].is_string(), "{resp}");

    let again = get(&backend, &id).await;
    assert_eq!(again["leftAt"], Value::Null);
    // The region fixture's `spawnPosition` and `spawnOrientation`.
    assert_eq!(again["position"], json!([0.0, 0.0, 10.0]));
    assert_eq!(again["orientation"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(again["joinedAt"], echo["joinedAt"]);
    // "`displayName`, `visualRef`, `visualType`, and `worldState` keep
    // their values".
    assert_eq!(again["displayName"], first["displayName"]);
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);

    let changes = call!(
        handle_scene_avatar_changes,
        &backend,
        ALICE,
        json!({ "sinceState": state })
    );
    assert_eq!(changes["created"], json!([]), "{changes}");
    assert_eq!(changes["updated"], json!([id]), "{changes}");
}

#[tokio::test]
async fn create_for_a_departed_avatar_is_already_exists() {
    // 6bcd9d8 L2232 "When the user already has a SceneAvatar record in
    // the same region, whether present or departed, the server MUST NOT
    // create a second record and MUST return an `alreadyExists` SetError
    // ... with `existingId` set to the id of that record".
    let backend = setup();
    let id = departed(&backend, REGION).await;
    let left = get(&backend, &id).await["leftAt"].clone();
    let err = create_error(&backend, ALICE, alice_enters()).await;
    assert_eq!(err["type"], "alreadyExists", "{err}");
    assert_eq!(err["existingId"], id);
    assert_eq!(get(&backend, &id).await["leftAt"], left);
    assert_eq!(active_avatar_count(&backend, REGION).await, 0);
}

#[tokio::test]
async fn reentry_may_patch_identity_properties() {
    // 6bcd9d8 L2363 "`displayName`, `visualRef`, `visualType`, and
    // `worldState` keep their values unless the same update patches them."
    let backend = setup();
    let id = departed(&backend, REGION).await;
    let patch = json!({
        "leftAt": null,
        "displayName": "Alice C.",
        "visualRef": "blob-avatar-alice-formal",
        "visualType": "model/gltf-binary",
        "worldState": { "animation": "waving" }
    });
    let resp = update(&backend, ALICE, &id, patch).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    let stored = get(&backend, &id).await;
    assert_eq!(stored["leftAt"], Value::Null);
    assert_eq!(stored["displayName"], "Alice C.");
    assert_eq!(stored["visualRef"], "blob-avatar-alice-formal");
    assert_eq!(stored["worldState"], json!({ "animation": "waving" }));
}

#[tokio::test]
async fn reentry_position_is_server_set() {
    // 6bcd9d8 L2363 "The server sets ... `position` to the region's
    // `spawnPosition`, and `orientation` to the region's
    // `spawnOrientation`"; only the identity properties may ride along.
    let backend = setup();
    let id = departed(&backend, REGION).await;
    let left = get(&backend, &id).await["leftAt"].clone();
    let patch = json!({ "leftAt": null, "position": [1, 2, 3] });
    let err = update_error(&backend, ALICE, &id, patch).await;
    assert_invalid(&err, &["position"]);
    assert_eq!(get(&backend, &id).await["leftAt"], left);
}

#[tokio::test]
async fn clearing_left_at_of_a_present_avatar_is_invalid() {
    // 6bcd9d8 L2363: only "A user who has left (has a non-null `leftAt`)"
    // re-enters.
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let err = update_error(&backend, ALICE, &id, reenter()).await;
    assert_invalid(&err, &["leftAt"]);
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);
}

#[tokio::test]
async fn reentry_without_access_is_not_found() {
    // 6bcd9d8 L2363 "a caller who no longer has access to the region
    // receives `notFound`, because the record is then hidden from the
    // caller"; L2595 access by "an active SceneAvatar in it", which a
    // departed avatar is not.
    let backend = setup();
    let invite = "01J5ABC0000000000000000003";
    let mut r = region(invite, "invite");
    r["invitedUserIds"] = json!([ALICE]);
    seed_as(&backend, "SceneRegion", r.clone());
    let id = departed(&backend, invite).await;
    // BOB withdraws the invitation.
    r["invitedUserIds"] = json!([]);
    seed_as(&backend, "SceneRegion", r);
    let err = update_error(&backend, ALICE, &id, reenter()).await;
    assert_eq!(err["type"], "notFound", "{err}");
}

#[tokio::test]
async fn reentry_beyond_max_avatars_is_over_quota() {
    // 6bcd9d8 L2363 "The access checks and errors for entering a region
    // ({{scene-avatar-create-errors}}) apply"; L2406 "`maxAvatarsPerRegion`
    // would be exceeded | The region has reached its concurrent-avatar
    // capacity."
    let mut cap = capability();
    cap["maxAvatarsPerRegion"] = json!(1);
    let backend = backend_with_regions(cap);
    let id = departed(&backend, REGION).await;
    let left = get(&backend, &id).await["leftAt"].clone();
    enter(&backend, BOB, REGION).await;
    let err = update_error(&backend, ALICE, &id, reenter()).await;
    assert_eq!(err["type"], "overQuota", "{err}");
    assert_eq!(get(&backend, &id).await["leftAt"], left);
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);
}

#[tokio::test]
async fn reentry_ejects_the_avatar_in_another_region() {
    // 6bcd9d8 L2232 "When the user has an active SceneAvatar in a
    // different region, the server SHOULD set `leftAt` on the previous
    // avatar ... (a user is present in at most one region at a time)",
    // which L2363 applies to re-entry.
    let backend = setup();
    let id = departed(&backend, REGION).await;
    let other = enter(&backend, ALICE, OTHER_REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(update(&backend, ALICE, &id, reenter()).await["notUpdated"].is_null());
    assert!(get(&backend, &other).await["leftAt"].is_string());
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);
    assert_eq!(active_avatar_count(&backend, OTHER_REGION).await, 0);
}

#[tokio::test]
async fn region_owner_cannot_reenter_another_users_avatar() {
    // 6bcd9d8 L2314: only the eject case lets a region owner update
    // another user's avatar, and an eject sets `leftAt`.
    let backend = setup();
    let id = departed(&backend, REGION).await;
    let err = update_error(&backend, BOB, &id, reenter()).await;
    assert_eq!(err["type"], "forbidden", "{err}");
}

#[tokio::test]
async fn max_avatars_per_region_is_over_quota() {
    // L1959 "The server MUST return `overQuota` when `maxAvatarsPerRegion`
    // would be exceeded"; L177 "Maximum concurrent avatars per region".
    let mut cap = capability();
    cap["maxAvatarsPerRegion"] = json!(1);
    let backend = backend_with_regions(cap);
    let alice = enter(&backend, ALICE, REGION).await;
    let err = create_error(&backend, BOB, json!({ "regionId": REGION })).await;
    assert_eq!(err["type"], "overQuota", "{err}");

    // A departed avatar is not concurrent.
    let id = alice["id"].as_str().unwrap();
    assert!(update(&backend, ALICE, id, leave()).await["notUpdated"].is_null());
    enter(&backend, BOB, REGION).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);
}

#[tokio::test]
async fn missing_region_is_invalid_region_id() {
    // L2172 "The server MUST return `invalidProperties` naming `regionId`
    // when `regionId` does not reference an existing SceneRegion."
    let backend = setup();
    let err = create_error(
        &backend,
        ALICE,
        json!({ "regionId": "01J5ABC0000000000000000099" }),
    )
    .await;
    assert_invalid(&err, &["regionId"]);
}

#[tokio::test]
async fn malformed_creates_are_invalid_properties() {
    // L2149 "`regionId` is missing"; L2150 "`position` contains non-finite
    // values (if explicitly supplied)"; L2151 "`visualRef` is present but
    // `visualType` is absent (or vice versa)".
    let backend = setup();
    let err = create_error(&backend, ALICE, json!({ "displayName": "Alice" })).await;
    assert_invalid(&err, &["regionId"]);
    let err = create_error(
        &backend,
        ALICE,
        json!({ "regionId": REGION, "position": [0, "up", 0] }),
    )
    .await;
    assert_invalid(&err, &["position"]);
    let err = create_error(
        &backend,
        ALICE,
        json!({ "regionId": REGION, "visualRef": "blob-avatar-alice-001" }),
    )
    .await;
    assert_invalid(&err, &["visualRef", "visualType"]);
    // L1924: `id`, `joinedAt` and `leftAt` are server-set.
    let err = create_error(
        &backend,
        ALICE,
        json!({ "regionId": REGION, "joinedAt": "2026-06-05T14:35:00Z" }),
    )
    .await;
    assert_invalid(&err, &["joinedAt"]);
}

#[tokio::test]
async fn supplied_position_is_kept() {
    // L2150 "Position, when supplied by the client (e.g., for
    // teleport-on-entry), must contain finite Numbers. Normally the server
    // sets position from `spawnPosition`."
    let backend = setup();
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "av0": { "regionId": REGION, "position": [3, 0, 4] } } }),
    )
    .await;
    assert_eq!(resp["created"]["av0"]["position"], json!([3.0, 0.0, 4.0]));
}

#[tokio::test]
async fn invite_region_without_access_is_invalid_region_id() {
    // L2172 "The server SHOULD return the same error when the caller has
    // no access to the region under its `accessPolicy` (for example, an
    // `\"invite\"` region whose `invitedUserIds` does not list the
    // caller ...)"; L2386 lists "Region `accessPolicy` is `\"invite\"` and
    // `invitedUserIds` does not list the user" under `invalidProperties`.
    let backend = setup();
    seed_as(
        &backend,
        "SceneRegion",
        region("01J5ABC0000000000000000003", "invite"),
    );
    let invite = json!({ "regionId": "01J5ABC0000000000000000003" });
    let err = create_error(&backend, ALICE, invite.clone()).await;
    assert_invalid(&err, &["regionId"]);

    // "Only users explicitly granted access may enter."
    backend.grant_region_access("01J5ABC0000000000000000003", ALICE);
    let resp = set(&backend, ALICE, json!({ "create": { "av0": invite } })).await;
    assert!(resp["notCreated"].is_null(), "{resp}");
}

#[tokio::test]
async fn banned_create_is_forbidden() {
    // L2393 `forbidden`: "User is banned or ejected with a cooldown period
    // still active ... The user already knows the region exists."
    let backend = setup();
    backend.ban_from_region(REGION, ALICE);
    let err = create_error(&backend, ALICE, json!({ "regionId": REGION })).await;
    assert_eq!(err["type"], "forbidden", "{err}");
}

#[tokio::test]
async fn banned_reentry_is_forbidden() {
    // L2363 "The access checks and errors for entering a region ...
    // apply"; L2393 a ban or active cooldown is `forbidden`.
    let backend = setup();
    let id = departed(&backend, REGION).await;
    backend.ban_from_region(REGION, ALICE);
    let err = update_error(&backend, ALICE, &id, reenter()).await;
    assert_eq!(err["type"], "forbidden", "{err}");
}

#[tokio::test]
async fn creating_another_users_avatar_is_forbidden() {
    // L2160 "Caller is creating an avatar record for a different user |
    // Users MUST only create their own avatar."
    let backend = setup();
    let err = create_error(
        &backend,
        ALICE,
        json!({ "regionId": REGION, "userId": BOB }),
    )
    .await;
    assert_eq!(err["type"], "forbidden", "{err}");
}

/// The L2449 example of a name that impersonates an administrative role.
const RESERVED: &str = "System Administrator";

#[tokio::test]
async fn reserved_display_name_create_is_forbidden() {
    // L2457 "The server SHOULD maintain a list of reserved display names
    // that correspond to system roles or administrative functions and MUST
    // reject `SceneAvatar/set` operations that attempt to use a reserved
    // name with a `SetError` of type `forbidden`."
    let backend = setup();
    backend.reserve_display_name(RESERVED);
    let err = create_error(
        &backend,
        ALICE,
        json!({ "regionId": REGION, "displayName": RESERVED }),
    )
    .await;
    assert_eq!(err["type"], "forbidden", "{err}");

    // The L1970-1975 create uses "Alice Chen", which is not reserved.
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "av0": alice_enters() } }),
    )
    .await;
    assert!(resp["notCreated"].is_null(), "{resp}");
}

#[tokio::test]
async fn reserved_display_name_update_is_forbidden() {
    // L2457 "MUST reject `SceneAvatar/set` operations that attempt to use a
    // reserved name": an update is a `SceneAvatar/set` operation too.
    let backend = setup();
    backend.reserve_display_name(RESERVED);
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let err = update_error(&backend, ALICE, &id, json!({ "displayName": RESERVED })).await;
    assert_eq!(err["type"], "forbidden", "{err}");
    assert_ne!(get(&backend, &id).await["displayName"], RESERVED);
}

#[tokio::test]
async fn no_reserved_list_admits_any_name() {
    // L2457 the list is the server's to "maintain"; with none configured
    // the hook fails open (bd:JMAP-bghk) and the L2449 example name is
    // stored as supplied.
    let backend = setup();
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "av0": { "regionId": REGION, "displayName": RESERVED } } }),
    )
    .await;
    assert_eq!(resp["created"]["av0"]["displayName"], RESERVED, "{resp}");
}

#[tokio::test]
async fn spec_teleport_example() {
    // L2038 "Position and orientation updates via `SceneAvatar/set` are
    // permitted"; the L2049-2052 patch. L144 "If the server does not
    // reject the value, it MUST normalize the quaternion to unit length
    // before storage": `[0, 0.924, 0, 0.383]` has magnitude 1.00023, so
    // unlike the L2070 `null` the echo carries the stored orientation
    // (RFC 8620 §5.3 echoes properties "changed in a way not explicitly
    // requested").
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({ "position": [25.0, 0, -12.3], "orientation": [0, 0.924, 0, 0.383] }),
    )
    .await;
    let n = (0.924f64 * 0.924 + 0.383 * 0.383).sqrt();
    let expected = json!([0.0, 0.924 / n, 0.0, 0.383 / n]);
    assert_eq!(resp["updated"][&id]["orientation"], expected, "{resp}");
    let stored = get(&backend, &id).await;
    assert_eq!(stored["position"], json!([25.0, 0.0, -12.3]));
    assert_eq!(stored["orientation"], expected);
}

#[tokio::test]
async fn spec_state_update_example() {
    // The L2024-2032 patch on the caller's own avatar.
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({
            "visualRef": "blob-avatar-alice-formal",
            "visualType": "model/gltf-binary",
            "worldState": { "animation": "waving" }
        }),
    )
    .await;
    assert_eq!(resp["updated"], json!({ (id.as_str()): null }), "{resp}");
    assert_eq!(
        get(&backend, &id).await["worldState"],
        json!({ "animation": "waving" })
    );
}

#[tokio::test]
async fn updating_another_users_avatar_is_forbidden() {
    // L2036 "The server MUST return `forbidden` if the caller's userId does
    // not match the SceneAvatar record's userId. Users MUST NOT update
    // other users' avatars."
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let err = update_error(
        &backend,
        CAROL,
        &id,
        json!({ "worldState": { "animation": "waving" } }),
    )
    .await;
    assert_eq!(err["type"], "forbidden", "{err}");
    // L2216 the eject exception lets a region owner set `leftAt`, not move
    // the avatar.
    let err = update_error(&backend, BOB, &id, json!({ "position": [1, 0, 1] })).await;
    assert_eq!(err["type"], "forbidden", "{err}");
}

#[tokio::test]
async fn region_id_and_server_set_properties_are_fixed() {
    // L553 `regionId` (String, immutable); L550 `id`, L556 `userId` and
    // L574 `joinedAt` are server-set.
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (patch, property) in [
        (json!({ "regionId": OTHER_REGION }), "regionId"),
        (json!({ "userId": BOB }), "userId"),
        (json!({ "joinedAt": "2026-06-05T14:35:00Z" }), "joinedAt"),
    ] {
        let err = update_error(&backend, ALICE, &id, patch).await;
        assert_invalid(&err, &[property]);
    }
    // L2210 "`position` or `orientation` contains non-finite values".
    let err = update_error(&backend, ALICE, &id, json!({ "orientation": [0, 0, 0, 2] })).await;
    assert_invalid(&err, &["orientation"]);
}

#[tokio::test]
async fn spec_leave_example() {
    // 6bcd9d8 L2302 "A user leaves by calling `SceneAvatar/set` with an
    // update setting `leftAt`. The server records its own current time as
    // `leftAt`; the value the client supplies only signals the intent to
    // leave." The supplied value is in the past, so the stored one differs
    // and RFC 8620 §5.3 echoes it as "changed in a way not explicitly
    // requested by the client".
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = update(&backend, ALICE, &id, leave()).await;
    let stored = get(&backend, &id).await["leftAt"].clone();
    assert!(stored.is_string(), "{stored}");
    assert_ne!(stored, leave()["leftAt"]);
    assert_eq!(
        resp["updated"],
        json!({ (id.as_str()): { "leftAt": stored } }),
        "{resp}"
    );
    // L293: no avatar is present any more.
    assert_eq!(active_avatar_count(&backend, REGION).await, 0);
}

#[tokio::test]
async fn supplied_future_left_at_is_not_stored() {
    // 6bcd9d8 L644 "a client update that sets `leftAt` to a non-null
    // value signals the intent to leave, and the server stores its own
    // current time instead of the supplied value": no server clock reads
    // 2999, and UTCDate strings of one format order as their instants.
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let future = "2999-12-31T23:59:59Z";
    let resp = update(&backend, ALICE, &id, json!({ "leftAt": future })).await;
    let stored = get(&backend, &id).await["leftAt"].clone();
    let stored_str = stored.as_str().expect("leftAt is set");
    assert!(stored_str < future, "{stored_str}");
    assert_eq!(resp["updated"][id.as_str()]["leftAt"], stored, "{resp}");
    assert_eq!(active_avatar_count(&backend, REGION).await, 0);
}

#[tokio::test]
async fn double_leave_is_invalid_properties() {
    // L2095 "The server MUST return `invalidProperties` naming `leftAt` if
    // the avatar has already left (`leftAt` is non-null)."
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(update(&backend, ALICE, &id, leave()).await["notUpdated"].is_null());
    let err = update_error(&backend, ALICE, &id, leave()).await;
    assert_invalid(&err, &["leftAt"]);
    assert_eq!(active_avatar_count(&backend, REGION).await, 0);
}

#[tokio::test]
async fn spec_eject_example() {
    // 6bcd9d8 L2624 "Region owners and administrators MAY eject avatars by
    // setting `leftAt` on another user's SceneAvatar; the server records
    // its own current time as `leftAt` ... An eject update patches only
    // `leftAt`"; the L3561-3569 patch; L3589 "The server decrements the
    // region's `activeAvatarCount`".
    let backend = setup();
    let alice = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    enter(&backend, CAROL, REGION).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 2);
    let eject = json!({ "leftAt": "2026-06-06T10:22:15Z" });

    // CAROL owns neither the region nor admin rights.
    let err = update_error(&backend, CAROL, &alice, eject.clone()).await;
    assert_eq!(err["type"], "forbidden", "{err}");

    // An eject that also patches `worldState` is not an eject; L2450
    // "Users may not update other users' avatars".
    let with_reason = json!({
        "leftAt": "2026-06-06T10:22:15Z",
        "worldState": { "ejectReason": "Disruptive behavior" }
    });
    let err = update_error(&backend, BOB, &alice, with_reason).await;
    assert_eq!(err["type"], "forbidden", "{err}");
    assert_eq!(get(&backend, &alice).await["leftAt"], Value::Null);

    // BOB owns REGION.
    let resp = update(&backend, BOB, &alice, eject.clone()).await;
    let stored = get(&backend, &alice).await["leftAt"].clone();
    assert!(stored.is_string(), "{stored}");
    assert_ne!(stored, eject["leftAt"]);
    assert_eq!(
        resp["updated"],
        json!({ (alice.as_str()): { "leftAt": stored } }),
        "{resp}"
    );
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);
}

#[tokio::test]
async fn destroy_is_forbidden() {
    // L2226 "Servers MUST return `forbidden` for destroy operations on
    // SceneAvatar."
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = set(&backend, ALICE, json!({ "destroy": [id] })).await;
    assert_eq!(resp["notDestroyed"][&id]["type"], "forbidden", "{resp}");
    assert_eq!(resp["oldState"], resp["newState"]);
    assert_eq!(get(&backend, &id).await["id"], id);
}

#[tokio::test]
async fn active_avatar_count_tracks_presence() {
    // L293 `activeAvatarCount`: "Number of avatars currently present in
    // the region." Counted by hand after each step.
    let backend = setup();
    let alice = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    enter(&backend, BOB, REGION).await;
    enter(&backend, CAROL, OTHER_REGION).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 2);
    assert_eq!(active_avatar_count(&backend, OTHER_REGION).await, 1);

    // CAROL moves to REGION (auto-eject from OTHER_REGION).
    enter(&backend, CAROL, REGION).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 3);
    assert_eq!(active_avatar_count(&backend, OTHER_REGION).await, 0);

    // A refused duplicate changes nothing.
    create_error(&backend, BOB, json!({ "regionId": REGION })).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 3);

    // ALICE leaves, then re-enters (6bcd9d8 L2363).
    update(&backend, ALICE, &alice, leave()).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 2);
    update(&backend, ALICE, &alice, reenter()).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 3);
}
