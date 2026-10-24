//! Generic JMAP method handlers shared across all server crates.
//!
//! Each function handles one RFC 8620 operation type for any object type `O`
//! and any backend `B: JmapBackend`. Domain crates call these for types that
//! have no domain-specific logic beyond the standard wire protocol.
//!
//! # Backend-error leak policy (bd:JMAP-wlip.2)
//!
//! Every handler in this module that maps a [`JmapBackend::Error`] to a
//! wire-format [`JmapError::server_fail`] MUST use the static description
//! [`SERVER_FAIL_INTERNAL_DESC`] rather than interpolating the backend
//! error's [`Display`](std::fmt::Display) output. The backend-error
//! contract on [`JmapBackend::Error`] (`crate::backend::JmapBackend`'s
//! associated-type doc comment) forbids credential / blob / PII in
//! `Display`, but a single accidental violation by a backend implementor
//! would land the leaked text in `serverFail.description` on every
//! affected response. Stripping the description at the handler layer
//! changes that from a wire-format security incident into a server-side
//! diagnostic gap that the operator can close with its own structured
//! logger wrapping the backend call.
//!
//! Extension `*-server` crates with their own per-method handlers
//! SHOULD follow the same pattern; the helper [`server_fail_from_backend`]
//! exists so each call site is one line and reviewable at a glance.

use crate::HandlerOutput;
use jmap_types::{Id, JmapError, State};
use serde_json::{Value, json};

use crate::backend::{
    BackendChangesError, ChangesParams, GetObject, JmapBackend, JmapObject, QueryChangesParams,
    QueryObject, QueryParams,
};
use crate::helpers::{
    MetadataProjection, enforce_max_objects_in_get, extract_account_id, filter_arg, not_found_json,
    optional_arg, query_anchor_args, serialize_value,
};

/// Static description used for every `serverFail` invocation that wraps a
/// [`JmapBackend::Error`] (bd:JMAP-wlip.2).
///
/// RFC 8620 §3.6.2 explicitly permits omitting the description; a static
/// "internal error" is RFC-compliant and forecloses the backend-error
/// Display leak path documented on `JmapBackend::Error`.
pub const SERVER_FAIL_INTERNAL_DESC: &str = "internal error";

/// Construct a [`JmapError::server_fail`] for a backend-originated error
/// without echoing the backend error's [`Display`](std::fmt::Display) output
/// onto the wire (bd:JMAP-wlip.2).
///
/// **The `err` parameter is intentionally discarded** (bd:JMAP-jfia.22).
/// It exists only to keep the call site ergonomic
/// (`.map_err(|e| server_fail_from_backend(&e))`) — the function never
/// reads it, logs it, or stashes it. Callers that want their backend
/// error visible in operator logs MUST log it explicitly at the call
/// site before invoking this helper; no logging happens here, because
/// a backend error's text can carry data that must not reach logs.
///
/// The backend error parameter is accepted by reference (and discarded) so
/// callers retain it for their own structured logging if they wire one. The
/// returned `JmapError` always carries the static
/// [`SERVER_FAIL_INTERNAL_DESC`] description; no caller-controlled text
/// reaches the wire from this helper.
///
/// The function is generic over any `Display` (not just
/// `JmapBackend::Error`) so the extension `*-server` crates' own per-method
/// handlers — which mix [`JmapBackend::Error`], domain-specific error
/// envelopes (`BackendSetError::Other`, `BackendChangesError::Other`), and
/// trait-method errors — can call it uniformly.
///
/// # Use at every site that maps a backend error to `serverFail`
///
/// Replace:
///
/// ```ignore
/// .map_err(|e| JmapError::server_fail(e.to_string()))
/// ```
///
/// with:
///
/// ```ignore
/// .map_err(|e| server_fail_from_backend(&e))
/// ```
pub fn server_fail_from_backend<E: std::fmt::Display + ?Sized>(_err: &E) -> JmapError {
    JmapError::server_fail(SERVER_FAIL_INTERNAL_DESC)
}

/// Map a [`JmapBackend::get_changes`](crate::JmapBackend::get_changes)
/// error to the `Foo/changes` method error (RFC 8620 §5.2).
///
/// `tooManyChanges` is a `Foo/queryChanges` error only (§5.6); §5.2 lists
/// `cannotCalculateChanges` as the sole changes-specific error, and a
/// server over `maxChanges` that "is unable to calculate an intermediate
/// state ... MUST return a "cannotCalculateChanges" error response
/// instead". So [`BackendChangesError::TooManyChanges`] becomes
/// `cannotCalculateChanges` here; every other variant maps as its
/// [`From`] impl does. Every `/changes` handler uses this instead of
/// `JmapError::from`; `/queryChanges` handlers keep `JmapError::from`.
pub fn changes_error_from_backend<E: std::error::Error>(err: BackendChangesError<E>) -> JmapError {
    match err {
        BackendChangesError::TooManyChanges { .. } => JmapError::cannot_calculate_changes(),
        other => JmapError::from(other),
    }
}

/// Construct the per-id `serverFail` [`Value`] used inside the
/// `notCreated`/`notUpdated`/`notDestroyed` maps of `/set` responses
/// (bd:JMAP-ic0j.68), without echoing the backend error's
/// [`Display`](std::fmt::Display) output onto the wire.
///
/// This is the [`Value`]-shaped sibling of [`server_fail_from_backend`].
/// The handler-layer helper returns a [`JmapError`] which is only useful
/// where the entire method invocation fails. Per-id `/set` failures are
/// expressed as a [`serde_json::Value`] keyed under each failing id in
/// `notCreated` / `notUpdated` / `notDestroyed`; the existing
/// `JmapError`-shaped helper does not fit those sites, so each crate
/// previously hand-rolled
/// `json!({ "type": "serverFail", "description": e.to_string() })` —
/// every one of which leaks the backend error's `Display` onto the wire,
/// in direct violation of the [`JmapBackend::Error`](crate::JmapBackend)
/// Display MUST-NOT contract.
///
/// **The `err` parameter is intentionally discarded** (bd:JMAP-jfia.22),
/// matching the contract on [`server_fail_from_backend`]. It exists only
/// to keep the call site ergonomic — the function never reads it, logs
/// it, or stashes it. Callers that want their backend error visible in
/// operator logs MUST log it explicitly at the call site before invoking
/// this helper.
///
/// The function is generic over any [`Display`](std::fmt::Display) so it
/// applies uniformly to [`JmapBackend::Error`](crate::JmapBackend),
/// [`BackendSetError::Other`](crate::BackendSetError),
/// [`BackendChangesError::Other`](crate::BackendChangesError), and any
/// extension-trait-specific error envelope. It also accepts the
/// `&String` shape produced by some legacy helpers that flattened a
/// backend error to `String` before propagating.
///
/// # Use at every per-id /set serverFail site
///
/// Replace:
///
/// ```ignore
/// json!({ "type": "serverFail", "description": e.to_string() })
/// ```
///
/// with:
///
/// ```ignore
/// server_fail_value_from_backend(&e)
/// ```
pub fn server_fail_value_from_backend<E: std::fmt::Display + ?Sized>(_err: &E) -> Value {
    json!({
        "type": "serverFail",
        "description": SERVER_FAIL_INTERNAL_DESC,
    })
}

