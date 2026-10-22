//! SceneAvatar/* method handlers (draft-atwood-jmap-scene-00 §SceneAvatar
//! Methods).
//!
//! `/get`, `/changes` and `/queryChanges` are the standard RFC 8620 §5.1,
//! §5.2 and §5.6 methods. `/set` applies §Entering a Region, §Updating
//! Avatar State, §Leaving a Region, §Re-entering a Region and §SetError
//! Conditions: create enters a region, an update that sets `leftAt`
//! leaves it (or, by a region owner or administrator, ejects another
//! user), an update that clears `leftAt` re-enters it, and destroy is
//! always `forbidden`. The one-record-per-region, auto-eject,
//! `maxAvatarsPerRegion` and `activeAvatarCount` rules are applied
//! atomically by [`SceneBackend::create_object`] and
//! [`SceneBackend::update_object`]. `/query` supplies the "Default sort:
//! `joinedAt` ascending" and the §Filter Validation `invalidArguments`
//! checks.

use jmap_scene_types::SceneAvatarComparator;
use jmap_scene_types::{SceneAvatar, SceneRegion};
use jmap_types::{Id, JmapError, PatchObject};
use serde_json::{json, Map, Value};

use crate::helpers::{
    backend_set_error_value, check_region_filter, default_sort, enforce_max_objects_in_set,
    extract_account_id, fetch_one, finalize_set_response, finite3, forbidden, handle_get_visible,
    invalid, normalized_quaternion, not_found, orientation_ok, reject_unsupported_sort,
    set_error_value, SetAccumulators,
};
use crate::SceneBackend;
use jmap_server::{
    apply_patch_object, now_utc_string, server_fail_from_backend, server_fail_value_from_backend,
    HandlerOutput, SetError, SetErrorType,
};

/// "The server sets `id`, `userId` (to the caller's userId), `position`
/// (to the region's `spawnPosition`), `orientation` (to the region's
/// `spawnOrientation`), `joinedAt`, and `leftAt` (to `null`)." A client
/// "MUST omit any properties that may only be set by the server" (RFC 8620
/// §5.3). `userId` is checked against the caller instead (`forbidden`), so
/// it is not listed.
const SERVER_SET_ON_CREATE: &[&str] = &["id", "position", "orientation", "joinedAt", "leftAt"];

/// Properties no update may patch: `id` and `regionId` are immutable,
/// `userId` and `joinedAt` server-set.
const FIXED_ON_UPDATE: &[&str] = &["id", "regionId", "userId", "joinedAt"];

/// What a region owner or administrator may patch on another user's
/// avatar: "An eject update patches only `leftAt`"
/// (draft-atwood-jmap-scene-00 §Avatar Permissions).
const EJECT_PROPERTIES: &[&str] = &["leftAt"];

/// Handle `SceneAvatar/get` (draft-atwood-jmap-scene-00 §SceneAvatar/get):
/// standard RFC 8620 §5.1 `/get`, with an avatar in a region the caller
/// has no access to ([`SceneBackend::may_access_region`]), of a user the
/// caller blocked ([`SceneBackend::is_user_blocked`]) or in a region whose
/// avatars the caller may not see ([`SceneBackend::may_see_region_avatars`],
/// a region bound to an E2EE call) reported as `notFound` (§Region Access,
/// §Blocked-Contact Spatial Presence, §Security Considerations).
pub async fn handle_scene_avatar_get<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    handle_get_visible::<SceneAvatar, B>(backend, caller, args).await
}

/// Handle `SceneAvatar/changes` (draft-atwood-jmap-scene-00
/// §SceneAvatar/changes): standard RFC 8620 §5.2 `/changes`.
pub async fn handle_scene_avatar_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_changes::<SceneAvatar, B>(backend, caller, args).await
}

/// Handle `SceneAvatar/query` (draft-atwood-jmap-scene-00
/// §SceneAvatar/query).
///
/// Supplies the "Default sort: `joinedAt` ascending" and refuses any other
/// sort property with `unsupportedSort`. Returns `invalidArguments` when
/// `regionId` is absent, when `withinRadius.radius` is zero or negative,
/// and when a `withinBounds.max` component is below `min`. An unknown
/// filter property is `unsupportedFilter`. The backend evaluates the
/// filter; [`crate::query::avatar_matches`] and
/// [`crate::query::compare_avatars`] implement it.
pub async fn handle_scene_avatar_query<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_query::<SceneAvatar, B>(backend, caller, prepare_query(args)?)
        .await
}

