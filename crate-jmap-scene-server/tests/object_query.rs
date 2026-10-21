//! `SceneObject/query` (draft-atwood-jmap-scene-00 §SceneObject/query,
//! L1750-1897 at jmap-chat-spec 3227c0f). Each assertion quotes the draft
//! text it checks; the expected id lists are the draft's example
//! responses or are worked out by hand from the seeded objects.

#[macro_use]
mod common;

use std::f64::consts::FRAC_1_SQRT_2;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{handle_scene_object_query, handle_scene_object_query_changes};
use jmap_types::JmapError;
use serde_json::{json, Value};

const REGION: &str = "01J5ABC0000000000000000001";
const OTHER_REGION: &str = "01J5ABC0000000000000000002";
const O1: &str = "01J5OBJ0000000000000000001";
const O2: &str = "01J5OBJ0000000000000000002";
/// Interactable, 21 m from the L1817 center `[5.0, 0, -2.0]`.
const FAR: &str = "01J5OBJ0000000000000000003";
/// Interactable, at the L1817 center, but in [`OTHER_REGION`].
const ELSEWHERE: &str = "01J5OBJ0000000000000000004";

/// The two objects of the L1379-1420 `/get` response, verbatim, plus
/// [`FAR`] and [`ELSEWHERE`].
fn backend_with_objects() -> MemoryBackend {
    let backend = backend();
    seed_as(
        &backend,
        "SceneObject",
        json!({
            "id": "01J5OBJ0000000000000000001",
            "regionId": "01J5ABC0000000000000000001",
            "parentId": null,
            "name": "Sculpture: Convergence",
            "position": [12.5, 0, -3.2],
            "orientation": [0, 0.707, 0, 0.707],
            "scale": [1, 1, 1],
            "visualRef": "blob-gltf-sculpture-001",
            "visualType": "model/gltf-binary",
            "assetUri": "https://cdn.example.com/assets/sculpture-001.glb",
            "physicsMode": "static",
            "interactable": true,
            "visible": true,
            "ownerId": "user:curator@example.com",
            "createdAt": "2026-06-01T12:00:00Z",
            "updatedAt": "2026-06-01T12:00:00Z",
            "worldState": {
                "artist": "Elena Voss",
                "year": 2025,
                "medium": "Bronze"
            }
        }),
    );
    seed_as(
        &backend,
        "SceneObject",
        json!({
            "id": "01J5OBJ0000000000000000002",
            "regionId": "01J5ABC0000000000000000001",
            "parentId": null,
            "name": "Info Kiosk",
            "position": [0, 0, 8],
            "orientation": [0, 0, 0, 1],
            "scale": [0.8, 0.8, 0.8],
            "visualRef": "blob-gltf-kiosk-001",
            "visualType": "model/gltf-binary",
            "assetUri": "https://cdn.example.com/assets/kiosk-001.glb",
            "physicsMode": "static",
            "interactable": true,
            "visible": true,
            "ownerId": "user:curator@example.com",
            "createdAt": "2026-06-01T12:30:00Z",
            "updatedAt": "2026-06-03T09:15:00Z",
            "worldState": null
        }),
    );
    let mut far = stored_object(FAR, REGION, json!([26, 0, -2]), "2026-06-01T11:00:00Z");
    far["interactable"] = json!(true);
    far["name"] = json!("Bench");
    far["physicsMode"] = json!("dynamic");
    seed_as(&backend, "SceneObject", far);
    let mut elsewhere = stored_object(
        ELSEWHERE,
        OTHER_REGION,
        json!([5, 0, -2]),
        "2026-06-01T09:00:00Z",
    );
    elsewhere["interactable"] = json!(true);
    seed_as(&backend, "SceneObject", elsewhere);
    backend
}

async fn query(backend: &MemoryBackend, args: Value) -> Result<Value, JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_scene_object_query(backend, &ALICE.to_owned(), args)
        .await
        .map(|o| o.primary)
}

async fn ids(backend: &MemoryBackend, args: Value) -> Value {
    query(backend, args).await.expect("query")["ids"].clone()
}

/// The draft's example responses carry `"total": 2` although their
/// requests omit `calculateTotal`; RFC 8620 §5.5 returns `total` "only if
/// requested". Check both: absent for the verbatim request `args`, 2 when
/// it sets `calculateTotal`.
async fn assert_total(backend: &MemoryBackend, mut args: Value, resp: &Value) {
    assert!(resp.get("total").is_none(), "{resp}");
    args["calculateTotal"] = json!(true);
    let resp = query(backend, args).await.expect("query");
    assert_eq!(resp["total"], 2);
}

