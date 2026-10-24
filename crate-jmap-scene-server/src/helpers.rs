//! Private helper utilities for the handler modules.

use jmap_types::{GetObject, Id, JmapError, JmapObject, State, UTCDate};
use serde_json::{Map, Value, json};

use jmap_scene_types::{SceneAvatar, SceneObject, SceneRegion};

use crate::SceneBackend;
use jmap_server::{
    HandlerOutput, enforce_max_objects_in_get, not_found_json, optional_arg, serialize_value,
    server_fail_from_backend, server_fail_value_from_backend,
};
use jmap_server::{SetError, SetErrorType};

pub(crate) use jmap_server::{enforce_max_objects_in_set, extract_account_id};

/// The values of `v` if it is an array of exactly `n` Numbers.
pub(crate) fn numbers(v: &Value, n: usize) -> Option<Vec<f64>> {
    let a = v.as_array().filter(|a| a.len() == n)?;
    a.iter().map(Value::as_f64).collect()
}

/// ponytail: "Servers MAY reject quaternions whose magnitude deviates from
/// 1.0 by more than a deployment-defined epsilon"; for SceneObject and
/// SceneAvatar `orientation` and SceneRegion `spawnOrientation` this one
/// is 1e-3, which accepts the draft's
/// own three-decimal examples (L1461 `[0, 0.707, 0, 0.707]`, magnitude
/// 0.99985; L1488 `[0, 0.383, 0, 0.924]`, 1.00023). Accepted values are
/// normalized (see [`normalized_quaternion`]).
const ORIENTATION_EPSILON: f64 = 1e-3;

/// `v` is an array of exactly 3 finite Numbers: "Position components MUST
/// be finite Numbers", "Scale factors must be finite."
pub(crate) fn finite3(v: &Value) -> bool {
    numbers(v, 3).is_some_and(|n| n.iter().all(|c| c.is_finite()))
}

/// The magnitude of `v` if it is a quaternion: 4 Numbers.
fn magnitude(v: &Value) -> Option<f64> {
    numbers(v, 4).map(|q| q.iter().map(|c| c * c).sum::<f64>().sqrt())
}

/// L1759 "Components must be finite and the magnitude non-zero; a server
/// that rejects non-unit values also rejects a magnitude outside its
/// epsilon"; this server rejects them.
pub(crate) fn orientation_ok(v: &Value) -> bool {
    magnitude(v).is_some_and(|n| (n - 1.0).abs() <= ORIENTATION_EPSILON)
}

/// The unit-length form of the accepted quaternion `m[key]` when it
/// differs from the stored value: "If the server does not reject the
/// value, it MUST normalize the quaternion to unit length before storage."
pub(crate) fn normalized_quaternion(m: &Map<String, Value>, key: &str) -> Option<Value> {
    let v = m.get(key)?;
    let n = magnitude(v)?;
    if n == 1.0 {
        return None;
    }
    let q: Vec<f64> = numbers(v, 4)?.iter().map(|c| c / n).collect();
    Some(json!(q))
}

/// The `/query` §Filter Validation shared by SceneObject and SceneAvatar,
/// in every condition of a FilterOperator tree, then `condition` on each
/// condition.
///
/// ponytail: "If `regionId` is absent, the server MUST return
/// `invalidArguments`" is applied to every condition, and an operator
/// with no conditions counts as absent, so no filter tree can reach past
/// the regions it names. Also `invalidArguments` for "`withinRadius.radius`
/// is zero or negative" and for a `withinBounds.max` component below `min`
/// (§Spatial Filters: "Each component MUST be greater than or equal to the
/// corresponding component of `min`").
pub(crate) fn check_region_filter(
    filter: &Value,
    condition: &dyn Fn(&Value) -> Result<(), JmapError>,
) -> Result<(), JmapError> {
    if let Some(conditions) = filter.get("conditions") {
        return match conditions.as_array() {
            Some(c) if !c.is_empty() => {
                c.iter().try_for_each(|f| check_region_filter(f, condition))
            }
            _ => Err(JmapError::invalid_arguments(
                "filter.regionId is required in every condition",
            )),
        };
    }
    if filter.get("regionId").is_none_or(Value::is_null) {
        return Err(JmapError::invalid_arguments("filter.regionId is required"));
    }
    if let Some(r) = filter
        .pointer("/withinRadius/radius")
        .and_then(Value::as_f64)
        && r <= 0.0
    {
        return Err(JmapError::invalid_arguments(
            "filter withinRadius.radius must be positive",
        ));
    }
    if let Some(b) = filter.get("withinBounds") {
        let corner = |k| b.get(k).and_then(|c| numbers(c, 3));
        if let (Some(min), Some(max)) = (corner("min"), corner("max"))
            && min.iter().zip(&max).any(|(lo, hi)| hi < lo)
        {
            return Err(JmapError::invalid_arguments(
                "filter withinBounds.max must be >= withinBounds.min on every axis",
            ));
        }
    }
    condition(filter)
}

