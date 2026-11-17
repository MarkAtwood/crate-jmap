//! draft-ietf-jmap-object-history-03 (`urn:ietf:params:jmap:object-history`),
//! with the -04 editor's copy changes listed in docs/design/object-history.md
//! §2a: the [`ObjectHistoryBackend`] trait, argument parsing, the history
//! `Foo/get`, `Foo/query` and `Foo/queryChanges` handlers, and the pieces a
//! data-type crate's history-aware `Foo/set` uses for the `restore` stage.
//!
//! Opt-in (docs/design/DECISIONS.md decision 1): a data-type crate registers
//! these handlers through `register_<x>_handlers_with_history` for a backend
//! that also implements [`ObjectHistoryBackend`], using
//! [`Dispatcher::register_history_aware`]. Plain handlers are unchanged.

use std::collections::{BTreeMap, BTreeSet};

use jmap_object_history_types::{
    DestroyedQueryArguments, HistoryEntry, HistoryGetArguments, HistoryObject,
    JMAP_OBJECT_HISTORY_URI, ObjectHistoryAccountCapability,
};
use jmap_types::{PatchObject, QueryObject, SetObject, State, UTCDate};
use serde_json::{Map, Value, json};

use crate::backend::{
    BackendChangesError, BackendQueryError, BackendSetError, JmapBackend, QueryChangesParams,
    QueryChangesResult, QueryParams, QueryResult,
};
use crate::handlers::{
    handle_get_with_metadata, handle_query_changes_with_i18n, handle_query_with_i18n,
    server_fail_from_backend,
};
use crate::helpers::{
    MetadataProjection, apply_patch_object, check_patch_paths, enforce_max_objects_in_get,
    extract_account_id, filter_arg, json_type_name, not_found_json, optional_arg,
    parse_max_changes, query_anchor_args, serialize_value,
};
use crate::i18n::I18nProjection;
use crate::{
    Dispatcher, HandlerOutput, Id, JmapError, JmapHandler, SetError, SetErrorType,
    backend_set_error_value,
};

/// The property object history adds to every entry (§3.1).
pub const OBJECT_HISTORY_PROPERTY: &str = "objectHistory";

// ---------------------------------------------------------------------------
// Backend trait and its parameter and result types
// ---------------------------------------------------------------------------

/// The parameters of [`ObjectHistoryBackend::get_object_history`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct HistoryGetParams<'a> {
    /// `ids`; `None` for every object of the type, and with
    /// `includeDestroyed` also "every destroyed object for which the server
    /// still retains any version" (§3).
    pub ids: Option<&'a [Id]>,
    /// `properties` without `objectHistory`; `None` for all.
    pub properties: Option<&'a [String]>,
    /// The parsed §3 arguments; at least one flag is true.
    pub arguments: &'a HistoryGetArguments,
}

impl<'a> HistoryGetParams<'a> {
    /// Parameters for `arguments`, every object and every property.
    pub fn new(arguments: &'a HistoryGetArguments) -> Self {
        Self {
            ids: None,
            properties: None,
            arguments,
        }
    }

    /// Set [`ids`](Self::ids).
    #[must_use]
    pub fn ids(mut self, ids: Option<&'a [Id]>) -> Self {
        self.ids = ids;
        self
    }

    /// Set [`properties`](Self::properties).
    #[must_use]
    pub fn properties(mut self, properties: Option<&'a [String]>) -> Self {
        self.properties = properties;
        self
    }
}

/// What [`ObjectHistoryBackend::get_object_history`] returns.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryGetResult<O> {
    /// The selected entries; each id's entries ascending by version (the
    /// handler stable-sorts them again).
    pub entries: Vec<HistoryEntry<O>>,
    /// Requested ids with no entry left (§3).
    pub not_found: Vec<Id>,
}

impl<O> HistoryGetResult<O> {
    /// `entries` and `not_found`.
    pub fn new(entries: Vec<HistoryEntry<O>>, not_found: Vec<Id>) -> Self {
        Self { entries, not_found }
    }
}

/// The parameters of [`ObjectHistoryBackend::query_destroyed`]: an ordinary
/// query over destroyed objects plus the §4 window.
#[non_exhaustive]
pub struct DestroyedQueryParams<'a, O: QueryObject> {
    /// `filter`, `sort`, `position`, `anchor`, `anchorOffset`, `limit`;
    /// applied to each object's final version.
    pub query: QueryParams<'a, O>,
    /// `destroyedAfter`: destroyed strictly after this time.
    pub destroyed_after: Option<&'a UTCDate>,
    /// `destroyedBefore`: destroyed strictly before this time.
    pub destroyed_before: Option<&'a UTCDate>,
}

impl<'a, O: QueryObject> DestroyedQueryParams<'a, O> {
    /// `query` with no window.
    pub fn new(query: QueryParams<'a, O>) -> Self {
        Self {
            query,
            destroyed_after: None,
            destroyed_before: None,
        }
    }

    /// Set [`destroyed_after`](Self::destroyed_after).
    #[must_use]
    pub fn destroyed_after(mut self, t: Option<&'a UTCDate>) -> Self {
        self.destroyed_after = t;
        self
    }

    /// Set [`destroyed_before`](Self::destroyed_before).
    #[must_use]
    pub fn destroyed_before(mut self, t: Option<&'a UTCDate>) -> Self {
        self.destroyed_before = t;
        self
    }
}

/// The parameters of [`ObjectHistoryBackend::query_destroyed_changes`]: an
/// ordinary queryChanges plus the window the client passed to the
/// `Foo/query` (-04 §4).
#[non_exhaustive]
pub struct DestroyedQueryChangesParams<'a, O: QueryObject> {
    /// `sinceQueryState`, `filter`, `sort`, `maxChanges`, `upToId`.
    pub query_changes: QueryChangesParams<'a, O>,
    /// `destroyedAfter`.
    pub destroyed_after: Option<&'a UTCDate>,
    /// `destroyedBefore`.
    pub destroyed_before: Option<&'a UTCDate>,
}

impl<'a, O: QueryObject> DestroyedQueryChangesParams<'a, O> {
    /// `query_changes` with no window.
    pub fn new(query_changes: QueryChangesParams<'a, O>) -> Self {
        Self {
            query_changes,
            destroyed_after: None,
            destroyed_before: None,
        }
    }

    /// Set [`destroyed_after`](Self::destroyed_after).
    #[must_use]
    pub fn destroyed_after(mut self, t: Option<&'a UTCDate>) -> Self {
        self.destroyed_after = t;
        self
    }

