//! `SceneAsset/set` (draft-atwood-jmap-scene-00 §SceneAsset/set, L616-798
//! at jmap-chat-spec 3227c0f). Each assertion quotes the draft text it
//! checks; the request and response bodies are the draft's examples.

#[macro_use]
mod common;

use common::*;
use jmap_scene_server::memory::MemoryBackend;
use jmap_scene_server::{
    handle_scene_asset_changes, handle_scene_asset_get, handle_scene_asset_set,
};
use jmap_types::Id;
use serde_json::{json, Value};

/// The L657-663 create, verbatim.
fn spec_create() -> Value {
    json!({
        "blobId": "Gc0f032d390a5d5fa8a35",
        "mediaType": "model/gltf-binary",
        "name": "Sculpture: Convergence",
        "assetUri": "https://cdn.example.com/assets/sculpture-001.glb",
        "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    })
}

async fn set(backend: &MemoryBackend, caller: &str, args: Value) -> Value {
    call!(handle_scene_asset_set, backend, caller, args)
}

/// The `notCreated` entry for one create, which must fail.
async fn create_error(backend: &MemoryBackend, create: Value) -> Value {
    let resp = set(backend, ALICE, json!({ "create": { "a0": create } })).await;
    assert!(resp["created"].is_null(), "create must fail: {resp}");
    resp["notCreated"]["a0"].clone()
}

/// The `created` entry for one create by [`ALICE`], which must succeed.
async fn create(backend: &MemoryBackend, create: Value) -> Value {
    let resp = set(backend, ALICE, json!({ "create": { "a0": create } })).await;
    assert!(resp["notCreated"].is_null(), "create failed: {resp}");
    resp["created"]["a0"].clone()
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

#[tokio::test]
async fn create_returns_the_server_set_properties() {
    // 6bcd9d8 L696 "The server sets `id`, `accountId`, `ownerId`, `size`,
    // and `createdAt`"; the L752-758 `created` entry carries exactly those
    // five, with the blob's size (L756 "size": 2483712) and the request's
    // account. L275-276 `ownerId`: "The userId of the user who created
    // this asset."
    let backend = backend();
    let created = create(&backend, spec_create()).await;
    let mut keys: Vec<&str> = created
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["accountId", "createdAt", "id", "ownerId", "size"]);
    assert_eq!(created["accountId"], json!("acct-001"));
    assert_eq!(created["ownerId"], json!(ALICE));
    assert_eq!(created["size"], json!(2483712));

    // L215 `id`: "A ULID assigned at creation": 26 Crockford base32 chars.
    let id = created["id"].as_str().unwrap();
    assert_eq!(id.len(), 26);
    assert!(id
        .bytes()
        .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b)));
}

#[tokio::test]
async fn created_asset_keeps_the_client_values() {
    // L620-628: the create's `blobId`, `mediaType`, `name`, `assetUri` and
    // `sha256` are stored as given.
    let backend = backend();
    let id = create(&backend, spec_create()).await["id"].clone();
    let got = call!(
        handle_scene_asset_get,
        &backend,
        ALICE,
        json!({ "ids": [id] })
    );
    let asset = &got["list"][0];
    for (key, value) in spec_create().as_object().unwrap() {
        assert_eq!(&asset[key], value, "{key}");
    }
}

#[tokio::test]
async fn create_advances_the_state() {
    // L677-678: "oldState": "state-asset-5", "newState": "state-asset-6".
    let backend = backend();
    let resp = set(
        &backend,
        ALICE,
        json!({ "create": { "a0": spec_create() } }),
    )
    .await;
    assert_ne!(resp["oldState"], resp["newState"]);

    // RFC 8620 §5.2: the create shows up in /changes since oldState.
    let changes = call!(
        handle_scene_asset_changes,
        &backend,
        ALICE,
        json!({ "sinceState": resp["oldState"] })
    );
    assert_eq!(changes["created"], json!([resp["created"]["a0"]["id"]]));
}

