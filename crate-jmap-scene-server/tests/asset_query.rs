//! `SceneAsset/query` (draft-atwood-jmap-scene-00 §SceneAsset/query,
//! L800-841 at jmap-chat-spec 3227c0f). Each assertion quotes the draft
//! text it checks.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::handle_scene_asset_query;
use jmap_scene_server::memory::MemoryBackend;
use serde_json::{json, Value};

/// Four assets around the L829-840 example query.
fn backend_with_assets() -> MemoryBackend {
    let backend = backend();
    for asset in [
        // Matches every L832-835 condition.
        stored_asset(
            "S1",
            "model/gltf-binary",
            json!("Sculpture: Convergence"),
            300,
            "2026-04-01T00:00:00Z",
        ),
        // Wrong media type.
        stored_asset(
            "S2",
            "model/gltf+json",
            json!("Sculpture: Convergence"),
            100,
            "2026-04-02T00:00:00Z",
        ),
        // Created exactly at createdBefore.
        stored_asset(
            "S3",
            "model/gltf-binary",
            json!("Sculpture: Late"),
            200,
            "2026-06-01T00:00:00Z",
        ),
        // Created exactly at createdAfter.
        stored_asset(
            "S4",
            "model/gltf-binary",
            json!("Bench"),
            400,
            "2026-03-01T00:00:00Z",
        ),
    ] {
        seed(&backend, ALICE, asset);
    }
    backend
}

async fn query(backend: &MemoryBackend, args: Value) -> Result<Value, jmap_types::JmapError> {
    let mut args = args;
    args["accountId"] = json!(ACCOUNT);
    handle_scene_asset_query(backend, &ALICE.to_owned(), args)
        .await
        .map(|o| o.primary)
}

async fn ids(backend: &MemoryBackend, args: Value) -> Value {
    query(backend, args).await.expect("query")["ids"].clone()
}

#[tokio::test]
async fn spec_example_query() {
    // L829-840, verbatim except accountId. L820 "An asset must match every
    // specified filter property"; L810 `name` is a "Substring match on
    // asset name" (the example's "sculpture" names "Sculpture:
    // Convergence", L640); L815 "created before this timestamp" excludes
    // S3 at 2026-06-01; S4 fails the name.
    let backend = backend_with_assets();
    let got = ids(
        &backend,
        json!({
            "filter": {
                "mediaType": "model/gltf-binary",
                "name": "sculpture",
                "createdAfter": "2026-03-01T00:00:00Z",
                "createdBefore": "2026-06-01T00:00:00Z"
            },
            "sort": [{"property": "createdAt", "isAscending": false}],
            "position": 0,
            "limit": 25
        }),
    )
    .await;
    assert_eq!(got, json!(["S1"]));
}

#[tokio::test]
async fn created_after_is_inclusive() {
    // L812 `createdAfter`: "Assets created at or after this timestamp."
    let backend = backend_with_assets();
    let got = ids(
        &backend,
        json!({ "filter": { "createdAfter": "2026-06-01T00:00:00Z" } }),
    )
    .await;
    assert_eq!(got, json!(["S3"]));
}

#[tokio::test]
async fn default_sort_is_created_at_descending() {
    // L824 "Default sort: `createdAt` descending."
    let backend = backend_with_assets();
    let got = ids(&backend, json!({})).await;
    assert_eq!(got, json!(["S3", "S2", "S1", "S4"]));
}

#[tokio::test]
async fn sort_by_name_and_size() {
    // L824 "Sort properties: `createdAt`, `name`, `size`."
    let backend = backend_with_assets();
    let by_size = ids(&backend, json!({ "sort": [{ "property": "size" }] })).await;
    assert_eq!(by_size, json!(["S2", "S3", "S1", "S4"]));
    let by_name = ids(
        &backend,
        json!({ "sort": [{ "property": "name" }, { "property": "size" }] }),
    )
    .await;
    assert_eq!(by_name, json!(["S4", "S2", "S1", "S3"]));
}

#[tokio::test]
async fn unknown_filter_property_is_unsupported_filter() {
    // L822 "If a client provides a filter property the server does not
    // support, the server MUST return an `unsupportedFilter` error."
    let backend = backend_with_assets();
    let err = query(&backend, json!({ "filter": { "size": 1 } }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type, "unsupportedFilter");
}

#[tokio::test]
async fn unlisted_sort_property_is_unsupported_sort() {
    // L824 lists the sort properties; RFC 8620 §5.5 "unsupportedSort: The
    // "sort" is syntactically valid, but includes a property the server
    // does not support sorting on".
    let backend = backend_with_assets();
    let err = query(&backend, json!({ "sort": [{ "property": "mediaType" }] }))
        .await
        .unwrap_err();
    assert_eq!(err.error_type, "unsupportedSort");
}