    /// Set [`destroyed_before`](Self::destroyed_before).
    #[must_use]
    pub fn destroyed_before(mut self, t: Option<&'a UTCDate>) -> Self {
        self.destroyed_before = t;
        self
    }
}

/// What [`ObjectHistoryBackend::restore_object`] returns.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Restored<O> {
    /// The object as stored after the restore.
    pub stored: O,
    /// The final version with the patch applied, as JSON: `restored`
    /// reports the properties of [`stored`](Self::stored) that differ
    /// from it (§5.1).
    pub baseline: Map<String, Value>,
}

impl<O> Restored<O> {
    /// `stored`, restored from `baseline`.
    pub fn new(stored: O, baseline: Map<String, Value>) -> Self {
        Self { stored, baseline }
    }
}

/// Storage for object history (docs/design/object-history.md §6): the
/// hooks behind the history `Foo/get`, destroyed `Foo/query` and
/// `Foo/queryChanges`, and `restore` in `Foo/set`.
///
/// A separate trait, so a backend that offers history implements every
/// method. None has a default: each would put wrong data on the wire.
///
/// # Contracts on the backend's other methods
///
/// - Every id-minting path (create, copy, import, ...) MUST NOT mint "the
///   id of a destroyed object for which it retains any history, other than
///   by restoring" (§5.1).
/// - Updates, destroys, server cascades and changes through other
///   protocols record versions; an object "counts as destroyed however it
///   was removed" (§3).
/// - `get_changes` classifies a restored id by whether it existed at
///   `sinceState` and exists now, "exactly as if the object had never been
///   destroyed between them" (-04 §5.1): both ⇒ `updated`, now only ⇒
///   `created`, `sinceState` only ⇒ `destroyed`, neither ⇒ not reported. A
///   restore is never a metadata-only change.
/// - Only current objects reference blobs: a non-current version counts for
///   neither garbage collection, `blobHasReference`, nor `Blob/lookup` (for
///   blob2 and RFC 9404 alike, decision 11). A version whose blob is gone
///   is never returned (§3.1).
/// - Retention: discard oldest first (decision 24). The final version of a
///   destroyed object is the newest one retained, and its `replaced` is
///   then the destruction time (-04 §3.1).
/// - §8.2: history never shows a version the caller could not read if it
///   were current, nor the existence of a destroyed object the caller
///   could not have seen.
pub trait ObjectHistoryBackend: JmapBackend {
    /// The `urn:ietf:params:jmap:object-history` value of `account_id`'s
    /// `accountCapabilities` (§2.1), or `None` for no history in the
    /// account. The handlers accept history arguments only for the types it
    /// lists, so advertisement and behaviour agree. List only types for
    /// which `jmap_object_history_types::is_history_type` is true.
    fn object_history_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
    ) -> Option<ObjectHistoryAccountCapability>;

    /// The §3 selection for a history `Foo/get` (at least one flag set).
    ///
    /// Per id: with `includeDestroyed` false a destroyed id is `notFound`;
    /// without `includeReplaced` only the live version (or a destroyed
    /// object's final version). `historyAfter`/`historyBefore` are strict;
    /// the live version is kept by after and dropped by before; then
    /// `historySkip` from the newest (the live version counts as one); then
    /// `historyLimit` keeps the newest. `notFound` only when nothing is left
    /// after the skip. Each id's entries ascend by `version`; the live
    /// version has the highest, with `replaced` and `expires` null;
    /// `replaced` values are distinct per object. `hasOlder` counts
    /// retained older versions whether returned or not. A supported type
    /// without retained history still returns the live version (§3.1).
    /// `jmap_object_history_server::VersionLog` implements all of this.
    ///
    /// No default: one would answer "no history" or a wrong `notFound`.
    fn get_object_history<O: HistoryObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        params: HistoryGetParams<'_>,
    ) -> impl std::future::Future<
        Output = Result<HistoryGetResult<O>, BackendQueryError<Self::Error>>,
    > + Send;

    /// `Foo/query` with `onlyDestroyed` (§4): over the destroyed objects
    /// with a retained version, restored ones excluded; `filter` and `sort`
    /// apply to the final version; the window is strict on the final
    /// version's `replaced`; a null or empty sort orders by destruction
    /// time, most recent first. An unsupported filter or sort is
    /// `BackendQueryError::Method` (`unsupportedFilter`/`unsupportedSort`).
    ///
    /// The returned `query_state` MUST encode that it is a destroyed query
    /// and its window (decision 25), so
    /// [`query_destroyed_changes`](Self::query_destroyed_changes) can tell
    /// a mismatched state and the plain `query_changes` never takes it for
    /// a live one.
    ///
    /// No default: one would claim nothing was destroyed.
    fn query_destroyed<O: HistoryObject + QueryObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        params: DestroyedQueryParams<'_, O>,
    ) -> impl std::future::Future<Output = Result<QueryResult, BackendQueryError<Self::Error>>> + Send;

    /// `Foo/queryChanges` with `onlyDestroyed` (-04 §4): a destroy adds an
    /// object, a restore or discard removes it. A `since_query_state` not
    /// minted by [`query_destroyed`](Self::query_destroyed) for the same
    /// window is `CannotCalculate`; so is a state the backend cannot
    /// track (§4 MAY).
    ///
    /// No default: one would claim nothing changed.
    fn query_destroyed_changes<O: HistoryObject + QueryObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        params: DestroyedQueryChangesParams<'_, O>,
    ) -> impl std::future::Future<
        Output = Result<QueryChangesResult, BackendChangesError<Self::Error>>,
    > + Send;

    /// Restore the destroyed object `id` (§5.1), applying `patch` (already
    /// shape-checked; `None` for none) to its final version.
    ///
    /// Atomically: `forbidden` without write access, or when policy
    /// declines (§8.5). `notFound` unless `id` is a destroyed object with a
    /// retained version (a current id is `notFound`). Start from the final
    /// version, the newest retained (-04 §3.1), and apply the patch with
    /// [`apply_restore_patch`] (or the type's own parser): values that do
    /// not make a valid object are `invalidProperties`. Validate as an
    /// update: a reference to a missing object is `invalidProperties`
    /// naming the property; a conflict is the error `Foo/set` would give
    /// (`alreadyExists`, ...). Change nothing an update could not (-04
    /// §5.1): immutable values are kept (an Email keeps `threadId`, and its
    /// Thread exists again under the same id), or the restore is
    /// `forbidden`; mutable server-set values may change. Quota counts the
    /// restored object (`overQuota`). Store it under the old id, continue
    /// its history, advance the type's state, and record the change for
    /// `get_changes`. Inside a conditional scope the write belongs to the
    /// scope.
    ///
    /// No default: it is a write with spec semantics.
    fn restore_object<O: HistoryObject + SetObject>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
        patch: Option<PatchObject>,
    ) -> impl std::future::Future<Output = Result<Restored<O>, BackendSetError<Self::Error>>> + Send;
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