#[tokio::test]
async fn spec_within_radius_example() {
    // L1810-1825 request; L1840-1844 response: "ids":
    // ["01J5OBJ0000000000000000002", "01J5OBJ0000000000000000001"],
    // "total": 2. [`FAR`] is 21 m from the center, [`ELSEWHERE`] is in
    // another region (L1798 "An object must match every specified filter
    // property").
    let backend = backend_with_objects();
    let args = json!({
        "filter": {
            "regionId": "01J5ABC0000000000000000001",
            "interactable": true,
            "withinRadius": {
                "center": [5.0, 0, -2.0],
                "radius": 20
            }
        },
        "sort": [
            {"property": "name", "isAscending": true}
        ],
        "position": 0,
        "limit": 25
    });
    let resp = query(&backend, args.clone()).await.expect("query");
    assert_eq!(resp["ids"], json!([O2, O1]));
    assert_total(&backend, args, &resp).await;
}

#[tokio::test]
async fn spec_within_bounds_example() {
    // L1856-1871 request; L1885-1889 response: "ids":
    // ["01J5OBJ0000000000000000001", "01J5OBJ0000000000000000002"],
    // "total": 2. [`FAR`] has x = 26, outside `max` x = 15.
    let backend = backend_with_objects();
    let args = json!({
        "filter": {
            "regionId": "01J5ABC0000000000000000001",
            "withinBounds": {
                "min": [-15, 0, -15],
                "max": [15, 10, 15]
            }
        },
        "sort": [
            {"property": "createdAt", "isAscending": true}
        ],
        "position": 0,
        "limit": 50
    });
    let resp = query(&backend, args.clone()).await.expect("query");
    assert_eq!(resp["ids"], json!([O1, O2]));
    assert_total(&backend, args, &resp).await;
}

#[tokio::test]
async fn default_sort_is_created_at_ascending() {
    // L1802 "Default sort: `createdAt` ascending." createdAt: FAR 11:00,
    // O1 12:00, O2 12:30.
    let backend = backend_with_objects();
    let got = ids(&backend, json!({ "filter": { "regionId": REGION } })).await;
    assert_eq!(got, json!([FAR, O1, O2]));
}

#[tokio::test]
async fn property_filters_are_anded() {
    // L1798 "All filter properties are combined with logical AND"; L1777
    // physicsMode, L1765 visualType "Exact match", L1762 name search.
    let backend = backend_with_objects();
    let got = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "physicsMode": "dynamic" } }),
    )
    .await;
    assert_eq!(got, json!([FAR]));
    let got = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "visualType": "model/gltf-binary", "name": "kiosk" } }),
    )
    .await;
    assert_eq!(got, json!([O2]));
}

/// A parent P at `[10, 0, 0]`, rotated +90 degrees about Y and scaled 2,
/// with child C at `[1, 0, 0]` and C's child G at `[0, 0, 1]`, all in
/// [`REGION`].
///
/// Hand-computed with L470 "relative to the parent's transform" and the
/// right-handed, Y-up convention of L129-130 (a +90 degree turn about Y
/// maps +x to -z and +z to +x):
/// - C: `2 * [1, 0, 0]` = `[2, 0, 0]`, turned to `[0, 0, -2]`, plus P's
///   position: `[10, 0, -2]`.
/// - G: `[0, 0, 1]` plus C's position is `[1, 0, 1]` in P's frame; scaled
///   `[2, 0, 2]`, turned `[2, 0, -2]`, plus P's position: `[12, 0, -2]`.
fn backend_with_graph() -> MemoryBackend {
    let backend = backend();
    let mut p = stored_object("P", REGION, json!([10, 0, 0]), "2026-06-01T10:00:00Z");
    p["orientation"] = json!([0, FRAC_1_SQRT_2, 0, FRAC_1_SQRT_2]);
    p["scale"] = json!([2, 2, 2]);
    let mut c = stored_object("C", REGION, json!([1, 0, 0]), "2026-06-01T10:01:00Z");
    c["parentId"] = json!("P");
    let mut g = stored_object("G", REGION, json!([0, 0, 1]), "2026-06-01T10:02:00Z");
    g["parentId"] = json!("C");
    for o in [p, c, g] {
        seed_as(&backend, "SceneObject", o);
    }
    backend
}