// ---------------------------------------------------------------------------
// handle_get
// ---------------------------------------------------------------------------

/// Generic `*/get` handler (RFC 8620 §5.1).
///
/// Fetches objects by id (or all objects when `ids` is absent or `null`) and
/// returns the standard `get` response shape. Returns the method-level
/// `requestTooLarge` error when `ids` holds more entries than
/// [`JmapBackend::max_objects_in_get`] allows; a null `ids` is not counted.
pub async fn handle_get<O: GetObject, B: JmapBackend>(
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

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<O>(caller, &account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<O>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // bd:JMAP-jfia.10 — batch-serialize the entire Vec<O> rather than
    // calling to_value per element. One serializer construction
    // instead of N. For Mailbox/get / Email/get over large accounts
    // (~100k+ objects) the saving is measurable. serde_json::to_value
    // on a Vec<O> always produces Value::Array(Vec<Value>) so the
    // wire shape is identical.
    let list_json = serialize_value(&list)?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    })))
}

/// [`handle_get`] for a type the backend may list for
/// draft-ietf-jmap-metadata-04 ([`JmapBackend::metadata_info`]).
///
/// `using` is the request's capability list ([`crate::CallContext::using`]).
/// When `O` is listed in `accountId`, `metadata` and `privateMetadata`
/// follow [`MetadataProjection`]: returned as an object (`{}` when nothing
/// is stored), included by default only with
/// `urn:ietf:params:jmap:metadata` in `using`, `metadata/<namespace>`
/// subselectors applied, and `privateMetadata` never returned when
/// `supportsPrivate` is false (-04 §2, §3, §3.1). The backend is asked for
/// the root property in place of each subselector. When `O` is not listed
/// this is exactly [`handle_get`], so an unrelated property that happens to
/// be named `metadata` is left alone.
///
/// # Errors
///
/// Those of [`handle_get`], and `invalidArguments` for a malformed
/// `metadata/` or `privateMetadata/` entry (-04 §3.1).
pub async fn handle_get_with_metadata<O: GetObject, B: JmapBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let info = args
        .get("accountId")
        .and_then(Value::as_str)
        .and_then(|s| Id::new_validated(s).ok())
        .and_then(|id| backend.metadata_info::<O>(caller, &id));
    let Some(info) = info else {
        return handle_get::<O, B>(backend, caller, args).await;
    };
    let properties: Option<Vec<String>> = match args.get("properties") {
        None | Some(Value::Null) => None,
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(props) => Some(props),
            // handle_get reports the malformed argument.
            Err(_) => return handle_get::<O, B>(backend, caller, args).await,
        },
    };
    let projection = MetadataProjection::new(properties.as_deref(), true, using, Some(&info))?;
    if let Some(props) = properties {
        let mut roots: Vec<String> = Vec::with_capacity(props.len());
        for p in props {
            let root = match p.split_once('/') {
                Some((r @ ("metadata" | "privateMetadata"), _)) => r.to_owned(),
                _ => p,
            };
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        args["properties"] = json!(roots);
    }
    let mut out = handle_get::<O, B>(backend, caller, args).await?;
    if let Some(Value::Array(list)) = out.primary.get_mut("list") {
        for item in list {
            let source = item.clone();
            projection.apply(&source, item);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// handle_changes
// ---------------------------------------------------------------------------

/// Generic `*/changes` handler (RFC 8620 §5.2).
///
/// `using` is the request's capability list ([`crate::CallContext::using`]).
/// When it includes `urn:ietf:params:jmap:metadata`, the optional
/// `ignoreMetadataOnlyChanges` argument (draft-ietf-jmap-metadata-04 §3.3)
/// is parsed, and must be a boolean; otherwise the backend is told to
/// ignore metadata-only changes, because -04 §3 says such a request "MUST
/// behave as if ignoreMetadataOnlyChanges were true".
///
/// `updatedProperties` is [`ChangesResult::updated_properties`]
/// (RFC 8621 §2.2, -04 §3.3), and `null` whenever `updated` is empty
/// (-04 §3.3: "It is always present, and is null when updated is empty").
/// Backends that do not track per-property change detail MUST leave it
/// `None`: an empty array would mean "nothing about the listed objects
/// actually changed".
///
/// [`ChangesResult::updated_properties`]: crate::ChangesResult::updated_properties
pub async fn handle_changes<O: JmapObject, B: JmapBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes = crate::parse_max_changes(args.get("maxChanges"))?;

    let uses_metadata = using
        .iter()
        .any(|u| u == jmap_metadata_types::JMAP_METADATA_URI);
    let ignore_metadata_only_changes = !uses_metadata
        || match args.get("ignoreMetadataOnlyChanges") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(v) => {
                return Err(JmapError::invalid_arguments(format!(
                    "ignoreMetadataOnlyChanges: expected a boolean, got {}",
                    crate::helpers::json_type_name(v)
                )));
            }
        };

    let result = backend
        .get_changes::<O>(
            caller,
            &account_id,
            ChangesParams::new(&since_state)
                .max_changes(max_changes)
                .ignore_metadata_only_changes(ignore_metadata_only_changes),
        )
        .await
        .map_err(changes_error_from_backend)?;
    let updated_properties = if result.updated.is_empty() {
        None
    } else {
        result.updated_properties
    };

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": updated_properties,
        // bd:JMAP-wlip.28 — Vec<Id> serializes directly via Id's
        // #[serde(transparent)] impl; no intermediate &str Vec needed.
        "created":   result.created,
        "updated":   result.updated,
        "destroyed": result.destroyed,
    })))
}

// ---------------------------------------------------------------------------
// handle_query
// ---------------------------------------------------------------------------

/// Generic `*/query` handler (RFC 8620 §5.5).
///
/// Parses filter and sort from args as `O::Filter` and `O::Comparator`, and
/// `anchor`/`anchorOffset` via [`query_anchor_args`], then delegates to
/// [`JmapBackend::query_objects`]. A
/// [`BackendQueryError::Method`](crate::backend::BackendQueryError::Method)
/// is returned to the client unchanged; any other backend error becomes the
/// redacted `serverFail`.
pub async fn handle_query<O: QueryObject, B: JmapBackend>(
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

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.get("limit") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {}",
                    crate::helpers::json_type_name(v)
                )));
            }
        },
    };

    let position: i64 = match args.get("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "position: expected an integer, got {}",
                crate::helpers::json_type_name(v)
            ))
        })?,
    };

    let (anchor, anchor_offset) = query_anchor_args(&args)?;

    let filter: Option<O::Filter> = filter_arg(&mut args)?;

    let sort: Option<Vec<O::Comparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_objects::<O>(
            caller,
            &account_id,
            QueryParams::new()
                .filter(filter.as_ref())
                .sort(sort.as_deref())
                .limit(limit)
                .position(position)
                .anchor(anchor.as_ref())
                .anchor_offset(anchor_offset),
        )
        .await
        .map_err(JmapError::from)?;

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        // bd:JMAP-wlip.28 — Vec<Id> serializes directly via Id's
        // #[serde(transparent)] impl.
        "ids": result.ids,
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// handle_query_changes
// ---------------------------------------------------------------------------