fn uses_history(using: &[String]) -> bool {
    using.iter().any(|u| u == JMAP_OBJECT_HISTORY_URI)
}

fn invalid(msg: impl Into<String>) -> JmapError {
    JmapError::invalid_arguments(msg.into())
}

fn without_uri(name: &str) -> JmapError {
    invalid(format!(
        "{name} needs {JMAP_OBJECT_HISTORY_URI} in using (decision 18)"
    ))
}

/// A Boolean argument with default false; `null` reads as the default.
fn take_flag(args: &mut Map<String, Value>, name: &str) -> Result<bool, JmapError> {
    match args.remove(name) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(b),
        Some(v) => Err(invalid(format!(
            "{name}: expected a boolean, got {}",
            json_type_name(&v)
        ))),
    }
}

fn parse_date(name: &str, v: Option<Value>) -> Result<Option<UTCDate>, JmapError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => UTCDate::new_validated(s)
            .map(Some)
            .map_err(|e| invalid(format!("{name}: {e}"))),
        Some(v) => Err(invalid(format!(
            "{name}: expected a UTCDate or null, got {}",
            json_type_name(&v)
        ))),
    }
}

fn parse_unsigned(name: &str, v: Option<Value>) -> Result<Option<u64>, JmapError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| {
            invalid(format!(
                "{name}: expected an UnsignedInt, got {}",
                json_type_name(&v)
            ))
        }),
    }
}

/// Take the §3 history arguments out of a `Foo/get`'s `args`.
///
/// `None` when both flags are false or absent: the other four are then
/// removed unread ("the server ignores historyAfter, historyBefore,
/// historySkip, and historyLimit"). Otherwise the parsed arguments.
///
/// # Errors
///
/// `invalidArguments` for a flag that is not a Boolean, a true flag
/// without the URI in `using` (decision 18), or (with a flag set) a
/// malformed `historyAfter`, `historyBefore`, `historySkip` or
/// `historyLimit`.
pub fn parse_history_get_arguments(
    args: &mut Map<String, Value>,
    using: &[String],
) -> Result<Option<HistoryGetArguments>, JmapError> {
    let include_replaced = take_flag(args, "includeReplaced")?;
    let include_destroyed = take_flag(args, "includeDestroyed")?;
    let after = args.remove("historyAfter");
    let before = args.remove("historyBefore");
    let skip = args.remove("historySkip");
    let limit = args.remove("historyLimit");
    if !include_replaced && !include_destroyed {
        return Ok(None);
    }
    if !uses_history(using) {
        return Err(without_uri(if include_replaced {
            "includeReplaced"
        } else {
            "includeDestroyed"
        }));
    }
    let mut out = HistoryGetArguments::new();
    out.include_replaced = include_replaced;
    out.include_destroyed = include_destroyed;
    out.history_after = parse_date("historyAfter", after)?;
    out.history_before = parse_date("historyBefore", before)?;
    out.history_skip = parse_unsigned("historySkip", skip)?.unwrap_or(0);
    out.history_limit = parse_unsigned("historyLimit", limit)?;
    Ok(Some(out))
}

/// Take the §4 arguments out of a `Foo/query` or `Foo/queryChanges`
/// (-04) `args`.
///
/// `None` when `onlyDestroyed` is false or absent. An explicit `null`
/// window is not "supplied" (its default is null).
///
/// # Errors
///
/// `invalidArguments` for a malformed argument; a non-null
/// `destroyedAfter` or `destroyedBefore` with `onlyDestroyed` false (§4);
/// `onlyDestroyed: true` or a non-null window without the URI in `using`
/// (decision 18).
pub fn parse_destroyed_query_arguments(
    args: &mut Map<String, Value>,
    using: &[String],
) -> Result<Option<DestroyedQueryArguments>, JmapError> {
    let only_destroyed = take_flag(args, "onlyDestroyed")?;
    let after = parse_date("destroyedAfter", args.remove("destroyedAfter"))?;
    let before = parse_date("destroyedBefore", args.remove("destroyedBefore"))?;
    let windowed = after.is_some() || before.is_some();
    if (only_destroyed || windowed) && !uses_history(using) {
        return Err(without_uri(if only_destroyed {
            "onlyDestroyed"
        } else {
            "destroyedAfter/destroyedBefore"
        }));
    }
    if !only_destroyed {
        if windowed {
            return Err(invalid(
                "destroyedAfter and destroyedBefore need onlyDestroyed: true (§4)",
            ));
        }
        return Ok(None);
    }
    let mut out = DestroyedQueryArguments::new();
    out.only_destroyed = true;
    out.destroyed_after = after;
    out.destroyed_before = before;
    Ok(Some(out))
}

/// Take `restore` (§5.1) out of a `Foo/set`'s `args`: each key as sent
/// with its raw value. Empty when absent, `null` or `{}`.
///
/// # Errors
///
/// `invalidArguments` for a `restore` that is neither an object nor
/// `null`, or a non-empty one without the URI in `using` (decision 18).
pub fn parse_restore_argument(
    args: &mut Map<String, Value>,
    using: &[String],
) -> Result<Map<String, Value>, JmapError> {
    let restore = match args.remove("restore") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m,
        Some(v) => {
            return Err(invalid(format!(
                "restore: expected an object or null, got {}",
                json_type_name(&v)
            )));
        }
    };
    if !restore.is_empty() && !uses_history(using) {
        return Err(without_uri("restore"));
    }
    Ok(restore)
}

/// Whether `O` has history in `account_id`.
///
/// # Errors
///
/// `invalidArguments` when the account's capability does not list `O`
/// (§3.1, §4, §5.1: the server "MUST reject the call").
pub fn require_history_support<O: HistoryObject, B: ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
) -> Result<(), JmapError> {
    match backend.object_history_capability(caller, account_id) {
        Some(cap) if cap.supports::<O>() => Ok(()),
        _ => Err(invalid(format!(
            "{} has no history in this account (supportedTypes)",
            O::TYPE_NAME
        ))),
    }
}

// ---------------------------------------------------------------------------
// Foo/get
// ---------------------------------------------------------------------------

