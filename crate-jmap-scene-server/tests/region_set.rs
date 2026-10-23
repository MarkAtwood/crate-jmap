//! `SceneRegion/set`, `/get` and `/changes` (draft-atwood-jmap-scene-00
//! §SceneRegion Methods, L849-1245 at jmap-chat-spec 3227c0f). Each
//! assertion quotes the draft text it checks; the request bodies are the
//! draft's examples where it has one.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    BindingProperty, BindingTarget, JmapBackend, handle_scene_avatar_changes,
    handle_scene_avatar_get, handle_scene_object_changes, handle_scene_object_get,
    handle_scene_region_changes, handle_scene_region_get, handle_scene_region_set,
};
use jmap_scene_types::{SceneAvatar, SceneObject};
use jmap_types::Id;
use serde_json::{Value, json};

const CHAT: &str = "urn:ietf:params:jmap:chat";
const VTC: &str = "urn:ietf:params:jmap:vtc";

/// The L1010-1028 create, verbatim.
fn main_plaza() -> Value {
    json!({
        "name": "Main Plaza",
        "description": "Central open-air gathering area",
        "bounds": {
            "min": [-500, -10, -500],
            "max": [500, 200, 500]
        },
        "environment": {
            "skyColor": "#4A90D9",
            "ambientIntensity": 0.8,
            "gravity": 9.81,
            "fogDensity": 0.002
        },
        "simulationUri": "wss://sim.example.com/regions/plaza",
        "viewHint": "3d",
        "spawnPosition": [0, 1, 15],
        "spawnOrientation": [0, 0, 0, 1],
        "accessPolicy": "public"
    })
}

/// The L983-993 create, verbatim.
fn design_review_room() -> Value {
    json!({
        "name": "Design Review Room",
        "bounds": {
            "min": [-20, 0, -20],
            "max": [20, 8, 20]
        },
        "spawnPosition": [0, 0, 5],
        "accessPolicy": "invite"
    })
}

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_scene_region_set, backend, caller, args)
}

async fn get(backend: &MemoryBackend, id: &str) -> Value {
    let resp = call!(
        handle_scene_region_get,
        backend,
        ALICE,
        json!({ "ids": [id] })
    );
    resp["list"][0].clone()
}

/// The `notCreated` entry for one create by [`ALICE`], which must fail.
async fn create_error(backend: &MemoryBackend, create: Value) -> Value {
    let resp = set(backend, ALICE, json!({ "create": { "r0": create } })).await;
    assert!(resp["created"].is_null(), "create must fail: {resp}");
    resp["notCreated"]["r0"].clone()
}

/// The id of a region [`ALICE`] creates, which must succeed.
async fn create(backend: &MemoryBackend, create: Value) -> String {
    let resp = set(backend, ALICE, json!({ "create": { "r0": create } })).await;
    assert!(resp["notCreated"].is_null(), "create failed: {resp}");
    resp["created"]["r0"]["id"].as_str().expect("id").to_owned()
}

async fn update(backend: &MemoryBackend, caller: &str, id: &str, patch: Value) -> Value {
    set(backend, caller, json!({ "update": { (id): patch } })).await
}

fn properties(err: &Value) -> Vec<&str> {
    let mut p: Vec<&str> = err["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .map(|v| v.as_str().expect("property name"))
        .collect();
    p.sort_unstable();
    p
}

fn assert_invalid(err: &Value, expected: &[&str]) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    assert_eq!(properties(err), expected, "{err}");
}

// ---- create ---------------------------------------------------------------

#[tokio::test]
async fn create_returns_the_server_set_properties() {
    // 6bcd9d8 L1059 "The server sets `id`, `accountId`, `ownerId` (to the
    // caller's userId), `activeAvatarCount` (to `0`), `createdAt`, and
    // `updatedAt`"; the `created` entry carries exactly those six.
    let backend = backend();
    let resp = set(&backend, ALICE, json!({ "create": { "r0": main_plaza() } })).await;
    let created = resp["created"]["r0"].as_object().expect("created entry");
    let mut keys: Vec<&str> = created.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "accountId",
            "activeAvatarCount",
            "createdAt",
            "id",
            "ownerId",
            "updatedAt"
        ]
    );
    assert_eq!(created["accountId"], ACCOUNT);
    assert_eq!(created["ownerId"], ALICE);
    assert_eq!(created["activeAvatarCount"], 0);
    assert_eq!(created["createdAt"], created["updatedAt"]);
    // L1042-1043: the state advances.
    assert_ne!(resp["oldState"], resp["newState"]);
}