/// Handle `SceneAvatar/queryChanges` (draft-atwood-jmap-scene-00
/// §SceneAvatar/queryChanges). RFC 8620 §5.6: the `filter` and `sort` are
/// those of the original query, so the `/query` rules apply here too.
pub async fn handle_scene_avatar_query_changes<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handlers::handle_query_changes::<SceneAvatar, B>(
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
            json!([{ "property": "joinedAt", "isAscending": true }]),
        );
    }
    reject_unsupported_sort::<SceneAvatarComparator>(args.get("sort"))?;
    match args.get("filter") {
        None | Some(Value::Null) => {
            Err(JmapError::invalid_arguments("filter.regionId is required"))
        }
        Some(filter) => check_region_filter(filter, &|_| Ok(())),
    }?;
    Ok(args)
}

/// Handle `SceneAvatar/set` (draft-atwood-jmap-scene-00 §SceneAvatar/set).
///
/// Create enters a region: it validates the request, requires an
/// existing region the caller has access to
/// ([`SceneBackend::may_access_region`], else `invalidProperties` naming
/// `regionId`) and [`SceneBackend::may_enter_region`], fills the server-set properties
/// and hands the avatar to [`SceneBackend::create_object`]. The `created`
/// entry is the stored avatar, as in the draft's example response. Update
/// of an avatar in a region the caller has no access to
/// ([`SceneBackend::may_access_region`]) is `notFound`; otherwise it is
/// limited to the caller's own avatar, except that
/// [`SceneBackend::may_eject_avatar`] lets a region owner or administrator
/// set `leftAt` on another's. An update that sets `leftAt` to `null` on a
/// departed avatar re-enters its region: [`SceneBackend::may_enter_region`]
/// `false` is `forbidden`, and the server sets `joinedAt`, `position` and
/// `orientation`. A create or update that sets a
/// `displayName` from [`SceneBackend::reserved_display_names`] is
/// `forbidden` (§Avatar Identity and Impersonation). Destroy is always
/// `forbidden`.
pub async fn handle_scene_avatar_set<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
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
        .get_state::<SceneAvatar>(caller, &account_id)
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
            match enter_region(backend, caller, &account_id, &create_id, obj).await {
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
            match update_avatar(backend, caller, &account_id, &id, patch).await {
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
        // "Servers MUST return `forbidden` for destroy operations on
        // SceneAvatar."
        for id_str in ids {
            acc.not_destroyed.insert(
                id_str.to_owned(),
                forbidden("SceneAvatar records are not destroyed; set leftAt to leave"),
            );
        }
    }

    finalize_set_response::<B, SceneAvatar>(backend, caller, &account_id, old_state, mutated, acc)
        .await
}

/// `true` if `s` has the RFC 8620 §1.4 UTCDate shape
/// `YYYY-MM-DDTHH:MM:SS[.fraction]Z`. Field ranges are not checked.
fn utc_date_shaped(s: &str) -> bool {
    let b = s.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let at = |i: usize, c: u8| b.get(i) == Some(&c);
    let fraction = match b.get(19..b.len().saturating_sub(1)) {
        Some([]) => true,
        Some([b'.', rest @ ..]) => !rest.is_empty() && rest.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    digits(0..4)
        && at(4, b'-')
        && digits(5..7)
        && at(7, b'-')
        && digits(8..10)
        && at(10, b'T')
        && digits(11..13)
        && at(13, b':')
        && digits(14..16)
        && at(16, b':')
        && digits(17..19)
        && fraction
        && s.ends_with('Z')
}

/// The properties of avatar `m` that break an `invalidProperties` row
/// that needs no backend fact: `regionId` missing, a non-finite
/// `position`, an `orientation` that is not a unit quaternion, `visualRef`
/// without `visualType` or the reverse, and a property of the wrong JSON
/// type.
fn malformed(m: &Map<String, Value>) -> Vec<&'static str> {
    let mut bad = Vec::new();
    let mut check = |key: &'static str, ok: bool| {
        if !ok {
            bad.push(key);
        }
    };
    let optional = |key: &str, ok: fn(&Value) -> bool| m.get(key).is_none_or(ok);

    check("regionId", m.get("regionId").is_some_and(Value::is_string));
    check("position", optional("position", finite3));
    check("orientation", optional("orientation", orientation_ok));
    check("displayName", optional("displayName", Value::is_string));
    for key in ["visualRef", "visualType"] {
        check(key, optional(key, |v| v.is_null() || v.is_string()));
    }
    check(
        "worldState",
        optional("worldState", |v| v.is_null() || v.is_object()),
    );
    // "`visualRef` is present but `visualType` is absent (or vice versa)
    // | Both must be present together or both absent."
    let present = |key: &str| m.get(key).is_some_and(|v| !v.is_null());
    if present("visualRef") != present("visualType") {
        bad.extend(["visualRef", "visualType"]);
    }
    bad
}

