//! `SceneObject/set`, `/get` and `/changes` (draft-atwood-jmap-scene-00
//! §SceneObject Methods, L1343-1748 at jmap-chat-spec 3227c0f). Each
//! assertion quotes the draft text it checks; the request bodies are the
//! draft's examples where it has one.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    handle_scene_object_changes, handle_scene_object_get, handle_scene_object_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

/// The region of the draft's examples, owned by [`BOB`].
const REGION: &str = "01J5ABC0000000000000000001";
/// A second region, owned by [`BOB`].
const OTHER_REGION: &str = "01J5ABC0000000000000000002";
/// The blobs the L1462 and L1490 examples reference.
const SCULPTURE_BLOB: &str = "blob-gltf-sculpture-001";
const PEDESTAL_BLOB: &str = "blob-gltf-pedestal-001";

fn backend_with_regions(capability: Value) -> MemoryBackend {
    let backend = backend_with(capability);
    for id in [REGION, OTHER_REGION] {
        let mut region = stored_region(id, "Gallery", "2026-06-01T10:00:00Z");
        region["ownerId"] = json!(BOB);
        seed_as(&backend, "SceneRegion", region);
    }
    for blob in [SCULPTURE_BLOB, PEDESTAL_BLOB] {
        backend.add_blob(&Id::from(ACCOUNT), blob, 1024);
    }
    backend
}

fn setup() -> MemoryBackend {
    backend_with_regions(capability())
}

/// The L1458-1466 create, verbatim.
fn sculpture() -> Value {
    json!({
        "regionId": "01J5ABC0000000000000000001",
        "position": [12.5, 0, -3.2],
        "orientation": [0, 0.707, 0, 0.707],
        "visualRef": "blob-gltf-sculpture-001",
        "visualType": "model/gltf-binary",
        "name": "Sculpture: Convergence",
        "physicsMode": "static",
        "interactable": true
    })
}

/// The L1483-1500 create, verbatim.
fn pedestal() -> Value {
    json!({
        "regionId": "01J5ABC0000000000000000001",
        "parentId": null,
        "name": "Pedestal A",
        "position": [8.0, 0, -6.5],
        "orientation": [0, 0.383, 0, 0.924],
        "scale": [1.2, 1.0, 1.2],
        "visualRef": "blob-gltf-pedestal-001",
        "visualType": "model/gltf-binary",
        "assetUri": "https://cdn.example.com/assets/pedestal-001.glb",
        "physicsMode": "static",
        "interactable": false,
        "visible": true,
        "worldState": {
            "material": "marble",
            "label": "Pedestal for rotating exhibit"
        }
    })
}

/// A minimal create: the two required properties of L1440-1444.
fn bare(region: &str) -> Value {
    json!({ "regionId": region, "position": [0, 0, 0] })
}

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(ids handle_scene_object_set, backend, caller, args)
}

async fn get(backend: &MemoryBackend, id: &str) -> Value {
    call!(
        handle_scene_object_get,
        backend,
        ALICE,
        json!({ "ids": [id] })
    )
}

/// The `notCreated` entry for one create by [`ALICE`], which must fail.
async fn create_error(backend: &MemoryBackend, create: Value) -> Value {
    let resp = set(backend, ALICE, json!({ "create": { "o0": create } })).await;
    assert!(resp["created"].is_null(), "create must fail: {resp}");
    resp["notCreated"]["o0"].clone()
}

/// The id of an object [`ALICE`] creates, which must succeed.
async fn create(backend: &MemoryBackend, create: Value) -> String {
    let resp = set(backend, ALICE, json!({ "create": { "o0": create } })).await;
    assert!(resp["notCreated"].is_null(), "create failed: {resp}");
    resp["created"]["o0"]["id"].as_str().expect("id").to_owned()
}

/// A root object in [`REGION`] followed by `n - 1` objects, each the child
/// of the previous one; returns their ids from the root down.
async fn chain(backend: &MemoryBackend, n: usize) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for _ in 0..n {
        let mut c = bare(REGION);
        if let Some(parent) = ids.last() {
            c["parentId"] = json!(parent);
        }
        ids.push(create(backend, c).await);
    }
    ids
}

async fn update(backend: &MemoryBackend, caller: &str, id: &str, patch: Value) -> Value {
    set(backend, caller, json!({ "update": { (id): patch } })).await
}

