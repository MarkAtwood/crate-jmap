//! Email/get, Email/changes, Email/query, Email/queryChanges, Email/set,
//! Email/copy, Email/import, Email/parse method handlers (RFC 8621 §4–5).

use std::collections::{HashMap, HashSet};

use jmap_mail_types::{Email, Keyword};
use jmap_types::{Id, Invocation, JmapError, State, UTCDate};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, EmailProperty, MailBackend};
use crate::helpers::extract_account_id;

/// Server-enforced ceiling on the number of email IDs fetched when
/// `collapseThreads=true`. Without this, a hostile client could trigger OOM
/// by querying a large account with no filter. 65 536 IDs × ~32 bytes each
/// is ~2 MiB of ID data — acceptable. Anything beyond this is truncated;
/// the reported total reflects only the fetched slice.
const COLLAPSE_THREADS_MAX_EMAILS: u64 = 65_536;

// ---------------------------------------------------------------------------
// Email/get (RFC 8621 §5.1)
// ---------------------------------------------------------------------------

/// Handle an `Email/get` method call (RFC 8621 §5.1).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_get<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let ids: Option<Vec<Id>> = match args.remove("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("ids must be an Id array"))?,
        ),
    };

    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<Email>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let prop_set: Option<HashSet<&str>> = properties
        .as_deref()
        .map(|props| props.iter().map(|s| s.as_str()).collect());
    let list_json: Vec<Value> = list
        .iter()
        .map(|email| {
            let val = serde_json::to_value(email)
                .expect("type derives Serialize and is always serializable");
            match &prop_set {
                Some(set) => filter_properties(&val, set),
                None => val,
            }
        })
        .collect();

    let not_found_json: Option<Vec<Value>> = if not_found.is_empty() {
        None
    } else {
        Some(
            not_found
                .iter()
                .map(|id| Value::String(id.as_ref().to_string()))
                .collect(),
        )
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json,
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/changes (RFC 8620 §5.2, as applied to Email)
// ---------------------------------------------------------------------------

/// Handle an `Email/changes` method call (RFC 8621 §5.2).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_changes<B: MailBackend>(
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
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let result = backend
        .get_changes::<Email>(&account_id, &since_state, max_changes)
        .await
        .map_err(JmapError::from)?;

    // RFC 8621 §5.2: updatedProperties — null for MemoryBackend (no partial-update tracking).
    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": since_state.as_ref(),
        "newState": result.new_state.as_ref(),
        "hasMoreChanges": result.has_more_changes,
        "created":   result.created.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updated":   result.updated.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "destroyed": result.destroyed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "updatedProperties": Value::Null,
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/query (RFC 8621 §4.4)
// ---------------------------------------------------------------------------

/// Handle an `Email/query` method call (RFC 8621 §4.4).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_query<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let filter: Option<jmap_mail_types::EmailFilter> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filter: {e}")))?,
        ),
    };

    let sort: Option<Vec<jmap_mail_types::EmailComparator>> = match args.remove("sort") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("sort: {e}")))?,
        ),
    };

    // limit is always a concrete u64 after parsing (default 256 when absent).
    // We never pass None to the backend from this handler — None would mean
    // "no limit", but we always impose at least 256.
    let limit: u64 = match args.remove("limit") {
        None | Some(Value::Null) => 256,
        Some(v) => match v.as_u64() {
            Some(n) => n,
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )));
            }
        },
    };

    let position: i64 = args
        .remove("position")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let collapse_threads: bool = args
        .remove("collapseThreads")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let sort_slice = sort.as_deref();

    // When collapseThreads is set, fetch all matching emails (no limit) to compute the
    // correct total (= unique thread count across the full result set), then paginate
    // the collapsed list ourselves. Without collapseThreads, delegate limit/position to
    // the backend and use the backend's authoritative total.
    let (ids, total, query_state, can_calculate_changes, reported_position) = if collapse_threads {
        let all = backend
            .query_objects::<Email>(
                &account_id,
                filter.as_ref(),
                sort_slice,
                Some(COLLAPSE_THREADS_MAX_EMAILS),
                0,
            )
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
        let all_collapsed = collapse_by_thread(backend, &account_id, all.ids)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
        let thread_total = all_collapsed.len() as u64;
        // RFC 8620 §5.5: negative position is relative to the end of the result set.
        let start = if position >= 0 {
            (position as usize).min(all_collapsed.len())
        } else {
            let neg = (-position) as usize;
            all_collapsed.len().saturating_sub(neg)
        };
        let page: Vec<Id> = all_collapsed
            .into_iter()
            .skip(start)
            .take(limit as usize)
            .collect();
        (
            page,
            Some(thread_total),
            all.query_state,
            all.can_calculate_changes,
            start as i64,
        )
    } else {
        let result = backend
            .query_objects::<Email>(&account_id, filter.as_ref(), sort_slice, Some(limit), position)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
        let pos = result.position;
        let total = result.total;
        (
            result.ids,
            total,
            result.query_state,
            result.can_calculate_changes,
            pos,
        )
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": can_calculate_changes,
        "position": reported_position,
        "ids": ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
        "total": total,
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/queryChanges (RFC 8620 §5.6, as applied to Email)
// ---------------------------------------------------------------------------

/// Handle an `Email/queryChanges` method call.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_query_changes<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let since_query_state: State = match args.remove("sinceQueryState") {
        Some(Value::String(s)) => State::from(s.as_str()),
        _ => return Err(JmapError::invalid_arguments("sinceQueryState is required")),
    };

    let filter: Option<jmap_mail_types::EmailFilter> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filter: {e}")))?,
        ),
    };

    let sort: Option<Vec<jmap_mail_types::EmailComparator>> = match args.remove("sort") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("sort: {e}")))?,
        ),
    };

    let max_changes: Option<u64> = match args.remove("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let up_to_id: Option<Id> = match args.remove("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Id::from(s.as_str())),
        Some(_) => None,
    };

    let sort_slice = sort.as_deref();
    let result = backend
        .query_changes::<Email>(
            &account_id,
            &since_query_state,
            filter.as_ref(),
            sort_slice,
            max_changes,
            up_to_id.as_ref(),
        )
        .await
        .map_err(JmapError::from)?;

    let added_json: Vec<Value> = result
        .added
        .iter()
        .map(|item| {
            json!({
                "id": item.id.as_ref(),
                "index": item.index,
            })
        })
        .collect();

    let removed_json: Vec<Value> = result
        .removed
        .iter()
        .map(|id| Value::String(id.as_ref().to_string()))
        .collect();

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "total": result.total,
        "removed": removed_json,
        "added": added_json,
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/set (RFC 8621 §5.5)
// ---------------------------------------------------------------------------

