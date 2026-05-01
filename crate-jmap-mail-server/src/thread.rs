//! Thread/get and Thread/changes method handlers (RFC 8621 §3).

use jmap_types::{Id, Invocation, JmapError, State};
use serde_json::{json, Value};

use crate::backend::MailBackend;
use crate::helpers::extract_account_id;

/// Handle a `Thread/get` method call (RFC 8621 §3.1).
///
/// Returns `(response_args, extra_invocations)`. For Thread/get the extra
/// invocations list is always empty.
pub async fn handle_thread_get<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    // ids: absent or null means "return all"; Some([]) means "return nothing".
    let ids: Option<Vec<Id>> = match args.get("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v.clone())
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<jmap_mail_types::Thread>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<jmap_mail_types::Thread>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(|t| {
            serde_json::to_value(t).expect("type derives Serialize and is always serializable")
        })
        .collect();

    let not_found_json: Option<Vec<Value>> = if not_found.is_empty() {
        None
    } else {
        Some(
            not_found
                .iter()
                .map(|id| Value::String(id.as_ref().to_string()))
                .collect(),
        )
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json,
    });

    Ok((resp, vec![]))
}

/// Handle a `Thread/changes` method call (RFC 8620 §5.2, as applied to Thread).
///
/// Returns `(response_args, extra_invocations)`. For Thread/changes the extra
/// invocations list is always empty.
pub async fn handle_thread_changes<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes: Option<u64> = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|&n| n > 0)
                .ok_or_else(|| JmapError::invalid_arguments("maxChanges must be a positive integer"))?,
        ),
    };

    let result = backend
        .get_changes::<jmap_mail_types::Thread>(&account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });

    Ok((resp, vec![]))
}