/// Remove `objectHistory` from a `properties` argument (§3.1: the client
/// MAY list it; it is returned "if and only if" a flag is true). Returns
/// whether it was listed.
pub fn strip_object_history_property(args: &mut Map<String, Value>) -> bool {
    let Some(Value::Array(props)) = args.get_mut("properties") else {
        return false;
    };
    let before = props.len();
    props.retain(|p| p.as_str() != Some(OBJECT_HISTORY_PROPERTY));
    props.len() != before
}

/// Write one history entry's `objectHistory` into `item`, and keep only
/// `properties` (plus `id` and `objectHistory`) when given.
pub fn attach_object_history(
    item: &mut Map<String, Value>,
    object_history: Value,
    properties: Option<&[String]>,
) {
    if let Some(props) = properties {
        item.retain(|k, _| k == "id" || props.iter().any(|p| p == k));
    }
    item.insert(OBJECT_HISTORY_PROPERTY.to_owned(), object_history);
}

/// `Foo/get` with object history (§3).
///
/// With neither flag set the history arguments are removed (the four
/// others unread), `objectHistory` is removed from `properties`, and the
/// call is [`handle_get_with_metadata`]. Otherwise: the account must list
/// `O` ([`require_history_support`]); `maxObjectsInGet` counts ids, not
/// entries; [`ObjectHistoryBackend::get_object_history`] selects the
/// entries; each is projected like a plain `Foo/get` object (`properties`,
/// with `id` and `objectHistory` always kept; metadata per
/// [`MetadataProjection`]); each id's entries are stable-sorted by
/// `version`.
///
/// # Errors
///
/// Those of [`handle_get_with_metadata`], of
/// [`parse_history_get_arguments`] and [`require_history_support`], and a
/// redacted `serverFail` for a backend error.
pub async fn handle_get_with_history<O: HistoryObject, B: ObjectHistoryBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    history_get::<O, B, _, _>(backend, caller, args, using, |args| {
        handle_get_with_metadata::<O, B>(backend, caller, args, using)
    })
    .await
}

/// [`handle_get_with_history`] with `plain` as the call made when no history
/// argument is set, for a type whose plain handler is its own (a custom
/// getter, a type-specific query).
///
/// # Errors
///
/// As [`handle_get_with_history`], with `plain`'s errors in place of the default
/// handler's.
pub async fn history_get<O: HistoryObject, B: ObjectHistoryBackend, F, Fut>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
    plain: F,
) -> Result<HandlerOutput, JmapError>
where
    F: FnOnce(Value) -> Fut,
    Fut: std::future::Future<Output = Result<HandlerOutput, JmapError>>,
{
    let Value::Object(mut map) = args else {
        return plain(args).await;
    };
    let Some(history) = parse_history_get_arguments(&mut map, using)? else {
        strip_object_history_property(&mut map);
        return plain(Value::Object(map)).await;
    };
    strip_object_history_property(&mut map);
    let (account_id, mut args) = extract_account_id(Value::Object(map))?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    require_history_support::<O, B>(backend, caller, &account_id)?;
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;
    let i18n = I18nProjection::from_args::<O>(
        &mut args,
        using,
        backend.i18n_info(caller, &account_id).as_ref(),
    )?;
    let ids: Option<Vec<Id>> = optional_arg(&mut args, "ids", || {
        JmapError::invalid_arguments("ids must be an Id array")
    })?;
    let properties: Option<Vec<String>> = optional_arg(&mut args, "properties", || {
        JmapError::invalid_arguments("properties must be a string array")
    })?;
    let metadata = match backend.metadata_info::<O>(caller, &account_id) {
        Some(info) => Some(MetadataProjection::new(
            properties.as_deref(),
            true,
            using,
            Some(&info),
        )?),
        None => None,
    };
    // The backend gets the root of each metadata subselector.
    let roots: Option<Vec<String>> = properties.as_ref().map(|props| {
        let mut roots: Vec<String> = Vec::with_capacity(props.len());
        for p in props {
            let root = match (metadata.is_some(), p.split_once('/')) {
                (true, Some((r @ ("metadata" | "privateMetadata"), _))) => r.to_owned(),
                _ => p.clone(),
            };
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        roots
    });
    let backend_properties = i18n.backend_properties(roots.as_deref());
    let result = backend
        .get_object_history::<O>(
            caller,
            &account_id,
            HistoryGetParams::new(&history)
                .ids(ids.as_deref())
                .properties(backend_properties.as_deref()),
        )
        .await
        .map_err(JmapError::from)?;
    let state = backend
        .get_state::<O>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let mut list = Vec::with_capacity(result.entries.len());
    for entry in result.entries {
        let version = entry.object_history.version;
        let info = serialize_value(&entry.object_history)?;
        let mut item = serialize_value(&entry.object)?;
        if let Value::Object(map) = &mut item {
            map.remove(OBJECT_HISTORY_PROPERTY);
            i18n.apply(map);
            if let Some(roots) = &roots {
                map.retain(|k, _| k == "id" || roots.iter().any(|p| p == k));
            }
        }
        if let Some(projection) = &metadata {
            let source = item.clone();
            projection.apply(&source, &mut item);
        }
        if let Value::Object(map) = &mut item {
            attach_object_history(map, info, None);
        }
        list.push((version, item));
    }
    sort_entries_by_version(&mut list);

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "notFound": not_found_json(&result.not_found),
    })))
}

/// Stable-sort each id's entries by version, keeping the position of each
/// id's first entry (§3: same-id entries "MUST be ordered by
/// objectHistory.version (ascending)"; ids MAY interleave).
fn sort_entries_by_version(list: &mut [(u64, Value)]) {
    let mut first: BTreeMap<String, usize> = BTreeMap::new();
    let keys: Vec<(usize, u64)> = list
        .iter()
        .enumerate()
        .map(|(i, (version, item))| {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            (*first.entry(id).or_insert(i), *version)
        })
        .collect();
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    let sorted: Vec<(u64, Value)> = order.iter().map(|&i| list[i].clone()).collect();
    for (slot, v) in list.iter_mut().zip(sorted) {
        *slot = v;
    }
}

// ---------------------------------------------------------------------------
// Foo/query and Foo/queryChanges
// ---------------------------------------------------------------------------

/// `Foo/query` with `onlyDestroyed` (§4).
///
/// Without `onlyDestroyed` it is [`handle_query_with_i18n`]. Otherwise the
/// account must list `O`, the ordinary query arguments are parsed as
/// `Foo/query` parses them, and
/// [`ObjectHistoryBackend::query_destroyed`] answers, in the `Foo/query`
/// response shape. A type whose `/query` takes arguments for current
/// objects only (`collapseThreads`, `expandRecurrences`) removes them
/// first: they are ignored for a destroyed query (§4).
///
/// # Errors
///
/// Those of [`handle_query_with_i18n`] and
/// [`parse_destroyed_query_arguments`], `invalidArguments` for an
/// unsupported type, and the backend's `BackendQueryError::Method`
/// (`unsupportedFilter`, `unsupportedSort`, `anchorNotFound`).
pub async fn handle_query_with_history<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError>
where
    O: HistoryObject + QueryObject,
    B: ObjectHistoryBackend,
{
    history_query::<O, B, _, _>(backend, caller, args, using, |args| {
        handle_query_with_i18n::<O, B>(backend, caller, args, using)
    })
    .await
}

