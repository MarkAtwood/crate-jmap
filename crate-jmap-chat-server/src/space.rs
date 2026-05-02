//! Space/* method handlers (JMAP Chat extension §Space).

use jmap_chat_types::{Space, SpaceInvite};
use jmap_types::{Id, Invocation, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{
    extract_account_id, iso8601_before, not_found_json, now_utc_string, ser, set_error_value,
};

// ---------------------------------------------------------------------------
// Space/get
// ---------------------------------------------------------------------------

/// Handle a `Space/get` method call.
pub async fn handle_space_get<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let ids: Option<Vec<Id>> = match args.remove("ids").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<Space>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Space>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list.iter().map(ser).collect::<Result<Vec<_>, _>>()?;

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
// Space/changes
// ---------------------------------------------------------------------------

/// Handle a `Space/changes` method call (RFC 8620 §5.2).
pub async fn handle_space_changes<B: ChatBackend>(
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
        Some(v) => Some(v.as_u64().filter(|&n| n > 0).ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let result = backend
        .get_changes::<Space>(&account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": since_state.as_ref(),
            "newState": result.new_state.as_ref(),
            "hasMoreChanges": result.has_more_changes,
            "updatedProperties": Value::Null,
            "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Space/query
// ---------------------------------------------------------------------------

/// Handle a `Space/query` method call (RFC 8620 §5.5).
///
/// Filter and sort are passed through to the backend unchanged.
pub async fn handle_space_query<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments(
            "arguments must be a JSON object",
        ));
    };

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.remove("limit").unwrap_or(Value::Null) {
        Value::Null => None,
        v => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )))
            }
        },
    };

    let position: i64 = match args.remove("position").unwrap_or(Value::Null) {
        Value::Null => 0,
        v => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let filter: Option<serde_json::Value> = match args.remove("filter").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(v),
    };

    let sort: Option<Vec<serde_json::Value>> = match args.remove("sort").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("sort must be an array"))?,
        ),
    };

    let result = backend
        .query_objects::<Space>(
            &account_id,
            filter.as_ref(),
            sort.as_deref(),
            limit,
            position,
        )
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "ids": result.ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Space/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Space/queryChanges` method call (RFC 8620 §5.6).
pub async fn handle_space_query_changes<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

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

    let result = backend
        .query_changes::<Space>(
            &account_id,
            &since_query_state,
            None,
            None,
            max_changes,
            up_to_id.as_ref(),
            false,
        )
        .await
        .map_err(JmapError::from)?;

    let removed: Vec<&str> = result.removed.iter().map(|id| id.as_ref()).collect();
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
        "removed": removed,
        "added": added,
    });
    if calculate_total {
        if let Some(t) = result.total {
            resp["total"] = json!(t);
        }
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Space/set
// ---------------------------------------------------------------------------

/// Handle a `Space/set` method call.
///
/// Validation enforced here (not in the backend):
/// - `name` is required on create.
/// - `id`, `createdAt`, `memberCount` are server-set and rejected in updates.
pub async fn handle_space_set<B: ChatBackend>(
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
        .get_state::<Space>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

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
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            let name = match obj_val.get("name").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_owned(),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["name"] }),
                    );
                    continue;
                }
            };
            if name.len() > 256 {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["name"] }),
                );
                continue;
            }

            let is_public = obj_val
                .get("isPublic")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_publicly_previewable = obj_val
                .get("isPubliclyPreviewable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_str());

            let mut space = Space::new(
                Id::from("placeholder"),
                name,
                vec![],
                vec![],
                vec![],
                vec![],
                now,
                is_public,
                is_publicly_previewable,
                0,
            );

            if let Some(desc) = obj_val.get("description").and_then(|v| v.as_str()) {
                space.description = Some(desc.to_owned());
            }

            match backend
                .create_object::<Space>(&account_id, create_id, space)
                .await
            {
                Ok((_server_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id.clone(),
                        serde_json::to_value(&created_obj).unwrap_or_else(
                            |e| json!({ "type": "serverFail", "description": e.to_string() }),
                        ),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_created.insert(
                        create_id.clone(),
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

            // Reject patches that include server-set or directly-overwritable fields.
            // `roles`, `members`, `categories`, and `uncategorizedChannelIds` are
            // managed through named semantic mutations (addRoles/removeRoles, etc.)
            // and must never be overwritten directly via a JSON Merge Patch.
            const SPACE_READONLY: &[&str] = &[
                "id",
                "createdAt",
                "memberCount",
                "roles",
                "members",
                "categories",
                "uncategorizedChannelIds",
            ];
            let bad_props: Vec<&str> = SPACE_READONLY
                .iter()
                .copied()
                .filter(|&field| patch_val.get(field).is_some())
                .collect();
            if !bad_props.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": bad_props }),
                );
                continue;
            }

            // Structural mutations require full permission-hierarchy support.
            const STRUCTURAL_MUTATIONS: &[&str] = &[
                "addRoles",
                "removeRoles",
                "updateRoles",
                "addMembers",
                "removeMembers",
                "updateMembers",
                "addChannels",
                "removeChannels",
                "updateChannels",
                "addCategories",
                "removeCategories",
                "updateCategories",
            ];
            let structural: Vec<&str> = STRUCTURAL_MUTATIONS
                .iter()
                .copied()
                .filter(|&k| patch_val.get(k).is_some())
                .collect();
            if !structural.is_empty() {
                let err = SetError::new(SetErrorType::Forbidden).with_description(
                    "Role, member, and channel mutations are not yet implemented",
                );
                not_updated.insert(id_str, set_error_value(&err));
                continue;
            }

            // Build a clean patch containing only the allowed metadata fields.
            const METADATA_FIELDS: &[&str] = &[
                "name",
                "description",
                "iconBlobId",
                "isPublic",
                "isPubliclyPreviewable",
            ];
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            let mut clean_patch = serde_json::Map::new();
            for &field in METADATA_FIELDS {
                if let Some(v) = patch_map.remove(field) {
                    clean_patch.insert(field.to_owned(), v);
                }
            }

            if clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch contains no valid fields" }),
                );
                continue;
            }

            match backend
                .update_object::<Space>(&account_id, &id, Value::Object(clean_patch))
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(id_str, serde_json::to_value(&obj).unwrap_or_else(|e| serde_json::json!({ "type": "serverFail", "description": e.to_string() })));
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
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let id = Id::from(id_str);

            match backend.destroy_object::<Space>(&account_id, &id).await {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&set_err));
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<Space>(&account_id)
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
            "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
            "updated": if updated.is_empty() { Value::Null } else { Value::Object(updated) },
            "destroyed": if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
            "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
            "notUpdated": if not_updated.is_empty() { Value::Null } else { Value::Object(not_updated) },
            "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Space/join
// ---------------------------------------------------------------------------

/// Handle a `Space/join` method call.
///
/// Accepts exactly one of `inviteCode` or `spaceId`. Validates the invite or
/// space, adds the caller as a member, and returns `{ "accountId": ..., "spaceId": ... }`.
pub async fn handle_space_join<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let invite_code = args
        .get("inviteCode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let space_id_str = args
        .get("spaceId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    // Validate the invite or space and collect the space_id, current members, and
    // (for the invite path) the pending invite-uses increment deferred until after
    // the already_member check.  Each branch produces
    // (space_id: Id, current_members: Vec<Value>, invite_update: Option<(Id, u64)>).
    let (space_id, current_members, invite_update): (Id, Vec<Value>, Option<(Id, u64)>) =
        match (invite_code, space_id_str) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(JmapError::invalid_arguments(
                    "exactly one of inviteCode or spaceId must be provided",
                ));
            }
            (Some(code), None) => {
                // NOTE: The MemoryBackend stores objects per-account, so invite code lookup
                // works only when the caller's account created the invite. A production backend
                // must maintain a global invite code index. This is a known architectural
                // limitation of the test backend.

                // Invite code path: scan all invites for matching code.
                let (invites, _) = backend
                    .get_objects::<SpaceInvite>(&account_id, None, None)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;

                let invite = invites
                    .into_iter()
                    .find(|inv| inv.code == code)
                    .ok_or_else(|| JmapError::invalid_arguments("invite code not found"))?;

                // Check expiry using second-precision prefix (see iso8601_before).
                // Pure lexicographic comparison on the full string is incorrect for
                // fractional-second timestamps ('.' < 'Z' in ASCII).
                if let Some(expires_at) = &invite.expires_at {
                    let now = now_utc_string();
                    if !iso8601_before(now.as_str(), expires_at.as_ref()) {
                        return Err(JmapError::invalid_arguments("invite has expired"));
                    }
                }

                // Check maxUses: if set, uses must be strictly less.
                if let Some(max) = invite.max_uses {
                    if invite.uses >= max {
                        return Err(JmapError::invalid_arguments(
                            "invite has reached its maximum number of uses",
                        ));
                    }
                }

                let invite_id = invite.id.clone();
                let new_uses = invite.uses.saturating_add(1);
                let space_id = invite.space_id.clone();

                // Do NOT increment uses yet — defer until after the already_member check
                // so that a failed rejoin attempt does not silently exhaust invite uses.

                // Fetch the space to get the current members list.
                let (spaces, _) = backend
                    .get_objects::<Space>(&account_id, Some(std::slice::from_ref(&space_id)), None)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;
                let members: Vec<Value> = spaces
                    .into_iter()
                    .next()
                    .map(|s| {
                        s.members
                            .into_iter()
                            .map(serde_json::to_value)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .unwrap_or(Ok(vec![]))
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;

                (space_id, members, Some((invite_id, new_uses)))
            }
            (None, Some(sid)) => {
                // Public space path: fetch the space by id and verify is_public.
                // The spec requires notPermitted when the space is not found or isPublic is false.
                // JmapError has no notPermitted constructor; forbidden() is the closest standard
                // equivalent and is what the existing tests expect.
                let space_id_typed = Id::from(sid.as_str());
                let (spaces, _) = backend
                    .get_objects::<Space>(&account_id, Some(&[space_id_typed]), None)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;

                let space = spaces
                    .into_iter()
                    .next()
                    .filter(|s| s.is_public)
                    .ok_or_else(JmapError::forbidden)?;

                let space_id = space.id.clone();
                let members: Vec<Value> = space
                    .members
                    .into_iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;

                (space_id, members, None)
            }
        };

    // Add the calling account as a Space member.
    // This bypasses the SPACE_READONLY guard in handle_space_set — Space/join calling
    // update_object directly is correct: it is an atomic server operation, not a client patch.
    let now_str = now_utc_string();
    let mut new_members = current_members;

    // Check if already a member
    let already_member = new_members
        .iter()
        .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(account_id.as_ref()));
    if already_member {
        return Err(JmapError::invalid_arguments(
            "account is already a member of this space",
        ));
    }

    // Apply the deferred invite uses increment now that we know the join will succeed.
    if let Some((invite_id, new_uses)) = invite_update {
        backend
            .update_object::<SpaceInvite>(&account_id, &invite_id, json!({"uses": new_uses}))
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
    }

    new_members.push(json!({
        "id": account_id.as_ref(),
        "roleIds": [],
        "joinedAt": now_str,
    }));
    // Concurrency note: this is a read-modify-write on the members array.
    // Two concurrent Space/join calls for different accounts can both read
    // the same stale members list and overwrite each other. Production backends
    // MUST implement this as an atomic array-append (e.g. via a transaction or
    // compare-and-swap) to prevent membership loss.
    backend
        .update_object::<Space>(&account_id, &space_id, json!({"members": new_members}))
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    Ok((
        json!({ "accountId": account_id.as_ref(), "spaceId": space_id.as_ref() }),
        vec![],
    ))
}
