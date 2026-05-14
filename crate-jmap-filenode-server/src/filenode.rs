//! FileNode/* method handlers (draft-ietf-jmap-filenode-13).
//!
//! Provides all six JMAP FileNode method handlers:
//! - [`handle_filenode_get`]
//! - [`handle_filenode_changes`]
//! - [`handle_filenode_set`]
//! - [`handle_filenode_copy`]
//! - [`handle_filenode_query`]
//! - [`handle_filenode_query_changes`]

use jmap_filenode_types::{FileNode, NodeType};
use jmap_types::{Id, Invocation, JmapError, PatchObject};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, FileNodeBackend};
use crate::helpers::{extract_account_id, finalize_set_response, set_error_value, SetAccumulators};
use jmap_server::server_fail_from_backend;

// ---------------------------------------------------------------------------
// FileNode/get
// ---------------------------------------------------------------------------

/// Handle a `FileNode/get` method call (draft-ietf-jmap-filenode-13 §3.2.1).
///
/// If `fetchParents` is `true` in the request, the ancestor nodes of all
/// returned nodes are fetched via [`FileNodeBackend::get_ancestors`] and
/// appended to the response `list` (deduplicated against already-present nodes).
/// Oracle: §3.2.1.
pub async fn handle_filenode_get<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let fetch_parents = args
        .get("fetchParents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let (account_id, args_map) = extract_account_id(args)?;

    // Reconstitute the args object for the generic handler. The generic helper
    // re-parses accountId itself, which is fine — the duplicate parse is cheap.
    let mut reconstituted = args_map;
    reconstituted.insert(
        "accountId".to_owned(),
        Value::String(account_id.as_ref().to_owned()),
    );
    let (mut response, tail) = jmap_server::handlers::handle_get::<FileNode, B>(
        backend,
        caller,
        Value::Object(reconstituted),
    )
    .await?;

    if fetch_parents {
        if let Some(Value::Array(list)) = response.get("list") {
            // Collect ids of nodes already in the response.
            let existing_ids: std::collections::HashSet<String> = list
                .iter()
                .filter_map(|item| {
                    item.get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_owned())
                })
                .collect();

            // Collect all returned node ids for the ancestor query.
            let node_ids: Vec<Id> = existing_ids.iter().map(|s| Id::from(s.as_str())).collect();

            if !node_ids.is_empty() {
                // draft-ietf-jmap-filenode-13 §3.2.1 makes fetchParents
                // part of the request contract: the response shape does
                // not advertise that ancestors are missing, so silently
                // dropping a backend error would let the client believe
                // the requested nodes are root-level when they may have
                // ancestors. Surface a serverFail instead.
                let ancestors = backend
                    .get_ancestors(caller, &account_id, &node_ids)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;
                let list = response["list"].as_array_mut().expect("list must be array");
                for ancestor in ancestors {
                    // Deduplicate: only append if not already in the list.
                    let ancestor_id = ancestor.id.as_ref().to_owned();
                    if !existing_ids.contains(&ancestor_id) {
                        if let Ok(v) = serde_json::to_value(&ancestor) {
                            list.push(v);
                        }
                    }
                }
            }
        }
    }

    Ok((response, tail))
}

// ---------------------------------------------------------------------------
// FileNode/changes
// ---------------------------------------------------------------------------

/// Handle a `FileNode/changes` method call (draft-ietf-jmap-filenode-13 §3.2.2).
pub async fn handle_filenode_changes<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<FileNode, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// FileNode/set
// ---------------------------------------------------------------------------

