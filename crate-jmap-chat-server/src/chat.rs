//! Chat/* method handlers (draft-atwood-jmap-chat-00 §Chat).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §Chat. The returned
//! `Value` is the corresponding method-response object per the same
//! section refs. `Chat/typing` (draft §Chat) is a Chat-specific signal
//! method with its own request/response shape.
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set` surface
//! in the `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use std::collections::{HashMap, HashSet};

use jmap_chat_types::{Chat, ChatKind};
use jmap_types::{Id, Invocation, JmapError, PatchObject, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, now_utc_string,
    set_error_value, SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// Chat/get
// ---------------------------------------------------------------------------

/// Handle a `Chat/get` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
// NOTE: properties forwarded via handle_get
pub async fn handle_chat_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Chat, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Chat/changes
// ---------------------------------------------------------------------------

/// Handle a `Chat/changes` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Chat, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Chat/query
// ---------------------------------------------------------------------------

/// Handle a `Chat/query` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`); the
/// returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<Chat, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Chat/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Chat/queryChanges` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<Chat, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Chat/set
// ---------------------------------------------------------------------------

/// Handle a `Chat/set` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// Validation enforced here (not in the backend):
/// - `kind` is required on create.
/// - `direct` chats require `contactId`.
/// - `channel` chats require `spaceId`.
/// - `id`, `createdAt`, `unreadCount`, `pinnedMessageIds` are server-set and
///   rejected in updates.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<Chat>(caller, &account_id)
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
        // Only pay the cost of a full get_objects fetch when the batch contains
        // at least one Direct create (JMAP-63k.4).
        let has_direct_create = create_map.values().any(|v| {
            v.get("kind")
                .and_then(|k| k.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("direct"))
        });

        // Fetch all existing chats once before the loop. For a batch of N
        // creates against an account with K existing chats this is O(K + N)
        // reads (one full-account scan + a HashSet lookup per create) rather
        // than the naive O(N * K) of one fetch per create. The per-batch fetch
        // still scales linearly in the account's chat count K, so production
        // backends serving accounts with large K should push the
        // contact-id-uniqueness check into a typed query method on
        // ChatBackend rather than rely on this hoisted scan (tracked by
        // JMAP-63k.9). Skipped entirely for non-Direct batches.
        let (existing_chats, mut known_direct_contact_ids): (Vec<Chat>, HashSet<String>) =
            if has_direct_create {
                let (chats, _) = backend
                    .get_objects::<Chat>(caller, &account_id, None, None)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;
                let known = chats
                    .iter()
                    .filter(|c| c.kind == ChatKind::Direct)
                    .filter_map(|c| c.contact_id.as_ref().map(|id| id.as_ref().to_owned()))
                    .collect();
                (chats, known)
            } else {
                (Vec::new(), HashSet::new())
            };

        // Maps contactId -> assigned new_id for Direct chats successfully
        // created earlier in this batch.  Used to resolve intra-batch duplicates
        // without a re-fetch (JMAP-63k.12).
        let mut batch_direct_ids: HashMap<String, Id> = HashMap::new();

        for (create_id, obj_val) in create_map {
            // kind is required.
            let Some(kind_str) = obj_val
                .get("kind")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
            else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["kind"] }),
                );
                continue;
            };

            // ChatKind::Other(_) is the deserialize-from-wire forward-compat
            // catch-all: it preserves round-trip fidelity when reading data
            // produced by a future server that knows a new spec variant. It
            // is NOT a legitimate value on /set create — a client supplying
            // an unrecognised kind must be rejected with invalidProperties,
            // otherwise junk Chats end up in storage and break every
            // downstream kind-dispatched invariant (kind-specific required
            // field checks, /get response shape, Message/set chatId
            // resolution, etc.).
            let kind = match serde_json::from_value::<ChatKind>(Value::String(kind_str)) {
                Ok(k) if !matches!(k, ChatKind::Other(_)) => k,
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["kind"] }),
                    );
                    continue;
                }
            };

            // Validate kind-specific required fields and extract per-kind state.
            // `direct_contact_id_str` is Some(id) for Direct chats and None for
            // all other kinds — it simultaneously encodes the "is direct" flag and
            // the contact ID, avoiding a bool+Option pair whose invariant (Some iff
            // direct) would otherwise be implicit.
            let direct_contact_id_str: Option<String> = match &kind {
                ChatKind::Direct => {
                    let Some(contact_id_str) = obj_val
                        .get("contactId")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                    else {
                        not_created.insert(
                            create_id.clone(),
                            json!({ "type": "invalidProperties", "properties": ["contactId"] }),
                        );
                        continue;
                    };

                    // Pre-check: reject if a direct chat with this contactId is
                    // already known from the hoisted fetch.
                    if let Some(dup) = existing_chats.iter().find(|c| {
                        c.kind == ChatKind::Direct
                            && c.contact_id.as_ref().map(|id| id.as_ref())
                                == Some(contact_id_str.as_str())
                    }) {
                        not_created.insert(
                            create_id.clone(),
                            serde_json::to_value(
                                SetError::new(SetErrorType::AlreadyExists)
                                    .with_existing_id(dup.id.clone()),
                            )
                            .expect("derive(Serialize) on plain data is infallible"),
                        );
                        continue;
                    }
                    // Also check contactIds created earlier in this same batch.
                    // Resolve the canonical id from the hoisted pre-fetch data or
                    // from the batch map — no re-fetch required (JMAP-63k.12).
                    if known_direct_contact_ids.contains(&contact_id_str) {
                        // Try the pre-fetch snapshot first (pre-existing chat).
                        let canonical_id = if let Some(c) = existing_chats.iter().find(|c| {
                            c.kind == ChatKind::Direct
                                && c.contact_id.as_ref().map(|id| id.as_ref())
                                    == Some(contact_id_str.as_str())
                        }) {
                            c.id.clone()
                        } else if let Some(id) = batch_direct_ids.get(&contact_id_str) {
                            // Created earlier in this batch.
                            id.clone()
                        } else {
                            // Should not happen: known_direct_contact_ids is only
                            // populated from existing_chats and batch_direct_ids.
                            not_created.insert(
                                create_id.clone(),
                                json!({
                                    "type": "serverFail",
                                    "description": "direct chat for contact not found after concurrent operation; retry"
                                }),
                            );
                            continue;
                        };
                        not_created.insert(
                            create_id.clone(),
                            serde_json::to_value(
                                SetError::new(SetErrorType::AlreadyExists)
                                    .with_existing_id(canonical_id),
                            )
                            .expect("derive(Serialize) on plain data is infallible"),
                        );
                        continue;
                    }
                    Some(contact_id_str)
                }
                ChatKind::Channel => {
                    if obj_val.get("spaceId").and_then(|v| v.as_str()).is_none() {
                        not_created.insert(
                            create_id.clone(),
                            json!({ "type": "invalidProperties", "properties": ["spaceId"] }),
                        );
                        continue;
                    }
                    None
                }
                _ => None,
            };

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_ref());

            let contact_id: Option<Id> = obj_val
                .get("contactId")
                .and_then(|v| v.as_str())
                .map(Id::from);
            let name: Option<String> = obj_val
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let description: Option<String> = obj_val
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let space_id: Option<Id> = obj_val
                .get("spaceId")
                .and_then(|v| v.as_str())
                .map(Id::from);
            let muted: bool = obj_val
                .get("muted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let receive_typing_indicators: bool = obj_val
                .get("receiveTypingIndicators")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let mut chat = Chat::new(
                // Id::from: wire-boundary validation deferred to JMAP-k9va; backend rejects unknown IDs.
                Id::from("placeholder"),
                kind,
                now,
                0,
                vec![],
                muted,
                receive_typing_indicators,
            );
            chat.contact_id = contact_id;
            chat.name = name;
            chat.description = description;
            chat.space_id = space_id;

            match backend
                .create_object::<Chat>(caller, &account_id, create_id, chat)
                .await
            {
                Ok((new_id, created_obj)) => {
                    // For Direct chats: re-fetch to detect a concurrent duplicate
                    // (optimistic create-then-validate, required for JMAP-0c9).
                    // We fetch all chats because the backend does not currently
                    // expose a filter-by-kind query; a tighter fetch (Direct only)
                    // would be preferable but requires backend support (JMAP-63k.9).
                    if let Some(contact_id_str) = direct_contact_id_str.as_deref() {
                        let (current_chats, _) = backend
                            .get_objects::<Chat>(caller, &account_id, None, None)
                            .await
                            .map_err(|e| server_fail_from_backend(&e))?;
                        let duplicates: Vec<&Chat> = current_chats
                            .iter()
                            .filter(|c| {
                                c.kind == ChatKind::Direct
                                    && c.contact_id.as_ref().map(|id| id.as_ref())
                                        == Some(contact_id_str)
                            })
                            .collect();
                        if duplicates.len() > 1 {
                            // Race occurred: pick lexicographically smallest id
                            // as the canonical winner.
                            let canonical_id: Id = duplicates
                                .iter()
                                .map(|c| c.id.as_ref())
                                .min()
                                .map(Id::from)
                                .unwrap_or_else(|| new_id.clone());
                            if new_id != canonical_id {
                                // We lost the race: destroy our copy.
                                if let Err(e) = backend
                                    .destroy_object::<Chat>(caller, &account_id, &new_id)
                                    .await
                                {
                                    // Cleanup failed — the duplicate is still
                                    // live. Return a retryable server error
                                    // rather than alreadyExists with a
                                    // potentially inconsistent state.
                                    //
                                    // Route through server_fail_value_from_backend
                                    // to redact backend Display text from the
                                    // wire description per workspace redaction
                                    // discipline.
                                    not_created.insert(
                                        create_id.clone(),
                                        server_fail_value_from_backend(&e),
                                    );
                                    continue;
                                }
                                // Cleanup succeeded: report alreadyExists
                                // pointing to the canonical winner.
                                not_created.insert(
                                    create_id.clone(),
                                    serde_json::to_value(
                                        SetError::new(SetErrorType::AlreadyExists)
                                            .with_existing_id(canonical_id),
                                    )
                                    .unwrap_or_else(|e| server_fail_value_from_backend(&e)),
                                );
                                continue;
                            }
                            // We won the race (our id is canonical): fall through
                            // to success path below.
                        }
                        // Exactly one (or we won): record contactId as known so
                        // subsequent creates in this batch are pre-checked.
                        known_direct_contact_ids.insert(contact_id_str.to_owned());
                        batch_direct_ids.insert(contact_id_str.to_owned(), new_id.clone());
                    }
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
            // Server-set fields that clients may not patch via Chat/set.
            // INVARIANT: this list must include every field on jmap_chat_types::Chat that
            // is set by the server rather than the client. Add new server-set fields here
            // at the same time as adding them to the Chat struct.
            const CHAT_READONLY: &[&str] = &["id", "createdAt", "unreadCount", "pinnedMessageIds"];
            let bad_props: Vec<&str> = CHAT_READONLY
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
                .update_object::<Chat>(caller, &account_id, &id, patch)
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
            let Some(id_str) = id_val.as_str() else {
                continue; // unreachable: validated above
            };
            let id = Id::from(id_str);

            match backend
                .destroy_object::<Chat>(caller, &account_id, &id)
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

    finalize_set_response::<B, Chat>(
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
// Chat/typing
// ---------------------------------------------------------------------------

/// Handle a `Chat/typing` method call (draft-atwood-jmap-chat-00 §Chat).
///
/// `args` is the draft §Chat `Chat/typing` request shape (`accountId`,
/// `chatId`); the returned `Value` is the §Chat response shape
/// (`accountId` echo). No persistent state is changed.
///
/// This method is ephemeral — it signals the user is typing in a chat.
/// No state is persisted. In a full implementation, the server would
/// fan out a typing event to chat participants; this handler validates
/// and returns. The sender identity is always derived server-side from
/// `accountId` — clients MUST NOT supply a `senderId` field.
///
/// # Blocked-sender suppression
///
/// Per draft-atwood-jmap-chat-00 commit `d68b4e3` ("close blocked-sender
/// suppression gaps for typing/presence"): when the requesting account
/// corresponds to a [`ChatContact`] whose `blocked` is `true` on the
/// recipient's contact list, the server MUST silently suppress the
/// typing event for that recipient. The sender is NOT informed.
///
/// [`ChatContact`]: jmap_chat_types::ChatContact
///
/// The kit consults [`ChatBackend::is_contact_blocked`] on the
/// direct-chat path for observability, but does NOT itself perform
/// transport-layer fan-out — push delivery (SSE / WS) is the
/// consumer's responsibility. The consumer's typing-event publisher
/// MUST consult this predicate per recipient before fanning out the
/// event. The kit's handler returns the same success response in
/// either case (echo `accountId`); the wire shape does not depend on
/// the predicate's result.
///
/// Group / channel chats (n recipients) are skipped on the kit side:
/// the handler has no way to enumerate fan-out recipients. That work
/// belongs entirely to the consumer's transport layer.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_chat_typing<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    let chat_id: Id = match args.get("chatId").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => Id::from(s),
        _ => return Err(JmapError::invalid_arguments("chatId is required")),
    };

    let _typing: bool = match args.get("typing") {
        Some(Value::Bool(b)) => *b,
        None => return Err(JmapError::invalid_arguments("typing is required")),
        Some(_) => return Err(JmapError::invalid_arguments("typing must be a boolean")),
    };

    // Blocked-sender suppression integration point (draft-atwood-
    // jmap-chat-00 commit `d68b4e3`). For direct chats, consult the
    // predicate so a production-grade backend override can observe /
    // record the consultation. The kit does not gate fan-out on the
    // result because the kit has no fan-out path; this site exists
    // as the documented hook for consumer transport layers.
    //
    // Pre-fetch the chat; if the id is unknown (not found) or fetch
    // fails, skip the predicate — handle_chat_typing's success
    // response is the same in either case, and a non-existent target
    // should not consume a blocked-check decision. Production
    // backends that need stricter behaviour (e.g. reject typing for
    // unknown chats) should validate participation inside their
    // own logic; the kit deliberately keeps the typing path
    // permissive.
    if let Ok((found, _not_found)) = backend
        .get_objects::<Chat>(
            caller,
            &account_id,
            Some(std::slice::from_ref(&chat_id)),
            None,
        )
        .await
    {
        if let Some(chat) = found.first() {
            if let Some(contact_id) = chat.contact_id.as_ref() {
                // Direct chat — there is exactly one recipient.
                let _blocked = backend
                    .is_contact_blocked(caller, &account_id, contact_id)
                    .await
                    .unwrap_or(false);
                // Result intentionally observed but not gated on
                // here — see the doc-comment on this function.
                // A consumer transport layer reads
                // `is_contact_blocked` itself and suppresses
                // accordingly.
            }
        }
    }

    Ok((
        json!({
            "accountId": account_id.as_ref(),
        }),
        vec![],
    ))
}
