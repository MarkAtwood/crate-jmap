//! SpaceInvite/* method handlers (draft-atwood-jmap-chat-00 §SpaceInvite).
//!
//! Methods: get, changes, set only (no query, no queryChanges per spec).
//! Updates are forbidden — the spec treats SpaceInvite as write-once.
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3), with the
//! type-specific arguments defined by draft-atwood-jmap-chat-00
//! §SpaceInvite. The returned `Value` is the corresponding
//! method-response object per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `cannotCalculateChanges` — per RFC 8620 §3.6 and §5). Per-target
//! failures inside `/set` (including the write-once `forbidden` update
//! rejection) surface in the `notCreated` / `notUpdated` / `notDestroyed`
//! maps within `Ok((Value, ...))`, not as `Err`.

use std::collections::HashSet;

use jmap_chat_types::{Space, SpaceInvite};
use jmap_types::{Id, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, not_found_json, now_utc_string, serialize_value, set_error_value,
    SetAccumulators,
};
use crate::helpers::{mistyped_create_props, PropType, PLACEHOLDER_ID};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend, HandlerOutput};

// ---------------------------------------------------------------------------
// SpaceInvite/get
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/get` method call (draft-atwood-jmap-chat-00 §SpaceInvite).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Each returned invite must pass [`ChatBackend::may_get_space_invite`];
/// a hidden invite is reported exactly like a missing one.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_invite_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (fetched, mut not_found) = backend
        .get_objects::<SpaceInvite>(caller, &account_id, ids_slice, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // A hidden invite is indistinguishable from a missing one: a requested
    // id goes to notFound, and a full fetch (`ids: null`) drops it, since
    // RFC 8620 §5.1 notFound lists only requested ids.
    let mut list = Vec::with_capacity(fetched.len());
    for invite in fetched {
        if backend
            .may_get_space_invite(caller, &account_id, &invite)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            list.push(invite);
        } else if ids_slice.is_some() {
            not_found.push(invite.id);
        }
    }

    let state = backend
        .get_state::<SpaceInvite>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(serialize_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    })))
}

// ---------------------------------------------------------------------------
// SpaceInvite/changes
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/changes` method call (draft-atwood-jmap-chat-00 §SpaceInvite).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// `created` and `updated` list only invites that still exist and pass
/// [`ChatBackend::may_get_space_invite`]; `destroyed` is not filtered.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_invite_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    let since_state: State = match args.get("sinceState").and_then(|v| v.as_str()) {
        Some(s) => State::from(s),
        None => return Err(JmapError::invalid_arguments("sinceState is required")),
    };

    let max_changes = jmap_server::parse_max_changes(args.get("maxChanges"))?;

    let result = backend
        .get_changes::<SpaceInvite>(caller, &account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    // Keep only created/updated ids whose invite still resolves and passes
    // the SpaceInvite/get visibility gate.
    // ponytail: `destroyed` is passed through unfiltered because a destroyed
    // invite can no longer be evaluated, so its id leaks to callers who could
    // not see it. Closing that needs the backend's change log to record each
    // invite's visibility at destroy time.
    let changed: Vec<Id> = result
        .created
        .iter()
        .chain(&result.updated)
        .cloned()
        .collect();
    let mut visible: HashSet<Id> = HashSet::with_capacity(changed.len());
    if !changed.is_empty() {
        let (invites, _) = backend
            .get_objects::<SpaceInvite>(caller, &account_id, Some(&changed), None)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        for invite in invites {
            if backend
                .may_get_space_invite(caller, &account_id, &invite)
                .await
                .map_err(|e| server_fail_from_backend(&e))?
            {
                visible.insert(invite.id);
            }
        }
    }
    let visible_ids = |ids: &[Id]| -> Vec<Value> {
        ids.iter()
            .filter(|id| visible.contains(*id))
            .map(|id| Value::from(id.as_ref()))
            .collect()
    };

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "updatedProperties": Value::Null,
        "created":   visible_ids(&result.created),
        "updated":   visible_ids(&result.updated),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    })))
}

// ---------------------------------------------------------------------------
// SpaceInvite/set
// ---------------------------------------------------------------------------