/// The `notUpdated` entry of an update by [`ALICE`], which must fail.
async fn update_error(backend: &MemoryBackend, id: &str, patch: Value) -> Value {
    let resp = update(backend, ALICE, id, patch).await;
    assert!(resp["updated"].is_null(), "update must fail: {resp}");
    resp["notUpdated"][id].clone()
}

fn assert_invalid(err: &Value, expected: &[&str]) {
    assert_eq!(err["type"], "invalidProperties", "{err}");
    let mut p: Vec<&str> = err["properties"]
        .as_array()
        .expect("properties")
        .iter()
        .map(|v| v.as_str().expect("property name"))
        .collect();
    p.sort_unstable();
    assert_eq!(p, expected, "{err}");
}

// ---- create ---------------------------------------------------------------

#[tokio::test]
async fn create_returns_the_server_set_properties() {
    // L1451 "The server sets `id`, `ownerId`, `createdAt`, and
    // `updatedAt`", `ownerId` being the creating user (L503 "The userId of
    // the user who created this object"). The pedestal's
    // `[0, 0.383, 0, 0.924]` is not unit length, so 29e9a41 L196 normalizes
    // it and RFC 8620 §5.3 adds it to `created` ("any properties that were
    // omitted or not set to the requested value by the server").
    let backend = setup();
    let resp = set(&backend, ALICE, json!({ "create": { "o0": pedestal() } })).await;
    let created = resp["created"]["o0"].as_object().expect("created entry");
    let mut keys: Vec<&str> = created.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["createdAt", "id", "orientation", "ownerId", "updatedAt"]
    );
    assert_eq!(created["ownerId"], ALICE);
    assert_eq!(created["createdAt"], created["updatedAt"]);
    // L1514-1515: the state advances.
    assert_ne!(resp["oldState"], resp["newState"]);

    let id = created["id"].as_str().expect("id");
    let stored = &get(&backend, id).await["list"][0];
    for (key, value) in pedestal().as_object().expect("object") {
        if key != "orientation" {
            assert!(
                same(&stored[key], value),
                "{key}: {} vs {value}",
                stored[key]
            );
        }
    }
}

/// JSON equality with Numbers compared by value, so that a stored `0.0`
/// equals a sent `0`.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

/// `q` as 4 Numbers.
fn quaternion(q: &Value) -> Vec<f64> {
    q.as_array()
        .expect("quaternion")
        .iter()
        .map(|c| c.as_f64().expect("number"))
        .collect()
}

/// `true` if `got` is `[0, y / n, 0, w / n]` with `n` the magnitude of
/// `[0, y, 0, w]`, to 1e-12.
fn is_normalized_yw(got: &Value, y: f64, w: f64) -> bool {
    let n = (y * y + w * w).sqrt();
    let want = [0.0, y / n, 0.0, w / n];
    quaternion(got)
        .iter()
        .zip(want)
        .all(|(g, w)| (g - w).abs() < 1e-12)
}

#[tokio::test]
async fn accepted_orientations_are_stored_normalized() {
    // L144 "If the server does not reject the value, it MUST normalize the
    // quaternion to unit length before storage." The draft's own creates
    // send L1461 `[0, 0.707, 0, 0.707]` and L1488 `[0, 0.383, 0, 0.924]`.
    let backend = setup();
    for (create_body, y, w) in [(sculpture(), 0.707, 0.707), (pedestal(), 0.383, 0.924)] {
        let id = create(&backend, create_body).await;
        let stored = &get(&backend, &id).await["list"][0]["orientation"];
        assert!(is_normalized_yw(stored, y, w), "{stored}");
    }
}

#[tokio::test]
async fn create_echoes_a_normalized_orientation() {
    // 29e9a41 L196 "If the server does not reject the value, it MUST
    // normalize the quaternion to unit length before storage"; RFC 8620
    // §5.3 `created` holds "any properties that were omitted or not set to
    // the requested value by the server".
    let backend = setup();
    let resp = set(&backend, ALICE, json!({ "create": { "o0": sculpture() } })).await;
    let created = &resp["created"]["o0"];
    let echoed = &created["orientation"];
    assert!(is_normalized_yw(echoed, 0.707, 0.707), "{resp}");
    let id = created["id"].as_str().expect("id");
    let stored = &get(&backend, id).await["list"][0]["orientation"];
    assert_eq!(stored, echoed);
}