#[tokio::test]
async fn missing_required_properties_are_invalid() {
    // L743 "`blobId` is missing", L744 "`mediaType` is missing":
    // `invalidProperties` with "`properties` lists each offending property".
    let backend = backend();
    for (missing, expect) in [
        (vec!["blobId"], vec!["blobId"]),
        (vec!["mediaType"], vec!["mediaType"]),
        (vec!["blobId", "mediaType"], vec!["blobId", "mediaType"]),
    ] {
        let mut req = spec_create();
        for key in &missing {
            req.as_object_mut().unwrap().remove(*key);
        }
        let err = create_error(&backend, req).await;
        assert_eq!(err["type"], json!("invalidProperties"), "{missing:?}");
        assert_eq!(properties(&err), expect);
    }
}

#[tokio::test]
async fn server_set_properties_are_refused_on_create() {
    // 6bcd9d8 L696 "The server sets `id`, `accountId`, `ownerId`, `size`,
    // and `createdAt`."
    let backend = backend();
    let mut req = spec_create();
    req["ownerId"] = json!(BOB);
    req["size"] = json!(1);
    req["createdAt"] = json!("2026-06-06T10:05:00Z");
    let err = create_error(&backend, req).await;
    assert_eq!(err["type"], json!("invalidProperties"));
    assert_eq!(properties(&err), ["createdAt", "ownerId", "size"]);
}

#[tokio::test]
async fn unsupported_media_type_is_invalid() {
    // L745 "`mediaType` is not in `supportedVisualTypes` and is not a
    // recognized audio/image type".
    let backend = backend();
    let mut req = spec_create();
    req["mediaType"] = json!("model/vnd.usdz+zip");
    let err = create_error(&backend, req).await;
    assert_eq!(err["type"], json!("invalidProperties"));
    assert_eq!(properties(&err), ["mediaType"]);
}

#[tokio::test]
async fn deployment_listed_media_type_is_accepted() {
    // L253 "Assets in deployment-specific formats ... are valid within
    // deployments that advertise those media types in
    // `supportedVisualTypes`".
    let mut cap = capability();
    cap["supportedVisualTypes"] = json!(["model/gltf-binary", "model/vnd.usdz+zip"]);
    let backend = backend_with(cap);
    let mut req = spec_create();
    req["mediaType"] = json!("model/vnd.usdz+zip");
    create(&backend, req).await;
}

#[tokio::test]
async fn audio_and_image_types_are_accepted() {
    // L626 "MUST be a value present in the account-level
    // `supportedVisualTypes` or a recognized audio/image type"; L227 gives
    // `"image/png"` and `"audio/mpeg"` as asset media types. Neither is in
    // the fixture's `supportedVisualTypes`.
    let backend = backend();
    for media_type in ["image/png", "audio/mpeg"] {
        let mut req = spec_create();
        req["mediaType"] = json!(media_type);
        create(&backend, req).await;
    }
}

#[tokio::test]
async fn gltf_binary_is_mandatory_to_implement() {
    // L243 "glTF 2.0 ... is the preferred and mandatory-to-implement visual
    // asset format. The IANA media type `model/gltf-binary` (.glb) MUST be
    // listed in `supportedVisualTypes`": the server accepts it even when
    // the deployment's list leaves it out.
    let mut cap = capability();
    cap["supportedVisualTypes"] = json!(["model/gltf+json"]);
    let backend = backend_with(cap);
    create(&backend, spec_create()).await;
}

#[tokio::test]
async fn malformed_sha256_is_invalid() {
    // L236 `sha256`: "SHA-256 hash of the asset file, lowercase
    // hexadecimal"; L739 `invalidProperties` covers "malformed fields".
    let backend = backend();
    for bad in [
        json!("E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855"),
        json!("e3b0c442"),
        json!(42),
    ] {
        let mut req = spec_create();
        req["sha256"] = bad.clone();
        let err = create_error(&backend, req).await;
        assert_eq!(err["type"], json!("invalidProperties"), "{bad}");
        assert_eq!(properties(&err), ["sha256"]);
    }
}