#[tokio::test]
async fn created_region_is_returned_by_get() {
    // L851 "Standard JMAP `/get`": the stored region carries the create's
    // properties (L1010-1028).
    let backend = backend();
    let id = create(&backend, main_plaza()).await;
    let region = get(&backend, &id).await;
    assert_eq!(region["name"], "Main Plaza");
    assert_eq!(region["bounds"]["max"], json!([500.0, 200.0, 500.0]));
    assert_eq!(region["environment"]["fogDensity"], 0.002);
    assert_eq!(
        region["simulationUri"],
        "wss://sim.example.com/regions/plaza"
    );
}

#[tokio::test]
async fn create_takes_the_draft_defaults() {
    // L983-993 omits spawnOrientation; L291 "Default: `[0, 0, 0, 1]`
    // (identity)". L272 description is "(String|null)".
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let region = get(&backend, &id).await;
    assert_eq!(region["spawnOrientation"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(region["accessPolicy"], "invite");
    assert_eq!(region["description"], Value::Null);
}

#[tokio::test]
async fn create_without_may_create_region_is_forbidden() {
    // L997 "The server MUST return `forbidden` when `mayCreateRegion` is
    // `false`."
    let mut cap = capability();
    cap["mayCreateRegion"] = json!(false);
    let backend = backend_with(cap);
    let err = create_error(&backend, design_review_room()).await;
    assert_eq!(err["type"], "forbidden");
}

#[tokio::test]
async fn create_beyond_max_regions_is_over_quota() {
    // L997 "The server MUST return `overQuota` when `maxRegionsPerAccount`
    // would be exceeded."
    let mut cap = capability();
    cap["maxRegionsPerAccount"] = json!(1);
    let backend = backend_with(cap);
    create(&backend, design_review_room()).await;
    let err = create_error(&backend, main_plaza()).await;
    assert_eq!(err["type"], "overQuota");
}

#[tokio::test]
async fn min_greater_than_max_names_bounds() {
    // L997 "`invalidProperties` naming `bounds` when `bounds.min`
    // components are not less than or equal to corresponding `bounds.max`
    // components"; L1206 "bounds.min[0] (100) is greater than bounds.max[0]
    // (50)".
    let backend = backend();
    let mut r = design_review_room();
    r["bounds"] = json!({ "min": [100, 0, 0], "max": [50, 10, 10] });
    assert_invalid(&create_error(&backend, r).await, &["bounds"]);
}

#[tokio::test]
async fn equal_min_and_max_is_accepted() {
    // L195 "Each component MUST be greater than or equal to the
    // corresponding component of `min`."
    let backend = backend();
    let mut r = design_review_room();
    r["bounds"] = json!({ "min": [1, 2, 3], "max": [1, 2, 3] });
    create(&backend, r).await;
}

#[tokio::test]
async fn bounds_corner_must_be_three_numbers() {
    // L1170 "`bounds.min` or `bounds.max` is not a 3-element numeric
    // array".
    let backend = backend();
    for bounds in [
        json!({ "min": [0, 0], "max": [1, 1, 1] }),
        json!({ "min": [0, 0, 0], "max": [1, "1", 1] }),
        json!({ "min": [0, 0, 0] }),
    ] {
        let mut r = design_review_room();
        r["bounds"] = bounds;
        assert_invalid(&create_error(&backend, r).await, &["bounds"]);
    }
}

#[tokio::test]
async fn name_and_bounds_are_required() {
    // L1167 "`name` is missing or empty"; L1168 "`bounds` is missing".
    let backend = backend();
    let mut r = design_review_room();
    r.as_object_mut().unwrap().remove("name");
    r.as_object_mut().unwrap().remove("bounds");
    assert_invalid(&create_error(&backend, r).await, &["bounds", "name"]);

    let mut r = design_review_room();
    r["name"] = json!("");
    assert_invalid(&create_error(&backend, r).await, &["name"]);
}

#[tokio::test]
async fn unknown_access_policy_is_invalid() {
    // L1171 "`accessPolicy` is not one of `\"public\"`, `\"invite\"`,
    // `\"space\"`": "Unrecognized access-policy values are invalid."
    let backend = backend();
    let mut r = design_review_room();
    r["accessPolicy"] = json!("friends");
    assert_invalid(&create_error(&backend, r).await, &["accessPolicy"]);
}

#[tokio::test]
async fn spawn_position_must_be_three_numbers() {
    // L1173 "Position must be `[x, y, z]` finite Numbers."
    let backend = backend();
    let mut r = design_review_room();
    r["spawnPosition"] = json!([0, 0]);
    assert_invalid(&create_error(&backend, r).await, &["spawnPosition"]);
}

#[tokio::test]
async fn spawn_orientation_must_be_a_unit_quaternion() {
    // L1270 "Quaternion must be `[x, y, z, w]` with finite components and
    // non-zero magnitude; a server that rejects non-unit values also
    // rejects a magnitude outside its epsilon"; this server rejects them.
    let backend = backend();
    let mut r = design_review_room();
    r["spawnOrientation"] = json!([0, 0, 0, 2]);
    assert_invalid(&create_error(&backend, r).await, &["spawnOrientation"]);

    // A turn about Y with magnitude sqrt(0.6^2 + 0.8^2) = sqrt(0.36 +
    // 0.64) = 1 (hand-computed).
    let mut r = design_review_room();
    r["spawnOrientation"] = json!([0, 0.6, 0, 0.8]);
    create(&backend, r).await;
}

/// `true` if `got` is `[0, y / n, 0, w / n]` with `n` the magnitude of
/// `[0, y, 0, w]`, to 1e-12.
fn is_normalized_yw(got: &Value, y: f64, w: f64) -> bool {
    let n = (y * y + w * w).sqrt();
    let want = [0.0, y / n, 0.0, w / n];
    let got: Vec<f64> = got
        .as_array()
        .expect("quaternion")
        .iter()
        .map(|c| c.as_f64().expect("number"))
        .collect();
    got.len() == 4 && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-12)
}

#[tokio::test]
async fn accepted_spawn_orientation_is_stored_normalized() {
    // 6bcd9d8 L196 "If the server does not reject the value, it MUST
    // normalize the quaternion to unit length before storage."
    // `[0, 0.707, 0, 0.707]` is the draft's own three-decimal turn
    // (magnitude 0.99985); RFC 8620 §5.3 `created` holds "any properties
    // that were omitted or not set to the requested value by the server".
    let backend = backend();
    let mut r = design_review_room();
    r["spawnOrientation"] = json!([0, 0.707, 0, 0.707]);
    let resp = set(&backend, ALICE, json!({ "create": { "r0": r } })).await;
    let created = &resp["created"]["r0"];
    assert!(
        is_normalized_yw(&created["spawnOrientation"], 0.707, 0.707),
        "{resp}"
    );
    let id = created["id"].as_str().expect("id");
    let stored = &get(&backend, id).await["spawnOrientation"];
    assert_eq!(stored, &created["spawnOrientation"]);
}

#[tokio::test]
async fn update_echoes_a_normalized_spawn_orientation() {
    // 6bcd9d8 L196 "it MUST normalize the quaternion to unit length before
    // storage"; RFC 8620 §5.3 `updated` carries "any properties that
    // changed in a way not explicitly requested by the PatchObject".
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({ "spawnOrientation": [0, 0.924, 0, 0.383] }),
    )
    .await;
    let echoed = &resp["updated"][&id]["spawnOrientation"];
    assert!(is_normalized_yw(echoed, 0.924, 0.383), "{resp}");
    let stored = &get(&backend, &id).await["spawnOrientation"];
    assert_eq!(stored, echoed);
}

