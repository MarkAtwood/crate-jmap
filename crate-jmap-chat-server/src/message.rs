//! Message/* method handlers (draft-atwood-jmap-chat-00 §Message).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §Message. The
//! returned `Value` is the corresponding method-response object per the
//! same section refs.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set`
//! (including the `slow_mode_check` `rateLimited` and reaction-patch
//! `invalidPatch` paths) surface in the `notCreated` / `notUpdated` /
//! `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_chat_types::{DeliveryState, Message, SenderId};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use std::collections::HashSet;

use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, filter_properties, finalize_set_response,
    iso8601_before, not_found_json, now_utc_string, serialize_value, set_error_value,
    SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// Message/get
// ---------------------------------------------------------------------------

/// Handle a `Message/get` method call (draft-atwood-jmap-chat-00 §Message).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    // RFC 8620 §5.1: when `properties` is specified, return only those fields
    // (plus `id` which is always included). `None` means return all fields.
    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (mut list, not_found) = backend
        .get_objects::<Message>(caller, &account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Edit-history retention gate (draft-atwood-jmap-chat-00 commit
    // `0783fc4` + §Message editHistory). When the backend does not
    // retain edit history, the `editHistory` field MUST be omitted
    // from every returned Message. Setting `edit_history = None`
    // here causes serde's `skip_serializing_if = "Option::is_none"`
    // to collapse it on the wire, satisfying the spec MUST.
    //
    // `Message/changes` per RFC 8620 §5.2 returns id arrays only
    // (no Message objects), so the spec's "omit from /changes" has
    // no wire-level effect — only `Message/get` needs this gate.
    if !backend.retains_edit_history() {
        for msg in list.iter_mut() {
            msg.edit_history = None;
        }
    }

    let state = backend
        .get_state::<Message>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = if let Some(ref props) = properties {
        let mut prop_set: HashSet<&str> = props.iter().map(|s| s.as_str()).collect();
        prop_set.insert("id");
        list.iter()
            .map(|obj| {
                let val = serialize_value(obj)?;
                Ok(filter_properties(&val, &prop_set))
            })
            .collect::<Result<Vec<_>, JmapError>>()?
    } else {
        list.iter()
            .map(serialize_value)
            .collect::<Result<Vec<_>, _>>()?
    };

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
// Message/changes
// ---------------------------------------------------------------------------

/// Handle a `Message/changes` method call (draft-atwood-jmap-chat-00 §Message).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Message, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Message/query
// ---------------------------------------------------------------------------

/// Handle a `Message/query` method call (draft-atwood-jmap-chat-00 §Message).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Filter and sort are passed through to the backend unchanged.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.remove("limit") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )))
            }
        },
    };

    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let filter: Option<serde_json::Value> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v),
    };

    // JMAP Chat spec §Message — chatId filter is required unless hasMention: true.
    let has_chat_id = filter
        .as_ref()
        .and_then(|f| f.get("chatId"))
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let has_mention_true = filter
        .as_ref()
        .and_then(|f| f.get("hasMention"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !has_chat_id && !has_mention_true {
        return Err(JmapError::unsupported_filter());
    }

    let sort: Option<Vec<serde_json::Value>> = match args.remove("sort") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("sort must be an array"))?,
        ),
    };

    let result = backend
        .query_objects::<Message>(
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
// Message/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Message/queryChanges` method call (draft-atwood-jmap-chat-00 §Message).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_query_changes<B: ChatBackend>(
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
        // Id::from: wire-boundary validation deferred to JMAP-k9va; backend rejects unknown IDs.
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
        .query_changes::<Message>(
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
// Message/set
// ---------------------------------------------------------------------------

/// Handle a `Message/set` method call (draft-atwood-jmap-chat-00 §Message).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Validation enforced here (not in the backend):
/// - `chatId` and `body` are required on create.
/// - `id`, `senderMsgId`, `senderId`, `sentAt`, `receivedAt`, `deliveryState`
///   are server-set and rejected in updates.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<Message>(caller, &account_id)
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
            let Some(chat_id_str) = obj_val.get("chatId").and_then(|v| v.as_str()) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["chatId"] }),
                );
                continue;
            };
            let chat_id = Id::from(chat_id_str);

            let Some(body) = obj_val
                .get("body")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
            else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["body"] }),
                );
                continue;
            };
            if body.len() > 100_000 {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["body"] }),
                );
                continue;
            }

            // sentAt is a UTCDate per RFC 8620 §1.4 (20-char
            // YYYY-MM-DDTHH:MM:SSZ). Validate the wire shape via
            // UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than silently flowing through
            // to storage where downstream lex-compares and slice ops
            // (helpers::iso8601_before) assume a validated 20-byte
            // ASCII shape.
            let Some(sent_at_str) = obj_val.get("sentAt").and_then(|v| v.as_str()) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["sentAt"] }),
                );
                continue;
            };
            let Ok(sent_at) = UTCDate::new_validated(sent_at_str) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["sentAt"] }),
                );
                continue;
            };

            let body_type = obj_val
                .get("bodyType")
                .and_then(|v| v.as_str())
                .unwrap_or("text/plain")
                .to_owned();

            let reply_to: Option<Id> = obj_val
                .get("replyTo")
                .and_then(|v| v.as_str())
                .map(Id::from);

            let thread_root_id: Option<Id> = obj_val
                .get("threadRootId")
                .and_then(|v| v.as_str())
                .map(Id::from);

            // senderExpiresAt is a UTCDate per RFC 8620 §1.4 (20-char
            // YYYY-MM-DDTHH:MM:SSZ). Validate the wire shape via
            // UTCDate::new_validated; a malformed value produces
            // invalidProperties rather than silently flowing through to
            // a downstream string compare with undefined ordering.
            let sender_expires_at: Option<UTCDate> =
                if let Some(s) = obj_val.get("senderExpiresAt").and_then(|v| v.as_str()) {
                    let Ok(d) = UTCDate::new_validated(s) else {
                        not_created.insert(
                            create_id.clone(),
                            json!({
                                "type": "invalidProperties",
                                "properties": ["senderExpiresAt"],
                            }),
                        );
                        continue;
                    };
                    Some(d)
                } else {
                    None
                };

            let burn_on_read: Option<bool> = obj_val.get("burnOnRead").and_then(|v| v.as_bool());

            if let Some(ref expires_at) = sender_expires_at {
                let now = now_utc_string();
                if !iso8601_before(&now, expires_at) {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["senderExpiresAt"] }),
                    );
                    continue;
                }
            }

            // Slow-mode rate-limit gate (draft-atwood-jmap-chat-00
            // §Chat `slowModeSeconds` + spec commit `de60acb`).
            //
            // Runs AFTER wire-format validation so malformed requests
            // don't consume rate-tracker slots, and BEFORE the
            // `create_object` call so a throttled sender never sees the
            // message touch storage. On a Throttled return the backend
            // tells us when the caller may retry; we surface that
            // verbatim as the `serverRetryAfter` extra field on the
            // `rateLimited` SetError, built via the typed
            // `SetError::with_extra` builder added in bd:JMAP-dha0.
            // `serverRetryAfter` is the workspace convention paired
            // with `rateLimited` (read by
            // `jmap_chat_client::server_retry_after` on the client
            // side); `SetErrorType::custom("rateLimited")` produces
            // the spec wire string. The reference `MemoryBackend`
            // never returns an Err here.
            if let Err(slow_err) = backend.slow_mode_check(caller, &account_id, &chat_id).await {
                let retry_after_str: String = slow_err.retry_after.as_ref().to_owned();
                let set_err = SetError::new(SetErrorType::custom("rateLimited"))
                    .with_description("Slow mode is active for this chat")
                    .with_extra("serverRetryAfter", json!(retry_after_str));
                not_created.insert(create_id.clone(), set_error_value(&set_err));
                continue;
            }

            let now_str = now_utc_string();
            let received_at: UTCDate = UTCDate::from(now_str.as_ref());

            let mut msg = Message::new(
                Id::from("placeholder"),
                Id::from(create_id.as_str()),
                SenderId::Owner,
                chat_id,
                body,
                body_type,
                sent_at,
                received_at,
                DeliveryState::Pending,
            );

            msg.reply_to = reply_to;
            msg.thread_root_id = thread_root_id;
            if let Some(d) = sender_expires_at {
                msg.sender_expires_at = Some(d);
            }
            msg.burn_on_read = burn_on_read;

            match backend
                .create_object::<Message>(caller, &account_id, create_id, msg)
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

            // Reject patches that include server-set fields.
            // MAINTENANCE: when adding a server-set field to Message in
            // jmap-chat-types, add its wire name here too.  A missing entry
            // silently allows clients to overwrite server-managed state.
            const MESSAGE_READONLY: &[&str] = &[
                "id",
                "senderMsgId",
                "senderId",
                "chatId",
                "sentAt",
                "receivedAt",
                "deliveryState",
            ];
            let bad_props: Vec<&str> = MESSAGE_READONLY
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

            // Build augmented patch with server-side timestamps.
            let mut augmented = patch_val;
            if augmented.get("body").is_some() {
                if let Some(obj) = augmented.as_object_mut() {
                    obj.insert("editedAt".to_owned(), json!(now_utc_string()));
                }
            }
            if augmented.get("deletedForAll").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(obj) = augmented.as_object_mut() {
                    obj.insert("deletedAt".to_owned(), json!(now_utc_string()));
                }
            }
            // Spec §557 + §1029: when readAt is set without readDisposition, default to "displayed".
            if let Some(obj) = augmented.as_object_mut() {
                if obj.contains_key("readAt") && !obj.contains_key("readDisposition") {
                    obj.insert("readDisposition".to_owned(), json!("displayed"));
                }
            }

            // Reaction patch processing
            // (draft-atwood-jmap-chat-00 §Message/set, §Reaction).
            //
            // Reactions are added/removed by patching
            // `reactions/{senderReactionId}` as RFC 8620 §5.3 JSON
            // Pointer entries:
            //
            // - value is an object: add or modify the reaction
            // - value is `null`: remove the reaction
            //
            // The backend's `json_merge_patch` is flat-key — it would
            // store a literal top-level field named `"reactions/X"`
            // rather than descending into the `reactions` map. So this
            // handler must rewrite all `reactions/X` entries into a
            // single nested `reactions` entry; RFC 7396 then merges
            // that into the stored reactions map per spec.
            //
            // While rewriting, the handler:
            //
            // 1. Rejects `reactions/{id}` keys containing `/` or `~`
            //    (RFC 6901 escape characters — the chat-client also
            //    rejects them; this is defense-in-depth).
            // 2. Rejects non-null non-object values as `invalidPatch`.
            // 3. Pre-fetches the Message and rejects with `forbidden`
            //    if the patched key targets an existing reaction whose
            //    `senderId` is not `"self"` — per spec, only the
            //    original sender may modify their own reactions.
            // 4. Server-overrides `senderId` to `"self"` on every add
            //    (spec MUST; defense-in-depth).
            // 5. Server-injects `sentAt` to the current time on adds
            //    that lack it, so the resulting `Reaction` round-trips
            //    through the typed shape (which requires `sentAt`).
            let reaction_pointer_keys: Vec<String> = augmented
                .as_object()
                .map(|obj| {
                    obj.keys()
                        .filter(|k| k.starts_with("reactions/"))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            if !reaction_pointer_keys.is_empty() {
                // Reject mixing top-level `reactions` with
                // `reactions/{id}` — the two have incompatible
                // semantics (wholesale replace vs per-key merge) and
                // a single patch should not attempt both.
                if augmented
                    .as_object()
                    .is_some_and(|obj| obj.contains_key("reactions"))
                {
                    not_updated.insert(
                        id_str,
                        json!({
                            "type": "invalidPatch",
                            "description":
                                "patch must not combine top-level `reactions` with `reactions/{id}` entries",
                        }),
                    );
                    continue;
                }

                // Validate each pointer key + value shape. The
                // `.collect::<Result<Vec<_>, _>>()` form short-circuits
                // on the first Err and is the idiomatic Rust expression
                // of "validate-or-fail" over an iterator. A future
                // contributor adding a new validation rule appends an
                // `if ... { return Err(...) }` inside the closure; the
                // short-circuit behavior is structural, not a hand-coded
                // flag invariant.
                let decoded: Result<Vec<(String, String)>, String> = reaction_pointer_keys
                    .iter()
                    .map(|raw_key| {
                        let suffix = &raw_key["reactions/".len()..];
                        if suffix.is_empty() || suffix.contains('/') || suffix.contains('~') {
                            return Err(format!(
                                "reactions/{{id}} pointer key {raw_key:?} has empty or forbidden suffix; '/' and '~' are reserved by RFC 6901",
                            ));
                        }
                        let val = augmented.get(raw_key).unwrap_or(&Value::Null);
                        if !val.is_null() {
                            let ok = val.as_object().is_some_and(|m| {
                                m.get("emoji")
                                    .and_then(|v| v.as_str())
                                    .is_some_and(|s| !s.is_empty())
                            });
                            if !ok {
                                return Err(format!(
                                    "reaction value for {raw_key:?} must be null or an object with a non-empty `emoji` string",
                                ));
                            }
                        }
                        Ok((raw_key.clone(), suffix.to_owned()))
                    })
                    .collect();
                let decoded = match decoded {
                    Ok(d) => d,
                    Err(desc) => {
                        not_updated.insert(
                            id_str,
                            json!({ "type": "invalidPatch", "description": desc }),
                        );
                        continue;
                    }
                };

                // Pre-fetch the Message to inspect existing
                // reactions. An update target that doesn't exist on
                // the backend will surface a normal `notFound` from
                // the subsequent `update_object` call — we don't
                // short-circuit here. A backend storage error becomes
                // `serverFail`.
                let forbidden = match backend
                    .get_objects::<Message>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(&id)),
                        None,
                    )
                    .await
                {
                    Ok((found, _not_found)) => {
                        let mut bad: Vec<String> = Vec::new();
                        if let Some(msg) = found.first() {
                            for (_, suffix) in &decoded {
                                if let Some(existing) = msg.reactions.get(suffix) {
                                    if existing.sender_id != SenderId::Owner {
                                        bad.push(suffix.clone());
                                    }
                                }
                            }
                        }
                        bad
                    }
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                };
                if !forbidden.is_empty() {
                    not_updated.insert(
                        id_str,
                        json!({
                            "type": "forbidden",
                            "description": format!(
                                "cannot modify reactions authored by another sender: {}",
                                forbidden.join(", "),
                            ),
                        }),
                    );
                    continue;
                }

                // Coalesce every `reactions/{key}` entry into a single
                // nested `reactions` patch entry, server-overriding
                // `senderId` and injecting `sentAt` where absent so
                // the stored Reaction round-trips through the typed
                // shape.
                let now_str = now_utc_string();
                if let Some(obj) = augmented.as_object_mut() {
                    let mut sub = serde_json::Map::new();
                    for (raw_key, suffix) in &decoded {
                        let val = obj.remove(raw_key).unwrap_or(Value::Null);
                        let final_val = match val {
                            Value::Null => Value::Null,
                            Value::Object(mut map) => {
                                // Spec MUST: server overrides senderId
                                // to "self" on every add, regardless
                                // of what the client supplied.
                                map.insert("senderId".to_owned(), json!("self"));
                                if !map.contains_key("sentAt") {
                                    map.insert("sentAt".to_owned(), json!(now_str));
                                }
                                Value::Object(map)
                            }
                            // Validation above rejects non-null
                            // non-object; this arm is unreachable.
                            other => other,
                        };
                        sub.insert(suffix.clone(), final_val);
                    }
                    obj.insert("reactions".to_owned(), Value::Object(sub));
                }
            }

            // Detect a non-null readAt assignment in the patch. A
            // `readAt: null` is a PatchObject clear (RFC 8620 §5.3), not a
            // "mark as read" event, so it does not trigger burn-on-read.
            // draft-atwood-jmap-chat-00 §Message `burnOnRead`: the
            // receiving server MUST hard-delete immediately after setting
            // readAt on a message whose `burnOnRead` is `true`.
            let patch_sets_read_at = augmented.get("readAt").is_some_and(|v| !v.is_null());

            // If the patch sets readAt, pre-fetch the message so we can
            // decide whether it is burn-on-read. We deliberately consult
            // the pre-patch state: a recipient setting readAt for the
            // first time should fire the burn, regardless of whether the
            // same patch attempts to also clear `burnOnRead` (which is a
            // sender-set field and the spec does not authorize a
            // recipient to flip).
            let pre_patch_burn_on_read = if patch_sets_read_at {
                match backend
                    .get_objects::<Message>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(&id)),
                        None,
                    )
                    .await
                {
                    Ok((found, _not_found)) => {
                        found.first().and_then(|m| m.burn_on_read) == Some(true)
                    }
                    Err(_) => false,
                }
            } else {
                false
            };

            // Convert the augmented wire-format Value into a typed
            // PatchObject (RFC 8620 §5.3). Non-object values yield
            // invalidPatch.
            let patch = match serde_json::from_value::<PatchObject>(augmented) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };
            let update_outcome = backend
                .update_object::<Message>(caller, &account_id, &id, patch)
                .await;
            let maybe_updated_obj = match update_outcome {
                Ok(maybe_obj) => maybe_obj,
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(id_str, set_error_value(&set_err));
                    continue;
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
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
            };

            // Burn-on-read fires AFTER the readAt patch has been applied.
            // draft-atwood-jmap-chat-00 §Message `burnOnRead`: hard-delete
            // immediately. The patch already succeeded above; the hard
            // delete is its own backend call so that a production backend
            // can either (a) leave the default no-op and override
            // `update_object` for atomic write+delete, or (b) override
            // this method for a separate two-step deletion. The reference
            // `MemoryBackend` uses (b). Both are conforming.
            if patch_sets_read_at && pre_patch_burn_on_read {
                if let Err(burn_err) = backend.expire_message(caller, &account_id, &id).await {
                    // The readAt patch already landed in storage but
                    // the spec-mandated hard-delete failed. Surface
                    // as serverFail; the recipient will retry, and
                    // the next pre-fetch will still see
                    // `burnOnRead: true` (a sender-set field a
                    // recipient cannot flip), so the burn will be
                    // attempted again. Production backends that
                    // need atomic readAt-and-burn semantics SHOULD
                    // override `update_object` to perform both
                    // inside a single transaction; the reference
                    // `MemoryBackend::expire_message` does not
                    // surface failures and so this branch is
                    // unreachable for it.
                    //
                    // Route through server_fail_value_from_backend to
                    // redact backend Display text from the wire
                    // description per workspace redaction discipline
                    // (otherwise the backend error message leaks to
                    // the JMAP client).
                    not_updated.insert(id_str, server_fail_value_from_backend(&burn_err));
                    // Bump mutated regardless, because the readAt
                    // write is committed even though we report
                    // notUpdated for the wire response. Callers
                    // relying on `mutated` to decide whether to
                    // rotate the state token will rotate, which
                    // correctly reflects that something changed in
                    // storage.
                    mutated = true;
                    continue;
                }
            }

            // Update succeeded (and burn, if applicable, succeeded too).
            mutated = true;
            match maybe_updated_obj {
                Some(obj) => {
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                None => {
                    updated.insert(id_str, Value::Null);
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
            let Some(id_str) = id_val.as_str() else {
                continue; // unreachable: validated above
            };
            let id = Id::from(id_str);

            match backend
                .destroy_object::<Message>(caller, &account_id, &id)
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

    finalize_set_response::<B, Message>(
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