#[tokio::test]
async fn spatial_filters_use_region_local_positions() {
    // L1788 "`center` (Number[3], required): Center point `[x, y, z]` in
    // region-local coordinates"; L1792 "objects whose position falls
    // within an axis-aligned bounding box". Expected positions: see
    // [`backend_with_graph`].
    let backend = backend_with_graph();
    let near = |center: [f64; 3]| json!({ "filter": { "regionId": REGION, "withinRadius": { "center": center, "radius": 0.001 } } });
    assert_eq!(ids(&backend, near([10.0, 0.0, -2.0])).await, json!(["C"]));
    assert_eq!(ids(&backend, near([12.0, 0.0, -2.0])).await, json!(["G"]));
    assert_eq!(ids(&backend, near([10.0, 0.0, 0.0])).await, json!(["P"]));
    // C's parent-relative `[1, 0, 0]` is not a region-local position.
    assert_eq!(ids(&backend, near([1.0, 0.0, 0.0])).await, json!([]));

    let got = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "withinBounds": { "min": [9, -1, -3], "max": [11, 1, -1] } } }),
    )
    .await;
    assert_eq!(got, json!(["C"]));
}

#[tokio::test]
async fn parent_id_filter_selects_roots_or_children() {
    // L1780 "When set to a valid Id, returns only objects whose `parentId`
    // matches. When set to `null`, returns only root-level objects".
    let backend = backend_with_graph();
    let roots = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "parentId": null } }),
    )
    .await;
    assert_eq!(roots, json!(["P"]));
    let kids = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "parentId": "C" } }),
    )
    .await;
    assert_eq!(kids, json!(["G"]));
}

#[tokio::test]
async fn region_id_is_required() {
    // L1758 "Servers MUST return `invalidArguments` when this property is
    // absent"; L1800 "If `regionId` is absent, the server MUST return
    // `invalidArguments`."
    let backend = backend_with_objects();
    for args in [
        json!({}),
        json!({ "filter": { "interactable": true } }),
        json!({ "filter": { "operator": "AND", "conditions": [
            { "regionId": REGION },
            { "visible": true }
        ] } }),
    ] {
        let err = query(&backend, args.clone()).await.expect_err("must fail");
        assert_eq!(err.error_type.to_string(), "invalidArguments", "{args}");
    }
}

#[tokio::test]
async fn unknown_physics_mode_filter_is_invalid_arguments() {
    // L1800 "If `physicsMode` contains a value not in the defined set,
    // the server MUST return `invalidArguments`."
    let backend = backend_with_objects();
    let err = query(
        &backend,
        json!({ "filter": { "regionId": REGION, "physicsMode": "floating" } }),
    )
    .await
    .expect_err("must fail");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

#[tokio::test]
async fn non_positive_radius_is_invalid_arguments() {
    // L1789 "Radius in meters. MUST be positive."; L1800 "If
    // `withinRadius.radius` is zero or negative, the server MUST return
    // `invalidArguments`."
    let backend = backend_with_objects();
    for radius in [json!(0), json!(-5)] {
        let err = query(
            &backend,
            json!({ "filter": { "regionId": REGION, "withinRadius": { "center": [0, 0, 0], "radius": radius } } }),
        )
        .await
        .expect_err("must fail");
        assert_eq!(err.error_type.to_string(), "invalidArguments", "{radius}");
    }
}

#[tokio::test]
async fn inverted_bounds_are_invalid_arguments() {
    // L1794 "Each component MUST be greater than or equal to the
    // corresponding component of `min`."
    let backend = backend_with_objects();
    let err = query(
        &backend,
        json!({ "filter": { "regionId": REGION, "withinBounds": { "min": [0, 5, 0], "max": [10, 1, 10] } } }),
    )
    .await
    .expect_err("must fail");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}

#[tokio::test]
async fn sort_outside_created_at_and_name_is_unsupported() {
    // L1802 "Sort properties: `createdAt`, `name`."; RFC 8620 §5.5
    // `unsupportedSort`.
    let backend = backend_with_objects();
    let err = query(
        &backend,
        json!({ "filter": { "regionId": REGION }, "sort": [{ "property": "position" }] }),
    )
    .await
    .expect_err("must fail");
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

#[tokio::test]
async fn query_changes_applies_the_filter_validation() {
    // RFC 8620 §5.6: `/queryChanges` takes "The filter argument that was
    // used with Foo/query", so L1800's `regionId` rule applies.
    let backend = backend_with_objects();
    let err = handle_scene_object_query_changes(
        &backend,
        &ALICE.to_owned(),
        json!({ "accountId": ACCOUNT, "sinceQueryState": "0", "filter": {} }),
    )
    .await
    .expect_err("must fail");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}
