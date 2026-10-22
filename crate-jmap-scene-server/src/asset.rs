//! SceneAsset/* method handlers (draft-atwood-jmap-scene-00 §SceneAsset
//! Methods).
//!
//! `/get`, `/changes` and `/queryChanges` are the standard RFC 8620 §5.1,
//! §5.2 and §5.6 methods. `/set` applies §SceneAsset/set and its §SetError
//! Conditions: required `blobId` and `mediaType`, the server-set
//! properties (including `ownerId`, the caller's userId), the media-type
//! rule with glTF as the mandatory-to-implement format,
//! `invalidProperties` for a missing blob, `overQuota` above `maxAssetSizeBytes`, the immutable
//! properties on update and the owner-or-admin rule on update and destroy.
//! `/query` supplies the "Default sort: `createdAt` descending" before the
//! standard RFC 8620 §5.5 handling.
//!
//! Every handler first returns `unknownMethod` when the account's
//! [`SceneCapability::supports_scene_asset`] is `false`: "When `false`, a
//! call to any SceneAsset method returns an `unknownMethod` error"
//! (§Account-Level Capability Object).

use jmap_scene_types::SceneAssetComparator;
use jmap_scene_types::{SceneAsset, SceneCapability};
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{json, Map, Value};

use crate::helpers::{
    backend_set_error_value, default_sort, enforce_max_objects_in_set, extract_account_id,
    fetch_one, finalize_set_response, forbidden, invalid, not_found, over_quota,
    reject_unsupported_sort, set_error_value, SetAccumulators,
};
use crate::SceneBackend;
use jmap_server::{
    now_utc_string, server_fail_from_backend, server_fail_value_from_backend, HandlerOutput,
    SetError, SetErrorType,
};

/// The mandatory-to-implement visual asset media type: "The IANA media
/// type `model/gltf-binary` (.glb) MUST be listed in
/// `supportedVisualTypes`" (draft-atwood-jmap-scene-00 §Preferred Asset
/// Format).
///
/// `SceneAsset/set` accepts it even when the backend's
/// [`SceneBackend::scene_capability`] leaves it out.
pub const GLTF_BINARY: &str = "model/gltf-binary";

/// Properties the client may not set on create: "The server sets `id`,
/// `accountId`, `ownerId`, `size`, and `createdAt`."
const SERVER_SET: &[&str] = &["id", "accountId", "ownerId", "size", "createdAt"];

/// Properties an update may not patch: "`id`, `accountId`, `ownerId`,
/// `blobId`, `mediaType`, `size`, and `createdAt` are immutable or
/// server-set."
const IMMUTABLE: &[&str] = &[
    "id",
    "accountId",
    "ownerId",
    "blobId",
    "mediaType",
    "size",
    "createdAt",
];

/// The optional `String|null` properties: "Optional: `name` (String),
/// `assetUri` (String), `sha256` (String)."
const OPTIONAL_STRINGS: &[&str] = &["name", "assetUri", "sha256"];

/// Capability gate shared by the `SceneAsset/*` handlers.
///
/// Returns `unknownMethod` (RFC 8620 §3.6.2) when
/// [`SceneBackend::scene_capability`] reports `supportsSceneAsset: false`
/// for the request's account. A missing, malformed or unknown `accountId`
/// passes through so the handler reports `invalidArguments` or
/// `accountNotFound` itself.
async fn ensure_scene_asset_supported<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: &Value,
) -> Result<(), JmapError> {
    let Some(account_id) = args
        .get("accountId")
        .and_then(Value::as_str)
        .and_then(|s| Id::new_validated(s).ok())
    else {
        return Ok(());
    };
    let exists = backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if !exists {
        return Ok(());
    }
    let cap = backend
        .scene_capability(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if !cap.supports_scene_asset {
        return Err(JmapError::unknown_method());
    }
    Ok(())
}

/// Handle `SceneAsset/get` (draft-atwood-jmap-scene-00 §SceneAsset/get):
/// standard RFC 8620 §5.1 `/get`.
pub async fn handle_scene_asset_get<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_scene_asset_supported(backend, caller, &args).await?;
    jmap_server::handlers::handle_get::<SceneAsset, B>(backend, caller, args).await
}