/// Resolve an Id-typed property value that may be a `#creationId`
/// reference (RFC 8620 §5.3) through [`jmap_server::resolve_creation_ref`].
///
/// A literal id is returned unchanged; its validation stays with the
/// caller.
///
/// # Errors
///
/// A boxed `invalidProperties` [`jmap_server::SetError`] naming `property` when
/// the reference matches no record created earlier in the request.
pub(crate) fn resolve_ref(
    raw: &str,
    property: &str,
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<jmap_types::Id, Box<jmap_server::SetError>> {
    jmap_server::resolve_creation_ref(raw, created_ids).ok_or_else(|| {
        Box::new(
            jmap_server::SetError::new(jmap_server::SetErrorType::InvalidProperties)
                .with_properties([property])
                .with_description(
                    "creation id reference matches no record created earlier in this request",
                ),
        )
    })
}

/// Rewrite each top-level `props` value of `obj`, a create or an update
/// patch, that is a `#creationId` reference to the id it names, through
/// [`resolve_ref`]. Other values are left for the caller to validate.
///
/// # Errors
///
/// As [`resolve_ref`], for the first unresolvable property.
pub(crate) fn resolve_refs(
    obj: &mut serde_json::Value,
    props: &[&str],
    created_ids: &std::collections::BTreeMap<jmap_types::Id, jmap_types::Id>,
) -> Result<(), Box<jmap_server::SetError>> {
    let Some(m) = obj.as_object_mut() else {
        return Ok(());
    };
    for p in props {
        if let Some(serde_json::Value::String(raw)) = m.get_mut(*p) {
            *raw = resolve_ref(raw, p, created_ids)?.as_ref().to_owned();
        }
    }
    Ok(())
}

/// Order a `/set` create map so each create whose `property` is `#k`
/// comes after the create `k` of the same call, as RFC 8620 §5.3 requires
/// for "records with references to the same type". A cycle or a
/// self-reference keeps its map order and fails reference resolution.
///
/// ponytail: repeated passes, O(n^2) in the create count, which
/// `maxObjectsInSet` bounds; a topological sort is the upgrade path.
pub(crate) fn parents_first(
    creates: serde_json::Map<String, serde_json::Value>,
    property: &str,
) -> Vec<(String, serde_json::Value)> {
    let keys: std::collections::BTreeSet<String> = creates.keys().cloned().collect();
    let mut pending: Vec<(String, serde_json::Value)> = creates.into_iter().collect();
    let mut ordered: Vec<(String, serde_json::Value)> = Vec::with_capacity(pending.len());
    loop {
        let before = pending.len();
        let (waiting, ready): (Vec<_>, Vec<_>) = std::mem::take(&mut pending)
            .into_iter()
            .partition(|(create_id, props)| {
                props
                    .get(property)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|p| p.strip_prefix('#'))
                    .is_some_and(|parent| {
                        parent != create_id.as_str()
                            && keys.contains(parent)
                            && !ordered.iter().any(|(done, _)| done.as_str() == parent)
                    })
            });
        ordered.extend(ready);
        pending = waiting;
        if pending.is_empty() || pending.len() == before {
            ordered.extend(pending);
            return ordered;
        }
    }
}

/// An `invalidProperties` SetError naming `properties` (none when empty).
pub(crate) fn invalid(properties: &[&str], description: &str) -> Value {
    let err = SetError::new(SetErrorType::InvalidProperties).with_description(description);
    serde_json::Value::from(&if properties.is_empty() {
        err
    } else {
        err.with_properties(properties.iter().copied())
    })
}

/// A `forbidden` SetError.
pub(crate) fn forbidden(description: &str) -> Value {
    serde_json::Value::from(&SetError::new(SetErrorType::Forbidden).with_description(description))
}

/// A `notFound` SetError.
pub(crate) fn not_found() -> Value {
    serde_json::Value::from(&SetError::new(SetErrorType::NotFound))
}

/// An `overQuota` SetError.
pub(crate) fn over_quota(description: &str) -> Value {
    serde_json::Value::from(&SetError::new(SetErrorType::OverQuota).with_description(description))
}

/// Fetch one object of type `O`, or `None` if the backend does not have it.
pub(crate) async fn fetch_one<O: GetObject + Send + Sync, B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Result<Option<O>, Value> {
    backend
        .get_objects::<O>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await
        .map(|(found, _)| found.into_iter().next())
        .map_err(|e| server_fail_value_from_backend(&e))
}

