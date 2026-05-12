//! Space/* method handlers (JMAP Chat extension §Space).

use jmap_chat_types::space_set::{
    CategoryPatch, ChannelCreate, ChannelPatch, MemberPatch, RolePatch,
};
use jmap_chat_types::{Category, Space, SpaceInvite, SpaceRole};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;

use crate::backend::{BackendSetError, ChatBackend, SpacePatchOp};
use crate::helpers::{
    extract_account_id, finalize_set_response, iso8601_before, not_found_json, now_utc_string, ser,
    set_error_value, SetAccumulators,
};

// ---------------------------------------------------------------------------
// Space/set structural-mutation parsing
// ---------------------------------------------------------------------------

/// Parse one structural wire key's payload
/// (draft-atwood-jmap-chat-00 §Space/set) into a `Vec<SpacePatchOp>`.
///
/// The 12 structural keys are pluralized arrays whose elements have a
/// per-key shape; this helper handles all of them.
///
/// Returns an error string describing why parsing failed, suitable for use
/// as the `description` field of an `invalidProperties` SetError. The
/// returned error is fatal for the containing update target — the handler
/// inserts the failing wire key into `notUpdated[id].properties` and skips
/// any remaining keys.
fn parse_structural_entries(
    canonical: &'static str,
    value: Value,
) -> Result<Vec<SpacePatchOp>, String> {
    let arr = match value {
        Value::Array(a) => a,
        other => {
            return Err(format!(
                "{canonical} must be a JSON array, got {}",
                json_value_kind(&other)
            ));
        }
    };

    let mut out = Vec::with_capacity(arr.len());
    for (idx, entry) in arr.into_iter().enumerate() {
        let op = match canonical {
            "addRoles" => SpacePatchOp::AddRole(parse_entry::<SpaceRole>(canonical, idx, entry)?),
            "removeRoles" => SpacePatchOp::RemoveRole(parse_id_entry(canonical, idx, entry)?),
            "updateRoles" => {
                let (id, patch) = parse_update_entry::<RolePatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateRole { id, patch }
            }
            "addMembers" => {
                let (user_id, role_ids) = parse_add_member_entry(canonical, idx, entry)?;
                SpacePatchOp::AddMember { user_id, role_ids }
            }
            "removeMembers" => SpacePatchOp::RemoveMember(parse_id_entry(canonical, idx, entry)?),
            "updateMembers" => {
                let (id, patch) = parse_update_entry::<MemberPatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateMember { user_id: id, patch }
            }
            "addChannels" => {
                SpacePatchOp::AddChannel(parse_entry::<ChannelCreate>(canonical, idx, entry)?)
            }
            "removeChannels" => SpacePatchOp::RemoveChannel(parse_id_entry(canonical, idx, entry)?),
            "updateChannels" => {
                let (id, patch) = parse_update_entry::<ChannelPatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateChannel { id, patch }
            }
            "addCategories" => {
                SpacePatchOp::AddCategory(parse_entry::<Category>(canonical, idx, entry)?)
            }
            "removeCategories" => {
                SpacePatchOp::RemoveCategory(parse_id_entry(canonical, idx, entry)?)
            }
            "updateCategories" => {
                let (id, patch) = parse_update_entry::<CategoryPatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateCategory { id, patch }
            }
            _ => return Err(format!("internal: unhandled structural key {canonical}")),
        };
        out.push(op);
    }
    Ok(out)
}

/// Deserialize one Add* entry into the per-key payload type.
fn parse_entry<T: serde::de::DeserializeOwned>(
    canonical: &'static str,
    idx: usize,
    entry: Value,
) -> Result<T, String> {
    serde_json::from_value(entry)
        .map_err(|e| format!("{canonical}[{idx}]: failed to parse entry: {e}"))
}

/// Parse one Remove* entry — a bare string id.
fn parse_id_entry(canonical: &'static str, idx: usize, entry: Value) -> Result<Id, String> {
    match entry {
        Value::String(s) => Ok(Id::from(s.as_str())),
        other => Err(format!(
            "{canonical}[{idx}] must be a string Id, got {}",
            json_value_kind(&other)
        )),
    }
}