/// Handle `SceneAsset/changes` (draft-atwood-jmap-scene-00
/// §SceneAsset/changes): standard RFC 8620 §5.2 `/changes`.
pub async fn handle_scene_asset_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_scene_asset_supported(backend, caller, &args).await?;
    jmap_server::handlers::handle_changes::<SceneAsset, B>(backend, caller, args).await
}

/// Handle `SceneAsset/query` (draft-atwood-jmap-scene-00
/// §SceneAsset/query).
///
/// Supplies the "Default sort: `createdAt` descending" and refuses any
/// other sort property with `unsupportedSort`; an unknown filter property
/// is `unsupportedFilter`. The backend evaluates the filter;
/// [`crate::query::asset_matches`] and [`crate::query::compare_assets`]
/// implement it.
pub async fn handle_scene_asset_query<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_scene_asset_supported(backend, caller, &args).await?;
    jmap_server::handlers::handle_query::<SceneAsset, B>(backend, caller, prepare_query(args)?)
        .await
}

/// Handle `SceneAsset/queryChanges` (draft-atwood-jmap-scene-00
/// §SceneAsset/queryChanges). RFC 8620 §5.6: the `filter` and `sort` are
/// those of the original query, so the `/query` rules apply here too.
pub async fn handle_scene_asset_query_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_scene_asset_supported(backend, caller, &args).await?;
    jmap_server::handlers::handle_query_changes::<SceneAsset, B>(
        backend,
        caller,
        prepare_query(args)?,
    )
    .await
}

fn prepare_query(mut args: Value) -> Result<Value, JmapError> {
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "createdAt", "isAscending": false }]),
        );
    }
    reject_unsupported_sort::<SceneAssetComparator>(args.get("sort"))?;
    Ok(args)
}

/// Handle `SceneAsset/set` (draft-atwood-jmap-scene-00 §SceneAsset/set).
///
/// Create validates the arguments, looks the blob up with
/// [`SceneBackend::blob_size`], applies `maxAssetSizeBytes` and hands the
/// asset to [`SceneBackend::create_object`]. Update and destroy require
/// [`SceneBackend::may_modify_asset`].
pub async fn handle_scene_asset_set<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_scene_asset_supported(backend, caller, &args).await?;
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2: an unknown accountId is `accountNotFound`.
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<SceneAsset>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(Value::as_str) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut acc = SetAccumulators::default();
    let mut mutated = false;

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj) in create_map {
            match create_asset(backend, caller, &account_id, &create_id, obj).await {
                Ok(created) => {
                    mutated = true;
                    acc.created.insert(create_id, created);
                }
                Err(e) => {
                    acc.not_created.insert(create_id, e);
                }
            }
        }
    }

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch) in update_map {
            let id = Id::from(id_str.as_str());
            match update_asset(backend, caller, &account_id, &id, patch).await {
                Ok(echo) => {
                    mutated = true;
                    acc.updated.insert(id_str, echo);
                }
                Err(e) => {
                    acc.not_updated.insert(id_str, e);
                }
            }
        }
    }

    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of `destroy` is an Id.
        let ids: Vec<&str> = destroy_arr
            .iter()
            .map(|v| v.as_str().ok_or(v))
            .collect::<Result<_, _>>()
            .map_err(|bad| {
                JmapError::invalid_arguments(format!(
                    "destroy: every element must be a string Id; got {bad}"
                ))
            })?;
        for id_str in ids {
            match destroy_asset(backend, caller, &account_id, &Id::from(id_str)).await {
                Ok(()) => {
                    mutated = true;
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed.insert(id_str.to_owned(), e);
                }
            }
        }
    }

    finalize_set_response::<B, SceneAsset>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// `true` if the server accepts `media_type` for a SceneAsset: "MUST be a
