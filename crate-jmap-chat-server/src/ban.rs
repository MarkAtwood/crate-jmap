//! SpaceBan/* method handlers (JMAP Chat extension §SpaceBan).
//!
//! SpaceBan supports get, changes, and set only (no query, no queryChanges).
//! `bannedBy` is always set server-side from the `accountId`; it is never
//! accepted from client request bodies.

use jmap_chat_types::SpaceBan;
use jmap_types::{Id, Invocation, JmapError, PatchObject, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend};
use crate::helpers::{
    extract_account_id, finalize_set_response, not_found_json, now_utc_string, serialize_value,
    set_error_value, SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// SpaceBan/get
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/get` method call.
pub async fn handle_ban_get<B: ChatBackend>(
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
        .get_objects::<SpaceBan>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<SpaceBan>(caller, &account_id)
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
// SpaceBan/changes
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/changes` method call (RFC 8620 §5.2).
pub async fn handle_ban_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<SpaceBan, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// SpaceBan/set
// ---------------------------------------------------------------------------

/// Handle a `SpaceBan/set` method call.
///
/// Validation enforced here (not in the backend):
/// - `spaceId` and `userId` are required on create.
/// - `bannedBy` is set server-side from `accountId`; never accepted from client.
/// - `id`, `spaceId`, `userId`, `bannedBy`, `createdAt` are server-set/immutable
///   and rejected in updates.
pub async fn handle_ban_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // Resolve the caller's identity via the foundation seam so newly
    // created bans carry `bannedBy = ChatContact.id` as required by
    // draft-atwood-jmap-chat-00 §SpaceBan.bannedBy ("ChatContact.id of
    // the Space member who issued this ban"). Falls back to `account_id`
    // in single-user / no-identity-wired posture per workspace
    // AGENTS.md "Caller identity (foundation seam)".
    let caller_identity: Id = B::principal_id(caller)
        .cloned()
        .unwrap_or_else(|| account_id.clone());

    let old_state = backend
        .get_state::<SpaceBan>(caller, &account_id)
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
            let space_id = match obj_val.get("spaceId").and_then(|v| v.as_str()) {
                Some(s) => Id::from(s),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["spaceId"] }),
                    );
                    continue;
                }
            };

            let user_id = match obj_val.get("userId").and_then(|v| v.as_str()) {
                Some(s) => Id::from(s),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["userId"] }),
                    );
                    continue;
                }
            };

            // bannedBy is always the acting caller's resolved identity
            // (foundation seam) — never from the client body. See
            // draft-atwood-jmap-chat-00 §SpaceBan.bannedBy.
            let banned_by = caller_identity.clone();

            let now_str = now_utc_string();
            let created_at = UTCDate::from(now_str.as_ref());

            let mut ban = SpaceBan::new(
                Id::from("placeholder"),
                space_id,
                user_id,
                banned_by,
                created_at,
            );

            if let Some(reason) = obj_val.get("reason").and_then(|v| v.as_str()) {
                if reason.len() > 1000 {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["reason"] }),
                    );
                    continue;
                }
                ban.reason = Some(reason.to_owned());
            }
            // expiresAt is a UTCDate per RFC 8620 §1.4. Validate the wire
            // shape via UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than storing an unparseable string.
            if let Some(expires) = obj_val.get("expiresAt").and_then(|v| v.as_str()) {
                match UTCDate::new_validated(expires) {
                    Ok(d) => ban.expires_at = Some(d),
                    Err(_) => {
                        not_created.insert(
                            create_id.clone(),
                            json!({ "type": "invalidProperties", "properties": ["expiresAt"] }),
                        );
                        continue;
                    }
                }
            }

            match backend
                .create_object::<SpaceBan>(caller, &account_id, create_id, ban)
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
                    not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
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

            // These fields are server-set or immutable after creation.
            const BAN_READONLY: &[&str] = &["id", "spaceId", "userId", "bannedBy", "createdAt"];
            let bad_props: Vec<&str> = BAN_READONLY
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

            const BAN_UPDATE_ALLOWED: &[&str] = &["reason", "expiresAt"];
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };
            let mut clean_patch = serde_json::Map::new();
            for &field in BAN_UPDATE_ALLOWED {
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

            match backend
                .update_object::<SpaceBan>(
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
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
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
                .destroy_object::<SpaceBan>(caller, &account_id, &id)
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
                    not_destroyed.insert(id_str.to_owned(), server_fail_value_from_backend(&e));
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

    finalize_set_response::<B, SpaceBan>(
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