/// [`handle_query_with_history`] with `plain` as the call made when no history
/// argument is set, for a type whose plain handler is its own (a custom
/// getter, a type-specific query).
///
/// # Errors
///
/// As [`handle_query_with_history`], with `plain`'s errors in place of the default
/// handler's.
pub async fn history_query<O, B, F, Fut>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
    plain: F,
) -> Result<HandlerOutput, JmapError>
where
    F: FnOnce(Value) -> Fut,
    Fut: std::future::Future<Output = Result<HandlerOutput, JmapError>>,
    O: HistoryObject + QueryObject,
    B: ObjectHistoryBackend,
{
    let Value::Object(mut map) = args else {
        return plain(args).await;
    };
    let Some(destroyed) = parse_destroyed_query_arguments(&mut map, using)? else {
        return plain(Value::Object(map)).await;
    };
    let (account_id, mut args) = extract_account_id(Value::Object(map))?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    require_history_support::<O, B>(backend, caller, &account_id)?;
    let i18n = I18nProjection::from_args::<O>(
        &mut args,
        using,
        backend.i18n_info(caller, &account_id).as_ref(),
    )?;
    let calculate_total = args
        .get("calculateTotal")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let limit = parse_unsigned("limit", args.remove("limit"))?;
    let position: i64 = match args.get("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            invalid(format!(
                "position: expected an integer, got {}",
                json_type_name(v)
            ))
        })?,
    };
    let (anchor, anchor_offset) = query_anchor_args(&args)?;
    let filter: Option<O::Filter> = filter_arg(&mut args)?;
    let sort: Option<Vec<O::Comparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;
    let result = backend
        .query_destroyed::<O>(
            caller,
            &account_id,
            DestroyedQueryParams::new(
                QueryParams::new()
                    .filter(filter.as_ref())
                    .sort(sort.as_deref())
                    .limit(limit)
                    .position(position)
                    .anchor(anchor.as_ref())
                    .anchor_offset(anchor_offset)
                    .language(i18n.language()),
            )
            .destroyed_after(destroyed.destroyed_after.as_ref())
            .destroyed_before(destroyed.destroyed_before.as_ref()),
        )
        .await
        .map_err(JmapError::from)?;
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "ids": result.ids,
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }
    Ok(HandlerOutput::new(resp))
}

/// `Foo/queryChanges` with `onlyDestroyed` (-04 §4: the client "MUST pass
/// the same values that it passed to Foo/query").
///
/// Without `onlyDestroyed` it is [`handle_query_changes_with_i18n`]; a
/// destroyed query's state sent there is unknown to the live
/// `query_changes`. Otherwise
/// [`ObjectHistoryBackend::query_destroyed_changes`] answers, in the
/// `Foo/queryChanges` response shape.
///
/// # Errors
///
/// As [`handle_query_with_history`], plus `cannotCalculateChanges` from
/// the backend (a state of another kind or window, or one it cannot
/// track).
pub async fn handle_query_changes_with_history<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError>
where
    O: HistoryObject + QueryObject,
    B: ObjectHistoryBackend,
{
    history_query_changes::<O, B, _, _>(backend, caller, args, using, |args| {
        handle_query_changes_with_i18n::<O, B>(backend, caller, args, using)
    })
    .await
}

/// [`handle_query_changes_with_history`] with `plain` as the call made when no history
/// argument is set, for a type whose plain handler is its own (a custom
/// getter, a type-specific query).
///
/// # Errors
///
/// As [`handle_query_changes_with_history`], with `plain`'s errors in place of the default
/// handler's.
pub async fn history_query_changes<O, B, F, Fut>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
    plain: F,
) -> Result<HandlerOutput, JmapError>
where
    F: FnOnce(Value) -> Fut,
    Fut: std::future::Future<Output = Result<HandlerOutput, JmapError>>,
    O: HistoryObject + QueryObject,
    B: ObjectHistoryBackend,
{
    let Value::Object(mut map) = args else {
        return plain(args).await;
    };
    let Some(destroyed) = parse_destroyed_query_arguments(&mut map, using)? else {
        return plain(Value::Object(map)).await;
    };
    let (account_id, mut args) = extract_account_id(Value::Object(map))?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    require_history_support::<O, B>(backend, caller, &account_id)?;
    let i18n = I18nProjection::from_args::<O>(
        &mut args,
        using,
        backend.i18n_info(caller, &account_id).as_ref(),
    )?;
    let since: State = match args.get("sinceQueryState").and_then(Value::as_str) {
        Some(s) => State::from(s),
        None => return Err(invalid("sinceQueryState is required")),
    };
    let max_changes = parse_max_changes(args.get("maxChanges"))?;
    let up_to_id: Option<Id> = match args.get("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(_) => return Err(invalid("upToId must be a string Id or null")),
    };
    let calculate_total = args
        .get("calculateTotal")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let filter: Option<O::Filter> = filter_arg(&mut args)?;
    let sort: Option<Vec<O::Comparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;
    let result = backend
        .query_destroyed_changes::<O>(
            caller,
            &account_id,
            DestroyedQueryChangesParams::new(
                QueryChangesParams::new(&since)
                    .filter(filter.as_ref())
                    .sort(sort.as_deref())
                    .max_changes(max_changes)
                    .up_to_id(up_to_id.as_ref())
                    .language(i18n.language()),
            )
            .destroyed_after(destroyed.destroyed_after.as_ref())
            .destroyed_before(destroyed.destroyed_before.as_ref()),
        )
        .await
        .map_err(JmapError::from)?;
    let added: Vec<Value> = result
        .added
        .iter()
        .map(|a| json!({"id": a.id.as_ref(), "index": a.index}))
        .collect();
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "removed": result.removed,
        "added": added,
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }
    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// Foo/set: the restore stage
// ---------------------------------------------------------------------------

/// Per-type hooks for [`run_restore_phase`].
pub trait RestoreHooks: Sync {
    /// Check a restore entry's patch as the type checks an update patch
    /// (`#creationId` references, metadata entries, ...). The default
    /// accepts it.
    ///
    /// # Errors
    ///
    /// The entry's SetError.
    fn check_patch(&self, id: &Id, patch: Option<&PatchObject>) -> Result<(), Box<SetError>> {
        let _ = (id, patch);
        Ok(())
    }