/// Immutable Email fields (RFC 8621 §5.5.4).
///
/// A patch key that equals or starts with `"<field>/"` for any of these names
/// is rejected with `invalidProperties`.
const IMMUTABLE_EMAIL_FIELDS: &[&str] = &[
    "id",
    "blobId",
    "threadId",
    "size",
    "receivedAt",
    "messageId",
    "inReplyTo",
    "references",
    "sender",
    "from",
    "to",
    "cc",
    "bcc",
    "replyTo",
    "subject",
    "sentAt",
    "bodyStructure",
    "bodyValues",
    "textBody",
    "htmlBody",
    "attachments",
    "hasAttachment",
    "preview",
    "headers",
];

/// Handle an `Email/set` method call (RFC 8621 §5.5).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_set<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let old_state = backend
        .get_state::<Email>(&account_id)
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
            // Validate: at least one mailboxId is required (RFC 8621 §5.5.3).
            let mailbox_ids_ok = obj_val
                .get("mailboxIds")
                .and_then(|v| v.as_object())
                .map(|m| !m.is_empty())
                .unwrap_or(false);

            if !mailbox_ids_ok {
                not_created.insert(
                    create_id.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": ["mailboxIds"],
                    }),
                );
                continue;
            }

            // Build the Email object from the creation payload.
            let email = match build_email_from_create(obj_val, &account_id, backend).await {
                Ok(e) => e,
                Err(desc) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({
                            "type": "invalidProperties",
                            "description": desc,
                        }),
                    );
                    continue;
                }
            };

            match backend
                .create_object::<Email>(&account_id, create_id, email)
                .await
            {
                Ok((server_id, created_obj)) => {
                    mutated = true;
                    // RFC 8621 §5.5: created map contains only server-set fields.
                    created.insert(
                        create_id.clone(),
                        json!({
                            "id": server_id.as_ref(),
                            "blobId": created_obj.blob_id.as_ref(),
                            "threadId": created_obj.thread_id.as_ref(),
                            "size": created_obj.size,
                        }),
                    );
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_created.insert(
                        create_id.clone(),
                        serde_json::to_value(&set_err)
                            .expect("type derives Serialize and is always serializable"),
                    );
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
    if let Some(update_map) = args.get("update").and_then(|v| v.as_object()) {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Check for immutable field violations in the patch keys.
            if let Some(bad_field) = find_immutable_patch_key(patch_val) {
                not_updated.insert(
                    id_str.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": [bad_field],
                    }),
                );
                continue;
            }

            match backend
                .update_object::<Email>(&account_id, &id, patch_val.clone())
                .await
            {
                Ok(_) => {
                    mutated = true;
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_updated.insert(
                        id_str.clone(),
                        serde_json::to_value(&set_err)
                            .expect("type derives Serialize and is always serializable"),
                    );
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str.clone(),
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

            match backend.destroy_object::<Email>(&account_id, &id).await {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_string()));
                }
                Err(BackendSetError::SetError(set_err)) => {
                    not_destroyed.insert(
                        id_str.to_string(),
                        serde_json::to_value(&set_err)
                            .expect("type derives Serialize and is always serializable"),
                    );
                }
                Err(BackendSetError::Other(e)) => {
                    not_destroyed.insert(
                        id_str.to_string(),
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<Email>(&account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?
    } else {
        old_state.clone()
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "updated": if updated.is_empty() { Value::Null } else { Value::Object(updated) },
        "destroyed": if destroyed_list.is_empty() { Value::Null } else { Value::Array(destroyed_list) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
        "notUpdated": if not_updated.is_empty() { Value::Null } else { Value::Object(not_updated) },
        "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return only the keys in `prop_set` from the JSON object `obj`.
///
/// The caller is responsible for building the `HashSet` once before iterating
/// over multiple objects, so the set is not rebuilt on every call.
fn filter_properties(obj: &Value, prop_set: &HashSet<&str>) -> Value {
    match obj {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(k, _)| prop_set.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Value::Object(filtered)
        }
        _ => obj.clone(),
    }
}

/// Return the first patch key that names an immutable Email field, if any.
///
/// A patch key violates immutability if it equals an immutable field name, or
/// starts with `"<field>/"` (JSON Merge Patch sub-path syntax).
fn find_immutable_patch_key(patch: &Value) -> Option<String> {
    let map = patch.as_object()?;
    for key in map.keys() {
        for &field in IMMUTABLE_EMAIL_FIELDS {
            // The byte-index check distinguishes three cases for `field = "messageId"`:
            //   "messageId"    → exact match (blocked)
            //   "messageId/0"  → sub-path match (blocked)
            //   "messageIdX"   → prefix but not a path segment (allowed)
            // A simple `starts_with` would incorrectly block the third case.
            if key == field
                || (key.starts_with(field) && key.as_bytes().get(field.len()) == Some(&b'/'))
            {
                return Some(field.to_string());
            }
        }
    }
    None
}

/// Build an [`Email`] from a creation payload (`obj_val`).
///
/// Extracts `mailboxIds`, `keywords`, and optional header fields from the
/// creation object. Assigns a `blobId` equal to the email's id (a MemoryBackend
/// convention), and assigns a thread id by searching existing emails for
/// matching `inReplyTo`/`references`.
async fn build_email_from_create<B: MailBackend>(
    obj_val: &Value,
    account_id: &Id,
    backend: &B,
) -> Result<Email, String> {
    // mailboxIds: required (already validated non-empty by caller).
    let mailbox_ids: HashMap<Id, bool> = obj_val
        .get("mailboxIds")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // keywords: optional; reject malformed values (same as Email/import).
    let keywords: HashMap<Keyword, bool> = match obj_val.get("keywords") {
        None | Some(Value::Null) => HashMap::new(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| "keywords: invalid keyword or format".to_string())?,
    };

    // Subject, inReplyTo, references — used for thread assignment.
    let subject: Option<String> = obj_val
        .get("subject")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    let in_reply_to: Option<Vec<String>> = obj_val
        .get("inReplyTo")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let references: Option<Vec<String>> = obj_val
        .get("references")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    // Thread assignment: look for an existing email whose messageId matches
    // any of the inReplyTo/references tokens.
    let thread_id = assign_thread(
        backend,
        account_id,
        in_reply_to.as_deref().unwrap_or(&[]),
        references.as_deref().unwrap_or(&[]),
    )
    .await
    .map_err(|e| e.to_string())?;

    // Size: use provided value or 0 (MemoryBackend does not parse raw bytes here).
    let size: u64 = obj_val.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

    // receivedAt: use provided value or now (RFC 8621 §5.5.3).
    let received_at: UTCDate = obj_val
        .get("receivedAt")
        .and_then(|v| v.as_str())
        .map(UTCDate::from)
        .unwrap_or_else(|| UTCDate::from(crate::helpers::now_utc_string().as_str()));

    // blobId: always use a placeholder. Per RFC 8621 §5.5, blobId is server-set
    // and must not be accepted from the client on Email/set create (accepting it
    // would allow clients to reference blobs they do not own). The backend
    // assigns the real blobId in create_object.
    let blob_id: Id = Id::from("placeholder-blob");

    // Use a placeholder id; create_object assigns the real one.
    let mut email = Email::new(
        Id::from("placeholder"),
        blob_id,
        thread_id,
        mailbox_ids,
        size,
        received_at,
    );
    email.keywords = keywords;
    email.subject = subject;
    email.in_reply_to = in_reply_to;
    email.references = references;

    Ok(email)
}

/// Assign a thread id for a new email.
///
/// Calls [`MailBackend::find_thread_by_message_ids`] with the union of
/// `in_reply_to` and `references` tokens. Returns the matching thread id if
/// found, a freshly generated id if no existing email references these tokens,
/// or propagates the backend error so the caller can surface it.
async fn assign_thread<B: MailBackend>(
    backend: &B,
    account_id: &Id,
    in_reply_to: &[String],
    references: &[String],
) -> Result<Id, B::Error> {
    if in_reply_to.is_empty() && references.is_empty() {
        return Ok(next_id());
    }

    let refs: Vec<&str> = in_reply_to
        .iter()
        .chain(references.iter())
        .map(|s| s.as_str())
        .collect();

    match backend
        .find_thread_by_message_ids(account_id, &refs)
        .await?
    {
        Some(thread_id) => Ok(thread_id),
        None => Ok(next_id()),
    }
}

/// Generate a unique opaque Id using an atomic counter seeded from the system clock.
///
/// The counter base is initialized to the current nanoseconds since the Unix epoch
/// on the first call. This makes IDs generated in separate process lifetimes
/// extremely unlikely to collide, which matters for persistent backends that store
/// thread IDs across restarts.
///
/// # Caveats
///
/// This is best-effort, not collision-proof. Persistent backends should override
/// [`MailBackend::find_thread_by_message_ids`] to supply thread IDs from their own
/// durable storage rather than relying on this counter.
fn next_id() -> Id {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static BASE: OnceLock<u64> = OnceLock::new();

    let base = *BASE.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1_000_000_000)
    });
    let n = base.wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed));
    Id::from(format!("{n:016x}"))
}

