//! The `supportsSceneAsset` gate on every `SceneAsset/*` method
//! (draft-atwood-jmap-scene-00 L235-236 at jmap-chat-spec 6bcd9d8):
//! "When `false`, a call to any SceneAsset method returns an
//! `unknownMethod` error ({{RFC8620}} Section 3.6.2)."

mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    handle_scene_asset_changes, handle_scene_asset_get, handle_scene_asset_query,
    handle_scene_asset_query_changes, handle_scene_asset_set,
};
use jmap_types::JmapError;
use serde_json::{json, Value};

fn unsupported_backend() -> MemoryBackend {
    let mut cap = capability();
    cap["supportsSceneAsset"] = json!(false);
    backend_with(cap)
}

/// Minimal valid arguments for each method, hand-written from RFC 8620
/// §5.1, §5.2, §5.3, §5.5 and §5.6.
fn args(method: &str, account: &str) -> Value {
    match method {
        "get" => json!({ "accountId": account, "ids": null }),
        "changes" => json!({ "accountId": account, "sinceState": "0" }),
        "set" => json!({ "accountId": account, "destroy": ["A1"] }),
        "query" => json!({ "accountId": account }),
        "queryChanges" => json!({ "accountId": account, "sinceQueryState": "0" }),
        _ => unreachable!("{method}"),
    }
}

async fn run(backend: &MemoryBackend, method: &str, account: &str) -> Result<(), JmapError> {
    let caller = ALICE.to_owned();
    let a = args(method, account);
    match method {
        "get" => handle_scene_asset_get(backend, &caller, a).await.map(drop),
        "changes" => handle_scene_asset_changes(backend, &caller, a)
            .await
            .map(drop),
        "set" => handle_scene_asset_set(backend, &caller, a).await.map(drop),
        "query" => handle_scene_asset_query(backend, &caller, a)
            .await
            .map(drop),
        "queryChanges" => handle_scene_asset_query_changes(backend, &caller, a)
            .await
            .map(drop),
        _ => unreachable!("{method}"),
    }
}

const METHODS: [&str; 5] = ["get", "changes", "set", "query", "queryChanges"];

#[tokio::test]
async fn every_method_is_unknown_when_unsupported() {
    let backend = unsupported_backend();
    for method in METHODS {
        let err = run(&backend, method, ACCOUNT).await.unwrap_err();
        assert_eq!(err.error_type, "unknownMethod", "SceneAsset/{method}");
    }
}

#[tokio::test]
async fn unknown_account_still_account_not_found() {
    // RFC 8620 §3.6.2 "accountNotFound: The accountId does not correspond
    // to a valid account": the per-account gate does not mask it.
    let backend = unsupported_backend();
    for method in METHODS {
        let err = run(&backend, method, "acct-missing").await.unwrap_err();
        assert_eq!(err.error_type, "accountNotFound", "SceneAsset/{method}");
    }
}

#[tokio::test]
async fn supported_account_is_not_gated() {
    let mut cap = capability();
    cap["supportsSceneAsset"] = json!(true);
    let backend = backend_with(cap);
    for method in ["get", "set", "query"] {
        run(&backend, method, ACCOUNT)
            .await
            .unwrap_or_else(|e| panic!("SceneAsset/{method}: {e:?}"));
    }
}