    /// The ids of objects of the same type that `final_version` refers to
    /// (a Mailbox's or FileNode's `parentId`). Restores that refer to
    /// another restore key run after it (-04 §5.1: "as for creates"). The
    /// default: none, so restores run in key order.
    fn same_type_references(&self, final_version: &Map<String, Value>) -> Vec<Id> {
        let _ = final_version;
        Vec::new()
    }
}

/// [`RestoreHooks`] with every default: no extra checks, no same-type
/// references.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRestoreHooks;

impl RestoreHooks for NoRestoreHooks {}

/// [`RestoreHooks`] for a type whose objects refer to others of the same
/// type through one Id-valued property, such as `parentId`.
#[derive(Debug, Clone, Copy)]
pub struct ParentRestoreHooks(pub &'static str);

impl RestoreHooks for ParentRestoreHooks {
    fn same_type_references(&self, final_version: &Map<String, Value>) -> Vec<Id> {
        final_version
            .get(self.0)
            .and_then(Value::as_str)
            .map(|s| vec![Id::from(s)])
            .unwrap_or_default()
    }
}

/// What [`run_restore_phase`] produced.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RestoreAccumulators {
    /// `restored`: id → the changed properties, or `null`.
    pub restored: Map<String, Value>,
    /// `notRestored`: id as sent → SetError.
    pub not_restored: Map<String, Value>,
    /// Whether any restore succeeded (the type's state changed).
    pub mutated: bool,
}

fn set_error(t: SetErrorType, description: impl Into<String>) -> SetError {
    SetError::new(t).with_description(description)
}

fn set_error_value(e: SetError) -> Value {
    serde_json::to_value(e).unwrap_or_else(|_| json!({"type": "serverFail"}))
}

/// Restore every entry of `restore` (from [`parse_restore_argument`]),
/// "before any create, update, or destroy in the same call" (§5.1).
///
/// Per entry: a key that is not an Id is `notFound` (it is "not the id of
/// a destroyed object"); a value that is neither `null` nor an object is
/// `invalidPatch`, and `{}` is the same as `null`; a patch with a bad path
/// is `invalidPatch`; an `objectHistory` key is `invalidProperties`; then
/// [`RestoreHooks::check_patch`] and
/// [`ObjectHistoryBackend::restore_object`]. Restores referring to
/// another restore key (by [`RestoreHooks::same_type_references`] on the
/// final versions) run after it; a cycle fails its entries with
/// `invalidProperties` naming `id`. `restored` holds the properties of the
/// stored object that differ from the final version with the patch applied
/// ([`restored_diff`]); a backend error becomes the entry's SetError,
/// `Other` redacted to `serverFail`.
///
/// # Errors
///
/// `invalidArguments` for an unsupported type when `restore` is
/// non-empty; a redacted `serverFail` when fetching final versions for the
/// order fails.
pub async fn run_restore_phase<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    restore: Map<String, Value>,
    hooks: &dyn RestoreHooks,
) -> Result<RestoreAccumulators, JmapError>
where
    O: HistoryObject + SetObject,
    B: ObjectHistoryBackend,
{
    let mut acc = RestoreAccumulators::default();
    if restore.is_empty() {
        return Ok(acc);
    }
    require_history_support::<O, B>(backend, caller, account_id)?;

    let mut ready: Vec<(String, Id, Option<PatchObject>)> = Vec::new();
    for (key, value) in restore {
        let Ok(id) = Id::new_validated(key.as_str()) else {
            acc.not_restored.insert(
                key,
                set_error_value(set_error(
                    SetErrorType::NotFound,
                    "not the id of a destroyed object",
                )),
            );
            continue;
        };
        let patch = match value {
            Value::Null => None,
            Value::Object(m) if m.is_empty() => None,
            Value::Object(m) => Some(m),
            v => {
                acc.not_restored.insert(
                    key,
                    set_error_value(set_error(
                        SetErrorType::InvalidPatch,
                        format!("expected a PatchObject or null, got {}", json_type_name(&v)),
                    )),
                );
                continue;
            }
        };
        if let Some(p) = &patch {
            if let Err(e) = check_patch_paths(p) {
                acc.not_restored.insert(
                    key,
                    set_error_value(set_error(SetErrorType::InvalidPatch, e.to_string())),
                );
                continue;
            }
            if p.keys()
                .any(|k| k == OBJECT_HISTORY_PROPERTY || k.starts_with("objectHistory/"))
            {
                acc.not_restored
                    .insert(key, set_error_value(object_history_set_error()));
                continue;
            }
        }
        let patch = patch.map(PatchObject::from_map);
        if let Err(e) = hooks.check_patch(&id, patch.as_ref()) {
            acc.not_restored.insert(key, set_error_value(*e));
            continue;
        }
        ready.push((key, id, patch));
    }

    let ready = order_restores::<O, B>(backend, caller, account_id, ready, hooks, &mut acc).await?;
    for (key, id, patch) in ready {
        match backend
            .restore_object::<O>(caller, account_id, &id, patch)
            .await
        {
            Ok(restored) => {
                let stored = serialize_value(&restored.stored)?;
                acc.restored.insert(
                    id.as_ref().to_owned(),
                    restored_diff(&restored.baseline, &stored),
                );
                acc.mutated = true;
            }
            Err(e) => {
                acc.not_restored.insert(key, backend_set_error_value(e));
            }
        }
    }
    Ok(acc)
}