/// Refuse `bad` (from [`malformed`]) with one `invalidProperties`.
fn refuse_malformed(mut bad: Vec<&'static str>) -> Result<(), Value> {
    if bad.is_empty() {
        return Ok(());
    }
    bad.sort_unstable();
    bad.dedup();
    Err(invalid(
        &bad,
        "regionId is required, position finite, orientation a unit quaternion, visualRef and \
         visualType together",
    ))
}

/// Refuse a `displayName` in `m` that the backend reserves: the server
/// "MUST reject `SceneAvatar/set` operations that attempt to use a
/// reserved name with a `SetError` of type `forbidden`."
async fn refuse_reserved_name<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    m: &Map<String, Value>,
) -> Result<(), Value> {
    let Some(name) = m.get("displayName").and_then(Value::as_str) else {
        return Ok(());
    };
    let reserved = backend
        .reserved_display_names(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if reserved.iter().any(|r| r == name) {
        return Err(forbidden("This displayName is reserved."));
    }
    Ok(())
}

fn present_in<'a>(keys: impl Iterator<Item = &'a str>, set: &[&str]) -> Vec<&'a str> {
    let mut found: Vec<&str> = keys.filter(|k| set.contains(k)).collect();
    found.sort_unstable();
    found.dedup();
    found
}

/// One §Entering a Region. The `Ok` value is the `created` entry: the
/// stored avatar.
async fn enter_region<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    create_id: &str,
    obj: Value,
) -> Result<Value, Value> {
    let Value::Object(mut m) = obj else {
        return Err(invalid(&[], "a SceneAvatar create must be a JSON object"));
    };

    let fixed = present_in(m.keys().map(String::as_str), SERVER_SET_ON_CREATE);
    if !fixed.is_empty() {
        return Err(invalid(&fixed, "these properties are set by the server"));
    }
    let user_id = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    // "Caller is creating an avatar record for a different user": forbidden.
    if m.get("userId")
        .is_some_and(|u| u.as_str() != Some(&user_id))
    {
        return Err(forbidden("Users MUST only create their own avatar."));
    }
    refuse_malformed(malformed(&m))?;

    // "The server MUST return `invalidProperties` naming `regionId` when
    // `regionId` does not reference an existing SceneRegion. The server
    // SHOULD return the same error when the caller has no access to the
    // region under its `accessPolicy`".
    let region_id = Id::from(m["regionId"].as_str().unwrap_or_default());
    let inaccessible = || invalid(&["regionId"], "regionId is not an accessible SceneRegion");
    let region: SceneRegion = fetch_one(backend, caller, account_id, &region_id)
        .await?
        .ok_or_else(inaccessible)?;
    if !backend
        .may_access_region(caller, account_id, &region_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(inaccessible());
    }
    // "User is banned or ejected with a cooldown period still active":
    // `forbidden`.
    if !backend
        .may_enter_region(caller, account_id, &region)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(forbidden("The caller may not enter this region."));
    }
    refuse_reserved_name(backend, caller, account_id, &m).await?;

    if !m.contains_key("displayName") {
        let name = backend
            .avatar_display_name(caller, account_id, &user_id)
            .await
            .map_err(|e| server_fail_value_from_backend(&e))?;
        m.insert("displayName".to_owned(), json!(name));
    }
    // "The server sets ... `position` (to the region's `spawnPosition`),
    // `orientation` (to the region's `spawnOrientation`)".
    m.insert("position".to_owned(), json!(region.spawn_position));
    m.insert("orientation".to_owned(), json!(region.spawn_orientation));
    if let Some(q) = normalized_quaternion(&m, "orientation") {
        m.insert("orientation".to_owned(), q);
    }
    for key in ["visualRef", "visualType", "worldState"] {
        m.entry(key).or_insert(Value::Null);
    }
    // `id` is required by the type, so the parse uses "", which is not an
    // Id (RFC 8620 §1.2: "at least 1 and a maximum of 255 octets"); the
    // `create_object` builder overwrites it with the minted id.
    m.insert("id".to_owned(), json!(""));
    m.insert("userId".to_owned(), json!(user_id));
    m.insert("joinedAt".to_owned(), json!(now_utc_string()));
    m.insert("leftAt".to_owned(), Value::Null);
    let mut avatar: SceneAvatar = serde_json::from_value(Value::Object(m))
        .map_err(|e| invalid(&[], &format!("invalid SceneAvatar: {e}")))?;
    let (_, stored) = backend
        .create_object(caller, account_id, create_id, move |id: &Id| {
            avatar.id = id.clone();
            avatar
        })
        .await
        .map_err(backend_set_error_value)?;
    Ok(serde_json::to_value(stored).expect("derive(Serialize) on plain data is infallible"))
}