/// A Scene type whose `/get` hides records from callers.
pub(crate) trait Visible: GetObject + Send + Sync {
    /// The record's `id`.
    fn id(&self) -> &Id;

    /// The region whose access [`SceneBackend::may_access_region`] decides.
    fn region(&self) -> &Id;

    /// The type's check after region access: the SceneObject visibility
    /// set, or the SceneAvatar blocked-contact and E2EE-call filters.
    fn visible<B: SceneBackend>(
        &self,
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
    ) -> impl std::future::Future<Output = Result<bool, B::Error>> + Send;

    /// The record as `/get` returns it to a caller who may see it; by
    /// default unchanged.
    fn redact<B: SceneBackend>(
        self,
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
    ) -> impl std::future::Future<Output = Result<Self, B::Error>> + Send {
        let _ = (backend, caller, account_id);
        async { Ok(self) }
    }
}

impl Visible for SceneRegion {
    fn id(&self) -> &Id {
        &self.id
    }

    fn region(&self) -> &Id {
        &self.id
    }

    async fn visible<B: SceneBackend>(
        &self,
        _backend: &B,
        _caller: &B::CallerCtx,
        _account_id: &Id,
    ) -> Result<bool, B::Error> {
        Ok(true)
    }

    /// `invitedUserIds`: "The server returns the list only to callers who
    /// may update the region; for every other caller, the server returns
    /// `[]`" (draft-atwood-jmap-scene-00 at 6bcd9d8, L457).
    async fn redact<B: SceneBackend>(
        mut self,
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
    ) -> Result<Self, B::Error> {
        if !self.invited_user_ids.is_empty()
            && !backend.may_update_region(caller, account_id, &self).await?
        {
            self.invited_user_ids.clear();
        }
        Ok(self)
    }
}

impl Visible for SceneObject {
    fn id(&self) -> &Id {
        &self.id
    }

    fn region(&self) -> &Id {
        &self.region_id
    }

    fn visible<B: SceneBackend>(
        &self,
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
    ) -> impl std::future::Future<Output = Result<bool, B::Error>> + Send {
        backend.is_object_visible(caller, account_id, self)
    }
}

impl Visible for SceneAvatar {
    fn id(&self) -> &Id {
        &self.id
    }

    fn region(&self) -> &Id {
        &self.region_id
    }

    async fn visible<B: SceneBackend>(
        &self,
        backend: &B,
        caller: &B::CallerCtx,
        account_id: &Id,
    ) -> Result<bool, B::Error> {
        Ok(!backend.is_user_blocked(caller, &self.user_id).await?
            && backend
                .may_see_region_avatars(caller, account_id, &self.region_id)
                .await?)
    }
}

/// `true` if the caller may see `o`: region access first, then the type's
/// own check ("Blocked-contact filtering is applied AFTER the visibility
/// filtering").
pub(crate) async fn is_visible<O: Visible, B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    o: &O,
) -> Result<bool, B::Error> {
    Ok(backend
        .may_access_region(caller, account_id, o.region())
        .await?
        && o.visible(backend, caller, account_id).await?)
}

/// RFC 8620 §5.1 `/get` of `O` that reports a record the caller may not
/// see ([`is_visible`]) exactly as a missing one: a requested id goes to
/// `notFound` (in request order, as are the missing ids), and a full
/// fetch (`ids` `null`) leaves it out. This is the
/// "MUST return `notFound` (not `forbidden`)" of draft-atwood-jmap-scene-00
/// §Region Access, and the "MUST NOT return an error, a filtered-result
/// indicator, or any other signal" of §Blocked-Contact Spatial Presence.
///
/// ponytail: one [`is_visible`] call per returned record, O(records)
/// hook calls on a full fetch, and an O(ids × records) `notFound` scan
/// bounded by `maxObjectsInGet`; a backend that wants fewer serves a
/// `/get` of its own.
pub(crate) async fn handle_get_visible<O: Visible, B: SceneBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;
    let ids: Option<Vec<Id>> = optional_arg(&mut args, "ids", || {
        JmapError::invalid_arguments("ids must be an Id array")
    })?;
    let properties: Option<Vec<String>> = optional_arg(&mut args, "properties", || {
        JmapError::invalid_arguments("properties must be a string array")
    })?;

    let (fetched, _) = backend
        .get_objects::<O>(caller, &account_id, ids.as_deref(), properties.as_deref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let mut list = Vec::with_capacity(fetched.len());
    for o in fetched {
        if is_visible(backend, caller, &account_id, &o)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            list.push(
                o.redact(backend, caller, &account_id)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?,
            );
        }
    }
    // `notFound` in request order, so a hidden id cannot be told from a
    // missing one by its position either.
    let not_found: Vec<Id> = ids
        .unwrap_or_default()
        .into_iter()
        .filter(|id| !list.iter().any(|o| o.id() == id))
        .collect();

    let state = backend
        .get_state::<O>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": serialize_value(&list)?,
        "notFound": not_found_json(&not_found),
    })))
}

