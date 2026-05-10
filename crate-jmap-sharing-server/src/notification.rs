//! ShareNotification/* method handlers (RFC 9670 §3).
//!
//! ShareNotifications are server-created immutable records.  Clients may only
//! query and destroy them.  Any attempt to create or update a ShareNotification
//! via `/set` MUST be rejected with `forbidden` at the handler layer — the
//! backend never sees create or update calls for this type.

use jmap_sharing_types::ShareNotification;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, SetError, SetErrorType, SharingBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value};

// ---------------------------------------------------------------------------
// ShareNotification/get
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/get` method call (RFC 9670 §3.1).
pub async fn handle_share_notification_get<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<ShareNotification, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ShareNotification/changes
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/changes` method call (RFC 9670 §3.2).
pub async fn handle_share_notification_changes<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<ShareNotification, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ShareNotification/set
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/set` method call (RFC 9670 §3.3).
///
/// **Destroy-only enforcement**: RFC 9670 §3.3 states that only `destroy` is
/// supported.  Any entries in the `create` or `update` maps receive an
/// immediate `forbidden` SetError without touching the backend. The `destroy`
/// list is forwarded to the backend normally.
pub async fn handle_share_notification_set<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let old_state = backend
        .get_state::<ShareNotification>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create — forbidden: ShareNotification is server-created only
    // -----------------------------------------------------------------------
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, _) in create_map {
            not_created.insert(
                create_id,
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // -----------------------------------------------------------------------
    // update — forbidden: ShareNotification is immutable
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, _) in update_map {
            not_updated.insert(
                id_str,
                set_error_value(&SetError::new(SetErrorType::Forbidden)),
            );
        }
    }

    // -----------------------------------------------------------------------
    // destroy — the only permitted operation (RFC 9670 §3.3)
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue, // unreachable: validated above
            };
            let id = Id::from(id_str.as_str());

            match backend
                .destroy_object::<ShareNotification>(&account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    finalize_set_response::<B, ShareNotification>(
        backend,
        &account_id,
        old_state,
        mutated,
        created,
        updated,
        destroyed_list,
        not_created,
        not_updated,
        not_destroyed,
    )
    .await
}

// ---------------------------------------------------------------------------
// ShareNotification/query
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/query` method call (RFC 9670 §3.4).
pub async fn handle_share_notification_query<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<ShareNotification, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// ShareNotification/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `ShareNotification/queryChanges` method call (RFC 9670 §3.5).
pub async fn handle_share_notification_query_changes<B: SharingBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<ShareNotification, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    /// Oracle: RFC 9670 §3.3 — create entries must produce `forbidden` in notCreated.
    /// No backend call is made for create (pure handler-layer enforcement).
    #[tokio::test]
    async fn set_create_returns_forbidden_not_created() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "id": "ignored", "created": "2024-01-01T00:00:00Z",
                         "changedBy": { "name": "Alice", "email": null, "principalId": null },
                         "objectType": "Mailbox", "objectAccountId": "acc2",
                         "objectId": "mb1", "oldRights": null, "newRights": null,
                         "name": "Team Inbox" },
                "c2": { "id": "ignored2", "created": "2024-01-02T00:00:00Z",
                         "changedBy": { "name": "Bob", "email": null, "principalId": null },
                         "objectType": "Calendar", "objectAccountId": "acc3",
                         "objectId": "cal1", "oldRights": null, "newRights": null,
                         "name": "Calendar" }
            }
        });
        let (resp, _) = handle_share_notification_set(&backend, args)
            .await
            .expect("must not return top-level error");

        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present for create attempts: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "forbidden",
            "c1 create must be forbidden: {resp}"
        );
        assert_eq!(
            not_created["c2"]["type"], "forbidden",
            "c2 create must be forbidden: {resp}"
        );
        // created must be null — nothing was actually created
        assert!(
            resp["created"].is_null(),
            "created must be null when all creates are forbidden: {resp}"
        );
    }

    /// Oracle: RFC 9670 §3.3 — update entries must produce `forbidden` in notUpdated.
    #[tokio::test]
    async fn set_update_returns_forbidden_not_updated() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "update": {
                "notif1": { "name": "Renamed" },
                "notif2": { "objectType": "Calendar" }
            }
        });
        let (resp, _) = handle_share_notification_set(&backend, args)
            .await
            .expect("must not return top-level error");

        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present for update attempts: {resp}"
        );
        assert_eq!(not_updated["notif1"]["type"], "forbidden");
        assert_eq!(not_updated["notif2"]["type"], "forbidden");
    }

    /// Oracle: RFC 9670 §3.3 — destroy proceeds normally even when create/update are
    /// also present (they get forbidden but destroy is forwarded to backend).
    #[tokio::test]
    async fn set_mixed_create_and_destroy_enforces_destroy_only() {
        let mut backend = MockBackend::new_with_account("acc1");
        // Pre-populate the mock with a notification to destroy.
        backend.add_notification("acc1", "notif1");

        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "id": "x", "created": "2024-01-01T00:00:00Z",
                         "changedBy": { "name": "Alice", "email": null, "principalId": null },
                         "objectType": "Mailbox", "objectAccountId": "a",
                         "objectId": "m1", "oldRights": null, "newRights": null,
                         "name": "N" }
            },
            "destroy": ["notif1"]
        });
        let (resp, _) = handle_share_notification_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // create → forbidden
        assert_eq!(resp["notCreated"]["c1"]["type"], "forbidden");
        // destroy → succeeded
        let destroyed = resp["destroyed"]
            .as_array()
            .expect("destroyed must be array");
        assert_eq!(destroyed.len(), 1);
        assert_eq!(destroyed[0], "notif1");
    }

    /// Oracle: ShareNotification/set destroy array with null element must return
    /// a top-level invalidArguments error.
    #[tokio::test]
    async fn set_destroy_null_element_returns_invalid_arguments() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": [null]
        });
        let result = handle_share_notification_set(&backend, args).await;
        let err = result.expect_err("must return top-level error for null destroy element");
        assert_eq!(err.error_type.as_str(), "invalidArguments");
    }

    /// Oracle: destroy of a non-existent notification → notFound in notDestroyed.
    #[tokio::test]
    async fn set_destroy_nonexistent_returns_not_found() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "destroy": ["doesnotexist"]
        });
        let (resp, _) = handle_share_notification_set(&backend, args)
            .await
            .expect("must not return top-level error");

        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["doesnotexist"]["type"], "notFound",
            "missing id must produce notFound: {resp}"
        );
    }
}
