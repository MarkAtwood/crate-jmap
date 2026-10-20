//! Private helper utilities for the handler modules.

use jmap_types::{GetObject, Id, JmapError, JmapObject, State, UTCDate};
use serde_json::{json, Map, Value};

use crate::backend::{BackendSetError, SceneBackend, SetError, SetErrorType};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend, HandlerOutput};

pub(crate) use jmap_server::{enforce_max_objects_in_set, extract_account_id};

/// Sentinel `id` the handler puts on an object before
/// [`SceneBackend::create_object`] replaces it. Mirrors the canonical
/// `crate-jmap-mail-server/src/helpers.rs` `PLACEHOLDER_ID`.
pub(crate) const PLACEHOLDER_ID: &str = "placeholder";

/// ponytail: "magnitude within epsilon of 1.0" with a fixed epsilon of
/// 1e-6, which accepts quaternions normalized in `f32`. A deployment that
/// wants a different tolerance registers its own `/set` handlers.
const UNIT_EPSILON: f64 = 1e-6;

/// The values of `v` if it is an array of exactly `n` Numbers.
pub(crate) fn numbers(v: &Value, n: usize) -> Option<Vec<f64>> {
    let a = v.as_array().filter(|a| a.len() == n)?;
    a.iter().map(Value::as_f64).collect()
}

/// "Quaternion must be `[x, y, z, w]` with magnitude within epsilon of
/// 1.0." A NaN or infinite component fails the comparison.
pub(crate) fn unit_quaternion(v: &Value) -> bool {
    numbers(v, 4).is_some_and(|q| {
        let norm = q.iter().map(|c| c * c).sum::<f64>().sqrt();
        (norm - 1.0).abs() <= UNIT_EPSILON
    })
}

/// Serialize a [`SetError`] for a `notCreated` / `notUpdated` /
/// `notDestroyed` map.
///
/// `SetError` is plain data with a derived `Serialize`, so the `.expect`
/// is unreachable.
pub(crate) fn set_error_value(e: &SetError) -> Value {
    serde_json::to_value(e).expect("derive(Serialize) on plain data is infallible")
}

/// An `invalidProperties` SetError naming `properties` (none when empty).
pub(crate) fn invalid(properties: &[&str], description: &str) -> Value {
    let err = SetError::new(SetErrorType::InvalidProperties).with_description(description);
    set_error_value(&if properties.is_empty() {
        err
    } else {
        err.with_properties(properties.iter().copied())
    })
}

/// A `forbidden` SetError.
pub(crate) fn forbidden(description: &str) -> Value {
    set_error_value(&SetError::new(SetErrorType::Forbidden).with_description(description))
}

/// A `notFound` SetError.
pub(crate) fn not_found() -> Value {
    set_error_value(&SetError::new(SetErrorType::NotFound))
}

/// An `overQuota` SetError.
pub(crate) fn over_quota(description: &str) -> Value {
    set_error_value(&SetError::new(SetErrorType::OverQuota).with_description(description))
}

/// The SetError JSON for a backend `/set` failure; storage errors become
/// `serverFail` without their `Display` text.
pub(crate) fn backend_set_error_value<E: std::fmt::Display>(e: BackendSetError<E>) -> Value {
    match e {
        BackendSetError::SetError(se) => set_error_value(&se),
        BackendSetError::Other(e) => server_fail_value_from_backend(&e),
        _ => json!({
            "type": "serverFail",
            "description": "unhandled backend error variant",
        }),
    }
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
pub(crate) fn reject_unsupported_sort<C: serde::de::DeserializeOwned>(
    sort: Option<&Value>,
) -> Result<(), JmapError> {
    if let Some(Value::Array(comparators)) = sort {
        for c in comparators {
            <C as serde::Deserialize>::deserialize(c).map_err(|_| JmapError::unsupported_sort())?;
        }
    }
    Ok(())
}

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
