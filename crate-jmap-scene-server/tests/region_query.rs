//! `SceneRegion/query` (draft-atwood-jmap-scene-00 §SceneRegion/query,
//! L1247-1340 at jmap-chat-spec 3227c0f). Each assertion quotes the draft
//! text it checks; the expected id lists are worked out by hand from the
//! seeded regions.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{handle_scene_region_query, handle_scene_region_query_changes};
use jmap_types::JmapError;
use serde_json::{Value, json};

/// Four regions, seeded with hand-picked properties:
///
/// | id | name | accessPolicy | viewHint | simulationUri | avatars | createdAt |
/// |---|---|---|---|---|---|---|
/// | r1 | Gallery East Wing | public | 3d | set | 3 | 2026-06-01 |
/// | r2 | Design Review Room | invite | 3d | null | 0 | 2026-06-02 |
/// | r3 | Main Plaza | public | com.example.isometric | set | 7 | 2025-12-31 |
/// | r4 | Gallery West Wing | public | 2d-topdown | null | 0 | 2026-01-01 |
fn backend_with_regions() -> MemoryBackend {
    let backend = backend();
    let rows = [
        (
            "r1",
            "Gallery East Wing",
            "public",
            "3d",
            true,
            3,
            "2026-06-01T10:00:00Z",
        ),
        (
            "r2",
            "Design Review Room",
            "invite",
            "3d",
            false,
            0,
            "2026-06-02T08:00:00Z",
        ),
        (
            "r3",
            "Main Plaza",
            "public",
            "com.example.isometric",
            true,
            7,
            "2025-12-31T23:59:59Z",
        ),
        (
            "r4",
            "Gallery West Wing",
            "public",
            "2d-topdown",
            false,
            0,
            "2026-01-01T00:00:00Z",
        ),
    ];
    for (id, name, policy, hint, sim, avatars, created) in rows {
        let mut r = stored_region(id, name, created);
        r["accessPolicy"] = json!(policy);
        r["viewHint"] = json!(hint);
        r["activeAvatarCount"] = json!(avatars);
        if sim {
            r["simulationUri"] = json!(format!("wss://sim.example.com/regions/{id}"));
        }
        seed_as(&backend, "SceneRegion", r);
    }
    backend
}

async fn query(backend: &MemoryBackend, args: Value) -> Result<Value, JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_scene_region_query(backend, &ALICE.to_owned(), args)
        .await
        .map(|o| o.primary)
}

