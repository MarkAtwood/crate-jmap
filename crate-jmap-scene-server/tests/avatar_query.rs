//! `SceneAvatar/query` (draft-atwood-jmap-scene-00 §SceneAvatar/query,
//! L2228-2309 at jmap-chat-spec fda85cb). Each assertion quotes the draft
//! text it checks; the expected id lists are the draft's example response
//! or are worked out by hand from the seeded avatars.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{handle_scene_avatar_query, handle_scene_avatar_query_changes};
use jmap_types::JmapError;
use serde_json::{Value, json};

const REGION: &str = "01J5ABC0000000000000000001";
const OTHER_REGION: &str = "01J5ABC0000000000000000002";
/// The two ids of the L2538-2541 response (jmap-chat-spec 6c38e2f).
const BOB_AV: &str = "01J5AVB0000000000000000002";
const CAROL_AV: &str = "01J5AVC0000000000000000003";
/// Active, 40 m from the L2273 center `[10.0, 0, -5.0]`.
const DAVE_AV: &str = "01J5AVD0000000000000000004";
/// At the L2273 center, but has left.
const ERIN_AV: &str = "01J5AVE0000000000000000005";
/// At the L2273 center, but in [`OTHER_REGION`].
const FRANK_AV: &str = "01J5AVF0000000000000000006";

/// A stored avatar, hand-written from the field list at L550-581.
fn avatar(
    id: &str,
    region: &str,
    name: &str,
    position: Value,
    joined_at: &str,
    left_at: Value,
) -> Value {
    json!({
        "id": id,
        "regionId": region,
        "userId": id,
        "displayName": name,
        "position": position,
        "orientation": [0, 0, 0, 1],
        "visualRef": null,
        "visualType": null,
        "joinedAt": joined_at,
        "leftAt": left_at,
        "worldState": null
    })
}

fn backend_with_avatars() -> MemoryBackend {
    let backend = backend();
    for a in [
        // Joined before CAROL, so first by `joinedAt` ascending, as in the
        // L2297-2300 response.
        avatar(
            BOB_AV,
            REGION,
            "Bob",
            json!([20.0, 0, -5.0]),
            "2026-06-06T10:01:00Z",
            json!(null),
        ),
        avatar(
            CAROL_AV,
            REGION,
            "Carol",
            json!([10.0, 0, 5.0]),
            "2026-06-06T10:05:00Z",
            json!(null),
        ),
        avatar(
            DAVE_AV,
            REGION,
            "Dave",
            json!([50.0, 0, -5.0]),
            "2026-06-06T10:00:00Z",
            json!(null),
        ),
        avatar(
            ERIN_AV,
            REGION,
            "Erin",
            json!([10.0, 0, -5.0]),
            "2026-06-06T09:00:00Z",
            json!("2026-06-06T09:30:00Z"),
        ),
        avatar(
            FRANK_AV,
            OTHER_REGION,
            "Frank",
            json!([10.0, 0, -5.0]),
            "2026-06-06T09:00:00Z",
            json!(null),
        ),
    ] {
        seed_as(&backend, "SceneAvatar", a);
    }
    backend
}

async fn query(backend: &MemoryBackend, args: Value) -> Result<Value, JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_scene_avatar_query(backend, &ALICE.to_owned(), args)
        .await
        .map(|o| o.primary)
}

async fn ids(backend: &MemoryBackend, args: Value) -> Value {
    query(backend, args).await.expect("query")["ids"].clone()
}

#[tokio::test]
async fn spec_proximity_example() {
    // The L2268-2281 request and the L2297-2300 ids: "active avatars
    // within 30 meters of a point". DAVE is 40 m away, ERIN has left
    // (L2241 `isActive` `true`: "avatars with `leftAt == null`"), FRANK is
    // in another region.
    let backend = backend_with_avatars();
    let resp = ids(
        &backend,
        json!({
            "filter": {
                "regionId": "01J5ABC0000000000000000001",
                "isActive": true,
                "withinRadius": { "center": [10.0, 0, -5.0], "radius": 30 }
            },
            "sort": [{ "property": "joinedAt", "isAscending": true }],
            "position": 0,
            "limit": 50
        }),
    )
    .await;
    assert_eq!(resp, json!([BOB_AV, CAROL_AV]));
}

