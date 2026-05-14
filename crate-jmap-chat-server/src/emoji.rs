//! CustomEmoji/* method handlers (JMAP Chat extension §CustomEmoji).

use jmap_chat_types::CustomEmoji;
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, EmojiSetOp};
use crate::helpers::{
    extract_account_id, finalize_set_response, not_found_json, now_utc_string, serialize_value,
    set_error_value, SetAccumulators,
};
use jmap_server::server_fail_from_backend;

// ---------------------------------------------------------------------------
// CustomEmoji/get
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/get` method call.
pub async fn handle_emoji_get<B: ChatBackend>(
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
        .get_objects::<CustomEmoji>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<CustomEmoji>(caller, &account_id)
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
// CustomEmoji/changes
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/changes` method call (RFC 8620 §5.2).
pub async fn handle_emoji_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

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
        .get_changes::<CustomEmoji>(caller, &account_id, &since_state, max_changes)
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
// CustomEmoji/query
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/query` method call (RFC 8620 §5.5).
///
/// Filter and sort are passed through to the backend unchanged.
pub async fn handle_emoji_query<B: ChatBackend>(
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
        .query_objects::<CustomEmoji>(
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
// CustomEmoji/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/queryChanges` method call (RFC 8620 §5.6).
pub async fn handle_emoji_query_changes<B: ChatBackend>(
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
        .query_changes::<CustomEmoji>(
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
// CustomEmoji/set
// ---------------------------------------------------------------------------

/// Handle a `CustomEmoji/set` method call.
///
/// Validation enforced here (not in the backend):
/// - `name` is required on create and must match `/^[a-z0-9_-]+$/`.
/// - `blobId` is required on create.
/// - `id`, `createdBy`, `createdAt`, `spaceId` are server-set or immutable and
///   rejected in updates.
pub async fn handle_emoji_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // Resolve the caller's identity via the foundation seam so newly
    // created CustomEmoji records carry `createdBy = ChatContact.id`
    // rather than the JMAP `accountId`. The draft does not enumerate
    // CustomEmoji.createdBy explicitly the way SpaceMember.id and
    // SpaceInvite.createdBy are enumerated, but the semantic is the
    // same: identity-bearing fields on chat objects carry the
    // ChatContact.id of the actor, not the account_id. Falls back to
    // `account_id` in single-user / no-identity-wired posture per
    // workspace AGENTS.md "Caller identity (foundation seam)".
    let caller_identity: Id = B::principal_id(caller)
        .cloned()
        .unwrap_or_else(|| account_id.clone());

    let old_state = backend
        .get_state::<CustomEmoji>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

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
            // Validate name: required, non-empty, matches /^[a-z0-9_-]+$/
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
            if !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["name"] }),
                );
                continue;
            }
            if name.len() > 64 {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["name"] }),
                );
                continue;
            }

            // blobId is required
            let blob_id = match obj_val.get("blobId").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => Id::from(s),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["blobId"] }),
                    );
                    continue;
                }
            };

            // spaceId is optional
            let space_id: Option<Id> = obj_val
                .get("spaceId")
                .and_then(|v| v.as_str())
                .map(Id::from);

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_ref());

            // Authorization gate (draft-atwood-jmap-chat-00 commit
            // `9344aec`). Runs AFTER wire-format validation so
            // malformed creates don't consume an authorization
            // decision, and BEFORE `create_object` so an unauthorized
            // emoji never touches storage. The target scope is
            // whatever spaceId the create payload supplies (None for
            // server-global).
            match backend
                .may_set_custom_emoji(caller, &account_id, space_id.as_ref(), EmojiSetOp::Create)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({
                            "type": "forbidden",
                            "description":
                                "Implementation-defined emoji authorization denied this operation.",
                        }),
                    );
                    continue;
                }
                Err(e) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                    continue;
                }
            }

            let mut emoji = CustomEmoji::new(
                Id::from("placeholder"),
                name,
                blob_id,
                caller_identity.clone(),
                now,
            );
            emoji.space_id = space_id;

            match backend
                .create_object::<CustomEmoji>(caller, &account_id, create_id, emoji)
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

            // Reject server-set and immutable fields.
            // spaceId is immutable after creation per spec.
            const EMOJI_READONLY: &[&str] = &["id", "createdBy", "createdAt", "spaceId"];
            let bad_props: Vec<&str> = EMOJI_READONLY
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

            const EMOJI_UPDATE_ALLOWED: &[&str] = &["name", "blobId"];
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            let mut clean_patch = serde_json::Map::new();
            for &field in EMOJI_UPDATE_ALLOWED {
                if let Some(v) = patch_map.remove(field) {
                    clean_patch.insert(field.to_owned(), v);
                }
            }
            if clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "no updatable fields in patch" }),
                );
                continue;
            }

            // Authorization gate (draft-atwood-jmap-chat-00 commit
            // `9344aec`). Pre-fetch the existing emoji to learn its
            // `spaceId` (which `target_space_id` carries verbatim into
            // the gate). If the pre-fetch reports the id as not
            // found, skip the gate entirely — `update_object` will
            // surface `notFound` and we don't want to consume an
            // authorization decision for a non-existent target. A
            // pre-fetch storage error is surfaced as `serverFail`.
            let existing_space_id: Option<Option<Id>> = match backend
                .get_objects::<CustomEmoji>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
            {
                Ok((found, _not_found)) => found.first().map(|emoji| emoji.space_id.clone()),
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            if let Some(scope) = existing_space_id.as_ref() {
                let scope_ref: Option<&Id> = scope.as_ref();
                match backend
                    .may_set_custom_emoji(caller, &account_id, scope_ref, EmojiSetOp::Update)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "forbidden",
                                "description":
                                    "Implementation-defined emoji authorization denied this operation.",
                            }),
                        );
                        continue;
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

            match backend
                .update_object::<CustomEmoji>(
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

            // Authorization gate (draft-atwood-jmap-chat-00 commit
            // `9344aec`). Pre-fetch the existing emoji to learn its
            // `spaceId`. If pre-fetch reports the id as not found,
            // skip the gate so `destroy_object` can surface
            // `notFound` naturally. A pre-fetch storage error is
            // surfaced as `serverFail`.
            let existing_space_id: Option<Option<Id>> = match backend
                .get_objects::<CustomEmoji>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&id)),
                    None,
                )
                .await
            {
                Ok((found, _not_found)) => found.first().map(|emoji| emoji.space_id.clone()),
                Err(e) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            if let Some(scope) = existing_space_id.as_ref() {
                let scope_ref: Option<&Id> = scope.as_ref();
                match backend
                    .may_set_custom_emoji(caller, &account_id, scope_ref, EmojiSetOp::Destroy)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        not_destroyed.insert(
                            id_str.to_owned(),
                            json!({
                                "type": "forbidden",
                                "description":
                                    "Implementation-defined emoji authorization denied this operation.",
                            }),
                        );
                        continue;
                    }
                    Err(e) => {
                        not_destroyed.insert(
                            id_str.to_owned(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                        continue;
                    }
                }
            }

            match backend
                .destroy_object::<CustomEmoji>(caller, &account_id, &id)
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

    finalize_set_response::<B, CustomEmoji>(
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
