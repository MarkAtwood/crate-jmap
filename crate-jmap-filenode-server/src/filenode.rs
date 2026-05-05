//! FileNode/* method handlers (draft-ietf-jmap-filenode-13).
//!
//! Provides all six JMAP FileNode method handlers:
//! - [`handle_filenode_get`]
//! - [`handle_filenode_changes`]
//! - [`handle_filenode_set`]
//! - [`handle_filenode_copy`]
//! - [`handle_filenode_query`]
//! - [`handle_filenode_query_changes`]

use jmap_filenode_types::FileNode;
use jmap_types::{Id, Invocation, JmapError};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, FileNodeBackend};
use crate::helpers::{extract_account_id, set_error_value};

// ---------------------------------------------------------------------------
// FileNode/get
// ---------------------------------------------------------------------------

/// Handle a `FileNode/get` method call (draft-ietf-jmap-filenode-13 §3.2.1).
pub async fn handle_filenode_get<B: FileNodeBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<FileNode, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// FileNode/changes
// ---------------------------------------------------------------------------

/// Handle a `FileNode/changes` method call (draft-ietf-jmap-filenode-13 §3.2.2).
pub async fn handle_filenode_changes<B: FileNodeBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<FileNode, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// FileNode/set
// ---------------------------------------------------------------------------

