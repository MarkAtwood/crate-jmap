//! A misspelt FilterOperator key or FilterCondition property is
//! `unsupportedFilter`, never an empty, match-all condition (bd:JMAP-6azm).
//!
//! Oracle: RFC 8620 §5.5 — a FilterOperator object has the properties
//! `operator` and `conditions`, and "`unsupportedFilter`: The filter is
//! syntactically valid, but the server cannot process it";
//! draft-atwood-jmap-scene-00 "If a client provides a filter property the
//! server does not support, the server MUST return an `unsupportedFilter`
//! error." Condition property names: §SceneAsset/query (`mediaType`),
//! §SceneRegion/query (`name`), §SceneObject/query and §SceneAvatar/query
//! (`regionId`, `parentId`, `userId`). The SceneAsset and SceneRegion
//! misspelt-property cases live in `asset_query.rs` and `region_query.rs`.

mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    handle_scene_asset_query, handle_scene_asset_query_changes, handle_scene_avatar_query,
    handle_scene_avatar_query_changes, handle_scene_object_query,
    handle_scene_object_query_changes, handle_scene_region_query,
    handle_scene_region_query_changes,
};
use jmap_server::HandlerOutput;
use jmap_types::JmapError;
use serde_json::{json, Value};

const REGION: &str = "01J5ABC0000000000000000001";

type Handler = for<'a> fn(
    &'a MemoryBackend,
    &'a String,
    Value,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<HandlerOutput, JmapError>> + 'a>,
>;

/// `(method, handler, spec condition)`, each `/query` followed by its
/// `/queryChanges`.
fn cases() -> [(&'static str, Handler, Value); 8] {
    let region = json!({ "regionId": REGION });
    [
        (
            "SceneAsset/query",
            |b, c, a| Box::pin(handle_scene_asset_query(b, c, a)),
            json!({ "mediaType": "model/gltf-binary" }),
        ),
        (
            "SceneAsset/queryChanges",
            |b, c, a| Box::pin(handle_scene_asset_query_changes(b, c, a)),
            json!({ "mediaType": "model/gltf-binary" }),
        ),
        (
            "SceneRegion/query",
            |b, c, a| Box::pin(handle_scene_region_query(b, c, a)),
            json!({ "name": "Plaza" }),
        ),
        (
            "SceneRegion/queryChanges",
            |b, c, a| Box::pin(handle_scene_region_query_changes(b, c, a)),
            json!({ "name": "Plaza" }),
        ),
        (
            "SceneObject/query",
            |b, c, a| Box::pin(handle_scene_object_query(b, c, a)),
            region.clone(),
        ),
        (
            "SceneObject/queryChanges",
            |b, c, a| Box::pin(handle_scene_object_query_changes(b, c, a)),
            region.clone(),
        ),
        (
            "SceneAvatar/query",
            |b, c, a| Box::pin(handle_scene_avatar_query(b, c, a)),
            region.clone(),
        ),
        (
            "SceneAvatar/queryChanges",
            |b, c, a| Box::pin(handle_scene_avatar_query_changes(b, c, a)),
            region,
        ),
    ]
}

async fn run(handler: Handler, filter: Value) -> Result<HandlerOutput, JmapError> {
    let backend = backend();
    let args = json!({ "accountId": ACCOUNT, "sinceQueryState": "0", "filter": filter });
    handler(&backend, &ALICE.to_owned(), args).await
}

fn assert_unsupported(method: &str, filter: &Value, result: Result<HandlerOutput, JmapError>) {
    let err = result.expect_err("a misspelt filter key must be refused");
    assert_eq!(
        err.error_type.to_string(),
        "unsupportedFilter",
        "{method} filter {filter}: {err:?}"
    );
}

#[tokio::test]
async fn misspelt_operator_is_unsupported_filter() {
    for (method, handler, condition) in cases() {
        let filter = json!({ "opperator": "AND", "conditions": [condition] });
        assert_unsupported(method, &filter, run(handler, filter.clone()).await);
    }
}

/// A misspelt property next to the required `regionId` is refused too.
#[tokio::test]
async fn misspelt_condition_property_is_unsupported_filter() {
    for (method, handler, _) in &cases()[4..] {
        let typo = if method.starts_with("SceneObject") {
            "parentID"
        } else {
            "userID"
        };
        let filter = json!({ "regionId": REGION, typo: "x" });
        assert_unsupported(method, &filter, run(*handler, filter.clone()).await);
    }
}

/// Negative control: each spec condition alone is accepted by `/query`,
/// so the refusals above come from the misspelt key alone.
#[tokio::test]
async fn spec_condition_succeeds() {
    for (method, handler, condition) in cases().into_iter().step_by(2) {
        run(handler, condition)
            .await
            .unwrap_or_else(|e| panic!("{method}: {e:?}"));
    }
}