#[tokio::test]
async fn unknown_blob_is_not_found() {
    // L747-751 "`notFound` -- referenced blob does not exist": "`blobId`
    // does not reference a valid blob".
    let backend = backend();
    let mut req = spec_create();
    req["blobId"] = json!("G-never-uploaded");
    let err = create_error(&backend, req).await;
    assert_eq!(err["type"], json!("notFound"));
}

#[tokio::test]
async fn asset_over_max_asset_size_is_over_quota() {
    // L646 "The server MUST return `overQuota` if the asset exceeds
    // `maxAssetSizeBytes`"; L763 "The blob's size exceeds the per-asset
    // size limit".
    let mut cap = capability();
    cap["maxAssetSizeBytes"] = json!(BLOB_SIZE - 1);
    let backend = backend_with(cap);
    let err = create_error(&backend, spec_create()).await;
    assert_eq!(err["type"], json!("overQuota"));
}

#[tokio::test]
async fn asset_at_max_asset_size_is_accepted() {
    // L646 "exceeds `maxAssetSizeBytes`": a blob of exactly the limit does
    // not exceed it.
    let mut cap = capability();
    cap["maxAssetSizeBytes"] = json!(BLOB_SIZE);
    let backend = backend_with(cap);
    create(&backend, spec_create()).await;
}

#[tokio::test]
async fn account_storage_quota_is_over_quota() {
    // L764 "Account asset storage quota exceeded | Deployment-defined total
    // storage limits across all assets."
    let backend = backend();
    backend.set_storage_quota(&Id::from(ACCOUNT), BLOB_SIZE + BLOB_SIZE / 2);
    create(&backend, spec_create()).await;
    let err = create_error(&backend, spec_create()).await;
    assert_eq!(err["type"], json!("overQuota"));
}

/// [`ALICE`]'s asset `S1`.
fn seeded() -> MemoryBackend {
    let backend = backend();
    seed(
        &backend,
        stored_asset(
            "S1",
            "model/gltf-binary",
            json!("Sculpture"),
            10,
            "2026-06-06T10:05:00Z",
        ),
    );
    backend
}

async fn update(backend: &MemoryBackend, caller: &str, patch: Value) -> Result<Value, Value> {
    let resp = set(backend, caller, json!({ "update": { "S1": patch } })).await;
    match resp["notUpdated"].get("S1") {
        Some(err) => Err(err.clone()),
        None => Ok(resp["updated"]["S1"].clone()),
    }
}

#[tokio::test]
async fn immutable_properties_are_refused_on_update() {
    // L772 "Attempting to change immutable fields (`blobId`, `id`,
    // `createdAt`) | These fields are immutable after creation."
    let backend = seeded();
    let err = update(
        &backend,
        ALICE,
        json!({ "blobId": "Gother", "id": "S2", "createdAt": "2026-01-01T00:00:00Z" }),
    )
    .await
    .unwrap_err();
    assert_eq!(err["type"], json!("invalidProperties"));
    assert_eq!(properties(&err), ["blobId", "createdAt", "id"]);
}