/// Deduplicate `ids` by `threadId`, keeping only the first email per thread.
///
/// Fetches the query-result emails from the backend to read their thread ids.
/// Propagates backend errors to the caller.
async fn collapse_by_thread<B: MailBackend>(
    backend: &B,
    account_id: &Id,
    ids: Vec<Id>,
) -> Result<Vec<Id>, B::Error> {
    // Fetch only the query-result emails (not all emails) to get their thread ids.
    // Pass a properties hint so backends with column stores can skip body data.
    let (emails, _) = backend
        .get_objects::<Email>(
            account_id,
            Some(&ids),
            Some(&[EmailProperty::Id, EmailProperty::ThreadId]),
        )
        .await?;
    let thread_map: HashMap<Id, Id> = emails.into_iter().map(|e| (e.id, e.thread_id)).collect();

    let mut seen_threads: HashSet<Id> = HashSet::new();
    let mut result = Vec::with_capacity(ids.len());

    for id in ids {
        match thread_map.get(&id) {
            Some(tid) => {
                if seen_threads.insert(tid.clone()) {
                    result.push(id);
                }
            }
            None => {
                // Email not in map (just created?); include it.
                result.push(id);
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Email/import (RFC 8621 §5.7)
// ---------------------------------------------------------------------------

/// Handle an `Email/import` method call (RFC 8621 §5.7).
///
/// Each entry in `emails` must name a blob already uploaded to the account.
/// The backend parses the raw bytes, assigns a thread, and stores the new email.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_import<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let emails = match args.get("emails").and_then(|v| v.as_object()) {
        Some(m) => m.clone(),
        None => return Err(JmapError::invalid_arguments("emails is required")),
    };

    let old_state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();

    for (import_id, entry) in &emails {
        let blob_id: Id = match entry.get("blobId").and_then(|v| v.as_str()) {
            Some(s) => Id::from(s),
            None => {
                not_created.insert(
                    import_id.clone(),
                    json!({"type": "invalidProperties", "properties": ["blobId"]}),
                );
                continue;
            }
        };

        let mailbox_ids: Vec<Id> = match entry.get("mailboxIds").and_then(|v| v.as_object()) {
            Some(m) => m.keys().map(|k| Id::from(k.as_str())).collect(),
            None => {
                not_created.insert(
                    import_id.clone(),
                    json!({"type": "invalidProperties", "properties": ["mailboxIds"]}),
                );
                continue;
            }
        };
        if mailbox_ids.is_empty() {
            not_created.insert(
                import_id.clone(),
                json!({"type": "invalidProperties", "properties": ["mailboxIds"],
                       "description": "at least one mailboxId is required (RFC 8621 §5.7)"}),
            );
            continue;
        }

        let keywords: Vec<jmap_mail_types::Keyword> = match entry.get("keywords") {
            None | Some(Value::Null) => vec![],
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(kws) => kws,
                Err(_) => {
                    not_created.insert(
                        import_id.clone(),
                        json!({"type": "invalidProperties", "properties": ["keywords"]}),
                    );
                    continue;
                }
            },
        };

        let received_at: Option<UTCDate> = entry
            .get("receivedAt")
            .and_then(|v| v.as_str())
            .map(UTCDate::from);

        match backend
            .import_email(
                &account_id,
                &blob_id,
                &mailbox_ids,
                &keywords,
                received_at.as_ref(),
            )
            .await
        {
            Ok((server_id, email)) => {
                let mut obj = serde_json::to_value(&email)
                    .expect("type derives Serialize and is always serializable");
                if let Value::Object(ref mut map) = obj {
                    map.insert(
                        "id".to_owned(),
                        Value::String(server_id.as_ref().to_string()),
                    );
                }
                created.insert(import_id.clone(), obj);
            }
            Err(BackendSetError::SetError(set_err)) => {
                not_created.insert(
                    import_id.clone(),
                    serde_json::to_value(&set_err)
                        .expect("type derives Serialize and is always serializable"),
                );
            }
            Err(BackendSetError::Other(e)) => {
                not_created.insert(
                    import_id.clone(),
                    json!({ "type": "serverFail", "description": e.to_string() }),
                );
            }
        }
    }

    let new_state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/parse (RFC 8621 §5.8)
// ---------------------------------------------------------------------------

/// Handle an `Email/parse` method call (RFC 8621 §5.8).
///
/// Parses the blobs identified by `blobIds` and returns Email objects without
/// storing them. Blobs not found → `notParsable`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_parse<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

    let blob_ids: Vec<Id> = match args.remove("blobIds") {
        Some(v) => serde_json::from_value(v)
            .map_err(|_| JmapError::invalid_arguments("blobIds must be an Id array"))?,
        None => return Err(JmapError::invalid_arguments("blobIds is required")),
    };

    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let prop_set: Option<HashSet<&str>> = properties
        .as_deref()
        .map(|props| props.iter().map(|s| s.as_str()).collect());

    let mut parsed = serde_json::Map::new();
    let mut not_parsable: Vec<Value> = Vec::new();
    let mut not_found: Vec<Value> = Vec::new();

    for blob_id in &blob_ids {
        match backend.parse_email(&account_id, blob_id).await {
            Ok(email) => {
                let val = serde_json::to_value(&email)
                    .expect("type derives Serialize and is always serializable");
                let val = match &prop_set {
                    Some(set) => filter_properties(&val, set),
                    None => val,
                };
                parsed.insert(blob_id.as_ref().to_string(), val);
            }
            Err(_) => {
                // RFC 8621 §5.8: distinguish "blob not found" from "not parsable".
                if backend.blob_exists(&account_id, blob_id).await {
                    not_parsable.push(Value::String(blob_id.as_ref().to_string()));
                } else {
                    not_found.push(Value::String(blob_id.as_ref().to_string()));
                }
            }
        }
    }

    let resp = json!({
        "accountId": account_id.as_ref(),
        "parsed": if parsed.is_empty() { Value::Null } else { Value::Object(parsed) },
        "notParsable": if not_parsable.is_empty() { Value::Null } else { Value::Array(not_parsable) },
        "notFound": if not_found.is_empty() { Value::Null } else { Value::Array(not_found) },
    });

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Email/copy (RFC 8621 §6.1 / RFC 8620 §6.3)
// ---------------------------------------------------------------------------

/// Handle an `Email/copy` method call (RFC 8621 §6.1).
///
/// Copies one or more emails from `fromAccountId` into the current account.
/// Supports `onSuccessDestroyOriginal` and `onSuccessUpdateOriginal`.
///
/// Returns `(response_args, extra_invocations)`. Extra invocations are
/// generated when `onSuccessDestroyOriginal: true` or `onSuccessUpdateOriginal`
/// is non-null, per RFC 8620 §6.3.
pub async fn handle_email_copy<B: MailBackend>(
    backend: &B,
    args: Value,
    call_id: &str,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    let from_account_id: Id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::from(s),
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };

    let create = match args.get("create").and_then(|v| v.as_object()) {
        Some(m) => m.clone(),
        None => return Err(JmapError::invalid_arguments("create is required")),
    };

    let on_success_destroy_original: bool = args
        .get("onSuccessDestroyOriginal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // ifFromInState: check source account state (RFC 8620 §5.4).
    if let Some(if_from_in_state) = args.get("ifFromInState").and_then(|v| v.as_str()) {
        let from_state = backend
            .get_state::<Email>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;
        if if_from_in_state != from_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let old_state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // ifInState: check destination account state (RFC 8620 §5.4).
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str()) {
        if if_in_state != old_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut copied_source_ids: Vec<(String, Id)> = Vec::new(); // (copy_id, source_id)

    for (copy_id, entry) in &create {
        let source_id: Id = match entry.get("id").and_then(|v| v.as_str()) {
            Some(s) => Id::from(s),
            None => {
                not_created.insert(
                    copy_id.clone(),
                    json!({"type": "invalidProperties", "properties": ["id"]}),
                );
                continue;
            }
        };

        let mailbox_ids: Vec<Id> = match entry.get("mailboxIds").and_then(|v| v.as_object()) {
            Some(m) => m.keys().map(|k| Id::from(k.as_str())).collect(),
            None => {
                not_created.insert(
                    copy_id.clone(),
                    json!({"type": "invalidProperties", "properties": ["mailboxIds"]}),
                );
                continue;
            }
        };

        let keywords: Vec<Keyword> = match entry.get("keywords") {
            None | Some(Value::Null) => vec![],
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(kws) => kws,
                Err(_) => {
                    not_created.insert(
                        copy_id.clone(),
                        json!({"type": "invalidProperties", "properties": ["keywords"]}),
                    );
                    continue;
                }
            },
        };

        match backend
            .copy_email(
                &from_account_id,
                &source_id,
                &account_id,
                &mailbox_ids,
                &keywords,
            )
            .await
        {
            Ok((new_id, new_email)) => {
                let mut obj = serde_json::to_value(&new_email)
                    .expect("type derives Serialize and is always serializable");
                if let Value::Object(ref mut map) = obj {
                    map.insert("id".to_owned(), Value::String(new_id.as_ref().to_string()));
                }
                created.insert(copy_id.clone(), obj);
                copied_source_ids.push((copy_id.clone(), source_id));
            }
            Err(BackendSetError::SetError(set_err)) => {
                not_created.insert(
                    copy_id.clone(),
                    serde_json::to_value(&set_err)
                        .expect("type derives Serialize and is always serializable"),
                );
            }
            Err(BackendSetError::Other(e)) => {
                not_created.insert(
                    copy_id.clone(),
                    json!({ "type": "serverFail", "description": e.to_string() }),
                );
            }
        }
    }

    let new_state = backend
        .get_state::<Email>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let resp = json!({
        "fromAccountId": from_account_id.as_ref(),
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    // Execute onSuccess* side effects and build a single implicit Email/set
    // response (RFC 8620 §6.3).
    //
    // The dispatcher appends extra invocations verbatim to methodResponses, so
    // we must build the full response object here — not request args.
    let mut extra: Vec<Invocation> = Vec::new();

    let has_on_success_destroy = on_success_destroy_original && !copied_source_ids.is_empty();
    let has_on_success_update = args
        .get("onSuccessUpdateOriginal")
        .filter(|v| !v.is_null())
        .is_some()
        && !copied_source_ids.is_empty();

    if has_on_success_destroy || has_on_success_update {
        let email_old_state = backend
            .get_state::<Email>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        let mut email_destroyed: Vec<Value> = Vec::new();
        let mut email_not_destroyed = serde_json::Map::new();
        let mut email_updated = serde_json::Map::new();
        let mut email_not_updated = serde_json::Map::new();

        // onSuccessDestroyOriginal: destroy each successfully copied source email.
        if on_success_destroy_original {
            for (_, source_id) in &copied_source_ids {
                match backend
                    .destroy_object::<Email>(&from_account_id, source_id)
                    .await
                {
                    Ok(()) => {
                        email_destroyed.push(Value::String(source_id.as_ref().to_string()));
                    }
                    Err(BackendSetError::SetError(set_err)) => {
                        email_not_destroyed.insert(
                            source_id.as_ref().to_string(),
                            serde_json::to_value(&set_err)
                                .expect("SetError is always serializable"),
                        );
                    }
                    Err(BackendSetError::Other(e)) => {
                        email_not_destroyed.insert(
                            source_id.as_ref().to_string(),
                            json!({ "type": "serverFail", "description": e.to_string() }),
                        );
                    }
                }
            }
        }

        // onSuccessUpdateOriginal: for each successfully copied email whose copy_id
        // appears in the map, apply the specified patch to the original.
        if let Some(on_success_update) = args
            .get("onSuccessUpdateOriginal")
            .and_then(|v| v.as_object())
        {
            for (copy_id, source_id) in &copied_source_ids {
                if let Some(patch) = on_success_update.get(copy_id) {
                    match backend
                        .update_object::<Email>(&from_account_id, source_id, patch.clone())
                        .await
                    {
                        Ok(_) => {
                            email_updated.insert(source_id.as_ref().to_string(), Value::Null);
                        }
                        Err(BackendSetError::SetError(set_err)) => {
                            email_not_updated.insert(
                                source_id.as_ref().to_string(),
                                serde_json::to_value(&set_err)
                                    .expect("SetError is always serializable"),
                            );
                        }
                        Err(BackendSetError::Other(e)) => {
                            email_not_updated.insert(
                                source_id.as_ref().to_string(),
                                json!({ "type": "serverFail", "description": e.to_string() }),
                            );
                        }
                    }
                }
            }
        }

        let email_new_state = backend
            .get_state::<Email>(&from_account_id)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        // RFC 8620 §6.3: a single implicit Email/set response appended after
        // the Email/copy response.
        let set_resp = json!({
            "accountId": from_account_id.as_ref(),
            "oldState": email_old_state.as_ref(),
            "newState": email_new_state.as_ref(),
            "created": Value::Null,
            "updated": if email_updated.is_empty() { Value::Null } else { Value::Object(email_updated) },
            "destroyed": if email_destroyed.is_empty() { Value::Null } else { Value::Array(email_destroyed) },
            "notCreated": Value::Null,
            "notUpdated": if email_not_updated.is_empty() { Value::Null } else { Value::Object(email_not_updated) },
            "notDestroyed": if email_not_destroyed.is_empty() { Value::Null } else { Value::Object(email_not_destroyed) },
        });
        extra.push((
            "Email/set".to_owned(),
            set_resp,
            format!("{call_id}-implicit"),
        ));
    }

    Ok((resp, extra))
}
