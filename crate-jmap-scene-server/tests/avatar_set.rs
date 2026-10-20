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

/// A region whose `spawnPosition` is the `[0, 0, 10]` the L1950 response
/// reports for an avatar entering it.
fn region(id: &str, access_policy: &str) -> Value {
    let mut r = stored_region(id, "Gallery", "2026-06-01T10:00:00Z");
    r["spawnPosition"] = json!([0, 0, 10]);
    r["accessPolicy"] = json!(access_policy);
    r
}

fn backend_with_regions(capability: Value) -> MemoryBackend {
    let backend = backend_with(capability);
    for id in [REGION, OTHER_REGION] {
        seed_as(&backend, "SceneRegion", BOB, region(id, "public"));
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

#[tokio::test]
async fn reentering_reuses_the_departed_record() {
    // L2139 "When a user who has left (non-null `leftAt`) re-enters the
    // same region, the server MUST update the existing SceneAvatar record
    // rather than creating a new one: clear `leftAt` to `null`, update
    // `joinedAt` to the current time, and set `position` to
    // `spawnPosition`."
    let backend = setup();
    let first = enter(&backend, ALICE, REGION).await;
    let id = first["id"].as_str().unwrap().to_owned();
    let teleport = json!({ "position": [25.0, 0, -12.3] });
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
    let again = enter(&backend, ALICE, REGION).await;
    assert_eq!(again["id"], first["id"]);
    assert_eq!(again["leftAt"], Value::Null);
    assert_eq!(again["position"], json!([0.0, 0.0, 10.0]));
    assert_ne!(again["joinedAt"], "2026-06-06T11:45:00Z");
    assert_eq!(active_avatar_count(&backend, REGION).await, 1);

    // RFC 8620 §5.2: an existing record that changed is `updated`, not
    // `created`.
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
async fn missing_region_is_not_found() {
    // L1959 "The server MUST return `notFound` when `regionId` does not
    // reference an existing SceneRegion".
    let backend = setup();
    let err = create_error(
        &backend,
        ALICE,
        json!({ "regionId": "01J5ABC0000000000000000099" }),
    )
    .await;
    assert_eq!(err["type"], "notFound", "{err}");
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
async fn invite_region_is_forbidden_without_access() {
    // L2157 "Region `accessPolicy` is `\"invite\"` and user has not been
    // granted access"; L2362 "The server MUST return `forbidden` for
    // `SceneAvatar/set` create from unauthorized users."
    let backend = setup();
    seed_as(
        &backend,
        "SceneRegion",
        BOB,
        region("01J5ABC0000000000000000003", "invite"),
    );
    let invite = json!({ "regionId": "01J5ABC0000000000000000003" });
    let err = create_error(&backend, ALICE, invite.clone()).await;
    assert_eq!(err["type"], "forbidden", "{err}");

    // "Only users explicitly granted access may enter."
    backend.grant_region_access("01J5ABC0000000000000000003", ALICE);
    let resp = set(&backend, ALICE, json!({ "create": { "av0": invite } })).await;
    assert!(resp["notCreated"].is_null(), "{resp}");
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
    // L2083 "A user leaves by calling `SceneAvatar/set` with an update
    // setting `leftAt`"; the L2105-2109 patch and the L2125-2127 response.
    let backend = setup();
    let id = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = update(&backend, ALICE, &id, leave()).await;
    assert_eq!(resp["updated"], json!({ (id.as_str()): null }), "{resp}");
    assert_eq!(get(&backend, &id).await["leftAt"], "2026-06-06T11:45:00Z");
    // L293: no avatar is present any more.
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
    // L2383 "Region owners and administrators MAY eject avatars by setting
    // `leftAt` on another user's SceneAvatar"; the L3252-3258 patch; L3281
    // "The server decrements the region's `activeAvatarCount`".
    let backend = setup();
    let alice = enter(&backend, ALICE, REGION).await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    enter(&backend, CAROL, REGION).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 2);
    let eject = json!({
        "leftAt": "2026-06-06T10:22:15Z",
        "worldState": {
            "ejectReason": "Disruptive behavior",
            "ejectedBy": "user:admin@example.com"
        }
    });

    // CAROL owns neither the region nor admin rights.
    let err = update_error(&backend, CAROL, &alice, eject.clone()).await;
    assert_eq!(err["type"], "forbidden", "{err}");

    // BOB owns REGION.
    let resp = update(&backend, BOB, &alice, eject).await;
    assert_eq!(resp["updated"], json!({ (alice.as_str()): null }), "{resp}");
    let stored = get(&backend, &alice).await;
    assert_eq!(stored["leftAt"], "2026-06-06T10:22:15Z");
    assert_eq!(stored["worldState"]["ejectReason"], "Disruptive behavior");
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

    // ALICE leaves, then re-enters.
    update(&backend, ALICE, &alice, leave()).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 2);
    enter(&backend, ALICE, REGION).await;
    assert_eq!(active_avatar_count(&backend, REGION).await, 3);
}