/// Generic `*/queryChanges` handler (RFC 8620 §5.6).
///
/// Parses filter and sort from args, then delegates to
/// [`JmapBackend::query_changes`] with `collapse_threads: false`. For
/// `Email/queryChanges` (which may need `collapseThreads: true`), use the
/// domain-specific handler in jmap-mail-server instead.
pub async fn handle_query_changes<O: QueryObject, B: JmapBackend>(
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

    let since_query_state: State = match args.get("sinceQueryState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceQueryState is required")),
    };

    let max_changes = crate::parse_max_changes(args.get("maxChanges"))?;

    let up_to_id: Option<Id> = match args.get("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(_) => {
            return Err(JmapError::invalid_arguments(
                "upToId must be a string Id or null",
            ));
        }
    };

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let filter: Option<O::Filter> = filter_arg(&mut args)?;

    let sort: Option<Vec<O::Comparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_changes::<O>(
            caller,
            &account_id,
            QueryChangesParams::new(&since_query_state)
                .filter(filter.as_ref())
                .sort(sort.as_deref())
                .max_changes(max_changes)
                .up_to_id(up_to_id.as_ref()),
        )
        .await
        .map_err(JmapError::from)?;

    let added: Vec<Value> = result
        .added
        .iter()
        .map(|item| {
            json!({
                "id": item.id.as_ref(),
                "index": item.index,
            })
        })
        .collect();

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        // bd:JMAP-wlip.28 — Vec<Id> serializes directly.
        "removed": result.removed,
        "added": added,
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle (bd:JMAP-wlip.2): [`server_fail_from_backend`] MUST NOT echo
    /// the backend error's `Display` text into the resulting JmapError's
    /// description. The defence-in-depth contract is that even if a
    /// backend implementor accidentally violates the
    /// [`JmapBackend::Error`](crate::JmapBackend) Display MUST-NOT
    /// (credential / blob / PII), the leaked text never reaches the wire.
    ///
    /// Test vector: an error whose Display contains a canary string
    /// resembling a credential leak. The canary literal is hand-built and
    /// not derived from any production type's behaviour.
    #[test]
    fn server_fail_from_backend_drops_display_text() {
        #[derive(Debug)]
        struct LeakyError(&'static str);
        impl std::fmt::Display for LeakyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }
        impl std::error::Error for LeakyError {}

        const CANARY: &str = "TOKEN-DO-NOT-LEAK-c0ffee";
        let err = LeakyError(CANARY);

        let jmap_err = server_fail_from_backend(&err);

        // Serialize to wire shape and assert the canary is absent from
        // every value in the resulting JSON. The error_invocation wraps
        // a JmapError as { "type": "serverFail", "description": "..." }
        // — both fields are wire-visible.
        let wire = serde_json::to_value(&jmap_err).expect("JmapError must serialize");
        let wire_str = wire.to_string();
        assert!(
            !wire_str.contains(CANARY),
            "server_fail_from_backend must not echo backend error Display \
             onto the wire; got {wire_str}"
        );
        // The description MUST be exactly SERVER_FAIL_INTERNAL_DESC.
        assert_eq!(
            wire["description"], SERVER_FAIL_INTERNAL_DESC,
            "description must be the static 'internal error' string"
        );
        assert_eq!(wire["type"], "serverFail");
    }

    /// Oracle: the helper accepts any `Display` — not just
    /// [`JmapBackend::Error`](crate::JmapBackend) — so the extension
    /// `*-server` crates' per-method handlers can use the same call
    /// site for `BackendSetError`, `BackendChangesError`, and any
    /// trait-method-specific error envelope.
    #[test]
    fn server_fail_from_backend_accepts_generic_display() {
        // String, &str, and a custom Display all compile-check that the
        // bound is `Display + ?Sized`.
        let _ = server_fail_from_backend("a string");
        let _ = server_fail_from_backend(&"&str".to_owned());
        let _ = server_fail_from_backend(&42_u64);
    }

    /// Oracle (bd:JMAP-ic0j.68): [`server_fail_value_from_backend`] MUST
    /// NOT echo the backend error's `Display` text into the per-id
    /// `serverFail` Value used inside `/set`'s `notCreated`/`notUpdated`/
    /// `notDestroyed` maps. Mirrors the
    /// [`server_fail_from_backend_drops_display_text`] oracle for the
    /// `JmapError`-shaped sibling helper.
    ///
    /// Test vector: an error whose `Display` contains a canary string
    /// resembling a credential leak. The canary literal is hand-built and
    /// not derived from any production type's behaviour.
    #[test]
    fn server_fail_value_from_backend_drops_display_text() {
        #[derive(Debug)]
        struct LeakyError(&'static str);
        impl std::fmt::Display for LeakyError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }
        impl std::error::Error for LeakyError {}

        const CANARY: &str = "TOKEN-DO-NOT-LEAK-d00d";
        let err = LeakyError(CANARY);

        let wire = server_fail_value_from_backend(&err);

        // The canary MUST NOT appear anywhere in the serialized wire shape.
        let wire_str = wire.to_string();
        assert!(
            !wire_str.contains(CANARY),
            "server_fail_value_from_backend must not echo backend error \
             Display onto the wire; got {wire_str}"
        );
        // Wire shape: { "type": "serverFail", "description": "internal error" }.
        assert_eq!(wire["type"], "serverFail");
        assert_eq!(
            wire["description"], SERVER_FAIL_INTERNAL_DESC,
            "description must be the static 'internal error' string"
        );
    }

    /// Oracle: the helper accepts any `Display` — not just
    /// [`JmapBackend::Error`](crate::JmapBackend) — so the extension
    /// `*-server` crates' per-method handlers can use the same call
    /// site for `BackendSetError::Other`, `BackendChangesError::Other`,
    /// and the `&String` shape produced by some legacy helpers.
    #[test]
    fn server_fail_value_from_backend_accepts_generic_display() {
        // String, &str, &String, and a custom Display all compile-check
        // that the bound is `Display + ?Sized`.
        let _ = server_fail_value_from_backend("a string");
        let _ = server_fail_value_from_backend(&"owned-String".to_owned());
        let owned: String = "owned".to_string();
        let _ = server_fail_value_from_backend(&owned);
        let _ = server_fail_value_from_backend(&42_u64);
    }

    // -----------------------------------------------------------------
    // handle_query: backend method errors pass through (bd:JMAP-yt1w.21)
    // -----------------------------------------------------------------

    use crate::backend::{
        BackendChangesError, BackendQueryError, ChangesResult, QueryChangesParams,
        QueryChangesResult, QueryParams,
    };

    /// Minimal `QueryObject` used only to instantiate [`handle_query`].
    #[derive(serde::Serialize, serde::Deserialize)]
    struct QueryProbe;

    impl JmapObject for QueryProbe {
        const TYPE_NAME: &'static str = "QueryProbe";
        type Property = ();
    }

    impl QueryObject for QueryProbe {
        type Filter = Value;
        type Comparator = Value;
    }

    #[derive(Debug)]
    struct LeakyQueryError(&'static str);

    impl std::fmt::Display for LeakyQueryError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for LeakyQueryError {}

    /// What [`QueryFailBackend::query_objects`] returns.
    enum QueryFailure {
        /// An RFC 8620 §5.5 method error the backend chose deliberately.
        Method(JmapError),
        /// An internal storage-layer failure whose `Display` must never
        /// reach the wire.
        Internal(&'static str),
    }

    /// Backend whose account exists and whose `query_objects` always fails
    /// with the configured [`QueryFailure`]. Every other method is unused.
    struct QueryFailBackend(QueryFailure);

    impl JmapBackend for QueryFailBackend {
        type Error = LeakyQueryError;
        type CallerCtx = ();

        async fn account_exists(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            unreachable!("only query_objects is exercised")
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            unreachable!("only query_objects is exercised")
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: ChangesParams<'_>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only query_objects is exercised")
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: QueryParams<'_, O>,
        ) -> Result<crate::backend::QueryResult, BackendQueryError<Self::Error>> {
            Err(match &self.0 {
                QueryFailure::Method(e) => BackendQueryError::Method(e.clone()),
                QueryFailure::Internal(text) => BackendQueryError::Other(LeakyQueryError(text)),
            })
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only query_objects is exercised")
        }
    }

    /// Run `QueryProbe/query` against a [`QueryFailBackend`] and return the
    /// method-level error serialized in its wire shape.
    async fn query_error_wire(failure: QueryFailure) -> Value {
        let backend = QueryFailBackend(failure);
        let args = json!({
            "accountId": "a1",
            "filter": { "text": "hello" },
            "anchor": "m1",
        });
        let err = match handle_query::<QueryProbe, _>(&backend, &(), args).await {
            Ok(HandlerOutput { primary: resp, .. }) => {
                panic!("query_objects failed but handle_query succeeded: {resp}")
            }
            Err(e) => e,
        };
        serde_json::to_value(&err).expect("JmapError must serialize")
    }

    /// Oracle: RFC 8620 §5.5 (/home/mark/PROJECT/IETF/specs/rfc8620.txt,
    /// Section 5.5 "/query"):
    ///
    /// > The following additional errors may be returned instead of the
    /// > "Foo/query" response:
    /// >
    /// > "unsupportedFilter": The "filter" is syntactically valid, but the
    /// > server cannot process it.
    ///
    /// Whether a syntactically valid filter can be processed is known only
    /// to the backend, so a backend-returned `unsupportedFilter` MUST reach
    /// the client unchanged (type and description), not as `serverFail`.
    #[tokio::test]
    async fn handle_query_passes_backend_unsupported_filter_through() {
        let mut expected = JmapError::unsupported_filter();
        expected.description = Some("Message/query requires a chatId condition".into());

        let wire = query_error_wire(QueryFailure::Method(expected.clone())).await;

        assert_eq!(
            wire,
            serde_json::to_value(&expected).expect("JmapError must serialize"),
            "backend unsupportedFilter must pass through handle_query unchanged"
        );
    }

    /// Oracle: RFC 8620 §5.5 (/home/mark/PROJECT/IETF/specs/rfc8620.txt,
    /// Section 5.5 "/query"):
    ///
    /// > "anchorNotFound": An anchor argument was supplied, but it cannot be
    /// > found in the results of the query.
    ///
    /// Only the backend evaluates the query results, so a backend-returned
    /// `anchorNotFound` MUST reach the client unchanged.
    #[tokio::test]
    async fn handle_query_passes_backend_anchor_not_found_through() {
        let expected = JmapError::anchor_not_found();

        let wire = query_error_wire(QueryFailure::Method(expected.clone())).await;

        assert_eq!(
            wire,
            serde_json::to_value(&expected).expect("JmapError must serialize"),
            "backend anchorNotFound must pass through handle_query unchanged"
        );
    }

    /// Oracle: the [`server_fail_from_backend_drops_display_text`] contract
    /// still holds on the `/query` path. An internal backend error is not
    /// one of the RFC 8620 §5.5 method errors and MUST surface as the
    /// redacted `serverFail`, never echoing the error's `Display`.
    #[tokio::test]
    async fn handle_query_redacts_backend_internal_error() {
        const CANARY: &str = "TOKEN-DO-NOT-LEAK-query-5ca1ab1e";

        let wire = query_error_wire(QueryFailure::Internal(CANARY)).await;

        assert_eq!(wire["type"], "serverFail");
        assert_eq!(wire["description"], SERVER_FAIL_INTERNAL_DESC);
        assert!(
            !wire.to_string().contains(CANARY),
            "internal backend error Display must not reach the wire; got {wire}"
        );
    }

    // -----------------------------------------------------------------
    // handle_get: maxObjectsInGet cap (bd:JMAP-yt1w.31)
    // -----------------------------------------------------------------

    /// Minimal `GetObject` used only to instantiate [`handle_get`].
    #[derive(serde::Serialize, serde::Deserialize)]
    struct GetProbe;

    impl JmapObject for GetProbe {
        const TYPE_NAME: &'static str = "GetProbe";
        type Property = ();
    }

    impl GetObject for GetProbe {}

    /// Backend whose account exists, holds no objects (every requested id
    /// is `notFound`), and keeps the default [`JmapBackend`] caps.
    struct GetProbeBackend;

    impl JmapBackend for GetProbeBackend {
        type Error = LeakyQueryError;
        type CallerCtx = ();

        async fn account_exists(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            Ok((vec![], ids.map(<[Id]>::to_vec).unwrap_or_default()))
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            Ok(State::from("s0"))
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: ChangesParams<'_>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only /get is exercised")
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: QueryParams<'_, O>,
        ) -> Result<crate::backend::QueryResult, BackendQueryError<Self::Error>> {
            unreachable!("only /get is exercised")
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only /get is exercised")
        }
    }

    /// Oracle: RFC 8620 §5.1 "/get" (/home/mark/PROJECT/IETF/specs/rfc8620.txt
    /// L1660-1665):
    ///
    /// > The following additional error may be returned instead of the "Foo/
    /// > get" response:
    /// >
    /// > "requestTooLarge": The number of ids requested by the client exceeds
    /// > the maximum number the server is willing to process in a single
    /// > method call.
    ///
    /// The backend keeps the default cap, mirroring the `max_objects_in_set`
    /// default of 500, which is RFC 8620 §2 (L546-547) "maxObjectsInGet ...
    /// Suggested minimum: 500." Requesting 501 ids MUST yield the
    /// method-level `requestTooLarge` instead of a `GetProbe/get` response.
    #[tokio::test]
    async fn handle_get_over_max_objects_is_request_too_large() {
        let ids: Vec<String> = (0..501).map(|i| format!("id{i}")).collect();
        let args = json!({ "accountId": "a1", "ids": ids });

        match handle_get::<GetProbe, _>(&GetProbeBackend, &(), args).await {
            Ok(HandlerOutput { primary: resp, .. }) => panic!(
                "501 ids exceed the maxObjectsInGet cap, but handle_get \
                 returned a response with {} notFound ids instead of \
                 requestTooLarge",
                resp["notFound"].as_array().map_or(0, Vec::len)
            ),
            Err(e) => assert_eq!(e.error_type.to_string(), "requestTooLarge"),
        }
    }

    /// Oracle: RFC 8620 §5.1 "/get" (/home/mark/PROJECT/IETF/specs/rfc8620.txt
    /// L1596-1601): "ids: "Id[]|null" The ids of the Foo objects to return.
    /// If null, then *all* records of the data type are returned, if this is
    /// supported for that data type and the number of records does not
    /// exceed the "maxObjectsInGet" limit."
    ///
    /// `requestTooLarge` counts "the number of ids requested by the client"
    /// (L1663), and a null `ids` requests none, so the generic handler MUST
    /// NOT reject it as over the cap; the record-count bound for fetch-all
    /// belongs to the backend, which knows how many records exist.
    #[tokio::test]
    async fn handle_get_null_ids_is_not_counted_against_cap() {
        let args = json!({ "accountId": "a1", "ids": null });

        let HandlerOutput { primary: resp, .. } =
            handle_get::<GetProbe, _>(&GetProbeBackend, &(), args)
                .await
                .expect("null ids must not be rejected by the maxObjectsInGet cap");
        assert_eq!(resp["list"], json!([]));
    }

    /// Backend whose account exists and which records the
    /// [`QueryParams`] / [`QueryChangesParams`] it receives as JSON, so a
    /// test can compare them with the request's wire arguments.
    #[derive(Default)]
    struct RecordingQueryBackend(std::sync::Mutex<Option<Value>>);

    impl RecordingQueryBackend {
        fn take(&self) -> Value {
            self.0
                .lock()
                .unwrap()
                .take()
                .expect("backend query method was not called")
        }
    }

    impl JmapBackend for RecordingQueryBackend {
        type Error = LeakyQueryError;
        type CallerCtx = ();

        async fn account_exists(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            unreachable!("only queries are exercised")
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            unreachable!("only queries are exercised")
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: ChangesParams<'_>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only queries are exercised")
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            params: QueryParams<'_, O>,
        ) -> Result<crate::backend::QueryResult, BackendQueryError<Self::Error>> {
            *self.0.lock().unwrap() = Some(json!({
                "filter": params.filter.map(|f| serde_json::to_value(f).unwrap()),
                "sort": params.sort.map(|s| serde_json::to_value(s).unwrap()),
                "limit": params.limit,
                "position": params.position,
                "anchor": params.anchor,
                "anchorOffset": params.anchor_offset,
            }));
            Ok(crate::backend::QueryResult::new(
                vec![],
                0,
                None,
                State::from("q0"),
                false,
            ))
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            params: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            *self.0.lock().unwrap() = Some(json!({
                "sinceQueryState": params.since_query_state,
                "filter": params.filter.map(|f| serde_json::to_value(f).unwrap()),
                "sort": params.sort.map(|s| serde_json::to_value(s).unwrap()),
                "maxChanges": params.max_changes,
                "upToId": params.up_to_id,
                "collapseThreads": params.collapse_threads,
            }));
            Ok(QueryChangesResult::new(
                params.since_query_state.clone(),
                State::from("q2"),
                None,
                vec![],
                vec![],
            ))
        }
    }

    /// Oracle: RFC 8620 §5.5 (rfc8620.txt L2368-2501) defines the
    /// `/query` arguments `filter`, `sort`, `position`, `anchor`,
    /// `anchorOffset` and `limit`. Every one the client sends MUST reach
    /// the backend unchanged in [`QueryParams`].
    #[tokio::test]
    async fn handle_query_passes_every_argument_to_backend() {
        let backend = RecordingQueryBackend::default();
        let args = json!({
            "accountId": "a1",
            "filter": {"text": "hello"},
            "sort": [{"property": "receivedAt", "isAscending": false}],
            "position": 3,
            "anchor": "m7",
            "anchorOffset": -2,
            "limit": 25,
        });
        handle_query::<QueryProbe, _>(&backend, &(), args)
            .await
            .expect("query must succeed");
        assert_eq!(
            backend.take(),
            json!({
                "filter": {"text": "hello"},
                "sort": [{"property": "receivedAt", "isAscending": false}],
                "limit": 25,
                "position": 3,
                "anchor": "m7",
                "anchorOffset": -2,
            })
        );
    }

    /// Oracle: RFC 8620 §5.5 (rfc8620.txt L2471, L2493): `position` and
    /// `anchorOffset` default to 0; `filter`, `sort`, `anchor` and `limit`
    /// are optional. A request with none of them reaches the backend as
    /// those defaults.
    #[tokio::test]
    async fn handle_query_omitted_arguments_reach_backend_as_rfc_defaults() {
        let backend = RecordingQueryBackend::default();
        handle_query::<QueryProbe, _>(&backend, &(), json!({"accountId": "a1"}))
            .await
            .expect("query must succeed");
        assert_eq!(
            backend.take(),
            json!({
                "filter": null,
                "sort": null,
                "limit": null,
                "position": 0,
                "anchor": null,
                "anchorOffset": 0,
            })
        );
    }

    /// Oracle: RFC 8620 §5.6 (rfc8620.txt L2649-2669) defines the
    /// `/queryChanges` arguments `filter`, `sort`, `sinceQueryState`,
    /// `maxChanges` and `upToId`; each MUST reach the backend unchanged in
    /// [`QueryChangesParams`]. `collapseThreads` exists only on
    /// `Email/queryChanges` (RFC 8621 §4.5), so the generic handler passes
    /// `false`.
    #[tokio::test]
    async fn handle_query_changes_passes_every_argument_to_backend() {
        let backend = RecordingQueryBackend::default();
        let args = json!({
            "accountId": "a1",
            "filter": {"text": "hello"},
            "sort": [{"property": "receivedAt"}],
            "sinceQueryState": "q1",
            "maxChanges": 10,
            "upToId": "m9",
        });
        handle_query_changes::<QueryProbe, _>(&backend, &(), args)
            .await
            .expect("queryChanges must succeed");
        assert_eq!(
            backend.take(),
            json!({
                "sinceQueryState": "q1",
                "filter": {"text": "hello"},
                "sort": [{"property": "receivedAt"}],
                "maxChanges": 10,
                "upToId": "m9",
                "collapseThreads": false,
            })
        );
    }

    /// Oracle: RFC 8620 §5.5 (rfc8620.txt L2368) types `filter` as
    /// "FilterOperator|FilterCondition|null", and both are objects
    /// (L2391 "A *FilterCondition* is an "object""); §5.6 (L2649) reuses
    /// that type. A non-object value is therefore "of the wrong type"
    /// (L1189 `invalidArguments`), not a filter that is "syntactically
    /// valid, but the server cannot process it" (L2626
    /// `unsupportedFilter`). The backend is never consulted.
    #[tokio::test]
    async fn non_object_filter_is_invalid_arguments() {
        for filter in [json!(42), json!("text"), json!(true), json!([])] {
            let backend = RecordingQueryBackend::default();
            let err = handle_query::<QueryProbe, _>(
                &backend,
                &(),
                json!({"accountId": "a1", "filter": filter}),
            )
            .await
            .expect_err("non-object filter must be rejected");
            assert_eq!(err.error_type.to_string(), "invalidArguments", "{filter}");

            let err = handle_query_changes::<QueryProbe, _>(
                &backend,
                &(),
                json!({"accountId": "a1", "filter": filter, "sinceQueryState": "q1"}),
            )
            .await
            .expect_err("non-object filter must be rejected");
            assert_eq!(err.error_type.to_string(), "invalidArguments", "{filter}");
            assert!(backend.0.lock().unwrap().is_none(), "{filter}");
        }
    }

    // -----------------------------------------------------------------
    // handle_changes / handle_query_changes / handle_query: response
    // shape and error branches (bd:JMAP-w8fu.17)
    // -----------------------------------------------------------------

    /// What [`CannedBackend`] returns from its `/changes`, `/query` and
    /// `/queryChanges` methods.
    #[derive(Clone, Copy)]
    enum Canned {
        /// The fixed hand-built result of each method.
        Ok,
        /// [`BackendChangesError::CannotCalculate`].
        CannotCalculate,
        /// [`BackendChangesError::TooManyChanges`] with this limit.
        TooManyChanges(u64),
        /// [`BackendChangesError::Other`] whose `Display` is this canary.
        Internal(&'static str),
    }

    impl Canned {
        fn changes_error(self) -> BackendChangesError<LeakyQueryError> {
            match self {
                Canned::Ok => unreachable!("Canned::Ok is not an error"),
                Canned::CannotCalculate => BackendChangesError::CannotCalculate,
                Canned::TooManyChanges(limit) => BackendChangesError::TooManyChanges { limit },
                Canned::Internal(text) => BackendChangesError::Other(LeakyQueryError(text)),
            }
        }
    }

    /// Backend that answers `/changes`, `/query` and `/queryChanges` with
    /// fixed hand-built values (or the configured error), and reports
    /// whether the account exists as configured.
    struct CannedBackend {
        account_exists: bool,
        outcome: Canned,
    }

    const CANNED_OK: CannedBackend = CannedBackend {
        account_exists: true,
        outcome: Canned::Ok,
    };

    impl JmapBackend for CannedBackend {
        type Error = LeakyQueryError;
        type CallerCtx = ();

        async fn account_exists(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<bool, Self::Error> {
            Ok(self.account_exists)
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            unreachable!("/get is not exercised")
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            unreachable!("/get is not exercised")
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: ChangesParams<'_>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            match self.outcome {
                Canned::Ok => Ok(ChangesResult::new(
                    vec![Id::from("c1")],
                    vec![Id::from("u1")],
                    vec![Id::from("d1")],
                    true,
                    State::from("s2"),
                )),
                other => Err(other.changes_error()),
            }
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: QueryParams<'_, O>,
        ) -> Result<crate::backend::QueryResult, BackendQueryError<Self::Error>> {
            Ok(crate::backend::QueryResult::new(
                vec![Id::from("m1")],
                0,
                Some(7),
                State::from("q1"),
                true,
            ))
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            params: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            match self.outcome {
                Canned::Ok => Ok(QueryChangesResult::new(
                    params.since_query_state.clone(),
                    State::from("q2"),
                    Some(7),
                    vec![Id::from("r1")],
                    vec![crate::backend::AddedItem::new(Id::from("n1"), 0)],
                )),
                other => Err(other.changes_error()),
            }
        }
    }

    /// Oracle: RFC 8620 §5.2 (rfc8620.txt L1708-1739) lists the
    /// `/changes` response arguments `accountId`, `oldState` ("the
    /// "sinceState" argument echoed back"), `newState`, `hasMoreChanges`,
    /// `updatedProperties`, `created`, `updated` and `destroyed`. The
    /// expected JSON is hand-written from that list and the backend's
    /// fixed result.
    #[tokio::test]
    async fn handle_changes_response_matches_rfc_shape() {
        let HandlerOutput { primary: resp, .. } = handle_changes::<QueryProbe, _>(
            &CANNED_OK,
            &(),
            json!({"accountId": "a1", "sinceState": "s1"}),
            &[],
        )
        .await
        .expect("/changes must succeed");
        assert_eq!(
            resp,
            json!({
                "accountId": "a1",
                "oldState": "s1",
                "newState": "s2",
                "hasMoreChanges": true,
                "updatedProperties": null,
                "created": ["c1"],
                "updated": ["u1"],
                "destroyed": ["d1"],
            })
        );
    }

    // -----------------------------------------------------------------
    // handle_changes: draft-ietf-jmap-metadata-04 §3, §3.3 (bd:JMAP-cbhv)
    // -----------------------------------------------------------------

    const CORE: &str = "urn:ietf:params:jmap:core";
    const METADATA: &str = "urn:ietf:params:jmap:metadata";

    /// `/changes`-only backend: records the [`ChangesParams`] it was
    /// given and answers with the configured `updated` list and
    /// `updated_properties`.
    struct MetadataChangesBackend {
        updated: Vec<Id>,
        updated_properties: Option<Vec<String>>,
        seen_ignore: std::sync::Mutex<Option<bool>>,
    }

    impl MetadataChangesBackend {
        fn new(updated: &[&str], updated_properties: Option<&[&str]>) -> Self {
            Self {
                updated: updated.iter().map(|u| Id::from(*u)).collect(),
                updated_properties: updated_properties
                    .map(|p| p.iter().map(|s| (*s).to_owned()).collect()),
                seen_ignore: std::sync::Mutex::new(None),
            }
        }
    }

    impl JmapBackend for MetadataChangesBackend {
        type Error = LeakyQueryError;
        type CallerCtx = ();

        async fn account_exists(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _ids: Option<&[Id]>,
            _properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            unreachable!("only get_changes is exercised")
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
        ) -> Result<State, Self::Error> {
            unreachable!("only get_changes is exercised")
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            params: ChangesParams<'_>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            *self.seen_ignore.lock().unwrap() = Some(params.ignore_metadata_only_changes);
            Ok(ChangesResult::new(
                Vec::new(),
                self.updated.clone(),
                Vec::new(),
                false,
                State::from("s2"),
            )
            .with_updated_properties(self.updated_properties.clone()))
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: QueryParams<'_, O>,
        ) -> Result<crate::backend::QueryResult, BackendQueryError<Self::Error>> {
            unreachable!("only get_changes is exercised")
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _params: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only get_changes is exercised")
        }
    }

    /// Run `QueryProbe/changes` with `extra` merged into the arguments and
    /// return the `ignoreMetadataOnlyChanges` value the backend received.
    async fn ignore_seen(using: &[&str], extra: Value) -> bool {
        let backend = MetadataChangesBackend::new(&["u1"], None);
        let mut args = json!({"accountId": "a1", "sinceState": "s1"});
        if let Value::Object(extra) = extra {
            args.as_object_mut().unwrap().extend(extra);
        }
        let using: Vec<String> = using.iter().map(|u| (*u).to_owned()).collect();
        handle_changes::<QueryProbe, _>(&backend, &(), args, &using)
            .await
            .expect("/changes must succeed");
        backend
            .seen_ignore
            .lock()
            .unwrap()
            .expect("backend must be called")
    }

    /// Oracle: draft-ietf-jmap-metadata-04 §3 (draft-ietf-jmap-metadata-04.txt
    /// L460-465): "These extensions apply to requests whose using includes
    /// urn:ietf:params:jmap:metadata. For other requests, ... Foo/changes
    /// MUST behave as if ignoreMetadataOnlyChanges were true." The
    /// argument is not defined for such requests, so a malformed value is
    /// not an error.
    #[tokio::test]
    async fn changes_without_metadata_capability_ignores_metadata_only_changes() {
        assert!(ignore_seen(&[CORE], json!({})).await);
        assert!(ignore_seen(&[CORE], json!({"ignoreMetadataOnlyChanges": false})).await);
        assert!(ignore_seen(&[CORE], json!({"ignoreMetadataOnlyChanges": "no"})).await);
        assert!(ignore_seen(&[], json!({})).await);
    }

    /// Oracle: draft-ietf-jmap-metadata-04 §3.3 (L598):
    /// "*ignoreMetadataOnlyChanges*: Boolean (default: false)".
    #[tokio::test]
    async fn changes_with_metadata_capability_passes_ignore_argument() {
        assert!(!ignore_seen(&[CORE, METADATA], json!({})).await);
        assert!(
            !ignore_seen(
                &[CORE, METADATA],
                json!({"ignoreMetadataOnlyChanges": null})
            )
            .await
        );
        assert!(
            !ignore_seen(
                &[CORE, METADATA],
                json!({"ignoreMetadataOnlyChanges": false})
            )
            .await
        );
        assert!(
            ignore_seen(
                &[CORE, METADATA],
                json!({"ignoreMetadataOnlyChanges": true})
            )
            .await
        );
    }

    /// Oracle: draft-ietf-jmap-metadata-04 §3.3 (L598) types the argument
    /// "Boolean"; RFC 8620 §3.6.2 "invalidArguments: One of the arguments
    /// is of the wrong type".
    #[tokio::test]
    async fn changes_rejects_non_boolean_ignore_argument() {
        let backend = MetadataChangesBackend::new(&["u1"], None);
        let err = handle_changes::<QueryProbe, _>(
            &backend,
            &(),
            json!({"accountId": "a1", "sinceState": "s1", "ignoreMetadataOnlyChanges": "yes"}),
            &[CORE.to_owned(), METADATA.to_owned()],
        )
        .await
        .expect_err("a string ignoreMetadataOnlyChanges must fail");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        assert!(
            backend.seen_ignore.lock().unwrap().is_none(),
            "backend must not be called"
        );
    }

    /// Oracle: draft-ietf-jmap-metadata-04 §3.3 (L624-643): when "the only
    /// properties that have changed on every id in the updated array ...
    /// are metadata and/or privateMetadata, this argument MUST be set to a
    /// list containing only those property names (e.g., ["metadata"]"; and
    /// the §3.3 example response (L1325) carries
    /// `"updatedProperties": ["metadata"]`.
    #[tokio::test]
    async fn changes_reports_backend_updated_properties() {
        let backend = MetadataChangesBackend::new(&["u1"], Some(&["metadata"]));
        let HandlerOutput { primary: resp, .. } = handle_changes::<QueryProbe, _>(
            &backend,
            &(),
            json!({"accountId": "a1", "sinceState": "s1"}),
            &[CORE.to_owned(), METADATA.to_owned()],
        )
        .await
        .expect("/changes must succeed");
        assert_eq!(
            resp,
            json!({
                "accountId": "a1",
                "oldState": "s1",
                "newState": "s2",
                "hasMoreChanges": false,
                "updatedProperties": ["metadata"],
                "created": [],
                "updated": ["u1"],
                "destroyed": [],
            })
        );
    }

    /// Oracle: draft-ietf-jmap-metadata-04 §3.3 (L638-639): updatedProperties
    /// "is always present, and is null when updated is empty".
    #[tokio::test]
    async fn changes_updated_properties_null_when_updated_empty() {
        let backend = MetadataChangesBackend::new(&[], Some(&["metadata"]));
        let HandlerOutput { primary: resp, .. } = handle_changes::<QueryProbe, _>(
            &backend,
            &(),
            json!({"accountId": "a1", "sinceState": "s1"}),
            &[CORE.to_owned(), METADATA.to_owned()],
        )
        .await
        .expect("/changes must succeed");
        assert_eq!(resp["updatedProperties"], Value::Null);
        assert!(
            resp.as_object().unwrap().contains_key("updatedProperties"),
            "updatedProperties must be present"
        );
    }

    /// Oracle: RFC 8620 §5.6 (rfc8620.txt L2699-2730) lists the
    /// `/queryChanges` response arguments; `oldQueryState` is "the
    /// "sinceQueryState" argument echoed back", each `added` entry is an
    /// AddedItem `{id, index}`, and `total` "MUST be omitted if the
    /// "calculateTotal" request argument is not true" (L2714).
    #[tokio::test]
    async fn handle_query_changes_response_matches_rfc_shape() {
        let HandlerOutput { primary: resp, .. } = handle_query_changes::<QueryProbe, _>(
            &CANNED_OK,
            &(),
            json!({"accountId": "a1", "sinceQueryState": "q1", "calculateTotal": true}),
        )
        .await
        .expect("/queryChanges must succeed");
        assert_eq!(
            resp,
            json!({
                "accountId": "a1",
                "oldQueryState": "q1",
                "newQueryState": "q2",
                "total": 7,
                "removed": ["r1"],
                "added": [{"id": "n1", "index": 0}],
            })
        );

        let HandlerOutput { primary: resp, .. } = handle_query_changes::<QueryProbe, _>(
            &CANNED_OK,
            &(),
            json!({"accountId": "a1", "sinceQueryState": "q1"}),
        )
        .await
        .expect("/queryChanges must succeed");
        assert!(
            resp.get("total").is_none(),
            "total must be omitted without calculateTotal; got {resp}"
        );
    }

    /// Oracle: RFC 8620 §5.5 (rfc8620.txt L2604-2608): `total` is "only if
    /// requested" and "MUST be omitted if the "calculateTotal" request
    /// argument is not true", even when the backend computed one.
    #[tokio::test]
    async fn handle_query_total_only_when_requested() {
        for (calculate_total, expected) in [(json!(true), Some(json!(7))), (json!(false), None)] {
            let HandlerOutput { primary: resp, .. } = handle_query::<QueryProbe, _>(
                &CANNED_OK,
                &(),
                json!({"accountId": "a1", "calculateTotal": calculate_total}),
            )
            .await
            .expect("/query must succeed");
            assert_eq!(resp.get("total").cloned(), expected, "{resp}");
        }
    }

    /// Oracle: RFC 8620 §5.2 (rfc8620.txt L1687) and §5.6 (L2657) type
    /// `sinceState` / `sinceQueryState` as a required "String"; a missing
    /// or non-string value is "of the wrong type" (L1189
    /// `invalidArguments`).
    #[tokio::test]
    async fn missing_or_non_string_since_state_is_invalid_arguments() {
        for since in [None, Some(json!(42))] {
            let mut args = json!({"accountId": "a1"});
            if let Some(v) = &since {
                args["sinceState"] = v.clone();
            }
            let err = handle_changes::<QueryProbe, _>(&CANNED_OK, &(), args, &[])
                .await
                .expect_err("/changes without a string sinceState must fail");
            assert_eq!(err.error_type.to_string(), "invalidArguments", "{since:?}");

            let mut args = json!({"accountId": "a1"});
            if let Some(v) = &since {
                args["sinceQueryState"] = v.clone();
            }
            let err = handle_query_changes::<QueryProbe, _>(&CANNED_OK, &(), args)
                .await
                .expect_err("/queryChanges without a string sinceQueryState must fail");
            assert_eq!(err.error_type.to_string(), "invalidArguments", "{since:?}");
        }
    }

    /// Oracle: RFC 8620 §5.6 (rfc8620.txt L2669) types `upToId` as
    /// "Id|null"; any other JSON type is `invalidArguments` (L1189).
    #[tokio::test]
    async fn non_string_up_to_id_is_invalid_arguments() {
        for up_to_id in [json!(5), json!(true), json!(["m1"]), json!({})] {
            let err = handle_query_changes::<QueryProbe, _>(
                &CANNED_OK,
                &(),
                json!({"accountId": "a1", "sinceQueryState": "q1", "upToId": up_to_id}),
            )
            .await
            .expect_err("non-Id upToId must be rejected");
            assert_eq!(err.error_type.to_string(), "invalidArguments", "{up_to_id}");
        }
    }

    /// Oracle: RFC 8620 §5.5 (rfc8620.txt L2501-2508): `limit` is
    /// "UnsignedInt|null" and "If a negative value is given, the call MUST
    /// be rejected with an "invalidArguments" error."
    #[tokio::test]
    async fn handle_query_negative_limit_is_invalid_arguments() {
        let backend = RecordingQueryBackend::default();
        let err =
            handle_query::<QueryProbe, _>(&backend, &(), json!({"accountId": "a1", "limit": -1}))
                .await
                .expect_err("negative limit must be rejected");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        assert!(
            backend.0.lock().unwrap().is_none(),
            "backend must not be consulted"
        );
    }

    /// Oracle: RFC 8620 §5.5 (rfc8620.txt L2471-2480): `position` is "Int"
    /// and "If a negative value is given, it is an offset from the end of
    /// the list", resolved against "the total number of results", which
    /// only the backend knows. The generic handler MUST pass the negative
    /// value through rather than reject or clamp it.
    #[tokio::test]
    async fn handle_query_negative_position_reaches_backend() {
        let backend = RecordingQueryBackend::default();
        handle_query::<QueryProbe, _>(&backend, &(), json!({"accountId": "a1", "position": -5}))
            .await
            .expect("negative position is valid");
        assert_eq!(backend.take()["position"], json!(-5));
    }

    /// Oracle: RFC 8620 §5.2 (rfc8620.txt L1822-1829) lists only
    /// `cannotCalculateChanges` as an error returned "instead of the
    /// Foo/changes response", and L1766-1768: a server over `maxChanges`
    /// that "is unable to calculate an intermediate state ... MUST return a
    /// "cannotCalculateChanges" error response instead"; §5.6 (L2806-2813)
    /// adds `tooManyChanges` for Foo/queryChanges only. An internal backend
    /// failure is neither and MUST surface as the redacted `serverFail`
    /// (the [`server_fail_from_backend_drops_display_text`] contract).
    #[tokio::test]
    async fn changes_backend_errors_map_to_rfc_errors() {
        const CANARY: &str = "TOKEN-DO-NOT-LEAK-changes-b0a710ad";
        for (outcome, changes_type, query_changes_type) in [
            (
                Canned::CannotCalculate,
                "cannotCalculateChanges",
                "cannotCalculateChanges",
            ),
            (
                Canned::TooManyChanges(10),
                "cannotCalculateChanges",
                "tooManyChanges",
            ),
            (Canned::Internal(CANARY), "serverFail", "serverFail"),
        ] {
            let backend = CannedBackend {
                account_exists: true,
                outcome,
            };
            let changes = handle_changes::<QueryProbe, _>(
                &backend,
                &(),
                json!({"accountId": "a1", "sinceState": "s1"}),
                &[],
            )
            .await;
            let query_changes = handle_query_changes::<QueryProbe, _>(
                &backend,
                &(),
                json!({"accountId": "a1", "sinceQueryState": "q1"}),
            )
            .await;
            for (result, expected_type) in
                [(changes, changes_type), (query_changes, query_changes_type)]
            {
                let err = result.expect_err("backend error must fail the call");
                let wire = serde_json::to_value(&err).expect("JmapError must serialize");
                assert_eq!(wire["type"], expected_type, "{wire}");
                assert!(!wire.to_string().contains(CANARY), "leaked: {wire}");
            }
        }
    }

    /// Oracle: RFC 8620 §3.6.2 (rfc8620.txt L1202-1203): "accountNotFound":
    /// The accountId does not correspond to a valid account.
    #[tokio::test]
    async fn unknown_account_is_account_not_found() {
        let backend = CannedBackend {
            account_exists: false,
            outcome: Canned::Ok,
        };
        let results = [
            handle_changes::<QueryProbe, _>(
                &backend,
                &(),
                json!({"accountId": "nope", "sinceState": "s1"}),
                &[],
            )
            .await,
            handle_query::<QueryProbe, _>(&backend, &(), json!({"accountId": "nope"})).await,
            handle_query_changes::<QueryProbe, _>(
                &backend,
                &(),
                json!({"accountId": "nope", "sinceQueryState": "q1"}),
            )
            .await,
        ];
        for result in results {
            let err = result.expect_err("unknown account must fail");
            assert_eq!(err.error_type.to_string(), "accountNotFound");
        }
    }

    /// RFC 8620 §5.5 types `limit` as "UnsignedInt|null" and `position` as
    /// "Int"; wrong types are `invalidArguments` (§3.6.2). The description
    /// names only the JSON type and never echoes the client value
    /// (§3.6.1: "unstructured English text"; bd JMAP-zegc).
    #[tokio::test]
    async fn handle_query_does_not_echo_wrongly_typed_value() {
        for name in ["limit", "position"] {
            let backend = RecordingQueryBackend::default();
            let mut args = json!({ "accountId": "a1" });
            args[name] = json!({ "note": "CANARY-zegc" });
            let err = handle_query::<QueryProbe, _>(&backend, &(), args)
                .await
                .expect_err("wrongly typed argument must be rejected");
            assert_eq!(err.error_type.to_string(), "invalidArguments", "{name}");
            let desc = err.description.as_deref().unwrap_or_default();
            assert!(desc.starts_with(name), "{desc}");
            assert!(desc.contains("got object"), "{desc}");
            assert!(!desc.contains("CANARY"), "{desc}");
        }
    }
}