/// Parse one Update* entry into (id, patch). The wire form is an object
/// with an `id` property plus the patch fields at the top level; we split
/// `id` off and deserialize the remainder as the typed patch.
fn parse_update_entry<P: serde::de::DeserializeOwned>(
    canonical: &'static str,
    idx: usize,
    entry: Value,
) -> Result<(Id, P), String> {
    let mut obj = match entry {
        Value::Object(o) => o,
        other => {
            return Err(format!(
                "{canonical}[{idx}] must be a JSON object, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let id_val = obj
        .remove("id")
        .ok_or_else(|| format!("{canonical}[{idx}] missing required \"id\" property"))?;
    let id = match id_val {
        Value::String(s) => Id::from(s.as_str()),
        other => {
            return Err(format!(
                "{canonical}[{idx}].id must be a string, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let patch: P = serde_json::from_value(Value::Object(obj))
        .map_err(|e| format!("{canonical}[{idx}]: failed to parse patch fields: {e}"))?;
    Ok((id, patch))
}

/// Parse one `addMembers` entry. Wire form:
/// `{"id": "<ChatContact.id>", "roleIds": ["<RoleId>", …] (optional)}`.
fn parse_add_member_entry(
    canonical: &'static str,
    idx: usize,
    entry: Value,
) -> Result<(Id, Vec<Id>), String> {
    let mut obj = match entry {
        Value::Object(o) => o,
        other => {
            return Err(format!(
                "{canonical}[{idx}] must be a JSON object, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let id_val = obj
        .remove("id")
        .ok_or_else(|| format!("{canonical}[{idx}] missing required \"id\" property"))?;
    let user_id = match id_val {
        Value::String(s) => Id::from(s.as_str()),
        other => {
            return Err(format!(
                "{canonical}[{idx}].id must be a string, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let role_ids: Vec<Id> = match obj.remove("roleIds") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| format!("{canonical}[{idx}].roleIds must be a string array: {e}"))?,
    };
    if let Some(extra) = obj.keys().next() {
        return Err(format!(
            "{canonical}[{idx}] has unexpected property \"{extra}\""
        ));
    }
    Ok((user_id, role_ids))
}

/// Stringify a `serde_json::Value`'s top-level kind for error messages.
fn json_value_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// Space/get
// ---------------------------------------------------------------------------

/// Handle a `Space/get` method call.
pub async fn handle_space_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let ids: Option<Vec<Id>> = match args.remove("ids").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<Space>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Space>(caller, &account_id)
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Space, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Space/query
// ---------------------------------------------------------------------------

/// Handle a `Space/query` method call (RFC 8620 §5.5).
///
/// Filter and sort are passed through to the backend unchanged.
pub async fn handle_space_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

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
            caller,
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

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
            caller,
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
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let old_state = backend
        .get_state::<Space>(caller, &account_id)
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

            // Resolve the Space owner from the caller's principal id.
            // Backends that have not wired identity (test fixtures and
            // single-user dev servers — see `JmapBackend::principal_id`
            // doc) return `None`; in that case the account id stands
            // in as the owner, matching the single-user-mode mental
            // model where each account has exactly one principal.
            let owner_id = B::principal_id(caller)
                .cloned()
                .unwrap_or_else(|| account_id.clone());

            let mut space = Space::new(
                Id::from("placeholder"),
                owner_id,
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
                .create_object::<Space>(caller, &account_id, create_id, space)
                .await
            {
                Ok((_server_id, created_obj)) => {
                    mutated = true;
                    created.insert(
                        create_id.clone(),
                        serde_json::to_value(&created_obj)
                            .expect("derive(Serialize) on plain data is infallible"),
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
                Err(_) => {
                    not_created.insert(
                        create_id.clone(),
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

            // Reject patches that include server-set or directly-overwritable fields.
            // `roles`, `members`, `categories`, and `uncategorizedChannelIds` are
            // managed through named semantic mutations (addRoles/removeRoles, etc.)
            // and must never be overwritten directly via a JSON Merge Patch.
            // `ownerId` is immutable per the field doc — owner transfer is
            // reserved for a future `Space/transferOwnership` method.
            const SPACE_READONLY: &[&str] = &[
                "id",
                "ownerId",
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

            // The patch must be a JSON object. Reject `null`, arrays, and
            // scalars up front so the rest of the handler can assume a map.
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };

            // Allowed metadata fields (RFC 8620 §5.3 partial update on
            // server-managed properties). These reach `update_object` via a
            // JSON Merge Patch.
            const METADATA_FIELDS: &[&str] = &[
                "name",
                "description",
                "iconBlobId",
                "isPublic",
                "isPubliclyPreviewable",
            ];

            // Structural mutation keys (draft-atwood-jmap-chat-00 §Space/set).
            // Each maps to one family of `SpacePatchOp` variants. The order
            // here defines the wire-level apply order when multiple keys are
            // present in a single patch: roles before members (because
            // member-add may reference newly-created roles), channels before
            // categories (because category-update may reference channel
            // ids), and add before update before remove within each family.
            // Per draft §Space/set, the ordering is implementation-defined;
            // the reference handler picks the order that minimizes
            // cross-key dangling-reference errors.
            const STRUCTURAL_KEYS: &[&str] = &[
                "addRoles",
                "updateRoles",
                "removeRoles",
                "addMembers",
                "updateMembers",
                "removeMembers",
                "addChannels",
                "updateChannels",
                "removeChannels",
                "addCategories",
                "updateCategories",
                "removeCategories",
            ];

            // Walk every key on the patch object and bucket it as:
            //   - structural (parsed into SpacePatchOp values),
            //   - metadata (forwarded to update_object),
            //   - unknown (rejected as invalidProperties).
            // Any parse error on a structural entry is fatal for this target.
            let mut ops: Vec<SpacePatchOp> = Vec::new();
            let mut clean_patch = serde_json::Map::new();
            let mut unknown_keys: Vec<String> = Vec::new();
            let mut bad_structural_key: Option<(&'static str, String)> = None;

            for (key, value) in std::mem::take(&mut patch_map) {
                if let Some(&canonical) = STRUCTURAL_KEYS.iter().find(|&&k| k == key) {
                    match parse_structural_entries(canonical, value) {
                        Ok(parsed) => ops.extend(parsed),
                        Err(reason) => {
                            bad_structural_key = Some((canonical, reason));
                            break;
                        }
                    }
                } else if METADATA_FIELDS.contains(&key.as_str()) {
                    clean_patch.insert(key, value);
                } else {
                    unknown_keys.push(key);
                }
            }

            if let Some((canonical, reason)) = bad_structural_key {
                not_updated.insert(
                    id_str,
                    json!({
                        "type": "invalidProperties",
                        "properties": [canonical],
                        "description": reason,
                    }),
                );
                continue;
            }

            if !unknown_keys.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": unknown_keys }),
                );
                continue;
            }

            if ops.is_empty() && clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch contains no valid fields" }),
                );
                continue;
            }

            // Apply structural ops first. If any op fails, surface the first
            // failure as a `notUpdated` entry and skip the metadata write —
            // RFC 8620 §5.3 requires each update target to land in exactly
            // one of `updated` / `notUpdated`, and a partial half-applied
            // outcome would be misleading.
            if !ops.is_empty() {
                match backend
                    .apply_space_patch(caller, &account_id, &id, ops)
                    .await
                {
                    Ok(op_results) => {
                        if let Some(first_err) =
                            op_results.iter().find_map(|r| r.outcome.as_ref().err())
                        {
                            not_updated.insert(id_str, set_error_value(first_err));
                            continue;
                        }
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        not_updated.insert(id_str, set_error_value(&set_err));
                        continue;
                    }
                    Err(BackendSetError::Other(e)) => {
                        not_updated.insert(
                            id_str,
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                    Err(_) => {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "serverFail",
                                "description": "unhandled backend error variant",
                            }),
                        );
                        continue;
                    }
                }
            }

            // Apply metadata (if any) via the existing JSON Merge Patch path.
            // If there were no metadata fields, structural ops alone count
            // as a successful update — emit a null sentinel into `updated`.
            if clean_patch.is_empty() {
                mutated = true;
                updated.insert(id_str, Value::Null);
                continue;
            }

            match backend
                .update_object::<Space>(
                    caller,
                    &account_id,
                    &id,
                    PatchObject::from_map(clean_patch),
                )
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
    if let Some(destroy_arr) = args.get("destroy").and_then(|v| v.as_array()) {
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
                Some(s) => s,
                None => continue, // unreachable: validated above
            };
            let id = Id::from(id_str);

            match backend
                .destroy_object::<Space>(caller, &account_id, &id)
                .await
            {
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
                Err(_) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({
                            "type": "serverFail",
                            "description": "unhandled backend error variant",
                        }),
                    );
                }
            }
        }
    }

    finalize_set_response::<B, Space>(
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
// Space/join
// ---------------------------------------------------------------------------

/// Handle a `Space/join` method call.
///
/// Accepts exactly one of `inviteCode` or `spaceId`. Validates the invite or
/// space, adds the caller as a member, and returns `{ "accountId": ..., "spaceId": ... }`.
pub async fn handle_space_join<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

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
                    .get_objects::<SpaceInvite>(caller, &account_id, None, None)
                    .await
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;

                // Constant-time invite-code compare (bd:JMAP-sc1b.89).
                //
                // `SpaceInvite.code` is an unguessable CSPRNG-derived credential
                // (see `ChatBackend::generate_invite_code` in `backend.rs`). A
                // plain `String == String` short-circuits at the first mismatched
                // byte, exposing a byte-by-byte timing oracle to any caller that
                // can issue `Space/join` requests. `ConstantTimeEq::ct_eq`
                // compares whole equal-length byte slices in constant time.
                //
                // Length-discrimination note: `ct_eq` returns `Choice(0)` cheaply
                // when lengths differ, so an attacker can learn whether their
                // supplied code matches the stored length. The canonical
                // generator emits fixed-length (32 hex chars) codes, so the
                // length is effectively public — only the content needs
                // constant-time protection.
                let invite = invites
                    .into_iter()
                    .find(|inv| inv.code.as_bytes().ct_eq(code.as_bytes()).into())
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
                    .get_objects::<Space>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(&space_id)),
                        None,
                    )
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
                    .get_objects::<Space>(caller, &account_id, Some(&[space_id_typed]), None)
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

    // Pre-check: reject if already a member. This check is NOT atomic with the
    // write below — two concurrent requests can both pass and create duplicate
    // member entries. The post-write duplicate detection below catches this in
    // the common case. Storage-layer unique constraints are the authoritative guard.
    let already_member = new_members
        .iter()
        .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(account_id.as_ref()));
    if already_member {
        return Err(JmapError::invalid_arguments(
            "account is already a member of this space",
        ));
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
    let mut members_patch = serde_json::Map::new();
    members_patch.insert("members".to_owned(), json!(new_members));
    backend
        .update_object::<Space>(
            caller,
            &account_id,
            &space_id,
            PatchObject::from_map(members_patch),
        )
        .await
        .map_err(|e: jmap_server::BackendSetError<_>| JmapError::server_fail(e.to_string()))?;

    // TOCTOU guard: re-read members after write and detect duplicate entries.
    // Two concurrent join requests can both pass the pre-check above and both
    // succeed at the write layer. If that happened, exactly one racer must
    // undo its write. We detect the duplicate here and return an error;
    // the storage layer SHOULD enforce a unique constraint on (space_id, user_id)
    // as the authoritative guard — this code is best-effort self-healing.
    let (post_spaces, _) = backend
        .get_objects::<Space>(
            caller,
            &account_id,
            Some(std::slice::from_ref(&space_id)),
            None,
        )
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;
    let post_members: Vec<Value> = post_spaces
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
    let duplicate_count = post_members
        .iter()
        .filter(|m| m.get("id").and_then(|v| v.as_str()) == Some(account_id.as_ref()))
        .count();
    if duplicate_count > 1 {
        // We lost the race — undo our write by removing the specific entry we added.
        // Match by both id AND joinedAt so we remove our entry, not the winner's.
        let deduped: Vec<Value> = {
            let mut removed_ours = false;
            post_members
                .into_iter()
                .filter(|m| {
                    if !removed_ours
                        && m.get("id").and_then(|v| v.as_str()) == Some(account_id.as_ref())
                        && m.get("joinedAt").and_then(|v| v.as_str()) == Some(now_str.as_str())
                    {
                        removed_ours = true;
                        false
                    } else {
                        true
                    }
                })
                .collect()
        };
        let mut deduped_patch = serde_json::Map::new();
        deduped_patch.insert("members".to_owned(), json!(deduped));
        let _ = backend
            .update_object::<Space>(
                caller,
                &account_id,
                &space_id,
                PatchObject::from_map(deduped_patch),
            )
            .await;
        return Err(JmapError::server_fail(
            "concurrent join detected; please retry",
        ));
    }

    // Apply the invite uses increment only on the success path (after TOCTOU check passes).
    // Deferring here prevents a race-loss from silently consuming an invite use.
    if let Some((invite_id, new_uses)) = invite_update {
        let mut uses_patch = serde_json::Map::new();
        uses_patch.insert("uses".to_owned(), json!(new_uses));
        backend
            .update_object::<SpaceInvite>(
                caller,
                &account_id,
                &invite_id,
                PatchObject::from_map(uses_patch),
            )
            .await
            .map_err(|e: jmap_server::BackendSetError<_>| JmapError::server_fail(e.to_string()))?;
    }

    Ok((
        json!({ "accountId": account_id.as_ref(), "spaceId": space_id.as_ref() }),
        vec![],
    ))
}