#[tokio::test]
async fn create_echoes_an_omitted_orientation_as_identity() {
    // 29e9a41 L1553 lists `orientation` as optional and L192 "The identity
    // orientation (no rotation) is `[0, 0, 0, 1]`"; RFC 8620 §5.3
    // `created` holds "any properties that were omitted".
    let backend = setup();
    let resp = set(&backend, ALICE, json!({ "create": { "o0": bare(REGION) } })).await;
    assert_eq!(
        resp["created"]["o0"]["orientation"],
        json!([0.0, 0.0, 0.0, 1.0]),
        "{resp}"
    );
}

#[tokio::test]
async fn create_does_not_echo_a_unit_orientation() {
    // `[0, 1, 0, 0]` (29e9a41 L1652's half turn) is already unit length, so the
    // server stores it as requested and RFC 8620 §5.3 does not require it
    // in `created`.
    let backend = setup();
    let mut o = bare(REGION);
    o["orientation"] = json!([0, 1, 0, 0]);
    let resp = set(&backend, ALICE, json!({ "create": { "o0": o } })).await;
    let created = resp["created"]["o0"].as_object().expect("created entry");
    assert!(!created.contains_key("orientation"), "{resp}");
}

#[tokio::test]
async fn update_echoes_a_normalized_orientation() {
    // L144 "it MUST normalize the quaternion to unit length before
    // storage"; RFC 8620 §5.3 `updated` carries "any properties that
    // changed in a way not explicitly requested by the PatchObject".
    let backend = setup();
    let id = create(&backend, bare(REGION)).await;
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({ "orientation": [0, 0.924, 0, 0.383] }),
    )
    .await;
    let echoed = &resp["updated"][&id]["orientation"];
    assert!(is_normalized_yw(echoed, 0.924, 0.383), "{resp}");
    let stored = &get(&backend, &id).await["list"][0]["orientation"];
    assert_eq!(stored, echoed);
}