#[tokio::test]
async fn every_offending_property_is_named() {
    // L1163 "`properties` lists each offending property".
    let backend = backend();
    let mut r = design_review_room();
    r["accessPolicy"] = json!("friends");
    r["spawnPosition"] = json!("here");
    r["environment"] = json!("sunny");
    assert_invalid(
        &create_error(&backend, r).await,
        &["accessPolicy", "environment", "spawnPosition"],
    );
}

#[tokio::test]
async fn server_set_properties_are_refused_on_create() {
    // L977 "The server sets `id`, `accountId`, `activeAvatarCount` (to
    // `0`), `createdAt`, and `updatedAt`."
    let backend = backend();
    let mut r = design_review_room();
    r["activeAvatarCount"] = json!(5);
    r["createdAt"] = json!("2026-06-01T10:00:00Z");
    assert_invalid(
        &create_error(&backend, r).await,
        &["activeAvatarCount", "createdAt"],
    );
}

#[tokio::test]
async fn active_call_id_is_not_a_create_property() {
    // L975 lists the create's optional properties; the bindings among them
    // are "`chatId` (String), `spaceId` (String), `channelId` (String)".
    // `activeCallId` is first patchable on update (L1065).
    let backend = backend();
    backend.advertise_capability(&Id::from(ACCOUNT), VTC);
    let mut r = design_review_room();
    r["activeCallId"] = json!("call-1");
    assert_invalid(&create_error(&backend, r).await, &["activeCallId"]);
}