/// The sortable part of a UTCDate: `YYYY-MM-DDTHH:MM:SS`.
///
/// Fractional seconds are dropped so that values with and without them
/// compare chronologically; for well-formed UTC timestamps the ASCII
/// order of this prefix is chronological order.
pub(crate) fn utc_key(d: &UTCDate) -> &str {
    let s: &str = d.as_ref();
    s.get(..19).unwrap_or(s)
}

/// Use `default` as the `/query` `sort` when the request gives none
/// (absent, `null` or `[]`), so every backend sees the draft's default.
pub(crate) fn default_sort(args: &mut Map<String, Value>, default: Value) {
    let unsorted = match args.get("sort") {
        None | Some(Value::Null) => true,
        Some(Value::Array(a)) => a.is_empty(),
        Some(_) => false,
    };
    if unsorted {
        args.insert("sort".to_owned(), default);
    }
}

/// Reject a `/query` `sort` property this server does not define: RFC 8620
/// §5.5 `unsupportedSort`. A value that is not an array is left for the
/// typed decode to reject.
pub(crate) fn reject_unsupported_sort<C: serde::de::DeserializeOwned + KnownSort>(
    sort: Option<&Value>,
) -> Result<(), JmapError> {
    if let Some(Value::Array(comparators)) = sort {
        for c in comparators {
            if !<C as serde::Deserialize>::deserialize(c).is_ok_and(|c| c.known()) {
                return Err(JmapError::unsupported_sort());
            }
        }
    }
    Ok(())
}

/// A comparator whose `property` decodes an unknown name to `Other`
/// (bd JMAP-wf92.11), so [`reject_unsupported_sort`] must check it.
pub(crate) trait KnownSort {
    /// `false` when `property` is `Other`, a name the draft does not list.
    fn known(&self) -> bool;
}

macro_rules! known_sort {
    ($($cmp:ty => $prop:path),+ $(,)?) => {$(
        impl KnownSort for $cmp {
            fn known(&self) -> bool {
                !matches!(self.property, $prop(_))
            }
        }
    )+};
}

known_sort!(
    jmap_scene_types::SceneAssetComparator => jmap_scene_types::SceneAssetSortProperty::Other,
    jmap_scene_types::SceneRegionComparator => jmap_scene_types::SceneRegionSortProperty::Other,
    jmap_scene_types::SceneObjectComparator => jmap_scene_types::SceneObjectSortProperty::Other,
    jmap_scene_types::SceneAvatarComparator => jmap_scene_types::SceneAvatarSortProperty::Other,
);

/// Per-`/set` accumulators emitted in the RFC 8620 §5.3 response envelope.
#[derive(Debug, Default)]
pub(crate) struct SetAccumulators {
    pub created: Map<String, Value>,
    pub updated: Map<String, Value>,
    pub destroyed: Vec<Value>,
    pub not_created: Map<String, Value>,
    pub not_updated: Map<String, Value>,
    pub not_destroyed: Map<String, Value>,
}

/// Build the final `/set` response, re-reading `newState` when anything
/// was mutated. Empty maps and arrays serialize as `null`.
pub(crate) async fn finalize_set_response<B, O>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    old_state: State,
    mutated: bool,
    acc: SetAccumulators,
) -> Result<HandlerOutput, JmapError>
where
    B: SceneBackend,
    O: JmapObject + Send + Sync,
{
    let new_state = if mutated {
        backend
            .get_state::<O>(caller, account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    } else {
        old_state.clone()
    };

    let SetAccumulators {
        created,
        updated,
        destroyed,
        not_created,
        not_updated,
        not_destroyed,
    } = acc;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created":      if created.is_empty()      { Value::Null } else { Value::Object(created) },
        "updated":      if updated.is_empty()      { Value::Null } else { Value::Object(updated) },
        "destroyed":    if destroyed.is_empty()    { Value::Null } else { Value::Array(destroyed) },
        "notCreated":   if not_created.is_empty()  { Value::Null } else { Value::Object(not_created) },
        "notUpdated":   if not_updated.is_empty()  { Value::Null } else { Value::Object(not_updated) },
        "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
    })))
}
