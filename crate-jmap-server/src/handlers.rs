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
use serde_json::{json, Value};

use crate::backend::{GetObject, JmapBackend, JmapObject, QueryObject};
use crate::helpers::{
    enforce_max_objects_in_get, extract_account_id, not_found_json, optional_arg,
    query_anchor_args, serialize_value,
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
/// site before invoking this helper; no logging happens here. The
/// crate's sealed dep set (workspace AGENTS.md) excludes `tracing`,
/// so a built-in log line is not on the table.
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

// ---------------------------------------------------------------------------
// handle_changes
// ---------------------------------------------------------------------------

/// Generic `*/changes` handler (RFC 8620 §5.2).
///
/// This implementation always returns `updatedProperties: null` (see RFC 8620
/// §5.2 for the field's semantics). For types with frequently-updated
/// server-computed counts (e.g. Mailbox `totalEmails`, `unreadEmails`), a
/// production backend MAY override or post-process the response to set
/// `updatedProperties` to the list of count fields when only those changed.
/// When non-null, compliant clients skip re-fetching non-count properties,
/// reducing traffic on large inboxes. Backends that do not track per-property
/// change detail MUST leave it null — returning an empty array would be
/// incorrect (that means "nothing about the listed objects actually changed").
pub async fn handle_changes<O: JmapObject, B: JmapBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
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

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let result = backend
        .get_changes::<O>(caller, &account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": Value::Null,
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
                    "limit: expected a non-negative integer, got {v}"
                )))
            }
        },
    };

    let position: i64 = match args.get("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let (anchor, anchor_offset) = query_anchor_args(&args)?;

    let filter: Option<O::Filter> =
        optional_arg(&mut args, "filter", JmapError::unsupported_filter)?;

    let sort: Option<Vec<O::Comparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_objects::<O>(
            caller,
            &account_id,
            filter.as_ref(),
            sort.as_deref(),
            limit,
            position,
            anchor.as_ref(),
            anchor_offset,
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
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
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

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let up_to_id: Option<Id> = match args.get("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(_) => {
            return Err(JmapError::invalid_arguments(
                "upToId must be a string Id or null",
            ))
        }
    };

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let filter: Option<O::Filter> =
        optional_arg(&mut args, "filter", JmapError::unsupported_filter)?;

    let sort: Option<Vec<O::Comparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_changes::<O>(
            caller,
            &account_id,
            &since_query_state,
            filter.as_ref(),
            sort.as_deref(),
            max_changes,
            up_to_id.as_ref(),
            false, // collapse_threads: only meaningful for Email/queryChanges
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
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
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
        BackendChangesError, BackendQueryError, ChangesResult, QueryChangesResult,
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
            _since_state: &State,
            _max_changes: Option<u64>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only query_objects is exercised")
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _limit: Option<u64>,
            _position: i64,
            _anchor: Option<&Id>,
            _anchor_offset: i64,
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
            _since_query_state: &State,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _max_changes: Option<u64>,
            _up_to_id: Option<&Id>,
            _collapse_threads: bool,
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
            _since_state: &State,
            _max_changes: Option<u64>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            unreachable!("only /get is exercised")
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _limit: Option<u64>,
            _position: i64,
            _anchor: Option<&Id>,
            _anchor_offset: i64,
        ) -> Result<crate::backend::QueryResult, BackendQueryError<Self::Error>> {
            unreachable!("only /get is exercised")
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            _account_id: &Id,
            _since_query_state: &State,
            _filter: Option<&O::Filter>,
            _sort: Option<&[O::Comparator]>,
            _max_changes: Option<u64>,
            _up_to_id: Option<&Id>,
            _collapse_threads: bool,
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
            Err(e) => assert_eq!(e.error_type.as_str(), "requestTooLarge"),
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
}