// ---- accessPolicy "space" and Chat bindings -------------------------------

#[tokio::test]
async fn space_policy_needs_the_chat_capability() {
    // L1176 "`accessPolicy` is `\"space\"` but `urn:ietf:params:jmap:chat`
    // is not in the server's capabilities".
    let backend = backend();
    let mut r = design_review_room();
    r["accessPolicy"] = json!("space");
    assert_invalid(&create_error(&backend, r).await, &["accessPolicy"]);
}

#[tokio::test]
async fn space_policy_needs_a_space_id() {
    // 6bcd9d8 L1273 "`accessPolicy` is `\"space\"` but `spaceId` is
    // null"; L1085 names the property: such a region "fails with
    // `invalidProperties` naming `spaceId`".
    let backend = backend();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    let mut r = design_review_room();
    r["accessPolicy"] = json!("space");
    r["spaceId"] = Value::Null;
    assert_invalid(&create_error(&backend, r).await, &["spaceId"]);

    backend.set_region_binding(BindingProperty::SpaceId, "space-1", BindingTarget::Usable);
    let mut r = design_review_room();
    r["accessPolicy"] = json!("space");
    r["spaceId"] = json!("space-1");
    create(&backend, r).await;
}

#[tokio::test]
async fn clearing_space_id_under_space_policy_names_space_id() {
    // 6bcd9d8 L1085 "an update that sets `spaceId` to `null` while
    // `accessPolicy` is `\"space\"` fails with `invalidProperties` naming
    // `spaceId`."
    let backend = backend();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    backend.set_region_binding(BindingProperty::SpaceId, "space-1", BindingTarget::Usable);
    let mut r = design_review_room();
    r["accessPolicy"] = json!("space");
    r["spaceId"] = json!("space-1");
    let id = create(&backend, r).await;
    let resp = update(&backend, ALICE, &id, json!({ "spaceId": null })).await;
    assert_invalid(&resp["notUpdated"][&id], &["spaceId"]);
    assert_eq!(get(&backend, &id).await["spaceId"], "space-1");
}

/// A region bound to Space `space` and channel `channel`.
fn bound_room(space: &str, channel: &str) -> Value {
    let mut r = design_review_room();
    r["spaceId"] = json!(space);
    r["channelId"] = json!(channel);
    r
}

#[tokio::test]
async fn channel_must_be_a_channel_of_the_space() {
    // 6bcd9d8 L1085 "the server SHOULD verify ... that a non-null
    // `channelId` is a channel of the Space named by `spaceId`. The server
    // SHOULD return `invalidProperties` naming the binding property
    // (`chatId`, `spaceId`, or `channelId`) if a check fails."
    let backend = backend();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    for (property, id) in [
        (BindingProperty::SpaceId, "space-1"),
        (BindingProperty::SpaceId, "space-2"),
        (BindingProperty::ChannelId, "channel-1"),
    ] {
        backend.set_region_binding(property, id, BindingTarget::Usable);
    }
    backend.set_channel_space("channel-1", "space-1");

    let err = create_error(&backend, bound_room("space-2", "channel-1")).await;
    assert_invalid(&err, &["channelId"]);

    let id = create(&backend, bound_room("space-1", "channel-1")).await;
    // Moving the region to another Space leaves channel-1 outside it.
    let resp = update(&backend, ALICE, &id, json!({ "spaceId": "space-2" })).await;
    assert_invalid(&resp["notUpdated"][&id], &["channelId"]);
    assert_eq!(get(&backend, &id).await["spaceId"], "space-1");
}