async fn ids(backend: &MemoryBackend, args: Value) -> Vec<String> {
    let resp = query(backend, args).await.expect("query");
    resp["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().expect("id").to_owned())
        .collect()
}

#[tokio::test]
async fn spec_example_public_by_population() {
    // L1288-1298: `"filter": {"accessPolicy": "public"}`, `"sort":
    // [{"property": "activeAvatarCount", "isAscending": false}]`. Public
    // regions are r1 (3), r3 (7), r4 (0); descending: r3, r1, r4.
    let backend = backend_with_regions();
    let got = ids(
        &backend,
        json!({
            "filter": { "accessPolicy": "public" },
            "sort": [{ "property": "activeAvatarCount", "isAscending": false }],
            "position": 0,
            "limit": 10
        }),
    )
    .await;
    assert_eq!(got, ["r3", "r1", "r4"]);
}

#[tokio::test]
async fn spec_example_public_simulated_after_date() {
    // L1326-1336: public, `"hasSimulationUri": true`, `"createdAfter":
    // "2026-01-01T00:00:00Z"`. r1 and r3 are public with a simulationUri;
    // r3 was created 2025-12-31, so only r1 remains.
    let backend = backend_with_regions();
    let got = ids(
        &backend,
        json!({
            "filter": {
                "accessPolicy": "public",
                "hasSimulationUri": true,
                "createdAfter": "2026-01-01T00:00:00Z"
            },
            "sort": [{ "property": "createdAt", "isAscending": false }],
            "position": 0,
            "limit": 50
        }),
    )
    .await;
    assert_eq!(got, ["r1"]);
}

#[tokio::test]
async fn default_sort_is_created_at_descending() {
    // L1280 "Default sort: `createdAt` descending." By createdAt: r2
    // (06-02), r1 (06-01), r4 (01-01), r3 (2025-12-31).
    let backend = backend_with_regions();
    assert_eq!(ids(&backend, json!({})).await, ["r2", "r1", "r4", "r3"]);
}

#[tokio::test]
async fn name_sort_ascending() {
    // L1280 "Sort properties: `createdAt`, `name`, `activeAvatarCount`."
    let backend = backend_with_regions();
    let got = ids(&backend, json!({ "sort": [{ "property": "name" }] })).await;
    assert_eq!(got, ["r2", "r1", "r4", "r3"]);
}

#[tokio::test]
async fn name_is_a_substring_match() {
    // L1254 "Substring match on region name": "gallery" is in r1 and r4.
    let backend = backend_with_regions();
    let got = ids(&backend, json!({ "filter": { "name": "gallery" } })).await;
    assert_eq!(got, ["r1", "r4"]);
}

#[tokio::test]
async fn view_hint_is_an_exact_match_including_reverse_domain() {
    // L1260 "Exact match on the `viewHint` field ... deployment-specific
    // values in reverse-domain notation are also valid filter values."
    let backend = backend_with_regions();
    let got = ids(&backend, json!({ "filter": { "viewHint": "3d" } })).await;
    assert_eq!(got, ["r2", "r1"]);
    let got = ids(
        &backend,
        json!({ "filter": { "viewHint": "com.example.isometric" } }),
    )
    .await;
    assert_eq!(got, ["r3"]);
}

#[tokio::test]
async fn has_simulation_uri_false_selects_null() {
    // L1263 "When `false`, filter to regions where `simulationUri` is
    // null": r2 and r4.
    let backend = backend_with_regions();
    let got = ids(&backend, json!({ "filter": { "hasSimulationUri": false } })).await;
    assert_eq!(got, ["r2", "r4"]);
}

#[tokio::test]
async fn has_active_avatars() {
    // L1266 "When `true`, filter to regions with `activeAvatarCount > 0`.
    // When `false`, filter to empty regions."
    let backend = backend_with_regions();
    let got = ids(&backend, json!({ "filter": { "hasActiveAvatars": true } })).await;
    assert_eq!(got, ["r1", "r3"]);
    let got = ids(&backend, json!({ "filter": { "hasActiveAvatars": false } })).await;
    assert_eq!(got, ["r2", "r4"]);
}

#[tokio::test]
async fn created_after_is_inclusive_created_before_exclusive() {
    // L1269 createdAfter "Regions created at or after this timestamp";
    // L1272 createdBefore "Regions created before this timestamp". r4 was
    // created exactly at 2026-01-01T00:00:00Z.
    let backend = backend_with_regions();
    let got = ids(
        &backend,
        json!({ "filter": { "createdAfter": "2026-01-01T00:00:00Z" } }),
    )
    .await;
    assert_eq!(got, ["r2", "r1", "r4"]);
    let got = ids(
        &backend,
        json!({ "filter": { "createdBefore": "2026-01-01T00:00:00Z" } }),
    )
    .await;
    assert_eq!(got, ["r3"]);
}

#[tokio::test]
async fn unknown_access_policy_filter_is_invalid_arguments() {
    // L1278 "If a client provides an invalid value for `accessPolicy` (not
    // one of the defined values), the server MUST return
    // `invalidArguments`."
    let backend = backend_with_regions();
    for filter in [
        json!({ "accessPolicy": "friends" }),
        json!({ "operator": "OR", "conditions": [
            { "name": "Plaza" },
            { "accessPolicy": "friends" }
        ] }),
    ] {
        let err = query(&backend, json!({ "filter": filter }))
            .await
            .unwrap_err();
        assert_eq!(err.error_type.to_string(), "invalidArguments");
    }
}

#[tokio::test]
async fn unknown_filter_property_is_unsupported_filter() {
    // L1278 "If a client provides a filter property the server does not
    // support, the server MUST return an `unsupportedFilter` error."
    let backend = backend_with_regions();
    let err = query(&backend, json!({ "filter": { "ownerId": "x" } }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.to_string(), "unsupportedFilter");
}

#[tokio::test]
async fn unlisted_sort_property_is_unsupported_sort() {
    // L1280 lists the sort properties; RFC 8620 §5.5 "unsupportedSort: The
    // "sort" is syntactically valid, but includes a property the server
    // does not support sorting on".
    let backend = backend_with_regions();
    let err = query(&backend, json!({ "sort": [{ "property": "updatedAt" }] }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type.to_string(), "unsupportedSort");
}

#[tokio::test]
async fn query_changes_applies_the_filter_rules() {
    // L1341 "Standard JMAP `/queryChanges`"; RFC 8620 §5.6: the `filter`
    // is "The filter argument that was used with Foo/query", so the
    // L1278 `invalidArguments` rule applies.
    let backend = backend_with_regions();
    let err = handle_scene_region_query_changes(
        &backend,
        &ALICE.to_owned(),
        json!({
            "accountId": ACCOUNT,
            "sinceQueryState": "0",
            "filter": { "accessPolicy": "friends" }
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type.to_string(), "invalidArguments");
}