#[tokio::test]
async fn default_sort_is_joined_at_ascending() {
    // L2259 "Default sort: `joinedAt` ascending."
    let backend = backend_with_avatars();
    let resp = ids(&backend, json!({ "filter": { "regionId": REGION } })).await;
    assert_eq!(resp, json!([ERIN_AV, DAVE_AV, BOB_AV, CAROL_AV]));
}

#[tokio::test]
async fn display_name_sort() {
    // L2259 "Sort properties: `joinedAt`, `displayName`."
    let backend = backend_with_avatars();
    let resp = ids(
        &backend,
        json!({ "filter": { "regionId": REGION }, "sort": [{ "property": "displayName", "isAscending": false }] }),
    )
    .await;
    assert_eq!(resp, json!([ERIN_AV, DAVE_AV, CAROL_AV, BOB_AV]));
}

#[tokio::test]
async fn departed_and_user_filters() {
    // L2241 "When `false`, filter to avatars who have left"; L2238
    // `userId` "Filter to avatars belonging to this user account"; L2255
    // "All filter properties are combined with logical AND."
    let backend = backend_with_avatars();
    let resp = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "isActive": false } }),
    )
    .await;
    assert_eq!(resp, json!([ERIN_AV]));
    let resp = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "userId": DAVE_AV } }),
    )
    .await;
    assert_eq!(resp, json!([DAVE_AV]));
    let resp = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "userId": FRANK_AV } }),
    )
    .await;
    assert_eq!(resp, json!([]));
}

#[tokio::test]
async fn within_bounds_uses_the_last_known_position() {
    // L2250 `withinBounds` "Same syntax as SceneObject/query"; L1792
    // "Filter to objects whose position falls within an axis-aligned
    // bounding box". Only CAROL is at z > 0.
    let backend = backend_with_avatars();
    let resp = ids(
        &backend,
        json!({ "filter": { "regionId": REGION, "withinBounds": { "min": [0, 0, 0], "max": [100, 10, 10] } } }),
    )
    .await;
    assert_eq!(resp, json!([CAROL_AV]));
}

#[tokio::test]
async fn region_id_is_required() {
    // L2235 "Servers MUST return `invalidArguments` when this property is
    // absent"; L2257 "If `regionId` is absent, the server MUST return
    // `invalidArguments`."
    let backend = backend_with_avatars();
    for args in [
        json!({}),
        json!({ "filter": { "isActive": true } }),
        json!({ "filter": { "operator": "OR", "conditions": [
            { "regionId": REGION },
            { "isActive": true }
        ] } }),
    ] {
        let err = query(&backend, args.clone()).await.expect_err("must fail");
        assert_eq!(err.error_type.to_string(), "invalidArguments", "{args}");
    }
}

#[tokio::test]
async fn non_positive_radius_is_invalid_arguments() {
    // L2257 "If `withinRadius.radius` is zero or negative, the server MUST
    // return `invalidArguments`."
    let backend = backend_with_avatars();
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
async fn sort_outside_joined_at_and_display_name_is_unsupported() {
    // L2259 "Sort properties: `joinedAt`, `displayName`."; RFC 8620 §5.5
    // `unsupportedSort`.
    let backend = backend_with_avatars();
    let err = query(
        &backend,
        json!({ "filter": { "regionId": REGION }, "sort": [{ "property": "leftAt" }] }),
    )
    .await
    .expect_err("must fail");
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

#[tokio::test]
async fn query_changes_applies_the_filter_validation() {
    // RFC 8620 §5.6: `filter` is "The filter argument that was used with
    // /query", so L2257 applies.
    let backend = backend_with_avatars();
    let err = handle_scene_avatar_query_changes(
        &backend,
        &ALICE.to_owned(),
        json!({ "accountId": ACCOUNT, "sinceQueryState": "0", "filter": {} }),
    )
    .await
    .expect_err("must fail");
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}
