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

use jmap_types::{Id, Invocation, JmapError, State};
use serde_json::{json, Value};

use crate::backend::{GetObject, JmapBackend, JmapObject, QueryObject};
use crate::helpers::{extract_account_id, not_found_json, optional_arg, serialize_value};

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

// ---------------------------------------------------------------------------
// handle_get
// ---------------------------------------------------------------------------

/// Generic `*/get` handler (RFC 8620 §5.1).
///
/// Fetches objects by id (or all objects when `ids` is absent or `null`) and
/// returns the standard `get` response shape.
pub async fn handle_get<O: GetObject, B: JmapBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

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

    let list_json: Vec<Value> = list
        .iter()
        .map(serialize_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "state": state.as_ref(),
            "list": list_json,
            "notFound": not_found_json(&not_found),
        }),
        vec![],
    ))
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
) -> Result<(Value, Vec<Invocation>), JmapError> {
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

    Ok((
        json!({
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
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// handle_query
// ---------------------------------------------------------------------------

/// Generic `*/query` handler (RFC 8620 §5.5).
///
/// Parses filter and sort from args as `O::Filter` and `O::Comparator`, then
/// delegates to [`JmapBackend::query_objects`].
pub async fn handle_query<O: QueryObject, B: JmapBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
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
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

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

    Ok((resp, vec![]))
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
) -> Result<(Value, Vec<Invocation>), JmapError> {
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

    Ok((resp, vec![]))
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
}