/// Handle a `SpaceInvite/set` method call (draft-atwood-jmap-chat-00 §SpaceInvite).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Per spec, SpaceInvite objects are write-once:
/// - create: accepted (`spaceId` required; server sets `code`, `createdBy`,
///   `uses`, `createdAt`).
/// - update: always rejected with `forbidden`.
/// - destroy: revokes the invite.
///
/// Create and destroy each consult [`ChatBackend::may_set_space_invite`]
/// for the invite's Space.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_invite_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

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
            // Optional properties: a wrongly-typed value is invalidProperties
            // (RFC 8620 §5.3), not a silent absence.
            if let Some(err) = mistyped_create_props(
                obj_val,
                &[
                    ("defaultChannelId", PropType::Str),
                    ("expiresAt", PropType::Str),
                    ("maxUses", PropType::UnsignedInt),
                ],
            ) {
                not_created.insert(create_id.clone(), err);
                continue;
            }
            // spaceId is required on create.
            let space_id = match obj_val.get("spaceId").and_then(|v| v.as_str()) {
                // Id::from: wire-boundary validation deferred to JMAP-k9va; backend rejects unknown IDs.
                Some(s) if !s.is_empty() => Id::from(s),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["spaceId"] }),
                    );
                    continue;
                }
            };

            // draft-atwood-jmap-chat-00 §Space `closedAt`: "The server
            // MUST also reject ... `SpaceInvite/set` create for the Space
            // with a `forbidden` SetError".
            match backend
                .get_objects::<Space>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&space_id)),
                    None,
                )
                .await
            {
                Ok((spaces, _)) if spaces.first().is_some_and(|s| s.closed_at.is_some()) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "forbidden", "description": "the Space is closed" }),
                    );
                    continue;
                }
                Ok(_) => {}
                Err(e) => {
                    not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
                    continue;
                }
            }

            let default_channel_id: Option<Id> = obj_val
                .get("defaultChannelId")
                .and_then(|v| v.as_str())
                .map(Id::from);

            // expiresAt is a UTCDate per RFC 8620 §1.4. Validate the wire
            // shape via UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than storing an unparseable string.
            let expires_at: Option<UTCDate> =
                if let Some(s) = obj_val.get("expiresAt").and_then(|v| v.as_str()) {
                    let Ok(d) = UTCDate::new_validated(s) else {
                        not_created.insert(
                            create_id.clone(),
                            json!({
                                "type": "invalidProperties",
                                "properties": ["expiresAt"],
                            }),
                        );
                        continue;
                    };
                    Some(d)
                } else {
                    None
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

            // Delegate code generation to the backend. The trait contract
            // (`ChatBackend::generate_invite_code`) requires a CSPRNG with
            // at least 128 bits of entropy and a fixed-length ASCII-safe
            // encoding; the reference `MemoryBackend` uses `getrandom::fill`
            // on a 16-byte buffer (bd:JMAP-sc1b.78, bd:JMAP-sc1b.93). The
            // handler does NOT re-validate the output shape — a backend
            // that fails the contract leaks the invite-code unguessability
            // bound.
            let code = backend.generate_invite_code();

            // Security: createdBy MUST be set server-side, never accepted
            // from the client body. The creator is the owner, referenced by
            // the accountId (draft-atwood-jmap-chat-00 §Terminology,
            // §SpaceInvite.createdBy).
            let invite = SpaceInvite::new(
                Id::from(PLACEHOLDER_ID),
                code,
                space_id,
                account_id.clone(),
                0,
                now,
                default_channel_id,
                expires_at,
                max_uses,
            );

            if let Some(err) =
                invite_denied(backend, caller, &account_id, &invite.space_id, None).await
            {
                not_created.insert(create_id.clone(), err);
                continue;
            }

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
            let Some(id_str) = id_val.as_str() else {
                continue; // unreachable: validated above
            };
            let id = Id::from(id_str);

            if let Some(err) = stored_invite_denied(backend, caller, &account_id, &id).await {
                not_destroyed.insert(id_str.to_owned(), err);
                continue;
            }

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

/// draft-atwood-jmap-chat-00 §SpaceInvite/set: create "Requires
/// `"manage_members"` permission"; destroy "Requires `"manage_members"`
/// permission or ownership of the invite"; the backend decides. Returns
/// the SetError value to report when the op is denied.
async fn invite_denied<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    space_id: &Id,
    created_by: Option<&Id>,
) -> Option<Value> {
    match backend
        .may_set_space_invite(caller, account_id, space_id, created_by)
        .await
    {
        Ok(Ok(())) => None,
        Ok(Err(set_err)) => Some(set_error_value(&set_err)),
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}

/// [`invite_denied`] for the stored invite `id`. An invite that does not
/// resolve is not gated, so the backend surfaces `notFound`.
async fn stored_invite_denied<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    id: &Id,
) -> Option<Value> {
    match backend
        .get_objects::<SpaceInvite>(caller, account_id, Some(std::slice::from_ref(id)), None)
        .await
    {
        Ok((invites, _)) => match invites.first() {
            Some(invite) => {
                invite_denied(
                    backend,
                    caller,
                    account_id,
                    &invite.space_id,
                    Some(&invite.created_by),
                )
                .await
            }
            None => None,
        },
        Err(e) => Some(server_fail_value_from_backend(&e)),
    }
}