#[tokio::test]
async fn chat_binding_without_the_chat_capability_is_invalid() {
    // L406 "The following fields are present only when the corresponding
    // companion capability is advertised on the same account"; L409
    // chatId "When `urn:ietf:params:jmap:chat` is present".
    let backend = backend();
    backend.set_region_binding(BindingProperty::ChatId, "chat-1", BindingTarget::Usable);
    let mut r = design_review_room();
    r["chatId"] = json!("chat-1");
    assert_invalid(&create_error(&backend, r).await, &["chatId"]);
}

#[tokio::test]
async fn chat_binding_must_reference_an_accessible_object() {
    // L999 "The server MUST return `invalidProperties` naming the binding
    // property (`chatId`, `spaceId`, or `channelId`) if the referenced
    // object does not exist or the caller cannot access it."
    let backend = backend();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    backend.set_region_binding(
        BindingProperty::SpaceId,
        "space-hidden",
        BindingTarget::Inaccessible,
    );
    for (property, id) in [
        ("chatId", "chat-missing"),
        ("spaceId", "space-hidden"),
        ("channelId", "channel-missing"),
    ] {
        let mut r = design_review_room();
        r[property] = json!(id);
        assert_invalid(&create_error(&backend, r).await, &[property]);
    }
}

#[tokio::test]
async fn usable_chat_binding_is_stored_with_null_siblings() {
    // L999 the referenced object "exists and the caller has access to
    // it"; L409-415 the unset bindings are "`null`", as in L922-925
    // ("chatId": "01J5CHAT000000000000000099", "spaceId": null,
    // "channelId": null, "activeCallId": null).
    let backend = backend();
    backend.advertise_capability(&Id::from(ACCOUNT), CHAT);
    backend.advertise_capability(&Id::from(ACCOUNT), VTC);
    backend.set_region_binding(
        BindingProperty::ChatId,
        "01J5CHAT000000000000000099",
        BindingTarget::Usable,
    );
    let mut r = design_review_room();
    r["chatId"] = json!("01J5CHAT000000000000000099");
    let id = create(&backend, r).await;
    let region = get(&backend, &id).await;
    assert_eq!(region["chatId"], "01J5CHAT000000000000000099");
    assert_eq!(region["spaceId"], Value::Null);
    assert_eq!(region["channelId"], Value::Null);
    assert_eq!(region["activeCallId"], Value::Null);
}

#[tokio::test]
async fn standalone_region_has_no_binding_fields() {
    // L406 binding fields "are present only when the corresponding
    // companion capability is advertised on the same account".
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let region = get(&backend, &id).await;
    for key in ["chatId", "spaceId", "channelId", "activeCallId"] {
        assert!(region.get(key).is_none(), "{key} present: {region}");
    }
}

// ---- update ---------------------------------------------------------------

/// The L1080-1086 patch, verbatim.
fn spec_patch() -> Value {
    json!({
        "accessPolicy": "public",
        "bounds": {
            "min": [-40, 0, -40],
            "max": [40, 12, 40]
        }
    })
}

#[tokio::test]
async fn owner_applies_the_spec_patch() {
    // L1065 "`update` supports patching: ... `bounds`, ... `accessPolicy`";
    // L1100-1104 the update succeeds and the state advances. RFC 8620
    // §5.3 `updated` holds "any properties that changed in a way not
    // explicitly requested by the PatchObject": the server-set
    // `updatedAt` (L398).
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let before = get(&backend, &id).await;
    let resp = update(&backend, ALICE, &id, spec_patch()).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_ne!(resp["oldState"], resp["newState"]);
    let echo = &resp["updated"][&id];
    let region = get(&backend, &id).await;
    assert_eq!(region["accessPolicy"], "public");
    assert_eq!(region["bounds"]["max"], json!([40.0, 12.0, 40.0]));
    assert_eq!(echo["updatedAt"], region["updatedAt"]);
    assert_eq!(region["createdAt"], before["createdAt"]);
}

#[tokio::test]
async fn non_owner_update_is_forbidden_admin_allowed() {
    // L1069 "The server MUST return `forbidden` when the caller is not the
    // region owner and does not have deployment-defined administrative
    // privileges."
    // The region is `"invite"`; L1231 makes a caller without access
    // `notFound`, so BOB is granted access to reach the `forbidden` row.
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    backend.grant_region_access(&id, BOB);
    let resp = update(&backend, BOB, &id, spec_patch()).await;
    assert_eq!(resp["notUpdated"][&id]["type"], "forbidden");
    let resp = update(&backend, ADMIN, &id, spec_patch()).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
}