/// Handle a `FileNode/set` method call (draft-ietf-jmap-filenode-13 §3.2.3).
///
/// ## FileNode-specific arguments
///
/// - `onDestroyRemoveChildren` (bool, default `false`): when `true`, the
///   backend is responsible for cascading the destroy to children. When
///   `false` (the default), a destroy of a node that has children returns
///   `notDestroyed` with error type `nodeHasChildren`.
/// - `onExists` (object, optional): collision-handling policy when a create
///   would produce a name that already exists in the same parent directory.
///   See the spec §3.2.3 for full semantics.
///   Implementation currently records the policy but defers full collision
///   detection to the backend.
///
/// ## Circular reference prevention
///
/// When an `update` sets `parentId` to a new value, the handler calls
/// [`FileNodeBackend::would_create_cycle`].  If a cycle would result, the
/// update is placed in `notUpdated` with `invalidProperties`.
pub async fn handle_filenode_set<B: FileNodeBackend>(
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
        .get_state::<FileNode>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    // Parse FileNode-specific set arguments.
    let on_destroy_remove_children: bool = args
        .get("onDestroyRemoveChildren")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // `onExists` is parsed but full collision-detection logic is delegated to
    // the backend (see TODO comment in the create section below).
    let _on_exists: Option<Value> = args.remove("onExists");

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            // Inject a placeholder `id` — the backend replaces it with the
            // server-assigned id on success.
            let obj_with_id = match obj_val {
                Value::Object(mut m) => {
                    m.entry("id")
                        .or_insert_with(|| Value::String("placeholder".to_owned()));
                    // Inject required-nullable fields with null defaults if absent,
                    // so serde can deserialize the struct without missing-field errors.
                    for field in &["parentId", "blobId", "target", "size", "type", "shareWith"] {
                        m.entry(*field).or_insert(Value::Null);
                    }
                    // `name` is required and non-nullable; if missing, serde will
                    // produce an `invalidProperties` error below.
                    Value::Object(m)
                }
                other => other,
            };

            // TODO: implement onExists collision detection per spec §3.2.3.
            // The spec defines: null (default, return alreadyExists), "replace",
            // and "rename" (with compareCaseInsensitively option). Full collision
            // detection requires a backend query for siblings with the same name
            // under the same parentId before forwarding to create_object.

            let node: FileNode = match serde_json::from_value(obj_with_id) {
                Ok(n) => n,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<FileNode>(&account_id, &create_id, node)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id,
                        serde_json::to_value(&created_obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Circular reference check: if the patch touches `parentId`,
            // verify the move would not create a cycle.
            if let Some(new_parent_val) = patch_val.get("parentId") {
                if let Some(new_parent_str) = new_parent_val.as_str() {
                    let new_parent_id = Id::from(new_parent_str);
                    if backend
                        .would_create_cycle(&account_id, &id, &new_parent_id)
                        .await
                    {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "invalidProperties",
                                "properties": ["parentId"],
                                "description": "setting parentId would create a cycle"
                            }),
                        );
                        continue;
                    }
                }
            }

            match backend
                .update_object::<FileNode>(&account_id, &id, patch_val)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                    );
                }
                Ok(None) => {
                    mutated = true;
                    updated.insert(id_str, Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------
    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue,
            };
            let id = Id::from(id_str.as_str());

            // If onDestroyRemoveChildren is false and the node has children,
            // return nodeHasChildren without touching the backend.
            if !on_destroy_remove_children && backend.node_has_children(&account_id, &id).await {
                not_destroyed.insert(id_str, json!({ "type": "nodeHasChildren" }));
                continue;
            }

            match backend.destroy_object::<FileNode>(&account_id, &id).await {
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

    let new_state = if mutated {
        backend
            .get_state::<FileNode>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "created":      if created.is_empty()        { Value::Null } else { Value::Object(created) },
            "updated":      if updated.is_empty()        { Value::Null } else { Value::Object(updated) },
            "destroyed":    if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
            "notCreated":   if not_created.is_empty()    { Value::Null } else { Value::Object(not_created) },
            "notUpdated":   if not_updated.is_empty()    { Value::Null } else { Value::Object(not_updated) },
            "notDestroyed": if not_destroyed.is_empty()  { Value::Null } else { Value::Object(not_destroyed) },
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// FileNode/copy
// ---------------------------------------------------------------------------

/// Handle a `FileNode/copy` method call (draft-ietf-jmap-filenode-13 §3.2.4).
///
/// `FileNode/copy` uses its own wire shape (source account + destination account),
/// separate from `*/set`. This handler parses the wire arguments and delegates
/// each copy entry to [`FileNodeBackend::create_object`] in the destination
/// account, using the source node's properties.
///
/// The source nodes are fetched with `get_objects` before copying.
pub async fn handle_filenode_copy<B: FileNodeBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let from_account_id: Id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::from(s),
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };
    let account_id = extract_account_id(&args)?;

    // Verify both accounts exist.
    if !backend
        .account_exists(&from_account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let old_state = backend
        .get_state::<FileNode>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut copied = serde_json::Map::new();
    let mut not_copied = serde_json::Map::new();
    let mut mutated = false;

    if let Some(Value::Object(create_map)) = args.remove("create") {
        for (create_id, obj_val) in create_map {
            // obj_val is the per-copy descriptor from the client: it must
            // contain `id` (the source node id in fromAccountId) plus any
            // property overrides for the copy.
            let source_id_str = match obj_val.get("id").and_then(|v| v.as_str()) {
                Some(s) => s.to_owned(),
                None => {
                    not_copied.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": "id (source) is required" }),
                    );
                    continue;
                }
            };
            let source_id = Id::from(source_id_str.as_str());

            // Fetch the source node.
            let (mut nodes, not_found): (Vec<FileNode>, _) = backend
                .get_objects::<FileNode>(
                    &from_account_id,
                    Some(std::slice::from_ref(&source_id)),
                    None,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;

            if !not_found.is_empty() || nodes.is_empty() {
                not_copied.insert(create_id, json!({ "type": "notFound" }));
                continue;
            }

            let mut source_node = nodes.remove(0);

            // Apply any overrides from the copy descriptor (e.g. new name or
            // parentId in the destination).
            if let Some(new_parent) = obj_val.get("parentId") {
                source_node.parent_id = if new_parent.is_null() {
                    None
                } else {
                    new_parent.as_str().map(Id::from)
                };
            }
            if let Some(new_name) = obj_val.get("name").and_then(|v| v.as_str()) {
                source_node.name = new_name.to_owned();
            }

            // Create in the destination account.
            match backend
                .create_object::<FileNode>(&account_id, &create_id, source_node)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    copied.insert(
                        create_id,
                        serde_json::to_value(&created_obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_copied.insert(create_id, set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_copied.insert(
                        create_id,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<FileNode>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    Ok((
        json!({
            "fromAccountId": from_account_id.as_ref(),
            "accountId": account_id.as_ref(),
            "oldState": old_state.as_ref(),
            "newState": new_state.as_ref(),
            "created":   if copied.is_empty()     { Value::Null } else { Value::Object(copied) },
            "notCreated": if not_copied.is_empty() { Value::Null } else { Value::Object(not_copied) },
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// FileNode/query
// ---------------------------------------------------------------------------

/// Handle a `FileNode/query` method call (draft-ietf-jmap-filenode-13 §3.2.5).
pub async fn handle_filenode_query<B: FileNodeBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<FileNode, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// FileNode/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `FileNode/queryChanges` method call (draft-ietf-jmap-filenode-13 §3.2.6).
pub async fn handle_filenode_query_changes<B: FileNodeBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<FileNode, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::test_support::MockBackend;

    // -----------------------------------------------------------------------
    // FileNode/get
    // -----------------------------------------------------------------------

    /// Oracle: FileNode/get with unknown accountId returns accountNotFound.
    /// Source: RFC 8620 §3.6.2.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MockBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let err = handle_filenode_get(&backend, args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(
            err.error_type.as_str(),
            "accountNotFound",
            "got: {:?}",
            err.error_type
        );
    }

    /// Oracle: FileNode/get with known account returns empty list (no nodes seeded).
    #[tokio::test]
    async fn get_known_account_returns_empty_list() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "ids": null });
        let (resp, _) = handle_filenode_get(&backend, args)
            .await
            .expect("must succeed for known account");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["list"].as_array().unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // FileNode/changes
    // -----------------------------------------------------------------------

    /// Oracle: FileNode/changes returns the standard changes response shape.
    #[tokio::test]
    async fn changes_returns_standard_shape() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let (resp, _) = handle_filenode_changes(&backend, args)
            .await
            .expect("must succeed");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["created"].is_array());
        assert!(resp["updated"].is_array());
        assert!(resp["destroyed"].is_array());
    }

    // -----------------------------------------------------------------------
    // FileNode/query
    // -----------------------------------------------------------------------

    /// Oracle: FileNode/query returns standard query response shape.
    #[tokio::test]
    async fn query_returns_standard_shape() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "filter": null, "sort": null });
        let (resp, _) = handle_filenode_query(&backend, args)
            .await
            .expect("must succeed");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["ids"].is_array());
    }

    // -----------------------------------------------------------------------
    // FileNode/set — nodeHasChildren guard
    // -----------------------------------------------------------------------

    /// Oracle: destroy of a node that has children when onDestroyRemoveChildren
    /// is false (the default) returns notDestroyed with type "nodeHasChildren".
    ///
    /// Source: draft-ietf-jmap-filenode-13 §3.2.3.
    #[tokio::test]
    async fn set_destroy_node_with_children_returns_node_has_children() {
        let backend = MockBackend::new_with_account("acc1");
        backend.set_has_children("dir1", true);

        let args = json!({
            "accountId": "acc1",
            "destroy": ["dir1"]
        });
        let (resp, _) = handle_filenode_set(&backend, args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp["destroyed"].is_null(),
            "nothing should be destroyed: {resp}"
        );
        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["dir1"]["type"], "nodeHasChildren",
            "must return nodeHasChildren error: {resp}"
        );
    }

    /// Oracle: destroy of a node with children when onDestroyRemoveChildren
    /// is true proceeds to the backend (no nodeHasChildren guard).
    #[tokio::test]
    async fn set_destroy_node_with_children_and_flag_true_calls_backend() {
        let backend = MockBackend::new_with_account("acc1");
        // Backend returns notFound for this id since we didn't pre-seed it.
        // The point: the handler must reach the backend, not short-circuit.
        backend.set_has_children("dir1", true);

        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveChildren": true,
            "destroy": ["dir1"]
        });
        let (resp, _) = handle_filenode_set(&backend, args)
            .await
            .expect("must not return top-level error");

        // Backend returned notFound — but it was called (no nodeHasChildren
        // interception). notDestroyed must exist with notFound, not nodeHasChildren.
        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["dir1"]["type"], "notFound",
            "backend error should be notFound (was called), not nodeHasChildren: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — circular reference guard
    // -----------------------------------------------------------------------

    /// Oracle: update that sets parentId to a value that would create a cycle
    /// returns notUpdated with invalidProperties on parentId.
    ///
    /// Source: draft-ietf-jmap-filenode-13 §3.2.3 (no cycles constraint).
    #[tokio::test]
    async fn set_update_circular_parent_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        // Instruct mock to report that moving "node1" under "node2" would cycle.
        backend.set_cycle_pair("node1", "node2", true);

        let args = json!({
            "accountId": "acc1",
            "update": {
                "node1": { "parentId": "node2" }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, args)
            .await
            .expect("must not return top-level error");

        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present: {resp}"
        );
        assert_eq!(
            not_updated["node1"]["type"], "invalidProperties",
            "cycle must produce invalidProperties: {resp}"
        );
        let props = &not_updated["node1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("parentId")))
                .unwrap_or(false),
            "parentId must be listed in properties: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — create with missing required field
    // -----------------------------------------------------------------------

    /// Oracle: create with missing `name` returns invalidProperties.
    #[tokio::test]
    async fn set_create_missing_name_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "parentId": null,
                    "blobId": null,
                    "target": null,
                    "size": null,
                    "type": null,
                    "shareWith": null
                }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "missing name must produce invalidProperties: {resp}"
        );
    }
}