/// Collision-handling policy for `FileNode/set` create operations.
#[derive(Debug)]
enum OnExists {
    /// null / absent (default) — return `alreadyExists`.
    Reject,
    /// `"replace"` — destroy the existing node and create the new one.
    Replace,
    /// `"rename"` — find a non-colliding name by appending a counter suffix.
    Rename,
}

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
///
/// ## Circular reference prevention
///
/// When an `update` sets `parentId` to a new value, the handler calls
/// [`FileNodeBackend::get_descendant_ids`] on the node being moved.  If the
/// proposed new parent is in the descendant set, a cycle would result and the
/// update is placed in `notUpdated` with `invalidProperties`.
pub async fn handle_filenode_set<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let old_state = backend
        .get_state::<FileNode>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

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

    // Parse onExists collision-handling policy (§3.2.3).
    let on_exists: OnExists = {
        let v = args.remove("onExists");
        match v.as_ref() {
            None | Some(Value::Null) => OnExists::Reject,
            Some(Value::String(s)) if s == "replace" => OnExists::Replace,
            Some(Value::String(s)) if s == "rename" => OnExists::Rename,
            Some(other) => {
                return Err(JmapError::invalid_arguments(format!(
                    "invalid onExists value: {other}"
                )));
            }
        }
    };

    let compare_case_insensitively: bool = args
        .get("compareCaseInsensitively")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

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
            let mut obj_with_id = match obj_val {
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

            // Tracks the rename applied by OnExists::Rename so the
            // post-create response can enforce the spec's MUST that the
            // renamed name appears in the response (§3.2.3 lines 572-575)
            // even if the backend echoes a different name.
            let mut renamed_to: Option<String> = None;

            // -------------------------------------------------------------------
            // Collision detection (onExists — §3.2.3).
            // -------------------------------------------------------------------
            let node_name = obj_with_id
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let parent_id_for_collision: Option<Id> = obj_with_id
                .get("parentId")
                .and_then(|v| v.as_str())
                .map(Id::from);

            if !node_name.is_empty() {
                match backend
                    .find_sibling_by_name(
                        caller,
                        &account_id,
                        parent_id_for_collision.as_ref(),
                        &node_name,
                        compare_case_insensitively,
                    )
                    .await
                {
                    Ok(Some(existing_id)) => {
                        match on_exists {
                            OnExists::Reject => {
                                not_created.insert(
                                    create_id,
                                    json!({ "type": "alreadyExists", "existingId": existing_id.as_ref() }),
                                );
                                continue;
                            }
                            OnExists::Replace => {
                                // draft-ietf-jmap-filenode-13 §3.2.3 lines
                                // 565-570: "If 'replace', the existing item
                                // will be destroyed. [...] if the replaced
                                // item is a directory which has children,
                                // then the server MUST respond with a
                                // nodeHasChildren error to this action
                                // UNLESS onDestroyRemoveChildren is true."
                                let desc_ids = match backend
                                    .get_descendant_ids(caller, &account_id, &existing_id)
                                    .await
                                {
                                    Ok(ids) => ids,
                                    Err(e) => {
                                        not_created.insert(
                                            create_id,
                                            json!({ "type": "serverFail", "description": e.to_string() }),
                                        );
                                        continue;
                                    }
                                };
                                if !desc_ids.is_empty() && !on_destroy_remove_children {
                                    not_created.insert(
                                        create_id,
                                        json!({ "type": "nodeHasChildren", "existingId": existing_id.as_ref() }),
                                    );
                                    continue;
                                }
                                // When descendants exist AND
                                // onDestroyRemoveChildren=true, cascade the
                                // destroy of descendants first. Same
                                // ordering as the regular destroy path
                                // (filenode.rs around §destroy: descendants
                                // first, then the targeted node).
                                let mut cascade_failed = false;
                                for desc_id in &desc_ids {
                                    match backend
                                        .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                        .await
                                    {
                                        Ok(()) => {
                                            mutated = true;
                                            destroyed_list
                                                .push(Value::String(desc_id.as_ref().to_owned()));
                                        }
                                        Err(BackendSetError::SetError(set_err)) => {
                                            not_created.insert(
                                                create_id.clone(),
                                                set_error_value(&set_err),
                                            );
                                            cascade_failed = true;
                                            break;
                                        }
                                        Err(BackendSetError::Other(e)) => {
                                            not_created.insert(
                                                create_id.clone(),
                                                json!({ "type": "serverFail", "description": e.to_string() }),
                                            );
                                            cascade_failed = true;
                                            break;
                                        }
                                        Err(_) => {
                                            not_created.insert(
                                                create_id.clone(),
                                                json!({
                                                    "type": "serverFail",
                                                    "description": "unhandled backend error variant",
                                                }),
                                            );
                                            cascade_failed = true;
                                            break;
                                        }
                                    }
                                }
                                if cascade_failed {
                                    continue;
                                }
                                // Now destroy the existing colliding node.
                                match backend
                                    .destroy_object::<FileNode>(caller, &account_id, &existing_id)
                                    .await
                                {
                                    Ok(()) => {
                                        mutated = true;
                                        destroyed_list
                                            .push(Value::String(existing_id.as_ref().to_owned()));
                                    }
                                    Err(BackendSetError::SetError(set_err)) => {
                                        not_created.insert(create_id, set_error_value(&set_err));
                                        continue;
                                    }
                                    Err(BackendSetError::Other(e)) => {
                                        not_created.insert(
                                            create_id,
                                            json!({ "type": "serverFail", "description": e.to_string() }),
                                        );
                                        continue;
                                    }
                                    Err(_) => {
                                        not_created.insert(
                                            create_id,
                                            json!({
                                                "type": "serverFail",
                                                "description": "unhandled backend error variant",
                                            }),
                                        );
                                        continue;
                                    }
                                }
                                // Fall through to create the new node.
                            }
                            OnExists::Rename => {
                                // Find a non-colliding name by appending a counter suffix.
                                let mut renamed = false;
                                let mut rename_error = false;
                                for suffix in 1_u32..=100 {
                                    let candidate = format!("{node_name}-{suffix}");
                                    match backend
                                        .find_sibling_by_name(
                                            caller,
                                            &account_id,
                                            parent_id_for_collision.as_ref(),
                                            &candidate,
                                            compare_case_insensitively,
                                        )
                                        .await
                                    {
                                        Ok(None) => {
                                            if let Value::Object(ref mut m) = obj_with_id {
                                                m.insert(
                                                    "name".to_owned(),
                                                    Value::String(candidate.clone()),
                                                );
                                            }
                                            renamed_to = Some(candidate);
                                            renamed = true;
                                            break;
                                        }
                                        Ok(Some(_)) => continue,
                                        Err(e) => {
                                            not_created.insert(
                                                create_id.clone(),
                                                json!({ "type": "serverFail", "description": e.to_string() }),
                                            );
                                            rename_error = true;
                                            break;
                                        }
                                    }
                                }
                                if rename_error {
                                    continue;
                                }
                                if !renamed {
                                    not_created.insert(
                                        create_id,
                                        json!({ "type": "serverFail", "description": "could not find unique name after 100 attempts" }),
                                    );
                                    continue;
                                }
                                // Fall through to create the renamed node.
                            }
                        }
                    }
                    Ok(None) => {} // No collision, proceed normally.
                    Err(e) => {
                        not_created.insert(
                            create_id,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                }
            }

            // -------------------------------------------------------------------
            // NodeType inference (§3.1): infer nodeType if absent or null.
            // -------------------------------------------------------------------
            obj_with_id = match obj_with_id {
                Value::Object(mut m) => {
                    if m.get("nodeType").map(|v| v.is_null()).unwrap_or(true) {
                        let has_blob = m.get("blobId").map(|v| !v.is_null()).unwrap_or(false);
                        let has_target = m.get("target").map(|v| !v.is_null()).unwrap_or(false);
                        let inferred = if has_blob {
                            "file"
                        } else if has_target {
                            "symlink"
                        } else {
                            "directory"
                        };
                        m.insert("nodeType".to_owned(), Value::String(inferred.to_owned()));
                    }
                    Value::Object(m)
                }
                other => other,
            };

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

            // -------------------------------------------------------------------
            // Consistency validation (§3.1 type constraints).
            // -------------------------------------------------------------------
            let has_blob = node.blob_id.is_some();
            let has_target = node.target.as_ref().map(|v| !v.is_empty()).unwrap_or(false);
            let consistency_error: Option<(&str, &str)> = match node.node_type.as_ref() {
                Some(NodeType::File) if !has_blob => Some(("blobId", "file node requires blobId")),
                Some(NodeType::Directory) if has_blob => {
                    Some(("blobId", "directory node must not have blobId"))
                }
                Some(NodeType::Directory) if has_target => {
                    Some(("target", "directory node must not have target"))
                }
                Some(NodeType::Symlink) if has_blob => {
                    Some(("blobId", "symlink node must not have blobId"))
                }
                Some(NodeType::Symlink) if !has_target => {
                    Some(("target", "symlink node requires target"))
                }
                _ => None,
            };
            if let Some((field, description)) = consistency_error {
                not_created.insert(
                    create_id,
                    json!({ "type": "invalidProperties", "properties": [field], "description": description }),
                );
                continue;
            }

            // For file nodes, verify the blob exists.
            if matches!(node.node_type.as_ref(), Some(NodeType::File)) {
                if let Some(ref blob_id) = node.blob_id {
                    if !backend.blob_exists(caller, &account_id, blob_id).await {
                        not_created.insert(
                            create_id,
                            json!({ "type": "invalidProperties", "properties": ["blobId"], "description": "blob not found" }),
                        );
                        continue;
                    }
                }
            }

            match backend
                .create_object::<FileNode>(caller, &account_id, &create_id, node)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    let mut value = serde_json::to_value(&created_obj)
                        .expect("derive(Serialize) on plain data is infallible");
                    // draft-ietf-jmap-filenode-13 §3.2.3 lines 572-575:
                    // "If the server changes the name, it MUST include
                    // the new 'name' value in the created or updated
                    // response field for this id." Enforce regardless
                    // of whether the backend echoed the supplied name.
                    if let Some(name) = renamed_to.as_deref() {
                        if let Value::Object(ref mut m) = value {
                            m.insert("name".to_owned(), Value::String(name.to_owned()));
                        }
                    }
                    created.insert(create_id, value);
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
                Err(_) => {
                    not_created.insert(
                        create_id,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
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
            // verify the move would not create a cycle. The introspection
            // is on the wire-format Value because we have not yet bound
            // the patch to PatchObject's stricter object-only contract.
            if let Some(new_parent_val) = patch_val.get("parentId") {
                if let Some(new_parent_str) = new_parent_val.as_str() {
                    let new_parent_id = Id::from(new_parent_str);
                    // A node is trivially an ancestor of itself
                    // (draft-ietf-jmap-filenode-13 §3.2.3), and
                    // `get_descendant_ids` documents that the
                    // returned set excludes the node itself — so
                    // the descendant check below cannot catch
                    // `parentId == id`. Reject it explicitly.
                    if new_parent_id == id {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "invalidProperties",
                                "properties": ["parentId"],
                                "description": "a node cannot be its own parent"
                            }),
                        );
                        continue;
                    }
                    match backend.get_descendant_ids(caller, &account_id, &id).await {
                        Ok(descendant_ids) => {
                            if descendant_ids.iter().any(|did| did == &new_parent_id) {
                                not_updated.insert(
                                    id_str,
                                    json!({
                                        "type": "invalidProperties",
                                        "properties": ["parentId"],
                                        "description": "moving this node to the proposed parent would create a cycle"
                                    }),
                                );
                                continue;
                            }
                        }
                        Err(e) => {
                            not_updated.insert(
                                id_str,
                                json!({ "type": "serverFail", "description": e.to_string() }),
                            );
                            continue;
                        }
                    }
                }
            }

            // Convert wire-format Value into a typed PatchObject. RFC 8620
            // §5.3 mandates a PatchObject is a JSON Object; non-object
            // values produce an `invalidPatch` SetError.
            let patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            match backend
                .update_object::<FileNode>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
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
                Err(_) => {
                    not_updated.insert(
                        id_str,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------

    // Collect the full set of IDs being destroyed in this request *before*
    // consuming the array, so the nodeHasChildren check can inspect it.
    let destroy_id_set: std::collections::HashSet<Id> =
        if let Some(Value::Array(arr)) = args.get("destroy") {
            arr.iter()
                .filter_map(|v| v.as_str().map(Id::from))
                .collect()
        } else {
            std::collections::HashSet::new()
        };

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

            // If onDestroyRemoveChildren is false, check whether all
            // descendants are also in this destroy request.
            // RFC §3.2.3: MUST NOT return nodeHasChildren if all descendants
            // are also being destroyed in the same request.
            if !on_destroy_remove_children {
                match backend.get_descendant_ids(caller, &account_id, &id).await {
                    Ok(desc_ids) => {
                        let all_covered = desc_ids.iter().all(|did| destroy_id_set.contains(did));
                        if !desc_ids.is_empty() && !all_covered {
                            not_destroyed.insert(id_str, json!({ "type": "nodeHasChildren" }));
                            continue;
                        }
                    }
                    Err(e) => {
                        not_destroyed.insert(
                            id_str,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                }
            }

            // When onDestroyRemoveChildren=true, cascade to all descendants first.
            if on_destroy_remove_children {
                match backend.get_descendant_ids(caller, &account_id, &id).await {
                    Ok(desc_ids) => {
                        let mut cascade_failed = false;
                        for desc_id in &desc_ids {
                            match backend
                                .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                .await
                            {
                                Ok(()) => {
                                    mutated = true;
                                    destroyed_list.push(Value::String(desc_id.as_ref().to_owned()));
                                }
                                Err(BackendSetError::SetError(set_err)) => {
                                    not_destroyed.insert(id_str.clone(), set_error_value(&set_err));
                                    cascade_failed = true;
                                    break;
                                }
                                Err(BackendSetError::Other(e)) => {
                                    not_destroyed.insert(
                                        id_str.clone(),
                                        json!({ "type": "serverFail", "description": e.to_string() }),
                                    );
                                    cascade_failed = true;
                                    break;
                                }
                                Err(_) => {
                                    not_destroyed.insert(
                                        id_str.clone(),
                                        json!({
                                            "type": "serverFail",
                                            "description": "unhandled backend error variant",
                                        }),
                                    );
                                    cascade_failed = true;
                                    break;
                                }
                            }
                        }
                        if cascade_failed {
                            continue;
                        }
                    }
                    Err(e) => {
                        not_destroyed.insert(
                            id_str,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                }
            }

            match backend
                .destroy_object::<FileNode>(caller, &account_id, &id)
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
                Err(_) => {
                    not_destroyed.insert(
                        id_str,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    finalize_set_response::<B, FileNode>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            created,
            updated,
            destroyed: destroyed_list,
            not_created,
            not_updated,
            not_destroyed,
        },
    )
    .await
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let from_account_id: Id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::from(s),
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };
    let (account_id, mut args) = extract_account_id(args)?;

    // Verify both accounts exist.
    if !backend
        .account_exists(caller, &from_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let old_state = backend
        .get_state::<FileNode>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    // draft-ietf-jmap-filenode-13 §3.2.4: "This is a standard Foo/copy
    // function with the same additional top-level arguments as
    // FileNode/set, onDestroyRemoveChildren and onExists, with the
    // same behaviour." Parse and apply them to the destination account
    // (find_sibling_by_name uses account_id, NOT from_account_id).
    let on_destroy_remove_children: bool = args
        .get("onDestroyRemoveChildren")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let on_exists: OnExists = {
        let v = args.remove("onExists");
        match v.as_ref() {
            None | Some(Value::Null) => OnExists::Reject,
            Some(Value::String(s)) if s == "replace" => OnExists::Replace,
            Some(Value::String(s)) if s == "rename" => OnExists::Rename,
            Some(other) => {
                return Err(JmapError::invalid_arguments(format!(
                    "invalid onExists value: {other}"
                )));
            }
        }
    };

    let compare_case_insensitively: bool = args
        .get("compareCaseInsensitively")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut copied = serde_json::Map::new();
    let mut not_copied = serde_json::Map::new();
    // The onExists=replace cascade destroys 0..N nodes per entry. RFC
    // 8620 §5.4 does not define a `destroyed` field on Foo/copy
    // responses, so a client has no in-response wire signal for which
    // nodes the copy destroyed. The handler still performs the
    // destroys (the FileNode draft §3.2.4 mandates the behaviour via
    // §3.2.3); clients learn about them via Foo/changes. Surfacing
    // them in the response shape is a separate workspace bead.
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
                    caller,
                    &from_account_id,
                    Some(std::slice::from_ref(&source_id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            if !not_found.is_empty() || nodes.is_empty() {
                not_copied.insert(create_id, json!({ "type": "notFound" }));
                continue;
            }

            let mut source_node = nodes.remove(0);

            // Apply any overrides from the copy descriptor (e.g. new name
            // or parentId in the destination). parentId must be a string
            // or null; non-string non-null values are
            // invalidProperties — silently coercing to None would
            // surface as a 'moved to root' side effect with no error
            // signal.
            if let Some(new_parent) = obj_val.get("parentId") {
                if new_parent.is_null() {
                    source_node.parent_id = None;
                } else if let Some(s) = new_parent.as_str() {
                    source_node.parent_id = Some(Id::from(s));
                } else {
                    not_copied.insert(
                        create_id,
                        json!({
                            "type": "invalidProperties",
                            "properties": ["parentId"],
                            "description": "parentId must be a string Id or null",
                        }),
                    );
                    continue;
                }
            }
            if let Some(new_name) = obj_val.get("name").and_then(|v| v.as_str()) {
                source_node.name = new_name.to_owned();
            }

            // Tracks the rename applied by OnExists::Rename so the
            // post-create response can enforce the spec's MUST that the
            // renamed name appears in the response (§3.2.3 lines 572-575)
            // even if the backend echoes a different name.
            let mut renamed_to: Option<String> = None;

            // -------------------------------------------------------------------
            // Collision detection in the DESTINATION account (§3.2.4 →
            // §3.2.3 onExists semantics). Mirrors handle_filenode_set's
            // collision block; the key difference is that account_id is
            // the destination, NOT from_account_id.
            // -------------------------------------------------------------------
            let node_name = source_node.name.clone();
            let parent_id_for_collision: Option<Id> = source_node.parent_id.clone();

            if !node_name.is_empty() {
                let collision = match backend
                    .find_sibling_by_name(
                        caller,
                        &account_id,
                        parent_id_for_collision.as_ref(),
                        &node_name,
                        compare_case_insensitively,
                    )
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        not_copied.insert(
                            create_id,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                };

                if let Some(existing_id) = collision {
                    match on_exists {
                        OnExists::Reject => {
                            not_copied.insert(
                                create_id,
                                json!({ "type": "alreadyExists", "existingId": existing_id.as_ref() }),
                            );
                            continue;
                        }
                        OnExists::Replace => {
                            // §3.2.3 lines 565-570: nodeHasChildren UNLESS
                            // onDestroyRemoveChildren=true; in which case
                            // cascade-destroy descendants first.
                            let desc_ids = match backend
                                .get_descendant_ids(caller, &account_id, &existing_id)
                                .await
                            {
                                Ok(ids) => ids,
                                Err(e) => {
                                    not_copied.insert(
                                        create_id,
                                        json!({ "type": "serverFail", "description": e.to_string() }),
                                    );
                                    continue;
                                }
                            };
                            if !desc_ids.is_empty() && !on_destroy_remove_children {
                                not_copied.insert(
                                    create_id,
                                    json!({ "type": "nodeHasChildren", "existingId": existing_id.as_ref() }),
                                );
                                continue;
                            }
                            let mut cascade_failed = false;
                            for desc_id in &desc_ids {
                                match backend
                                    .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                    .await
                                {
                                    Ok(()) => {
                                        mutated = true;
                                        // No wire signal — see note on
                                        // destroyed_list omission above.
                                        let _ = desc_id;
                                    }
                                    Err(BackendSetError::SetError(set_err)) => {
                                        not_copied
                                            .insert(create_id.clone(), set_error_value(&set_err));
                                        cascade_failed = true;
                                        break;
                                    }
                                    Err(BackendSetError::Other(e)) => {
                                        not_copied.insert(
                                            create_id.clone(),
                                            json!({ "type": "serverFail", "description": e.to_string() }),
                                        );
                                        cascade_failed = true;
                                        break;
                                    }
                                    Err(_) => {
                                        not_copied.insert(
                                            create_id.clone(),
                                            json!({
                                                "type": "serverFail",
                                                "description": "unhandled backend error variant",
                                            }),
                                        );
                                        cascade_failed = true;
                                        break;
                                    }
                                }
                            }
                            if cascade_failed {
                                continue;
                            }
                            // Now destroy the colliding node itself.
                            match backend
                                .destroy_object::<FileNode>(caller, &account_id, &existing_id)
                                .await
                            {
                                Ok(()) => {
                                    mutated = true;
                                    // No wire signal — see note above.
                                    let _ = &existing_id;
                                }
                                Err(BackendSetError::SetError(set_err)) => {
                                    not_copied.insert(create_id, set_error_value(&set_err));
                                    continue;
                                }
                                Err(BackendSetError::Other(e)) => {
                                    not_copied.insert(
                                        create_id,
                                        json!({ "type": "serverFail", "description": e.to_string() }),
                                    );
                                    continue;
                                }
                                Err(_) => {
                                    not_copied.insert(
                                        create_id,
                                        json!({
                                            "type": "serverFail",
                                            "description": "unhandled backend error variant",
                                        }),
                                    );
                                    continue;
                                }
                            }
                            // Fall through to create the new node.
                        }
                        OnExists::Rename => {
                            // Find a non-colliding name by appending a
                            // counter suffix; matches handle_filenode_set's
                            // 100-attempt bound.
                            let mut renamed = false;
                            let mut rename_error = false;
                            for suffix in 1_u32..=100 {
                                let candidate = format!("{node_name}-{suffix}");
                                match backend
                                    .find_sibling_by_name(
                                        caller,
                                        &account_id,
                                        parent_id_for_collision.as_ref(),
                                        &candidate,
                                        compare_case_insensitively,
                                    )
                                    .await
                                {
                                    Ok(None) => {
                                        source_node.name = candidate.clone();
                                        renamed_to = Some(candidate);
                                        renamed = true;
                                        break;
                                    }
                                    Ok(Some(_)) => continue,
                                    Err(e) => {
                                        not_copied.insert(
                                            create_id.clone(),
                                            json!({ "type": "serverFail", "description": e.to_string() }),
                                        );
                                        rename_error = true;
                                        break;
                                    }
                                }
                            }
                            if rename_error {
                                continue;
                            }
                            if !renamed {
                                not_copied.insert(
                                    create_id,
                                    json!({
                                        "type": "alreadyExists",
                                        "description": "no available name within 100 rename attempts",
                                    }),
                                );
                                continue;
                            }
                        }
                    }
                }
            }

            // Create in the destination account.
            match backend
                .create_object::<FileNode>(caller, &account_id, &create_id, source_node)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    let mut value = serde_json::to_value(&created_obj)
                        .expect("derive(Serialize) on plain data is infallible");
                    // §3.2.3 lines 572-575 MUST: enforce the renamed
                    // name in the response regardless of whether the
                    // backend echoed it.
                    if let Some(name) = renamed_to.as_deref() {
                        if let Value::Object(ref mut m) = value {
                            m.insert("name".to_owned(), Value::String(name.to_owned()));
                        }
                    }
                    copied.insert(create_id, value);
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
                Err(_) => {
                    not_copied.insert(
                        create_id,
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<FileNode>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
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
///
/// Supports the `depth` argument (§3.2.5): when `depth > 0`, the query is
/// recursively expanded by re-querying with `parentId = <matched_id>` for up to
/// `depth` additional levels. IDs are deduplicated across all levels.
///
/// When `depth` is absent, `null`, or `0`, the query is a flat one-liner
/// delegated to [`jmap_server::handlers::handle_query`].
pub async fn handle_filenode_query<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // Extract depth before delegating — the generic handler strips unrecognised args.
    let depth: u64 = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(0);

    let (account_id, args_map) = crate::helpers::extract_account_id(args)?;

    // Reconstitute the args object for the generic handler. The generic helper
    // re-parses accountId itself, which is fine — the duplicate parse is cheap.
    let mut reconstituted = args_map;
    reconstituted.insert(
        "accountId".to_owned(),
        Value::String(account_id.as_ref().to_owned()),
    );

    // Delegate to the generic query handler for the first (level-0) result.
    let (mut response, tail) = jmap_server::handlers::handle_query::<FileNode, B>(
        backend,
        caller,
        Value::Object(reconstituted),
    )
    .await?;

    if depth == 0 {
        return Ok((response, tail));
    }

    // depth > 0: use backend.query_subtree for recursive expansion.
    // query_subtree returns all descendant IDs up to `depth` levels deep.
    // root_ids are the IDs from the initial query result.
    let root_ids: Vec<Id> = response["ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(Id::from)).collect())
        .unwrap_or_default();

    if root_ids.is_empty() {
        return Ok((response, tail));
    }

    // draft-ietf-jmap-filenode-13 §3.2.5: when `depth > 0` the client is
    // requesting an expanded subtree. A backend error here is not optional
    // — silently downgrading to a flat result would surface as a
    // successful depth=N query containing only the level-0 ids, with no
    // way for the client to detect the partiality.
    let descendant_ids = backend
        .query_subtree(caller, &account_id, &root_ids, depth)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Merge root_ids + descendant_ids, preserving root order then descendant order.
    let mut combined: Vec<Value> = root_ids
        .iter()
        .map(|id| Value::String(id.as_ref().to_owned()))
        .collect();
    for id in &descendant_ids {
        combined.push(Value::String(id.as_ref().to_owned()));
    }
    let total = combined.len() as u64;
    response["ids"] = Value::Array(combined);
    if let Some(t) = response.get_mut("total") {
        *t = Value::Number(total.into());
    }

    Ok((response, tail))
}

// ---------------------------------------------------------------------------
// FileNode/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `FileNode/queryChanges` method call (draft-ietf-jmap-filenode-13 §3.2.6).
pub async fn handle_filenode_query_changes<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<FileNode, B>(backend, caller, args).await
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
        let err = handle_filenode_get(&backend, &(), args)
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
        let (resp, _) = handle_filenode_get(&backend, &(), args)
            .await
            .expect("must succeed for known account");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["list"].as_array().unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // FileNode/get — fetchParents
    // -----------------------------------------------------------------------

    /// Oracle: fetchParents=false → response unchanged (no ancestor expansion).
    /// Source: draft-ietf-jmap-filenode-13 §3.2.1.
    #[tokio::test]
    async fn get_fetch_parents_false_unchanged() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "ids": null, "fetchParents": false });
        let (resp, _) = handle_filenode_get(&backend, &(), args)
            .await
            .expect("must succeed");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["list"].as_array().unwrap().is_empty());
    }

    /// Oracle: fetchParents=true with no nodes → list unchanged (no ancestors to fetch).
    /// Source: draft-ietf-jmap-filenode-13 §3.2.1.
    #[tokio::test]
    async fn get_fetch_parents_true_no_nodes() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "ids": null, "fetchParents": true });
        let (resp, _) = handle_filenode_get(&backend, &(), args)
            .await
            .expect("must succeed");
        assert!(resp["list"].as_array().unwrap().is_empty());
    }

    /// Oracle: fetchParents=true with a non-empty list, and the backend
    /// returns `Err` from `get_ancestors`: the handler MUST surface the
    /// error as a JMAP serverFail rather than silently returning the
    /// un-expanded list (which would look identical on the wire to a
    /// successful fetchParents=true response where every node is at root).
    ///
    /// Regression for bd JMAP-510h.6 — the prior code used
    /// `if let Ok(ancestors) = ... { ... } // Non-fatal` which discarded
    /// the error.
    #[tokio::test]
    async fn get_fetch_parents_propagates_get_ancestors_err() {
        let backend = MockBackend::new_with_account("acc1");
        // Seed one FileNode so the response `list` is non-empty and
        // the fetchParents branch is taken. Minimal valid FileNode
        // shape; the type crate's deserializer accepts these fields.
        backend.add_get_objects_node(json!({
            "id": "child",
            "name": "child",
            "parentId": null,
            "role": null,
            "isExecutable": false,
            "isTopLevel": true,
            "nodeType": "directory",
            "target": null,
            "blobId": null,
            "type": null,
            "size": 0,
            "shareWith": null,
            "myRights": null,
            "annotations": null,
            "created": "1970-01-01T00:00:00Z",
            "modified": "1970-01-01T00:00:00Z",
            "accessed": null
        }));
        backend.set_get_ancestors_err("simulated DB timeout");

        let args = json!({
            "accountId": "acc1",
            "ids": ["child"],
            "fetchParents": true
        });
        let result = handle_filenode_get(&backend, &(), args).await;
        assert!(
            result.is_err(),
            "fetchParents + get_ancestors Err must propagate as JMAP \
             error, not return a silently-truncated list. got Ok: {:?}",
            result.ok()
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/changes
    // -----------------------------------------------------------------------

    /// Oracle: FileNode/changes returns the standard changes response shape.
    #[tokio::test]
    async fn changes_returns_standard_shape() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let (resp, _) = handle_filenode_changes(&backend, &(), args)
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
        let (resp, _) = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["ids"].is_array());
    }

    // ── depth parameter (draft-ietf-jmap-filenode-13 §3.2.5) ────────────────

    /// Oracle: depth absent → flat query, same result as before.
    #[tokio::test]
    async fn query_depth_absent_flat_query() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "filter": null, "sort": null });
        let (resp, _) = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed for absent depth");
        assert!(resp["ids"].as_array().unwrap().is_empty());
    }

    /// Oracle: depth=0 → same as absent, no recursion.
    #[tokio::test]
    async fn query_depth_zero_flat_query() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({ "accountId": "acc1", "depth": 0, "filter": null, "sort": null });
        let (resp, _) = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed for depth=0");
        assert!(resp["ids"].as_array().unwrap().is_empty());
    }

    /// Oracle: depth=1 → initial result plus direct children of matched nodes.
    /// Backend is seeded so that "dir1" has children ["child1", "child2"].
    /// The initial query returns ["dir1"] (via parentId=None/root filter);
    /// depth=1 expansion fetches parentId="dir1" and adds children.
    #[tokio::test]
    async fn query_depth_one_returns_children() {
        let backend = MockBackend::new_with_account("acc1");
        // Root-level node returned by the initial query.
        backend.set_children(None, &["dir1"]);
        // dir1's children.
        backend.set_children(Some("dir1"), &["child1", "child2"]);

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let (resp, _) = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed for depth=1");
        let ids = resp["ids"].as_array().expect("ids must be array");
        // dir1 + child1 + child2 = 3 ids total.
        assert_eq!(
            ids.len(),
            3,
            "depth=1 must include dir and its children: {ids:?}"
        );
        let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
        assert!(id_strs.contains(&"dir1"), "dir1 must be in result");
        assert!(id_strs.contains(&"child1"), "child1 must be in result");
        assert!(id_strs.contains(&"child2"), "child2 must be in result");
    }

    /// Oracle: depth=1 with a deep tree stops at 1 level.
    /// child1 has grandchildren, but depth=1 must NOT return them.
    #[tokio::test]
    async fn query_depth_one_stops_at_first_level() {
        let backend = MockBackend::new_with_account("acc1");
        backend.set_children(None, &["dir1"]);
        backend.set_children(Some("dir1"), &["child1"]);
        backend.set_children(Some("child1"), &["grandchild1"]);

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let (resp, _) = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        let ids = resp["ids"].as_array().expect("ids must be array");
        // dir1 + child1 = 2; grandchild1 must NOT appear.
        assert_eq!(ids.len(), 2, "depth=1 must stop at direct children");
        let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            !id_strs.contains(&"grandchild1"),
            "grandchild must not appear at depth=1"
        );
    }

    /// Oracle: depth=2 → two levels of expansion.
    #[tokio::test]
    async fn query_depth_two_returns_grandchildren() {
        let backend = MockBackend::new_with_account("acc1");
        backend.set_children(None, &["dir1"]);
        backend.set_children(Some("dir1"), &["child1"]);
        backend.set_children(Some("child1"), &["grandchild1"]);

        let args = json!({
            "accountId": "acc1",
            "depth": 2,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let (resp, _) = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        let ids = resp["ids"].as_array().expect("ids must be array");
        // dir1 + child1 + grandchild1 = 3.
        assert_eq!(ids.len(), 3, "depth=2 must include grandchild");
        let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            id_strs.contains(&"grandchild1"),
            "grandchild1 must appear at depth=2"
        );
    }

    /// Oracle: query_subtree with depth=1 returns same results as the manual loop.
    /// Regression test: the refactored implementation must produce identical output.
    #[tokio::test]
    async fn query_depth_one_via_subtree_matches_manual_loop() {
        let backend = MockBackend::new_with_account("acc1");
        backend.set_children(None, &["dir1"]);
        backend.set_children(Some("dir1"), &["child1", "child2"]);

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let (resp, _) = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        let ids = resp["ids"].as_array().expect("ids array");
        assert_eq!(ids.len(), 3, "dir1 + child1 + child2 = 3");
    }

    /// Oracle: handle_filenode_query with depth>0 must propagate a backend
    /// failure FROM THE DEPTH-EXPANSION PATH as a JMAP serverFail, not
    /// silently downgrade to a flat (level-0-only) result.
    ///
    /// Setup: seed level-0 (root) so the initial top-level query succeeds,
    /// then arm `query_objects` to fail on its second call (the first
    /// per-level call inside `query_subtree`). The handler must surface
    /// that Err rather than swallowing it and returning the level-0
    /// result.
    ///
    /// Regression for bd JMAP-510h.5 — the prior code used
    /// `match backend.query_subtree(...) { Err(e) => { let _ = e;
    /// return Ok((response, tail)); } }` which made a depth>0 failure
    /// indistinguishable from a depth>0 success with zero descendants.
    #[tokio::test]
    async fn query_depth_gt_zero_propagates_query_subtree_err() {
        let backend = MockBackend::new_with_account("acc1");
        backend.set_children(None, &["root"]);
        // Succeed on the initial top-level query_objects (call 0),
        // fail on the per-level recursion inside query_subtree (call 1).
        backend.set_query_objects_err_after("simulated DB timeout", 1);

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let result = handle_filenode_query(&backend, &(), args).await;
        assert!(
            result.is_err(),
            "depth>0 + backend failure during expansion must produce \
             JMAP error, not a silently-downgraded flat result. got Ok: {:?}",
            result.ok()
        );
    }

    /// Oracle: when the backend's `query_objects` returns `Err` on a per-level
    /// recursion call from the default `query_subtree` impl, the error MUST
    /// propagate to the caller. Workspace policy treats silent-drop in a
    /// query result as a server-side correctness bug (workspace AGENTS.md,
    /// Filter algebra exclusion §1).
    ///
    /// Regression for bd JMAP-510h.4 — the prior impl had
    /// `if let Ok(result) = ...` which swallowed transient backend errors
    /// and returned a truncated subtree.
    #[tokio::test]
    async fn query_subtree_default_impl_propagates_query_objects_err() {
        use crate::backend::FileNodeBackend;

        let backend = MockBackend::new_with_account("acc1");
        // Seed level 0 so the loop has work to do, then arm the failure
        // injection for the per-level query_objects call.
        backend.set_children(None, &["root"]);
        backend.set_query_objects_err("simulated DB timeout");

        let root_ids = [Id::from("root")];
        let result = backend
            .query_subtree(&(), &Id::from("acc1"), &root_ids, 2)
            .await;
        assert!(
            result.is_err(),
            "query_subtree must propagate query_objects Err, got Ok({:?})",
            result.ok()
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("simulated DB timeout"),
            "propagated error must carry the backend error message, got: {err_msg}"
        );
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
        // Declare dir1 has one child not in this destroy request.
        backend.set_descendants("dir1", &["child1"]);

        let args = json!({
            "accountId": "acc1",
            "destroy": ["dir1"]
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
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
        backend.set_descendants("dir1", &["child1"]);

        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveChildren": true,
            "destroy": ["dir1"]
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
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
    // FileNode/set — cascade destroy (onDestroyRemoveChildren=true)
    // -----------------------------------------------------------------------

    /// Oracle: onDestroyRemoveChildren=true; descendants go to backend destroy
    /// (notFound from mock), NOT with nodeHasChildren error.
    /// Source: draft-ietf-jmap-filenode-13 §3.2.3.
    #[tokio::test]
    async fn set_destroy_with_remove_children_true_includes_descendants_in_destroyed() {
        let backend = MockBackend::new_with_account("acc1");
        backend.set_descendants("dir1", &["child1", "child2"]);

        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveChildren": true,
            "destroy": ["dir1"]
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // Mock destroy always returns notFound, so descendants go to notDestroyed.
        // They must NOT be notFound due to nodeHasChildren — they should be notFound
        // from the backend, meaning cascade was attempted.
        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );

        // The cascade hit child1 first and failed with notFound (backend error),
        // so it was reported under the parent id. No nodeHasChildren error.
        assert_ne!(
            not_destroyed["dir1"]["type"].as_str(),
            Some("nodeHasChildren"),
            "nodeHasChildren MUST NOT appear when onDestroyRemoveChildren=true: {resp}"
        );
        // The error must be notFound (propagated from backend cascade failure).
        assert_eq!(
            not_destroyed["dir1"]["type"], "notFound",
            "cascade failure should report backend's notFound: {resp}"
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
        // "node2" is a descendant of "node1", so moving node1 under node2
        // would create a cycle.
        backend.set_descendants("node1", &["node2"]);

        let args = json!({
            "accountId": "acc1",
            "update": {
                "node1": { "parentId": "node2" }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
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

    /// Oracle: update that sets parentId equal to the node's own id is the
    /// trivial cycle (a node cannot be its own parent). It must produce
    /// notUpdated with invalidProperties on parentId.
    ///
    /// Source: draft-ietf-jmap-filenode-13 §3.2.3 — "an attempt to move a
    /// node to a parent for which this node is also an ancestor is an
    /// error". A node is trivially an ancestor of itself. The standard
    /// descendant-set check cannot catch this because `get_descendant_ids`
    /// excludes the node itself by contract (backend.rs §76).
    #[tokio::test]
    async fn set_update_self_parent_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        // No descendants seeded: the only thing the check has to fall
        // back on is the explicit self-parent guard.
        let args = json!({
            "accountId": "acc1",
            "update": {
                "node1": { "parentId": "node1" }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present: {resp}"
        );
        assert_eq!(
            not_updated["node1"]["type"], "invalidProperties",
            "self-parent must produce invalidProperties: {resp}"
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
    // FileNode/set — destroy parent + all children in same request
    // -----------------------------------------------------------------------

    /// Oracle: draft-ietf-jmap-filenode-13 §3.2.3 — MUST NOT return
    /// nodeHasChildren when all children of the destroyed node are also in
    /// the same destroy request.
    #[tokio::test]
    async fn set_destroy_parent_and_all_children_succeeds() {
        let backend = MockBackend::new_with_account("acc1");
        // node_parent has one child: node_child. Both are in the destroy request.
        backend.set_descendants("node_parent", &["node_child"]);

        let args = json!({
            "accountId": "acc1",
            "destroy": ["node_parent", "node_child"]
            // onDestroyRemoveChildren is absent (defaults to false)
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // Both should be in notDestroyed with type=notFound (from mock backend),
        // NOT with type=nodeHasChildren. The all-covered check must pass.
        let not_destroyed = resp["notDestroyed"].as_object();
        if let Some(nd) = not_destroyed {
            for (_, err) in nd {
                assert_ne!(
                    err["type"].as_str(),
                    Some("nodeHasChildren"),
                    "nodeHasChildren MUST NOT appear when all children are in the destroy set"
                );
            }
        }
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
        let (resp, _) = handle_filenode_set(&backend, &(), args)
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

    // -----------------------------------------------------------------------
    // FileNode/set — nodeType inference and blobId validation (§3.1)
    // -----------------------------------------------------------------------

    /// Oracle: create with no nodeType, blobId, or target → inferred as
    /// "directory" and succeeds (no consistency error).
    /// Source: draft-ietf-jmap-filenode-13 §3.1.
    #[tokio::test]
    async fn set_create_directory_without_blobid_infers_directory_type() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "mydir", "role": null }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        // No notCreated: directory inference should succeed.
        assert!(
            resp["notCreated"].is_null(),
            "directory inference must not produce an error: {resp}"
        );
        assert!(
            resp["created"].is_object(),
            "created must be present: {resp}"
        );
    }

    /// Oracle: create with nodeType="file" and blobId=null → invalidProperties
    /// on "blobId" (file node requires blobId).
    /// Source: draft-ietf-jmap-filenode-13 §3.1.
    #[tokio::test]
    async fn set_create_file_without_blobid_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "myfile", "nodeType": "file", "blobId": null, "role": null }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "file without blobId must produce invalidProperties: {resp}"
        );
        let props = &not_created["c1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("blobId")))
                .unwrap_or(false),
            "blobId must be listed in properties: {resp}"
        );
    }

    /// Oracle: create with nodeType="symlink" and target=null → invalidProperties
    /// on "target" (symlink node requires target).
    /// Source: draft-ietf-jmap-filenode-13 §3.1.
    #[tokio::test]
    async fn set_create_symlink_without_target_returns_invalid_properties() {
        let backend = MockBackend::new_with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "mylink", "nodeType": "symlink", "target": null, "role": null }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "symlink without target must produce invalidProperties: {resp}"
        );
        let props = &not_created["c1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("target")))
                .unwrap_or(false),
            "target must be listed in properties: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — onExists collision detection (§3.2.3)
    // -----------------------------------------------------------------------

    /// Oracle: create when a sibling with the same name exists and onExists is
    /// absent (default Reject) → notCreated with type "alreadyExists".
    /// Source: draft-ietf-jmap-filenode-13 §3.2.3.
    #[tokio::test]
    async fn set_create_collision_reject_returns_already_exists() {
        let backend = MockBackend::new_with_account("acc1");
        // Register a sibling named "foo" under root (no parent).
        backend.set_sibling(None, "foo", "existing-id-1");

        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "foo", "parentId": null, "role": null }
            }
            // onExists absent → default Reject
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "alreadyExists",
            "collision must produce alreadyExists: {resp}"
        );
        assert_eq!(
            not_created["c1"]["existingId"], "existing-id-1",
            "existingId must match: {resp}"
        );
    }

    /// Oracle: create with onExists="rename" → name is suffixed to avoid collision.
    /// Source: draft-ietf-jmap-filenode-13 §3.2.3.
    #[tokio::test]
    async fn set_create_collision_rename_uses_suffixed_name() {
        let backend = MockBackend::new_with_account("acc1");
        // "foo" collides; "foo-1" does not.
        backend.set_sibling(None, "foo", "existing-id-1");

        let args = json!({
            "accountId": "acc1",
            "onExists": "rename",
            "create": {
                "c1": { "name": "foo", "parentId": null, "role": null }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        // With rename, creation should succeed (mock create_object returns Ok).
        assert!(
            resp["notCreated"].is_null(),
            "rename must not produce notCreated: {resp}"
        );
        assert!(
            resp["created"].is_object(),
            "created must be present: {resp}"
        );
    }

    /// Oracle: draft-ietf-jmap-filenode-13 §3.2.3 lines 572-575 — "If
    /// the server changes the name, it MUST include the new 'name'
    /// value in the created or updated response field for this id."
    ///
    /// The handler must enforce this MUST regardless of what the
    /// backend echoes back: a backend that normalises names (strips
    /// trailing whitespace, applies NFC, etc.) and returns a value
    /// different from the supplied one would otherwise hide the rename
    /// from the client. Regression for bd JMAP-510h.8.
    #[tokio::test]
    async fn set_create_rename_response_name_overrides_backend_echo() {
        let backend = MockBackend::new_with_account("acc1");
        // Pre-seed collision on "foo".
        backend.set_sibling(None, "foo", "existing-id-1");
        // Simulate a backend that returns a different name from what
        // the handler asked it to store. Without the post-create
        // enforcement, the response would carry "backend-normalised"
        // instead of the renamed "foo-1".
        backend.set_create_object_override_name("backend-normalised");

        let args = json!({
            "accountId": "acc1",
            "onExists": "rename",
            "create": {
                "c1": { "name": "foo", "parentId": null, "role": null }
            }
        });
        let (resp, _) = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp["created"].is_object() && resp["created"]["c1"].is_object(),
            "created must be present: {resp}"
        );
        let name = resp["created"]["c1"]["name"]
            .as_str()
            .expect("created.c1.name must be present");
        assert_eq!(
            name, "foo-1",
            "the renamed name MUST appear in the response (§3.2.3 MUST), \
             not the backend's echoed name: {resp}"
        );
    }
}
