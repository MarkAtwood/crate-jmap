//! Thread/get and Thread/changes method handlers (RFC 8621 §3).

use std::collections::HashSet;

use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::MailBackend;
use crate::helpers::{extract_account_id, filter_properties, not_found_json, ser};

/// Handle a `Thread/get` method call (RFC 8621 §3.1).
///
/// Returns `(response_args, extra_invocations)`. For Thread/get the extra
/// invocations list is always empty.
pub async fn handle_thread_get<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    // ids: absent or null means "return all"; Some([]) means "return nothing".
    let ids: Option<Vec<Id>> = match args.remove("ids").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    // RFC 8620 §5.1: when `properties` is specified return only those fields
    // (plus `id` which is always included). `None` means return all fields.
    let properties: Option<Vec<String>> = match args.remove("properties").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<jmap_mail_types::Thread>(
            caller,
            &account_id,
            ids_slice,
            properties.as_deref(),
        )
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<jmap_mail_types::Thread>(caller, &account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = if let Some(ref props) = properties {
        // Build the effective property set once; always include "id" per RFC 8620 §5.1.
        let mut prop_set: HashSet<&str> = props.iter().map(|s| s.as_str()).collect();
        prop_set.insert("id");
        list.iter()
            .map(|obj| {
                let val = ser(obj)?;
                Ok(filter_properties(&val, &prop_set))
            })
            .collect::<Result<Vec<_>, JmapError>>()?
    } else {
        list.iter().map(ser).collect::<Result<Vec<_>, _>>()?
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    });

    Ok((resp, vec![]))
}

/// Handle a `Thread/changes` method call (RFC 8620 §5.2, as applied to Thread).
pub async fn handle_thread_changes<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<jmap_mail_types::Thread, B>(backend, caller, args).await
}