#[tokio::test]
async fn create_takes_the_draft_defaults() {
    // L482 scale "Default: `[1, 1, 1]`"; L494 physicsMode "Default:
    // `\"static\"`"; L497 interactable "Default: `false`"; L500 visible
    // "Default: `true`". L1449 lists `orientation` as optional; an absent
    // one is L140 "The identity orientation (no rotation) is `[0, 0, 0,
    // 1]`".
    let backend = setup();
    let id = create(&backend, bare(REGION)).await;
    let o = &get(&backend, &id).await["list"][0];
    assert_eq!(o["scale"], json!([1.0, 1.0, 1.0]));
    assert_eq!(o["physicsMode"], "static");
    assert_eq!(o["interactable"], false);
    assert_eq!(o["visible"], true);
    assert_eq!(o["orientation"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(o["parentId"], Value::Null);
    assert_eq!(o["visualRef"], Value::Null);
}

#[tokio::test]
async fn create_without_may_create_object_is_forbidden() {
    // L1472 "The server MUST return `forbidden` when `mayCreateObject` is
    // `false`."
    let mut cap = capability();
    cap["mayCreateObject"] = json!(false);
    let backend = backend_with_regions(cap);
    assert_eq!(
        create_error(&backend, sculpture()).await["type"],
        "forbidden"
    );
}

#[tokio::test]
async fn create_beyond_max_objects_per_region_is_over_quota() {
    // L1472 "The server MUST return `overQuota` when `maxObjectsPerRegion`
    // would be exceeded"; L1674 "The per-region object cap has been
    // reached": another region still has room.
    let mut cap = capability();
    cap["maxObjectsPerRegion"] = json!(2);
    let backend = backend_with_regions(cap);
    create(&backend, bare(REGION)).await;
    create(&backend, bare(REGION)).await;
    assert_eq!(
        create_error(&backend, bare(REGION)).await["type"],
        "overQuota"
    );
    create(&backend, bare(OTHER_REGION)).await;
}

#[tokio::test]
async fn region_id_and_position_are_required() {
    // L1643 "`regionId` is missing"; L1644 "`position` is missing".
    let backend = setup();
    let err = create_error(&backend, json!({ "name": "nothing" })).await;
    assert_invalid(&err, &["position", "regionId"]);
}

#[tokio::test]
async fn unknown_region_is_invalid_properties() {
    // L1576 "The server MUST return `invalidProperties` naming `regionId`
    // when `regionId` does not exist".
    let backend = setup();
    let err = create_error(&backend, bare("01J5NOPE000000000000000000")).await;
    assert_invalid(&err, &["regionId"]);
}

#[tokio::test]
async fn coordinates_must_be_well_formed() {
    // L117 "`Number[N]` denotes a JSON array of exactly N Number values";
    // L1645 "Position components MUST be finite Numbers"; L1646
    // "`orientation` is not a unit quaternion | Magnitude must be within
    // epsilon of 1.0"; L1647 "Scale factors must be finite."
    let backend = setup();
    let mut c = bare(REGION);
    c["position"] = json!([1, 2]);
    c["orientation"] = json!([0, 0, 0, 2]);
    c["scale"] = json!([1, "1", 1]);
    assert_invalid(
        &create_error(&backend, c).await,
        &["orientation", "position", "scale"],
    );
}

#[tokio::test]
async fn unknown_physics_mode_is_invalid() {
    // L1648 "`physicsMode` is not one of `\"static\"`, `\"dynamic\"`,
    // `\"kinematic\"`, `\"none\"`".
    let backend = setup();
    let mut c = bare(REGION);
    c["physicsMode"] = json!("floating");
    assert_invalid(&create_error(&backend, c).await, &["physicsMode"]);
}

#[tokio::test]
async fn unsupported_visual_type_names_visual_type() {
    // L1472 "The server MUST return `invalidProperties` naming
    // `visualType` when `visualType` is not in `supportedVisualTypes`."
    // The fixture capability lists model/gltf-binary and model/gltf+json.
    let backend = setup();
    let mut c = sculpture();
    c["visualType"] = json!("model/vnd.usdz+zip");
    assert_invalid(&create_error(&backend, c).await, &["visualType"]);
    let mut c = sculpture();
    c["visualType"] = json!("model/gltf+json");
    create(&backend, c).await;
}

#[tokio::test]
async fn visual_ref_and_visual_type_go_together() {
    // L1472 "The server MUST return `invalidProperties` naming `visualRef`
    // and `visualType` when `visualRef` is present but `visualType` is
    // absent, or vice versa."
    let backend = setup();
    for drop in ["visualType", "visualRef"] {
        let mut c = sculpture();
        c.as_object_mut().expect("object").remove(drop);
        assert_invalid(
            &create_error(&backend, c).await,
            &["visualRef", "visualType"],
        );
    }
}

#[tokio::test]
async fn visual_ref_must_reference_an_uploaded_blob() {
    // 6bcd9d8 L1769 lists "`visualRef` does not reference a valid blob |
    // The blobId must have been previously uploaded via the JMAP upload
    // endpoint." under create `invalidProperties`.
    let backend = setup();
    let mut c = sculpture();
    c["visualRef"] = json!("blob-never-uploaded");
    assert_invalid(&create_error(&backend, c).await, &["visualRef"]);
}

#[tokio::test]
async fn update_to_unknown_visual_ref_names_visual_ref() {
    // L1809 "Same field-level validations as create"; L1769 lists
    // "`visualRef` does not reference a valid blob" under create
    // `invalidProperties`.
    let backend = setup();
    let id = create(&backend, sculpture()).await;
    let err = update_error(&backend, &id, json!({ "visualRef": "blob-never-uploaded" })).await;
    assert_invalid(&err, &["visualRef"]);
}

#[tokio::test]
async fn server_set_properties_are_refused_on_create() {
    // L1451 "The server sets `id`, `ownerId`, `createdAt`, and
    // `updatedAt`."
    let backend = setup();
    let mut c = bare(REGION);
    c["ownerId"] = json!(BOB);
    c["createdAt"] = json!("2026-06-01T12:00:00Z");
    assert_invalid(&create_error(&backend, c).await, &["createdAt", "ownerId"]);
}

#[tokio::test]
async fn unknown_parent_names_parent_id() {
    // L1768 lists "`parentId` does not reference an existing SceneObject"
    // under create `invalidProperties`.
    let backend = setup();
    let mut c = bare(REGION);
    c["parentId"] = json!("01J5OBJNOPE000000000000000");
    assert_invalid(&create_error(&backend, c).await, &["parentId"]);
}

#[tokio::test]
async fn parent_in_another_region_names_parent_id() {
    // L1472 "The server MUST return `invalidProperties` naming `parentId`
    // when `parentId` references a SceneObject in a different region."
    let backend = setup();
    let parent = create(&backend, bare(OTHER_REGION)).await;
    let mut c = bare(REGION);
    c["parentId"] = json!(parent);
    assert_invalid(&create_error(&backend, c).await, &["parentId"]);
}

#[tokio::test]
async fn scene_graph_is_at_most_sixteen_levels_deep() {
    // L1653 "`parentId` would cause scene-graph depth to exceed deployment
    // limit | Servers MAY impose a maximum hierarchy depth"; the
    // implementer guide L340 "A depth limit of 16 levels" and L342
    // "returning `invalidProperties` if the depth would exceed the limit".
    let backend = setup();
    let ids = chain(&backend, 16).await;
    let mut c = bare(REGION);
    c["parentId"] = json!(ids[15]);
    assert_invalid(&create_error(&backend, c).await, &["parentId"]);
    let mut c = bare(REGION);
    c["parentId"] = json!(ids[14]);
    create(&backend, c).await;
}

// ---- update ---------------------------------------------------------------

#[tokio::test]
async fn owner_applies_the_spec_move() {
    // L1543-1546, the "move an object" patch; L1582 the update succeeds.
    let backend = setup();
    let id = create(&backend, sculpture()).await;
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({ "position": [15.0, 0, -5.0], "orientation": [0, 1, 0, 0] }),
    )
    .await;
    assert!(resp["updated"][&id].is_object(), "{resp}");
    let o = &get(&backend, &id).await["list"][0];
    assert_eq!(o["position"], json!([15.0, 0.0, -5.0]));
    assert_eq!(o["orientation"], json!([0.0, 1.0, 0.0, 0.0]));
}

#[tokio::test]
async fn only_owner_region_owner_or_admin_may_update() {
    // L1707 "Only the object's `ownerId`, the region owner, or a
    // deployment-admin may modify another user's object"; L1728 the
    // refusal is `forbidden`. [`BOB`] owns the region.
    let backend = setup();
    let id = create(&backend, sculpture()).await;
    let carol = "user:carol@example.com";
    let err = &update(&backend, carol, &id, json!({ "name": "x" })).await["notUpdated"][&id];
    assert_eq!(err["type"], "forbidden");
    for caller in [ALICE, BOB, ADMIN] {
        let resp = update(&backend, caller, &id, json!({ "name": caller })).await;
        assert!(resp["notUpdated"].is_null(), "{caller}: {resp}");
    }
}

#[tokio::test]
async fn region_id_is_immutable() {
    // L1536 "`regionId` is immutable and cannot be changed via update".
    let backend = setup();
    let id = create(&backend, bare(REGION)).await;
    let err = update_error(&backend, &id, json!({ "regionId": OTHER_REGION })).await;
    assert_invalid(&err, &["regionId"]);
}

#[tokio::test]
async fn update_applies_the_create_validations() {
    // L1701 "Same field-level validations as create: non-finite
    // position/orientation, invalid `physicsMode`, `visualRef`/`visualType`
    // mismatch".
    let backend = setup();
    let id = create(&backend, sculpture()).await;
    let err = update_error(&backend, &id, json!({ "physicsMode": "floating" })).await;
    assert_invalid(&err, &["physicsMode"]);
    let err = update_error(&backend, &id, json!({ "visualRef": null })).await;
    assert_invalid(&err, &["visualRef", "visualType"]);
    let err = update_error(&backend, &id, json!({ "orientation": [1, 1, 0, 0] })).await;
    assert_invalid(&err, &["orientation"]);
    // Clearing both together leaves a consistent invisible object (L485
    // "`null` for invisible objects").
    let resp = update(
        &backend,
        ALICE,
        &id,
        json!({ "visualRef": null, "visualType": null }),
    )
    .await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
}

#[tokio::test]
async fn reparenting_into_a_cycle_is_invalid() {
    // L1652 "Setting `parentId` such that the object is its own ancestor
    // (directly or transitively) is invalid"; L1701 applies it to update.
    let backend = setup();
    let ids = chain(&backend, 3).await;
    let err = update_error(&backend, &ids[0], json!({ "parentId": ids[2] })).await;
    assert_invalid(&err, &["parentId"]);
    let err = update_error(&backend, &ids[1], json!({ "parentId": ids[1] })).await;
    assert_invalid(&err, &["parentId"]);
}

#[tokio::test]
async fn reparenting_counts_the_moved_subtree_depth() {
    // L1701 "excess hierarchy depth"; guide L340 "A depth limit of 16
    // levels". A 14-level chain (deepest at level 14) and a separate
    // 3-level chain: hung under level 13 its levels become 14, 15, 16;
    // hung under level 14 its leaf would be at level 17.
    let backend = setup();
    let long = chain(&backend, 14).await;
    let short = chain(&backend, 3).await;
    let err = update_error(&backend, &short[0], json!({ "parentId": long[13] })).await;
    assert_invalid(&err, &["parentId"]);
    let resp = update(&backend, ALICE, &short[0], json!({ "parentId": long[12] })).await;
    assert!(resp["notUpdated"].is_null(), "{resp}");
}

#[tokio::test]
async fn reparenting_to_unknown_parent_names_parent_id() {
    // L1809 "Same field-level validations as create"; L1768 lists
    // "`parentId` does not reference an existing SceneObject" under create
    // `invalidProperties`.
    let backend = setup();
    let id = create(&backend, bare(REGION)).await;
    let err = update_error(
        &backend,
        &id,
        json!({ "parentId": "01J5OBJNOPE000000000000000" }),
    )
    .await;
    assert_invalid(&err, &["parentId"]);
}

#[tokio::test]
async fn reparenting_into_another_region_is_invalid() {
    // L1651 "Parent-child relationships are region-scoped."
    let backend = setup();
    let id = create(&backend, bare(REGION)).await;
    let other = create(&backend, bare(OTHER_REGION)).await;
    let err = update_error(&backend, &id, json!({ "parentId": other })).await;
    assert_invalid(&err, &["parentId"]);
}

#[tokio::test]
async fn update_of_unknown_object_is_not_found() {
    // L1713 "The object id does not exist" is `notFound`.
    let backend = setup();
    let err = update_error(
        &backend,
        "01J5OBJNOPE000000000000000",
        json!({ "name": "x" }),
    )
    .await;
    assert_eq!(err["type"], "notFound");
}

// ---- destroy --------------------------------------------------------------

#[tokio::test]
async fn destroy_removes_children_recursively() {
    // L1595 "`destroy` removes the SceneObject and all its children
    // (objects whose `parentId` references this object, recursively)";
    // the L1624-1626 response lists only the requested id.
    let backend = setup();
    let ids = chain(&backend, 3).await;
    let sibling = create(&backend, bare(REGION)).await;
    let before = get(&backend, &sibling).await["state"].clone();

    let resp = set(&backend, ALICE, json!({ "destroy": [ids[0]] })).await;
    assert_eq!(resp["destroyed"], json!([ids[0]]));

    let after = call!(
        handle_scene_object_get,
        &backend,
        ALICE,
        json!({ "ids": [ids[0], ids[1], ids[2], sibling] })
    );
    assert_eq!(after["notFound"], json!([ids[0], ids[1], ids[2]]));
    assert_eq!(after["list"][0]["id"], sibling.as_str());

    // RFC 8620 §5.2: every removed object is reported destroyed.
    let changes = call!(
        handle_scene_object_changes,
        &backend,
        ALICE,
        json!({ "sinceState": before })
    );
    let mut destroyed: Vec<&str> = changes["destroyed"]
        .as_array()
        .expect("destroyed")
        .iter()
        .map(|v| v.as_str().expect("id"))
        .collect();
    destroyed.sort_unstable();
    let mut expected: Vec<&str> = ids.iter().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(destroyed, expected);
}

#[tokio::test]
async fn only_owner_region_owner_or_admin_may_destroy() {
    // L1742 "Caller is not the object owner, region owner, or admin | Same
    // permission model as update" is `forbidden`.
    let backend = setup();
    let id = create(&backend, bare(REGION)).await;
    let carol = "user:carol@example.com";
    let resp = set(&backend, carol, json!({ "destroy": [id] })).await;
    assert_eq!(resp["notDestroyed"][&id]["type"], "forbidden");
    let resp = set(&backend, BOB, json!({ "destroy": [id] })).await;
    assert_eq!(resp["destroyed"], json!([id]));
}

#[tokio::test]
async fn destroy_of_unknown_object_is_not_found() {
    // L1748 "The object id does not exist" is `notFound`.
    let backend = setup();
    let resp = set(
        &backend,
        ALICE,
        json!({ "destroy": ["01J5OBJNOPE000000000000000"] }),
    )
    .await;
    assert_eq!(
        resp["notDestroyed"]["01J5OBJNOPE000000000000000"]["type"],
        "notFound"
    );
}
