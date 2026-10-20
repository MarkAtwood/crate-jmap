//! Shared fixtures. Line numbers cite draft-atwood-jmap-scene-00.md at
//! jmap-chat-spec 3227c0f.

#![allow(dead_code)]

use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_types::SceneCapability;
use jmap_types::Id;
use serde_json::{json, Value};

/// The account of the L652-668 create example.
pub const ACCOUNT: &str = "acct-001";
pub const ALICE: &str = "user:alice@example.com";
pub const BOB: &str = "user:bob@example.com";
pub const ADMIN: &str = "user:admin@example.com";

/// The blob of the L656-662 create example, and the `size` the L683
/// response reports for it.
pub const BLOB: &str = "Gc0f032d390a5d5fa8a35";
pub const BLOB_SIZE: u64 = 2_483_712;

/// An account-level capability object, hand-written from L160-181.
pub fn capability() -> Value {
    json!({
        "mayCreateRegion": true,
        "mayCreateObject": true,
        "supportedVisualTypes": ["model/gltf-binary", "model/gltf+json"],
        "maxRegionsPerAccount": null,
        "maxObjectsPerRegion": null,
        "maxAvatarsPerRegion": null,
        "maxAssetSizeBytes": null
    })
}

/// A backend with [`ACCOUNT`] registered under `capability`, holding
/// [`BLOB`], with [`ADMIN`] as an admin.
pub fn backend_with(capability: Value) -> MemoryBackend {
    let backend = MemoryBackend::new();
    let cap: SceneCapability = serde_json::from_value(capability).expect("capability fixture");
    backend.register_account(&Id::from(ACCOUNT), cap);
    backend.add_blob(&Id::from(ACCOUNT), BLOB, BLOB_SIZE);
    backend.add_admin(ADMIN);
    backend
}

/// A backend with [`ACCOUNT`] registered under [`capability`].
pub fn backend() -> MemoryBackend {
    backend_with(capability())
}

/// Run handler `$method` on [`ACCOUNT`] as `$caller` with `$args` (without
/// `accountId`) and return the response.
#[allow(unused_macros)]
macro_rules! call {
    ($method:path, $backend:expr, $caller:expr, $args:expr $(,)?) => {{
        let mut args: serde_json::Value = $args;
        args["accountId"] = serde_json::json!($crate::common::ACCOUNT);
        $method($backend, &$caller.to_owned(), args)
            .await
            .expect("method")
            .primary
    }};
}

/// A stored asset, hand-written from the field list at L214-239.
pub fn stored_asset(id: &str, media_type: &str, name: Value, size: u64, created_at: &str) -> Value {
    json!({
        "id": id,
        "accountId": ACCOUNT,
        "blobId": format!("blob-{id}"),
        "assetUri": null,
        "mediaType": media_type,
        "name": name,
        "size": size,
        "sha256": null,
        "createdAt": created_at
    })
}

/// Seed `asset` (from [`stored_asset`]) into the backend, owned by `owner`.
pub fn seed(backend: &MemoryBackend, owner: &str, asset: Value) {
    let id = asset["id"].as_str().expect("id").to_owned();
    backend.insert_object_for_test("SceneAsset", &Id::from(ACCOUNT), &id, owner, asset);
}