/// value present in the account-level `supportedVisualTypes` or a
/// recognized audio/image type", with [`GLTF_BINARY`] always supported.
///
/// Media type names are case-insensitive (RFC 6838 §4.2).
///
/// ponytail: "recognized audio/image type" is read as any media type whose
/// top-level type is `audio` or `image` (RFC 6838 §4.2.3, §4.2.2), without
/// consulting a registry. A deployment that restricts audio and image
/// subtypes registers its own `SceneAsset/set` handler.
fn media_type_supported(cap: &SceneCapability, media_type: &str) -> bool {
    let listed = |t: &str| t.eq_ignore_ascii_case(media_type);
    let audio_or_image = media_type.split_once('/').is_some_and(|(top, sub)| {
        !sub.is_empty() && (top.eq_ignore_ascii_case("audio") || top.eq_ignore_ascii_case("image"))
    });
    listed(GLTF_BINARY) || cap.supported_visual_types.iter().any(|t| listed(t)) || audio_or_image
}

/// `sha256`: "SHA-256 hash of the asset file, lowercase hexadecimal."
fn is_lowercase_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The offending names among `mediaType` and the optional string
/// properties present in `m`, for an `invalidProperties` SetError.
///
/// A `mediaType` key must hold a string the server supports. The optional
/// properties must be a string or `null`, and `sha256` lowercase hex.
fn malformed<'a>(m: &'a Map<String, Value>, cap: &SceneCapability) -> Vec<&'a str> {
    let mut bad = Vec::new();
    if let Some((key, v)) = m.get_key_value("mediaType") {
        if !v.as_str().is_some_and(|t| media_type_supported(cap, t)) {
            bad.push(key.as_str());
        }
    }
    for (key, v) in m
        .iter()
        .filter(|(k, _)| OPTIONAL_STRINGS.contains(&k.as_str()))
    {
        let ok = match v {
            Value::Null => true,
            Value::String(s) => key != "sha256" || is_lowercase_sha256(s),
            _ => false,
        };
        if !ok {
            bad.push(key.as_str());
        }
    }
    bad
}

async fn capability<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
) -> Result<SceneCapability, Value> {
    backend
        .scene_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))
}