/// One §Updating Avatar State, §Leaving a Region or §Re-entering a Region.
/// `Ok` is the `updated` echo of the properties the server set (RFC 8620
/// §5.3: "any properties that changed in a way not explicitly requested by
/// the PatchObject"): `joinedAt`, `position` and `orientation` on
/// re-entry, the server-set `leftAt` on leave or eject, a normalized
/// `orientation` when the patch set one, else `null`.
async fn update_avatar<B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
    patch: Value,
) -> Result<Value, Value> {
    let mut patch: PatchObject = serde_json::from_value(patch).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    // "The avatar id does not exist or caller lacks access to the
    // containing region": `notFound`.
    let avatar: SceneAvatar = fetch_one(backend, caller, account_id, id)
        .await?
        .ok_or_else(not_found)?;
    if !backend
        .may_access_region(caller, account_id, &avatar.region_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?
    {
        return Err(not_found());
    }

    let top = |k: &str| k.split('/').next().unwrap_or(k).to_owned();
    let patched: Vec<String> = patch.as_map().keys().map(|k| top(k)).collect();
    let touched = |k: &str| patched.iter().any(|p| p == k);
    let left_to = patch.as_map().get("leftAt").cloned();

    let user_id = backend
        .caller_user_id(caller, account_id)
        .await
        .map_err(|e| server_fail_value_from_backend(&e))?;
    if avatar.user_id != user_id {
        // "Users MUST NOT update other users' avatars. Exception: region
        // owners and administrators may set `leftAt` on another user's
        // avatar to eject them."
        // Re-entry (`leftAt: null`) is not an eject.
        let eject = left_to.as_ref().is_some_and(|v| !v.is_null())
            && patched.iter().all(|p| EJECT_PROPERTIES.contains(&&**p));
        if !eject
            || !backend
                .may_eject_avatar(caller, account_id, &avatar)
                .await
                .map_err(|e| server_fail_value_from_backend(&e))?
        {
            return Err(forbidden("Users MUST NOT update other users' avatars."));
        }
    }

    let fixed = present_in(patched.iter().map(String::as_str), FIXED_ON_UPDATE);
    if !fixed.is_empty() {
        return Err(invalid(
            &fixed,
            "these properties are immutable or set by the server",
        ));
    }
    let mut echo = Map::new();
    if left_to.as_ref().is_some_and(Value::is_null) {
        // §Re-entering a Region: "A user who has left (has a non-null
        // `leftAt`) re-enters the same region by updating their existing
        // SceneAvatar record to set `leftAt` to `null`."
        if avatar.left_at.is_none() {
            return Err(invalid(
                &["leftAt"],
                "the avatar is present; only a departed avatar re-enters",
            ));
        }
        // "The server sets `joinedAt` to the current time, `position` to
        // the region's `spawnPosition`, and `orientation` to the region's
        // `spawnOrientation`."
        let spawn = present_in(
            patched.iter().map(String::as_str),
            &["position", "orientation"],
        );
        if !spawn.is_empty() {
            return Err(invalid(
                &spawn,
                "re-entry sets position and orientation from the region's spawn point",
            ));
        }
        // The caller passed `may_access_region` above, so lost access is
        // already `notFound`, as is a region that no longer exists. A
        // ban or cooldown is `forbidden`, as for a create.
        let region: SceneRegion = fetch_one(backend, caller, account_id, &avatar.region_id)
            .await?
            .ok_or_else(not_found)?;
        if !backend
            .may_enter_region(caller, account_id, &region)
            .await
            .map_err(|e| server_fail_value_from_backend(&e))?
        {
            return Err(forbidden("The caller may not enter this region."));
        }
        let mut set = Map::new();
        set.insert("joinedAt".to_owned(), json!(now_utc_string()));
        set.insert("position".to_owned(), json!(region.spawn_position));
        set.insert("orientation".to_owned(), json!(region.spawn_orientation));
        if let Some(q) = normalized_quaternion(&set, "orientation") {
            set.insert("orientation".to_owned(), q);
        }
        patch.as_map_mut().extend(set.clone());
        echo = set;
    } else if touched("leftAt") {
        // "The server MUST return `invalidProperties` naming `leftAt` if
        // the avatar has already left (`leftAt` is non-null)."
        if avatar.left_at.is_some() {
            return Err(invalid(&["leftAt"], "the avatar has already left"));
        }
        if !left_to
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(utc_date_shaped)
        {
            return Err(invalid(&["leftAt"], "leftAt must be a UTCDate or null"));
        }
        // §Leaving a Region: "The server records its own current time as
        // `leftAt`; the value the client supplies only signals the intent
        // to leave." RFC 8620 §5.3 echoes a value "changed in a way not
        // explicitly requested by the client".
        let now = json!(now_utc_string());
        patch.as_map_mut().insert("leftAt".to_owned(), now.clone());
        echo.insert("leftAt".to_owned(), now);
    }

    let Value::Object(mut result) =
        serde_json::to_value(&avatar).expect("derive(Serialize) on plain data is infallible")
    else {
        unreachable!("a SceneAvatar serializes as a JSON object")
    };
    apply_patch_object(&mut result, patch.as_map()).map_err(|e| {
        set_error_value(&SetError::new(SetErrorType::InvalidPatch).with_description(e.to_string()))
    })?;
    refuse_malformed(malformed(&result))?;
    if touched("displayName") {
        refuse_reserved_name(backend, caller, account_id, &result).await?;
    }

    if touched("orientation") {
        if let Some(q) = normalized_quaternion(&result, "orientation") {
            let map = patch.as_map_mut();
            map.retain(|k, _| top(k) != "orientation");
            map.insert("orientation".to_owned(), q.clone());
            echo.insert("orientation".to_owned(), q);
        }
    }
    match backend
        .update_object::<SceneAvatar>(caller, account_id, id, patch)
        .await
        .map_err(backend_set_error_value)?
    {
        Some(avatar) => Ok(
            serde_json::to_value(avatar).expect("derive(Serialize) on plain data is infallible")
        ),
        None if echo.is_empty() => Ok(Value::Null),
        None => Ok(Value::Object(echo)),
    }
}

#[cfg(test)]
mod tests {
    use super::utc_date_shaped;

    #[test]
    fn utc_date_shape() {
        // Oracle: RFC 8620 §1.4 UTCDate: a Date "where the "time-offset"
        // component MUST be "Z""; draft L2089 "2026-06-05T15:45:00Z".
        assert!(utc_date_shaped("2026-06-05T15:45:00Z"));
        assert!(utc_date_shaped("2026-06-05T15:45:00.123Z"));
        for bad in [
            "",
            "2026-06-05T15:45:00",
            "2026-06-05T15:45:00+00:00",
            "2026-06-05 15:45:00Z",
            "2026-06-05T15:45:00.Z",
            "yesterday",
        ] {
            assert!(!utc_date_shaped(bad), "{bad}");
        }
    }
}