#[tokio::test]
async fn only_name_asset_uri_and_sha256_are_patchable() {
    // 6bcd9d8 L844 "Attempting to change a property other than `name`,
    // `assetUri`, or `sha256` | `id`, `accountId`, `ownerId`, `blobId`,
    // `mediaType`, `size`, and `createdAt` are immutable or server-set."
    let backend = seeded();
    let err = update(
        &backend,
        ALICE,
        json!({
            "accountId": "acct-002",
            "ownerId": BOB,
            "mediaType": "model/gltf+json",
            "size": 1
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(err["type"], json!("invalidProperties"));
    assert_eq!(
        properties(&err),
        ["accountId", "mediaType", "ownerId", "size"]
    );

    // L698 "`update` supports patching `name`, `assetUri`, and `sha256`."
    update(
        &backend,
        ALICE,
        json!({
            "name": "Renamed",
            "assetUri": "https://cdn.example.com/assets/renamed.glb",
            "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        }),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn created_asset_is_owned_by_its_creator() {
    // 6bcd9d8 L275-276 `ownerId`: "The userId of the user who created this
    // asset"; L850 "Caller is not the asset's `ownerId` and lacks admin
    // privileges" is `forbidden`.
    let backend = backend();
    let resp = set(&backend, BOB, json!({ "create": { "a0": spec_create() } })).await;
    let id = resp["created"]["a0"]["id"].as_str().unwrap().to_owned();
    assert_eq!(resp["created"]["a0"]["ownerId"], json!(BOB));
    let patch = json!({ "update": { id.clone(): { "name": "x" } } });
    let resp = set(&backend, ALICE, patch.clone()).await;
    assert_eq!(resp["notUpdated"][&id]["type"], json!("forbidden"));
    let resp = set(&backend, BOB, patch).await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
}

#[tokio::test]
async fn owner_and_admin_may_update_metadata() {
    // L778 "Only the asset owner or an admin may modify asset metadata."
    let backend = seeded();
    update(&backend, ALICE, json!({ "name": "Renamed" }))
        .await
        .unwrap();
    update(&backend, ADMIN, json!({ "name": "Renamed again" }))
        .await
        .unwrap();
    let got = call!(
        handle_scene_asset_get,
        &backend,
        ALICE,
        json!({ "ids": ["S1"] })
    );
    assert_eq!(got["list"][0]["name"], json!("Renamed again"));
}

#[tokio::test]
async fn non_owner_update_is_forbidden() {
    // L778 "Caller does not own the asset and lacks admin privileges" is
    // `forbidden`.
    let backend = seeded();
    let err = update(&backend, BOB, json!({ "name": "Mine now" }))
        .await
        .unwrap_err();
    assert_eq!(err["type"], json!("forbidden"));
}

#[tokio::test]
async fn update_of_unknown_asset_is_not_found() {
    // L784 "The asset id does not exist | No SceneAsset with that id is
    // known."
    let backend = backend();
    let err = update(&backend, ALICE, json!({ "name": "x" }))
        .await
        .unwrap_err();
    assert_eq!(err["type"], json!("notFound"));
}

#[tokio::test]
async fn update_to_unsupported_media_type_is_invalid() {
    // 6bcd9d8 L284 `mediaType` "(String, immutable)"; L844 lists it among
    // the properties an update may not change, supported or not.
    let backend = seeded();
    let err = update(
        &backend,
        ALICE,
        json!({ "mediaType": "model/vnd.usdz+zip" }),
    )
    .await
    .unwrap_err();
    assert_eq!(err["type"], json!("invalidProperties"));
    assert_eq!(properties(&err), ["mediaType"]);
}

#[tokio::test]
async fn destroy_example_destroys_the_asset() {
    // L705-707 destroy request and L724-726 `"destroyed": [...]` response.
    let backend = backend();
    let id = create(&backend, spec_create()).await["id"].clone();
    let resp = set(&backend, ALICE, json!({ "destroy": [id] })).await;
    assert_eq!(resp["destroyed"], json!([id]));
    assert!(resp["notDestroyed"].is_null());
    let got = call!(
        handle_scene_asset_get,
        &backend,
        ALICE,
        json!({ "ids": [id] })
    );
    assert_eq!(got["notFound"], json!([id]));
}

#[tokio::test]
async fn non_owner_destroy_is_forbidden() {
    // L792 "Caller does not own the asset and lacks admin privileges |
    // Same permission model as update."
    let backend = seeded();
    let resp = set(&backend, BOB, json!({ "destroy": ["S1"] })).await;
    assert_eq!(resp["notDestroyed"]["S1"]["type"], json!("forbidden"));

    let resp = set(&backend, ADMIN, json!({ "destroy": ["S1"] })).await;
    assert_eq!(resp["destroyed"], json!(["S1"]));
}

#[tokio::test]
async fn destroy_of_unknown_asset_is_not_found() {
    // L798 "The asset id does not exist".
    let backend = backend();
    let resp = set(&backend, ALICE, json!({ "destroy": ["S9"] })).await;
    assert_eq!(resp["notDestroyed"]["S9"]["type"], json!("notFound"));
}