/// One §SceneAsset/set create. The `Ok` value is the `created` entry: the
/// server-set `id`, `accountId`, `ownerId` (the caller's
/// [`SceneBackend::caller_user_id`]), `size` and `createdAt`.
async fn create_asset<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    obj: Value,
) -> Result<Value, Value> {
    let Value::Object(mut m) = obj else {
        return Err(invalid(&[], "a SceneAsset create must be a JSON object"));
    };

    let server_set: Vec<&str> = SERVER_SET
        .iter()
        .copied()
        .filter(|k| m.contains_key(*k))
        .collect();
    if !server_set.is_empty() {
        return Err(invalid(
            &server_set,
            "these properties are set by the server",
        ));
    }

    let cap = capability(backend, caller, account_id).await?;
    // "`invalidProperties` (`properties` lists each offending property) --
    // missing or malformed fields": `blobId` and `mediaType` are required.
    let mut bad = malformed(&m, &cap);
    if !m.contains_key("mediaType") {
        bad.push("mediaType");
    }
    let blob_id = m
        .get("blobId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    if blob_id.is_none() {
        bad.push("blobId");
    }
    let Some(blob_id) = blob_id.map(Id::from).filter(|_| bad.is_empty()) else {
        bad.sort_unstable();
        return Err(invalid(
            &bad,
            "blobId and mediaType are required; mediaType must be supported; \
             name, assetUri and sha256 must be strings or null, sha256 lowercase hex",
        ));
    };

    // "The server MUST reject the create if `blobId` does not reference a
    // valid blob"; the `invalidProperties` table covers that case "whether
    // the blobId is malformed or well-formed but absent".
    let size = backend
        .blob_size(caller, account_id, &blob_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
        .ok_or_else(|| invalid(&["blobId"], "blobId does not reference an uploaded blob"))?;
    // "The server MUST return `overQuota` if the asset exceeds
    // `maxAssetSizeBytes`."
    if cap.max_asset_size_bytes.is_some_and(|max| size > max) {
        return Err(over_quota("the asset exceeds maxAssetSizeBytes"));
    }
    let owner_id = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;

    for key in OPTIONAL_STRINGS {
        m.entry(*key).or_insert(Value::Null);
    }
    // `id` is required by the type, so the parse uses "", which is not an
    // Id (RFC 8620 §1.2: "at least 1 and a maximum of 255 octets"); the
    // `create_object` builder overwrites it with the minted id.
    m.insert("id".to_owned(), json!(""));
    m.insert("accountId".to_owned(), json!(account_id));
    m.insert("ownerId".to_owned(), json!(owner_id));
    m.insert("size".to_owned(), json!(size));
    m.insert("createdAt".to_owned(), json!(now_utc_string()));
    let mut asset: SceneAsset = serde_json::from_value(Value::Object(m))
        .map_err(|e| invalid(&[], &format!("invalid SceneAsset: {e}")))?;
    let (id, stored) = backend
        .create_object(caller, account_id, create_id, move |id: &Id| {
            asset.id = id.clone();
            asset
        })
        .await
        .map_err(backend_set_error_value)?;
    Ok(json!({
        "id": id,
        "accountId": stored.account_id,
        "ownerId": stored.owner_id,
        "size": stored.size,
        "createdAt": stored.created_at,
    }))
}

/// Fetch asset `id` and require [`SceneBackend::may_modify_asset`]: "Caller
/// does not own the asset and lacks admin privileges" is `forbidden`, "The
/// asset id does not exist" is `notFound`.
async fn fetch_modifiable<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<SceneAsset, Value> {
    let asset: SceneAsset = fetch_one(backend, caller, account_id, id)
        .await?
        .ok_or_else(not_found)?;
    if !backend
        .may_modify_asset(caller, account_id, &asset)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(forbidden(
            "only the asset owner or an admin may modify this asset",
        ));
    }
    Ok(asset)
}

/// One §SceneAsset/set update. `Ok` is the `updated` echo.
async fn update_asset<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    fetch_modifiable(backend, caller, account_id, id).await?;

    // "Attempting to change a property other than `name`, `assetUri`, or
    // `sha256`" is `invalidProperties` for each of the [`IMMUTABLE`]
    // properties; vendor extension properties stay patchable.
    let mut fixed: Vec<&str> = patch
        .as_map()
        .keys()
        .map(|k| k.split('/').next().unwrap_or(k))
        .filter(|k| IMMUTABLE.contains(k))
        .collect();
    fixed.sort_unstable();
    fixed.dedup();
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "these properties cannot be changed"));
    }
    let cap = capability(backend, caller, account_id).await?;
    let bad = malformed(patch.as_map(), &cap);
    if !bad.is_empty() {
        return Err(invalid(
            &bad,
            "name, assetUri and sha256 must be strings or null, sha256 lowercase hex",
        ));
    }

    match backend
        .update_object::<SceneAsset>(caller, account_id, id, patch)
        .await
        .map_err(backend_set_error_value)?
    {
        Some(asset) => {
            Ok(serde_json::to_value(asset).expect("derive(Serialize) on plain data is infallible"))
        }
        None => Ok(Value::Null),
    }
}

/// One §SceneAsset/set destroy, under the "Same permission model as
/// update".
async fn destroy_asset<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<(), Value> {
    fetch_modifiable(backend, caller, account_id, id).await?;
    backend
        .destroy_object::<SceneAsset>(caller, account_id, id)
        .await
        .map_err(backend_set_error_value)
}