/// Order `ready` so a restore that refers to another restore key runs
/// after it (-04 §5.1). Entries in a cycle go to `notRestored`.
async fn order_restores<O, B>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    ready: Vec<(String, Id, Option<PatchObject>)>,
    hooks: &dyn RestoreHooks,
    acc: &mut RestoreAccumulators,
) -> Result<Vec<(String, Id, Option<PatchObject>)>, JmapError>
where
    O: HistoryObject + SetObject,
    B: ObjectHistoryBackend,
{
    if ready.len() < 2 {
        return Ok(ready);
    }
    let ids: Vec<Id> = ready.iter().map(|(_, id, _)| id.clone()).collect();
    let mut args = HistoryGetArguments::new();
    args.include_destroyed = true;
    let finals = backend
        .get_object_history::<O>(
            caller,
            account_id,
            HistoryGetParams::new(&args).ids(Some(&ids)),
        )
        .await
        .map_err(JmapError::from)?;
    let keys: BTreeSet<&Id> = ids.iter().collect();
    let mut deps: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for entry in &finals.entries {
        let Ok(Value::Object(map)) = serde_json::to_value(&entry.object) else {
            continue;
        };
        let Some(id) = map.get("id").and_then(Value::as_str).map(Id::from) else {
            continue;
        };
        let refs: Vec<Id> = hooks
            .same_type_references(&map)
            .into_iter()
            .filter(|r| r != &id && keys.contains(r))
            .collect();
        deps.entry(id).or_default().extend(refs);
    }
    if deps.values().all(Vec::is_empty) {
        return Ok(ready);
    }
    // Kahn's algorithm, taking ready entries in their original order.
    let mut pending = ready;
    let mut done: BTreeSet<Id> = BTreeSet::new();
    let mut out = Vec::with_capacity(pending.len());
    loop {
        let before = pending.len();
        let mut rest = Vec::new();
        for entry in pending {
            let blocked = deps
                .get(&entry.1)
                .is_some_and(|d| d.iter().any(|r| !done.contains(r)));
            if blocked {
                rest.push(entry);
            } else {
                done.insert(entry.1.clone());
                out.push(entry);
            }
        }
        pending = rest;
        if pending.is_empty() {
            return Ok(out);
        }
        if pending.len() == before {
            for (key, _, _) in pending {
                acc.not_restored.insert(
                    key,
                    set_error_value(
                        SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(vec!["id".to_owned()])
                            .with_description("restore cycle among same-type references"),
                    ),
                );
            }
            return Ok(out);
        }
    }
}

/// A history-aware `Foo/set` around `plain`, the type's own `/set`
/// handler, which runs unchanged for creates, updates and destroys.
///
/// It takes `restore` out of the arguments ([`parse_restore_argument`]),
/// moves each create or update naming `objectHistory` to
/// `notCreated`/`notUpdated` (§7.3, §7.4), and, when there are restores:
/// counts them toward `maxObjectsInSet` (-04 §5.1), checks `ifInState`
/// against the state before them and removes it, runs
/// [`run_restore_phase`] "before any create, update, or destroy" (§5.1),
/// calls `plain`, and puts the state read before the restores in
/// `oldState`. With the URI in `using` the response always has `restored`
/// and `notRestored` ([`append_restore_results`]).
///
/// A method-level error from `plain` after restores were applied is
/// returned as is; the restores stay applied unless the call runs in a
/// conditional `atomic` scope.
///
/// # Errors
///
/// Those of `plain`, [`parse_restore_argument`] and [`run_restore_phase`];
/// `accountNotFound`, `stateMismatch` and `requestTooLarge` when there are
/// restores.
pub async fn history_set<O, B, F, Fut>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
    hooks: &dyn RestoreHooks,
    plain: F,
) -> Result<HandlerOutput, JmapError>
where
    O: HistoryObject + SetObject,
    B: ObjectHistoryBackend,
    F: FnOnce(Value) -> Fut,
    Fut: std::future::Future<Output = Result<HandlerOutput, JmapError>>,
{
    let Value::Object(mut map) = args else {
        return plain(args).await;
    };
    let restore = parse_restore_argument(&mut map, using)?;
    let mut acc = RestoreAccumulators::default();
    let mut old_state = None;
    if !restore.is_empty() {
        let account_id = match map.get("accountId").and_then(Value::as_str) {
            Some(s) => Id::new_validated(s)
                .map_err(|e| invalid(format!("accountId is not a valid Id: {e}")))?,
            None => return Err(invalid("accountId is required")),
        };
        if !backend
            .account_exists(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            return Err(JmapError::account_not_found());
        }
        enforce_max_objects_in_set_with_restore(
            &map,
            restore.len(),
            backend.max_objects_in_set(caller, &account_id),
        )?;
        let state = backend
            .get_state::<O>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        match map.remove("ifInState") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s == state.as_ref() => {}
            Some(Value::String(_)) => return Err(JmapError::state_mismatch()),
            Some(v) => {
                return Err(invalid(format!(
                    "ifInState: expected a string or null, got {}",
                    json_type_name(&v)
                )));
            }
        }
        acc = run_restore_phase::<O, B>(backend, caller, &account_id, restore, hooks).await?;
        old_state = Some(state);
    }
    let mut early_not_created = Map::new();
    let mut early_not_updated = Map::new();
    for (key, out) in [
        ("create", &mut early_not_created),
        ("update", &mut early_not_updated),
    ] {
        if let Some(Value::Object(entries)) = map.get_mut(key) {
            let named: Vec<String> = entries
                .iter()
                .filter(|(_, v)| v.as_object().is_some_and(names_object_history))
                .map(|(k, _)| k.clone())
                .collect();
            for k in named {
                entries.remove(&k);
                out.insert(k, set_error_value(object_history_set_error()));
            }
        }
    }
    let mut out = plain(Value::Object(map)).await?;
    if let Value::Object(primary) = &mut out.primary {
        if let Some(state) = old_state
            && primary.contains_key("oldState")
        {
            primary.insert("oldState".to_owned(), json!(state.as_ref()));
        }
        for (key, early) in [
            ("notCreated", early_not_created),
            ("notUpdated", early_not_updated),
        ] {
            if early.is_empty() {
                continue;
            }
            let slot = primary.entry(key.to_owned()).or_insert(Value::Null);
            if !slot.is_object() {
                *slot = Value::Object(Map::new());
            }
            if let Value::Object(m) = slot {
                m.extend(early);
            }
        }
    }
    if uses_history(using) {
        append_restore_results(&mut out.primary, &acc);
    }
    Ok(out)
}

/// The SetError for `objectHistory` in a create, update or restore patch:
/// it is reserved (§7.3, §7.4) and never stored, so `invalidProperties`.
pub fn object_history_set_error() -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties(vec![OBJECT_HISTORY_PROPERTY.to_owned()])
        .with_description("objectHistory is returned by Foo/get only")
}

/// Whether a create object or update patch names `objectHistory` (or a
/// path below it).
pub fn names_object_history(object: &Map<String, Value>) -> bool {
    object
        .keys()
        .any(|k| k == OBJECT_HISTORY_PROPERTY || k.starts_with("objectHistory/"))
}