#[tokio::test]
async fn update_applies_the_create_validations() {
    // L1218 "Same field-level validations as create apply: `bounds.min >=
    // bounds.max`, invalid `accessPolicy`, invalid `viewHint`, non-unit
    // `spawnOrientation`".
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    for (patch, property) in [
        (json!({ "bounds/min": [100, 0, 0] }), "bounds"),
        (json!({ "accessPolicy": "friends" }), "accessPolicy"),
        (json!({ "viewHint": 3 }), "viewHint"),
        (
            json!({ "spawnOrientation": [1, 1, 0, 0] }),
            "spawnOrientation",
        ),
        (json!({ "name": "" }), "name"),
    ] {
        let resp = update(&backend, ALICE, &id, patch).await;
        assert_invalid(&resp["notUpdated"][&id], &[property]);
    }
    // Nothing was applied.
    assert_eq!(get(&backend, &id).await["accessPolicy"], "invite");
}

#[tokio::test]
async fn owner_id_is_server_set_and_immutable() {
    // 6bcd9d8 L327 "`ownerId` (String, immutable, server-set)"; L1316
    // "Attempting to change `id`, `accountId`, `ownerId`, ... is also
    // invalid."
    let backend = backend();
    let mut r = design_review_room();
    r["ownerId"] = json!(BOB);
    assert_invalid(&create_error(&backend, r).await, &["ownerId"]);
    let id = create(&backend, design_review_room()).await;
    assert_eq!(get(&backend, &id).await["ownerId"], ALICE);
    let resp = update(&backend, ALICE, &id, json!({ "ownerId": BOB })).await;
    assert_invalid(&resp["notUpdated"][&id], &["ownerId"]);
}

#[tokio::test]
async fn invited_user_ids_must_be_a_string_array() {
    // 6bcd9d8 L1275 "`invitedUserIds` is not an array of Strings |
    // `invitedUserIds` is a String[]."
    let backend = backend();
    for bad in [json!("user:bob@example.com"), json!([1]), json!(null)] {
        let mut r = design_review_room();
        r["invitedUserIds"] = bad;
        assert_invalid(&create_error(&backend, r).await, &["invitedUserIds"]);
    }
    let id = create(&backend, design_review_room()).await;
    let resp = update(&backend, ALICE, &id, json!({ "invitedUserIds": [true] })).await;
    assert_invalid(&resp["notUpdated"][&id], &["invitedUserIds"]);
}

#[tokio::test]
async fn invited_user_ids_admit_and_are_shown_only_to_updaters() {
    // The 6bcd9d8 L1062-1079 create. L2601 `"invite"` admits "The users
    // whose userIds are listed in `invitedUserIds`"; L457 "The server
    // returns the list only to callers who may update the region; for
    // every other caller, the server returns `[]`."
    let backend = backend();
    let mut r = design_review_room();
    r["invitedUserIds"] = json!(["user:alice@example.com", "user:bob@example.com"]);
    let id = create(&backend, r).await;
    let get_as = |caller: &'static str| {
        let backend = &backend;
        let id = id.clone();
        async move {
            call!(
                handle_scene_region_get,
                backend,
                caller,
                json!({ "ids": [id] })
            )
        }
    };
    let invited = json!(["user:alice@example.com", "user:bob@example.com"]);
    assert_eq!(get_as(ALICE).await["list"][0]["invitedUserIds"], invited);
    assert_eq!(get_as(ADMIN).await["list"][0]["invitedUserIds"], invited);
    let bob = get_as(BOB).await;
    assert_eq!(bob["list"][0]["invitedUserIds"], json!([]), "{bob}");
    let carol = get_as("user:carol@example.com").await;
    assert_eq!(carol["notFound"], json!([id]), "{carol}");
}

#[tokio::test]
async fn server_set_properties_cannot_be_patched() {
    // L395 createdAt "(UTCDate, immutable, server-set)"; L293
    // activeAvatarCount "(UnsignedInt, server-set)". L1065 does not list
    // them among the patchable properties.
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({ "createdAt": "2020-01-01T00:00:00Z", "activeAvatarCount": 9 }),
    )
    .await;
    assert_invalid(
        &resp["notUpdated"][&id],
        &["activeAvatarCount", "createdAt"],
    );
}

