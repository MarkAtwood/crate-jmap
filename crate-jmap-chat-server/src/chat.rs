//! Chat/* method handlers (JMAP Chat extension §Chat).

use std::collections::{HashMap, HashSet};

use jmap_chat_types::{Chat, ChatKind};
use jmap_types::{Id, Invocation, JmapError, PatchObject, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, ChatBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, now_utc_string, set_error_value};

// ---------------------------------------------------------------------------
// Chat/get
// ---------------------------------------------------------------------------

/// Handle a `Chat/get` method call.
// NOTE: properties forwarded via handle_get
pub async fn handle_chat_get<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Chat, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Chat/changes
// ---------------------------------------------------------------------------

/// Handle a `Chat/changes` method call (RFC 8620 §5.2).
pub async fn handle_chat_changes<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Chat, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Chat/query
// ---------------------------------------------------------------------------

/// Handle a `Chat/query` method call (RFC 8620 §5.5).
pub async fn handle_chat_query<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<Chat, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Chat/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Chat/queryChanges` method call (RFC 8620 §5.6).
pub async fn handle_chat_query_changes<B: ChatBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<Chat, B>(backend, args).await
}

// ---------------------------------------------------------------------------
// Chat/set
// ---------------------------------------------------------------------------

/// Handle a `Chat/set` method call.
///
/// Validation enforced here (not in the backend):
/// - `kind` is required on create.
/// - `direct` chats require `contactId`.
/// - `channel` chats require `spaceId`.
/// - `id`, `createdAt`, `unreadCount`, `pinnedMessageIds` are server-set and
///   rejected in updates.
pub async fn handle_chat_set<B: ChatBackend>(
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
        .get_state::<Chat>(&account_id)
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
        // Only pay the cost of a full get_objects fetch when the batch contains
        // at least one Direct create (JMAP-63k.4).
        let has_direct_create = create_map.values().any(|v| {
            v.get("kind")
                .and_then(|k| k.as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("direct"))
        });

        // Fetch all existing chats once before the loop (O(1) fetch instead of
        // O(n) per-create fetches) and build a set of already-known Direct
        // contactIds for the pre-check.  Skipped entirely for non-Direct batches.
        let existing_chats: Vec<Chat>;
        let mut known_direct_contact_ids: HashSet<String>;
        if has_direct_create {
            let (chats, _) = backend
                .get_objects::<Chat>(&account_id, None, None)
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
            known_direct_contact_ids = chats
                .iter()
                .filter(|c| c.kind == ChatKind::Direct)
                .filter_map(|c| c.contact_id.as_ref().map(|id| id.as_ref().to_owned()))
                .collect();
            existing_chats = chats;
        } else {
            existing_chats = Vec::new();
            known_direct_contact_ids = HashSet::new();
        }

        // Maps contactId -> assigned new_id for Direct chats successfully
        // created earlier in this batch.  Used to resolve intra-batch duplicates
        // without a re-fetch (JMAP-63k.12).
        let mut batch_direct_ids: HashMap<String, Id> = HashMap::new();

        for (create_id, obj_val) in create_map {
            // kind is required.
            let kind_str = match obj_val.get("kind").and_then(|v| v.as_str()) {
                Some(s) => s.to_owned(),
                None => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["kind"] }),
                    );
                    continue;
                }
            };

            let kind: ChatKind = match serde_json::from_value(Value::String(kind_str.clone())) {
                Ok(k) => k,
                Err(_) => ChatKind::Other(kind_str),
            };

            // Validate kind-specific required fields and extract per-kind state.
            // `direct_contact_id_str` is Some(id) for Direct chats and None for
            // all other kinds — it simultaneously encodes the "is direct" flag and
            // the contact ID, avoiding a bool+Option pair whose invariant (Some iff
            // direct) would otherwise be implicit.
            let direct_contact_id_str: Option<String> = match &kind {
                ChatKind::Direct => {
                    let contact_id_str = match obj_val.get("contactId").and_then(|v| v.as_str()) {
                        Some(s) => s.to_owned(),
                        None => {
                            not_created.insert(
                                create_id.clone(),
                                json!({ "type": "invalidProperties", "properties": ["contactId"] }),
                            );
                            continue;
                        }
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
            let now: UTCDate = UTCDate::from(now_str.as_str());

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
                .create_object::<Chat>(&account_id, create_id, chat)
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
                            .get_objects::<Chat>(&account_id, None, None)
                            .await
                            .map_err(|e| JmapError::server_fail(e.to_string()))?;
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
                                if let Err(e) =
                                    backend.destroy_object::<Chat>(&account_id, &new_id).await
                                {
                                    // Cleanup failed — the duplicate is still
                                    // live. Return a retryable server error
                                    // rather than alreadyExists with a
                                    // potentially inconsistent state.
                                    not_created.insert(
                                        create_id.clone(),
                                        json!({
                                            "type": "serverFail",
                                            "description": format!(
                                                "failed to clean up duplicate Direct chat; retry ({})",
                                                e
                                            )
                                        }),
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
                                    .unwrap_or_else(|e| {
                                        json!({ "type": "serverFail", "description": e.to_string() })
                                    }),
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
            match backend.update_object::<Chat>(&account_id, &id, patch).await {
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

            match backend.destroy_object::<Chat>(&account_id, &id).await {
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
            .get_state::<Chat>(&account_id)
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
// Chat/typing
// ---------------------------------------------------------------------------

/// Handle a `Chat/typing` method call.
///
/// This method is ephemeral — it signals the user is typing in a chat.
/// No state is persisted. In a full implementation, the server would
/// fan out a typing event to chat participants; this handler validates
/// and returns. The sender identity is always derived server-side from
/// `accountId` — clients MUST NOT supply a `senderId` field.
pub async fn handle_chat_typing<B: ChatBackend>(
    _backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let _chat_id: String = match args.get("chatId").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_owned(),
        _ => return Err(JmapError::invalid_arguments("chatId is required")),
    };

    let _typing: bool = match args.get("typing") {
        Some(Value::Bool(b)) => *b,
        None => return Err(JmapError::invalid_arguments("typing is required")),
        Some(_) => return Err(JmapError::invalid_arguments("typing must be a boolean")),
    };

    // NOTE: In a production implementation, validate that the account is a
    // participant of chatId and fan out a typing event to subscribers.
    // The sender identity is always derived from accountId (server-side),
    // never from a client-supplied field. See RISK-014.

    Ok((
        json!({
            "accountId": account_id.as_ref(),
        }),
        vec![],
    ))
}