/// Apply a restore `patch` to `final_version` (§5.1: the patch "follows the
/// same rules as a PatchObject in an update in Foo/set"), all or nothing.
///
/// A patch that changes a [`SetObject::SERVER_SET`] or
/// [`SetObject::IMMUTABLE`] property is `invalidProperties` naming each;
/// one that repeats the final version's value is allowed (-04: "a client
/// can send a whole version of the object as the patch"). A bad path is
/// `invalidPatch`.
///
/// # Errors
///
/// The SetError above.
pub fn apply_restore_patch<O: SetObject>(
    final_version: &Map<String, Value>,
    patch: Option<&PatchObject>,
) -> Result<Map<String, Value>, Box<SetError>> {
    let mut out = final_version.clone();
    let Some(patch) = patch else {
        return Ok(out);
    };
    apply_patch_object(&mut out, patch.as_map())
        .map_err(|e| Box::new(set_error(SetErrorType::InvalidPatch, e.to_string())))?;
    let mut changed: Vec<String> = O::SERVER_SET
        .iter()
        .chain(O::IMMUTABLE)
        .filter(|p| **p != "id" || patch.as_map().contains_key("id"))
        .filter(|p| final_version.get(**p) != out.get(**p))
        .map(|p| (*p).to_owned())
        .collect();
    changed.sort();
    changed.dedup();
    if changed.is_empty() {
        Ok(out)
    } else {
        Err(Box::new(
            SetError::new(SetErrorType::InvalidProperties)
                .with_properties(changed)
                .with_description("a restore may not change a server-set or immutable property"),
        ))
    }
}

/// The `restored` value for one object (§5.1): the top-level properties of
/// `stored` whose values differ from `baseline` (the final version with the
/// patch applied), or `null` when none do. `id` is never listed.
pub fn restored_diff(baseline: &Map<String, Value>, stored: &Value) -> Value {
    let Value::Object(stored) = stored else {
        return Value::Null;
    };
    let diff: Map<String, Value> = stored
        .iter()
        .filter(|(k, v)| *k != "id" && baseline.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if diff.is_empty() {
        Value::Null
    } else {
        Value::Object(diff)
    }
}

/// Add `restored` and `notRestored` to a `Foo/set` response (§5.1: "The
/// response has the following additional properties"), each `null` when
/// empty. Call it whenever the URI is in `using`.
pub fn append_restore_results(primary: &mut Value, acc: &RestoreAccumulators) {
    let Value::Object(map) = primary else {
        return;
    };
    let as_value = |m: &Map<String, Value>| {
        if m.is_empty() {
            Value::Null
        } else {
            Value::Object(m.clone())
        }
    };
    map.insert("restored".to_owned(), as_value(&acc.restored));
    map.insert("notRestored".to_owned(), as_value(&acc.not_restored));
}

/// `maxObjectsInSet` over creates, updates, destroys and `restore_count`
/// restores (-04 §5.1: "Each entry in restore counts as one object").
///
/// # Errors
///
/// `requestTooLarge` with description `maxObjectsInSet`.
pub fn enforce_max_objects_in_set_with_restore(
    args: &Map<String, Value>,
    restore_count: usize,
    max: u64,
) -> Result<(), JmapError> {
    let len = |k: &str| -> u64 {
        let n = match args.get(k) {
            Some(Value::Object(m)) => m.len(),
            Some(Value::Array(a)) => a.len(),
            _ => 0,
        };
        u64::try_from(n).unwrap_or(u64::MAX)
    };
    let count = len("create")
        .saturating_add(len("update"))
        .saturating_add(len("destroy"))
        .saturating_add(u64::try_from(restore_count).unwrap_or(u64::MAX));
    if count > max {
        let mut err = JmapError::request_too_large();
        err.description = Some("maxObjectsInSet".into());
        return Err(err);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatcher marks
// ---------------------------------------------------------------------------

impl<C: Clone + Send + 'static> Dispatcher<C> {
    /// Register a history-aware handler for `method` and mark it, for
    /// [`object_history_gaps`](Self::object_history_gaps). A later plain
    /// [`register`](Self::register) of the same name clears the mark.
    pub fn register_history_aware(
        &mut self,
        method: impl Into<String>,
        handler: std::sync::Arc<dyn JmapHandler<C>>,
    ) {
        let method = method.into();
        self.register(method.clone(), handler);
        self.history_aware.insert(method);
    }

    /// The registered `/get`, `/set`, `/query` and `/queryChanges` methods
    /// of `supported_types` that are not history-aware, sorted. A server
    /// MUST NOT list a type in `supportedTypes` while it has a gap here,
    /// for example after a plain `register_<x>_handlers` ran last and
    /// silently dropped `restore`.
    pub fn object_history_gaps(&self, supported_types: &[&str]) -> Vec<String> {
        let mut gaps: Vec<String> = supported_types
            .iter()
            .flat_map(|t| {
                ["get", "set", "query", "queryChanges"]
                    .into_iter()
                    .map(move |m| format!("{t}/{m}"))
            })
            .filter(|m| self.handlers.contains_key(m) && !self.history_aware.contains(m))
            .collect();
        gaps.sort();
        gaps
    }
}

// ---------------------------------------------------------------------------
// Dispatcher-wide check without the URI (decision 18)
// ---------------------------------------------------------------------------

/// Decision 18: without the object-history URI in `using`, reject a
/// history argument that asks for history, and remove the ones at their
/// defaults, before any handler runs, plain or history-aware.
///
/// `*/get`: `includeReplaced`, `includeDestroyed` other than `false` or
/// `null`. `*/query` and `*/queryChanges` (-04): `onlyDestroyed` other than
/// `false` or `null`, `destroyedAfter`/`destroyedBefore` other than `null`.
/// `*/set`: `restore` other than `null` or `{}`. `historyAfter`,
/// `historyBefore`, `historySkip` and `historyLimit` are left alone: §3
/// ignores them when both flags are false. With the URI, nothing happens.
pub(crate) fn check_history_without_uri(
    method: &str,
    args: &mut Value,
    using: &[String],
) -> Result<(), JmapError> {
    if uses_history(using) {
        return Ok(());
    }
    let Value::Object(map) = args else {
        return Ok(());
    };
    let Some((_, verb)) = method.rsplit_once('/') else {
        return Ok(());
    };
    let keys: &[&str] = match verb {
        "get" => &["includeReplaced", "includeDestroyed"],
        "query" | "queryChanges" => &["onlyDestroyed", "destroyedAfter", "destroyedBefore"],
        "set" => &["restore"],
        _ => return Ok(()),
    };
    for key in keys {
        let at_default = match map.get(*key) {
            None | Some(Value::Null) => true,
            Some(Value::Bool(false)) => !key.starts_with("destroyed") && *key != "restore",
            Some(Value::Object(m)) => *key == "restore" && m.is_empty(),
            Some(_) => false,
        };
        if !at_default {
            return Err(without_uri(key));
        }
    }
    for key in keys {
        map.remove(*key);
    }
    Ok(())
}