#[tokio::test]
async fn update_of_unknown_region_is_not_found() {
    // L1328 "The region id does not exist": `notFound`.
    let backend = backend();
    let resp = update(&backend, ALICE, "01J5NOPE000000000000000000", spec_patch()).await;
    assert_eq!(
        resp["notUpdated"]["01J5NOPE000000000000000000"]["type"],
        "notFound"
    );
}

#[tokio::test]
async fn active_call_id_needs_the_vtc_capability() {
    // L417 activeCallId "When `urn:ietf:params:jmap:vtc` is present".
    let backend = backend();
    backend.set_region_binding(
        BindingProperty::ActiveCallId,
        "call-1",
        BindingTarget::Usable,
    );
    let id = create(&backend, design_review_room()).await;
    let resp = update(&backend, ALICE, &id, json!({ "activeCallId": "call-1" })).await;
    assert_invalid(&resp["notUpdated"][&id], &["activeCallId"]);
}

#[tokio::test]
async fn active_call_id_checks_the_call() {
    // L1067 "The server MUST return `invalidProperties` naming
    // `activeCallId` if the VTCCall does not exist or is ended, and
    // `forbidden` if the caller lacks access."
    let backend = backend();
    backend.advertise_capability(&Id::from(ACCOUNT), VTC);
    backend.set_region_binding(
        BindingProperty::ActiveCallId,
        "call-ended",
        BindingTarget::Missing,
    );
    backend.set_region_binding(
        BindingProperty::ActiveCallId,
        "call-other",
        BindingTarget::Inaccessible,
    );
    backend.set_region_binding(
        BindingProperty::ActiveCallId,
        "call-live",
        BindingTarget::Usable,
    );
    let id = create(&backend, design_review_room()).await;

    for call in ["call-unknown", "call-ended"] {
        let resp = update(&backend, ALICE, &id, json!({ "activeCallId": call })).await;
        assert_invalid(&resp["notUpdated"][&id], &["activeCallId"]);
    }
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({ "activeCallId": "call-other" }),
    )
    .await;
    assert_eq!(resp["notUpdated"][&id]["type"], "forbidden");

    let resp = update(&backend, ALICE, &id, json!({ "activeCallId": "call-live" })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
    assert_eq!(get(&backend, &id).await["activeCallId"], "call-live");

    // Clearing the binding to `null` needs no check: L418 "`null` if no
    // call is active".
    let resp = update(&backend, ALICE, &id, json!({ "activeCallId": null })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
}

// ---- destroy --------------------------------------------------------------

/// A SceneObject in `region`, hand-written from the field list at
/// L461-512.
fn object_in(id: &str, region: &str) -> Value {
    json!({
        "id": id, "regionId": region, "parentId": null, "name": null,
        "position": [0, 0, 0], "orientation": [0, 0, 0, 1],
        "visualRef": null, "visualType": null, "assetUri": null,
        "ownerId": BOB,
        "createdAt": "2026-06-01T10:00:00Z", "updatedAt": "2026-06-01T10:00:00Z"
    })
}

/// A SceneAvatar in `region`, hand-written from the field list at
/// L548-581.
fn avatar_in(id: &str, region: &str) -> Value {
    json!({
        "id": id, "regionId": region, "userId": BOB, "displayName": "Bob",
        "position": [0, 0, 5], "orientation": [0, 0, 0, 1],
        "visualRef": null, "visualType": null,
        "joinedAt": "2026-06-01T10:00:00Z", "leftAt": null
    })
}

#[tokio::test]
async fn destroy_removes_contained_objects_and_avatars() {
    // L1210 (jmap-chat-spec 33702a5) "`destroy` removes the SceneRegion
    // and all SceneObject and SceneAvatar records in it"; L1244-1246 the
    // region id is in `destroyed`.
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let other = create(&backend, main_plaza()).await;
    seed_as(&backend, "SceneObject", object_in("obj-in", &id));
    seed_as(&backend, "SceneObject", object_in("obj-out", &other));
    seed_as(&backend, "SceneAvatar", avatar_in("av-in", &id));

    let resp = set(&backend, ALICE, json!({ "destroy": [id] })).await;
    assert_eq!(resp["destroyed"], json!([id]));
    assert!(get(&backend, &id).await.is_null());

    let acct = Id::from(ACCOUNT);
    let caller = ALICE.to_owned();
    let (objects, missing) = backend
        .get_objects::<SceneObject>(
            &caller,
            &acct,
            Some(&[Id::from("obj-in"), Id::from("obj-out")]),
            None,
        )
        .await
        .unwrap();
    assert_eq!(missing, [Id::from("obj-in")]);
    assert_eq!(objects.len(), 1);
    let (avatars, _) = backend
        .get_objects::<SceneAvatar>(&caller, &acct, None, None)
        .await
        .unwrap();
    assert!(avatars.is_empty());
}

#[tokio::test]
async fn destroy_reports_contained_records_destroyed_in_changes() {
    // L1210 (jmap-chat-spec 6bcd9d8) "`SceneObject/changes` and
    // `SceneAvatar/changes` report those records as destroyed".
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let other = create(&backend, main_plaza()).await;
    seed_as(&backend, "SceneObject", object_in("obj-in", &id));
    seed_as(&backend, "SceneObject", object_in("obj-out", &other));
    seed_as(&backend, "SceneAvatar", avatar_in("av-in", &id));
    let object_since = call!(
        handle_scene_object_get,
        &backend,
        ALICE,
        json!({ "ids": [] })
    )["state"]
        .clone();
    let avatar_since = call!(
        handle_scene_avatar_get,
        &backend,
        ALICE,
        json!({ "ids": [] })
    )["state"]
        .clone();

    let resp = set(&backend, ALICE, json!({ "destroy": [id] })).await;
    assert_eq!(resp["destroyed"], json!([id]));

    let objects = call!(
        handle_scene_object_changes,
        &backend,
        ALICE,
        json!({ "sinceState": object_since })
    );
    assert_eq!(objects["destroyed"], json!(["obj-in"]));
    assert_eq!(objects["created"], json!([]));
    assert_eq!(objects["updated"], json!([]));
    let avatars = call!(
        handle_scene_avatar_changes,
        &backend,
        ALICE,
        json!({ "sinceState": avatar_since })
    );
    assert_eq!(avatars["destroyed"], json!(["av-in"]));
    assert_eq!(avatars["created"], json!([]));
    assert_eq!(avatars["updated"], json!([]));
}

#[tokio::test]
async fn destroy_by_non_owner_non_admin_is_forbidden() {
    // L1210 "The server SHOULD return `forbidden` when the caller is not
    // the region owner and does not have deployment-defined administrative
    // privileges"; L1337 "Only the region owner or an admin may destroy a
    // region." The region is `"invite"`; L1343 makes a caller without
    // access `notFound`, so BOB is granted access to reach the `forbidden`
    // row.
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    backend.grant_region_access(&id, BOB);
    let resp = set(&backend, BOB, json!({ "destroy": [id] })).await;
    assert_eq!(resp["notDestroyed"][&id]["type"], "forbidden");
    assert_eq!(get(&backend, &id).await["id"], json!(id));
}

#[tokio::test]
async fn admin_may_destroy_region_it_does_not_own() {
    // L1337 "Only the region owner or an admin may destroy a region."
    let backend = backend();
    let id = create(&backend, design_review_room()).await;
    let resp = set(&backend, ADMIN, json!({ "destroy": [id] })).await;
    assert_eq!(resp["destroyed"], json!([id]));
    assert!(get(&backend, &id).await.is_null());
}

#[tokio::test]
async fn destroy_of_unknown_region_is_not_found() {
    // L1343 "The region id does not exist or the caller lacks access":
    // `notFound`.
    let backend = backend();
    let resp = set(
        &backend,
        ALICE,
        json!({ "destroy": ["01J5NOPE000000000000000000"] }),
    )
    .await;
    assert_eq!(
        resp["notDestroyed"]["01J5NOPE000000000000000000"]["type"],
        "notFound"
    );
}

// ---- changes --------------------------------------------------------------

#[tokio::test]
async fn changes_reports_created_regions() {
    // L959 "Standard JMAP `/changes`"; RFC 8620 §5.2 `created` lists ids
    // created since `sinceState`.
    let backend = backend();
    let resp = set(&backend, ALICE, json!({})).await;
    let since = resp["oldState"].clone();
    let id = create(&backend, design_review_room()).await;
    let resp = call!(
        handle_scene_region_changes,
        &backend,
        ALICE,
        json!({ "sinceState": since })
    );
    assert_eq!(resp["created"], json!([id]));
}
