//! SpaceInvite/* method handlers (JMAP Chat extension §SpaceInvite).
//!
//! Methods: get, changes, set only (no query, no queryChanges per spec).
//! Updates are forbidden — the spec treats SpaceInvite as write-once.

use jmap_chat_types::SpaceInvite;
use jmap_types::{Id, Invocation, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{
    extract_account_id, finalize_set_response, not_found_json, now_utc_string, serialize_value,
    set_error_value, SetAccumulators,
};
use jmap_server::server_fail_from_backend;

// ---------------------------------------------------------------------------
// SpaceInvite/get
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/get` method call.
pub async fn handle_invite_get<B: ChatBackend>(
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
        .get_objects::<SpaceInvite>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<SpaceInvite>(caller, &account_id)
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
// SpaceInvite/changes
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/changes` method call (RFC 8620 §5.2).
pub async fn handle_invite_changes<B: ChatBackend>(
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
        .get_changes::<SpaceInvite>(caller, &account_id, &since_state, max_changes)
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
// SpaceInvite/set
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/set` method call.
///
/// Per spec, SpaceInvite objects are write-once:
/// - create: accepted (`spaceId` required; server sets `code`, `createdBy`,
///   `uses`, `createdAt`).
/// - update: always rejected with `forbidden`.
/// - destroy: allowed.
pub async fn handle_invite_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // Resolve the caller's identity via the foundation seam so newly
    // created invites carry `createdBy = ChatContact.id` as required by
    // draft-atwood-jmap-chat-00 §SpaceInvite.createdBy ("ChatContact.id
    // of the member who created this invite"). Falls back to
    // `account_id` in single-user / no-identity-wired posture per
    // workspace AGENTS.md "Caller identity (foundation seam)".
    let caller_identity: Id = B::principal_id(caller)
        .cloned()
        .unwrap_or_else(|| account_id.clone());

    let old_state = backend
        .get_state::<SpaceInvite>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            // spaceId is required on create.
            let space_id = match obj_val.get("spaceId").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => Id::from(s),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["spaceId"] }),
                    );
                    continue;
                }
            };

            let default_channel_id: Option<Id> = obj_val
                .get("defaultChannelId")
                .and_then(|v| v.as_str())
                .map(Id::from);

            // expiresAt is a UTCDate per RFC 8620 §1.4. Validate the wire
            // shape via UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than storing an unparseable string.
            let expires_at: Option<UTCDate> =
                match obj_val.get("expiresAt").and_then(|v| v.as_str()) {
                    Some(s) => match UTCDate::new_validated(s) {
                        Ok(d) => Some(d),
                        Err(_) => {
                            not_created.insert(
                                create_id.clone(),
                                json!({
                                    "type": "invalidProperties",
                                    "properties": ["expiresAt"],
                                }),
                            );
                            continue;
                        }
                    },
                    None => None,
                };

            let max_uses: Option<u64> = obj_val.get("maxUses").and_then(|v| v.as_u64());
            if max_uses == Some(0) {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["maxUses"] }),
                );
                continue;
            }

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_ref());

            // Delegate code generation to the backend so production
            // implementations can use a CSPRNG.  The default implementation
            // is nanosecond-seeded and NOT cryptographically secure — see
            // ChatBackend::generate_invite_code.
            let code = backend.generate_invite_code();

            // Security: createdBy MUST be set server-side from the caller's
            // resolved identity (foundation seam), never accepted from the
            // client body. See draft-atwood-jmap-chat-00 §SpaceInvite.createdBy.
            let invite = SpaceInvite::new(
                Id::from("placeholder"),
                code,
                space_id,
                caller_identity.clone(),
                0,
                now,
                default_channel_id,
                expires_at,
                max_uses,
            );

            match backend
                .create_object::<SpaceInvite>(caller, &account_id, create_id, invite)
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
    // update — forbidden: SpaceInvite objects are write-once per spec
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
                .destroy_object::<SpaceInvite>(caller, &account_id, &id)
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

    finalize_set_response::<B, SpaceInvite>(
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
