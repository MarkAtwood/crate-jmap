//! Mailbox/* method handlers (RFC 8621 §2).

use jmap_mail_types::{Email, EmailFilter, EmailFilterCondition, Mailbox, MailboxFilterCondition};
use jmap_types::{Id, Invocation, JmapError, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::extract_account_id;

// ---------------------------------------------------------------------------
// Mailbox/get (RFC 8621 §2.1)
// ---------------------------------------------------------------------------

/// Handle a `Mailbox/get` method call (RFC 8621 §2.1).
pub async fn handle_mailbox_get<B: MailBackend>(
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

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<Mailbox>(&account_id, ids_slice, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(|m| {
            serde_json::to_value(m).expect("type derives Serialize and is always serializable")
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

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "state": state.as_ref(),
            "list": list_json,
            "notFound": not_found_json,
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Mailbox/changes (RFC 8621 §2.2)
// ---------------------------------------------------------------------------

/// Handle a `Mailbox/changes` method call (RFC 8621 §2.2).
///
/// Includes `updatedProperties` in the response, always `null` for
/// MemoryBackend (no partial-property-update tracking).
pub async fn handle_mailbox_changes<B: MailBackend>(
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
        Some(v) => Some(
            v.as_u64()
                .filter(|&n| n > 0)
                .ok_or_else(|| JmapError::invalid_arguments("maxChanges must be a positive integer"))?,
        ),
    };

    let result = backend
        .get_changes::<Mailbox>(&account_id, &since_state, max_changes)
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
// Mailbox/query (RFC 8621 §2.3)
// ---------------------------------------------------------------------------

/// Handle a `Mailbox/query` method call (RFC 8621 §2.3).
///
/// Applies simple in-process filtering for the filter fields defined in
/// RFC 8621 §2.3: `parentId`, `name`, `role`, `hasAnyRole`, `isSubscribed`.
pub async fn handle_mailbox_query<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.get("limit") {
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
    let position: i64 = match args.get("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    // RFC 8620 §5.5: anchor-based pagination overrides position.
    let anchor: Option<jmap_types::Id> = match args.get("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(jmap_types::Id::from(s.as_str())),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {v}"
            )))
        }
    };
    let anchor_offset: i64 = match args.get("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "anchorOffset: expected an integer, got {v}"
            ))
        })?,
    };

    // Reject any client-supplied sort request: Mailbox/query is implemented
    // in-process and cannot honour RFC 8621 §2.3 comparators. RFC 8620 §5.5
    // requires an unsupportedSort error rather than silently ignoring sort.
    if let Some(sort) = args.get("sort") {
        if !sort.is_null() && sort.as_array().map(|a| !a.is_empty()).unwrap_or(true) {
            return Err(JmapError::unsupported_sort());
        }
    }

    // Fetch all mailboxes and filter in-process.
    let (all_mailboxes, _) = backend
        .get_objects::<Mailbox>(&account_id, None, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let query_state = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Reject unknown filter condition keys (RFC 8620 §5.5 requires unsupportedFilter).
    const KNOWN_FILTER_KEYS: &[&str] = &["parentId", "name", "role", "hasAnyRole", "isSubscribed"];
    if let Some(filter_obj) = args.get("filter").and_then(|v| v.as_object()) {
        for key in filter_obj.keys() {
            if !KNOWN_FILTER_KEYS.contains(&key.as_str()) {
                return Err(JmapError::unsupported_filter());
            }
        }
    }

    // Parse filter into MailboxFilterCondition so the struct fields drive the
    // in-process filter directly, eliminating a duplicate field list.
    let filter: Option<MailboxFilterCondition> = match args.get("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v.clone())
                .map_err(|e| JmapError::invalid_arguments(format!("filter: {e}")))?,
        ),
    };

    let mut matching: Vec<Id> = all_mailboxes
        .into_iter()
        .filter(|m| {
            let Some(ref f) = filter else { return true };
            // parentId: three-way — absent = no filter; null = top-level only; string = specific parent.
            if let Some(ref pv) = f.parent_id {
                match pv {
                    Value::Null => {
                        if m.parent_id.is_some() {
                            return false;
                        }
                    }
                    Value::String(id_str) => {
                        if m.parent_id.as_ref().map(|p| p.as_ref()) != Some(id_str.as_str()) {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref name_substr) = f.name {
                if !m.name.contains(name_substr.as_str()) {
                    return false;
                }
            }
            if let Some(ref role_str) = f.role {
                match &m.role {
                    Some(r) => {
                        if &r.to_string() != role_str {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            if let Some(want_any_role) = f.has_any_role {
                if m.role.is_some() != want_any_role {
                    return false;
                }
            }
            if let Some(want_subscribed) = f.is_subscribed {
                if m.is_subscribed != want_subscribed {
                    return false;
                }
            }
            true
        })
        .map(|m| m.id.clone())
        .collect();

    // Sort deterministically by id string for stable pagination.
    matching.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));

    let total = matching.len() as u64;

    // Resolve start position: anchor overrides position.
    let start = if let Some(ref anchor_id) = anchor {
        let anchor_idx = matching
            .iter()
            .position(|id| id == anchor_id)
            .ok_or_else(|| JmapError::anchor_not_found())?;
        // RFC 8620 §5.5: clamp effective position to [0, len].
        let raw = anchor_idx as i64 + anchor_offset;
        raw.max(0).min(matching.len() as i64) as usize
    } else if position >= 0 {
        (position as usize).min(matching.len())
    } else {
        // saturating_neg() avoids i64::MIN overflow (i64::MIN.saturating_neg() = i64::MAX).
        let neg = position.saturating_neg() as usize;
        matching.len().saturating_sub(neg)
    };

    let page: Vec<&str> = matching[start..]
        .iter()
        .take(limit.map_or(usize::MAX, |n| n as usize))
        .map(|id| id.as_ref())
        .collect();

    // RFC 8620 §5.5: total MUST be omitted when calculateTotal is false (default).
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": true,
        "position": start as i64,
        "ids": page,
    });
    if calculate_total {
        resp["total"] = json!(total);
    }

    Ok((resp, vec![]))
}

// ---------------------------------------------------------------------------
// Mailbox/queryChanges (RFC 8621 §2.4)
// ---------------------------------------------------------------------------

/// Handle a `Mailbox/queryChanges` method call (RFC 8621 §2.4).
pub async fn handle_mailbox_query_changes<B: MailBackend>(
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
        Some(v) => Some(
            v.as_u64()
                .filter(|&n| n > 0)
                .ok_or_else(|| JmapError::invalid_arguments("maxChanges must be a positive integer"))?,
        ),
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
        .query_changes::<Mailbox>(
            &account_id,
            &since_query_state,
            None,
            None,
            max_changes,
            up_to_id.as_ref(),
            false, // collapseThreads does not apply to Mailbox
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

    // RFC 8620 §5.6: total MUST be omitted unless calculateTotal is true.
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
// Mailbox/set (RFC 8621 §2.5)
// ---------------------------------------------------------------------------

/// Handle a `Mailbox/set` method call (RFC 8621 §2.5).
///
/// Enforces:
/// - `name` required on create
/// - role uniqueness per account
/// - server-set field immutability on update
/// - `onDestroyRemoveEmails` cascade logic
pub async fn handle_mailbox_set<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;

    // Check ifInState.
    let current_state = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    if let Some(Value::String(if_in_state)) = args.get("ifInState") {
        if if_in_state.as_str() != current_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let on_destroy_remove_emails = args
        .get("onDestroyRemoveEmails")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Fetch all existing mailboxes once for role-uniqueness and child checks.
    let (all_mailboxes, _) = backend
        .get_objects::<Mailbox>(&account_id, None, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // -----------------------------------------------------------------------
    // Create
    // -----------------------------------------------------------------------

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();

    if let Some(Value::Object(creates)) = args.get("create") {
        for (create_id, props) in creates {
            // name is required.
            if props.get("name").and_then(|v| v.as_str()).is_none() {
                not_created.insert(
                    create_id.clone(),
                    serde_json::to_value(
                        SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(["name"]),
                    )
                    .expect("SetError derives Serialize and is always serializable"),
                );
                continue;
            }

            // Role uniqueness check.
            if let Some(role_val) = props.get("role").filter(|v| !v.is_null()) {
                if let Some(role_str) = role_val.as_str() {
                    let role_taken = all_mailboxes.iter().any(|m| {
                        m.role.as_ref().map_or(false, |r| r.to_string() == role_str)
                    });
                    // Also check what we already successfully created in this request.
                    let role_just_created = created
                        .values()
                        .any(|v| v.get("role").and_then(|r| r.as_str()) == Some(role_str));
                    if role_taken || role_just_created {
                        not_created.insert(
                            create_id.clone(),
                            serde_json::to_value(
                                SetError::new(SetErrorType::InvalidProperties)
                                    .with_properties(["role"]),
                            )
                            .expect("SetError derives Serialize and is always serializable"),
                        );
                        continue;
                    }
                }
            }

            // Build Mailbox from props.
            match build_mailbox_from_props(props) {
                Err(err_val) => {
                    not_created.insert(create_id.clone(), err_val);
                }
                Ok(mailbox) => {
                    match backend
                        .create_object::<Mailbox>(&account_id, create_id, mailbox)
                        .await
                    {
                        Ok((_id, obj)) => {
                            let obj_val = serde_json::to_value(&obj)
                                .expect("type derives Serialize and is always serializable");
                            created.insert(create_id.clone(), obj_val);
                        }
                        Err(BackendSetError::SetError(se)) => {
                            not_created.insert(
                                create_id.clone(),
                                serde_json::to_value(se)
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
        }
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    let mut updated: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    // Track roles assigned by earlier updates in this same request for uniqueness.
    let mut roles_updated_this_request: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Server-set fields that may not appear in a patch.
    const SERVER_SET: &[&str] = &[
        "totalEmails",
        "unreadEmails",
        "totalThreads",
        "unreadThreads",
        "myRights",
        "id",
    ];

    if let Some(Value::Object(updates)) = args.get("update") {
        for (id_str, patch) in updates {
            let id = Id::from(id_str.as_str());

            // Reject patches that touch server-set fields.
            if let Some(obj) = patch.as_object() {
                let bad_props: Vec<String> = SERVER_SET
                    .iter()
                    .filter(|&&field| obj.contains_key(field))
                    .map(|&s| s.to_owned())
                    .collect();
                if !bad_props.is_empty() {
                    not_updated.insert(
                        id_str.clone(),
                        serde_json::to_value(
                            SetError::new(SetErrorType::InvalidProperties)
                                .with_properties(bad_props),
                        )
                        .expect("SetError derives Serialize and is always serializable"),
                    );
                    continue;
                }

                // Role uniqueness on update: check against pre-request state,
                // any role already assigned by an earlier update in this request,
                // and any role assigned by the create loop earlier in this request.
                if let Some(role_val) = obj.get("role").filter(|v| !v.is_null()) {
                    if let Some(role_str) = role_val.as_str() {
                        let role_taken = all_mailboxes.iter().any(|m| {
                            m.id != id
                                && m.role.as_ref().map_or(false, |r| r.to_string() == role_str)
                        });
                        let role_just_updated = roles_updated_this_request.contains(role_str);
                        let role_just_created = created
                            .values()
                            .any(|v| v.get("role").and_then(|r| r.as_str()) == Some(role_str));
                        if role_taken || role_just_updated || role_just_created {
                            not_updated.insert(
                                id_str.clone(),
                                serde_json::to_value(
                                    SetError::new(SetErrorType::InvalidProperties)
                                        .with_properties(["role"]),
                                )
                                .expect("SetError derives Serialize and is always serializable"),
                            );
                            continue;
                        }
                        roles_updated_this_request.insert(role_str.to_string());
                    }
                }
            }

            match backend
                .update_object::<Mailbox>(&account_id, &id, patch.clone())
                .await
            {
                Ok(_) => {
                    updated.insert(id_str.clone(), Value::Null);
                }
                Err(BackendSetError::SetError(se)) => {
                    not_updated.insert(
                        id_str.clone(),
                        serde_json::to_value(se)
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
    // Destroy
    // -----------------------------------------------------------------------

    let mut destroyed: Vec<String> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();

    if let Some(Value::Array(destroy_ids)) = args.get("destroy") {
        // Re-fetch mailboxes after creates and updates so that any newly-created
        // or reparented child mailboxes are visible to the parent-check below.
        let (mailboxes_after_mutations, _) = backend
            .get_objects::<Mailbox>(&account_id, None, None)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        for id_val in destroy_ids {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let id = Id::from(id_str);

            // Check for child mailboxes using the post-create snapshot.
            let has_child = mailboxes_after_mutations
                .iter()
                .any(|m| m.parent_id.as_ref() == Some(&id));
            if has_child {
                not_destroyed.insert(
                    id_str.to_owned(),
                    serde_json::to_value(SetError::new(SetErrorType::MailboxHasChild))
                        .expect("SetError derives Serialize and is always serializable"),
                );
                continue;
            }

            // Fetch emails in this mailbox. The inMailbox filter allows backends
            // with indexes to return only the relevant subset; the in-handler
            // filter below is a correctness safety net for backends that do not
            // support the filter.
            let mut email_filter = EmailFilterCondition::default();
            email_filter.in_mailbox = Some(id.clone());
            let query_result = backend
                .query_objects::<Email>(
                    &account_id,
                    Some(&EmailFilter::Condition(email_filter)),
                    None,
                    None,
                    0,
                )
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
            let (fetched, _) = backend
                .get_objects::<Email>(&account_id, Some(&query_result.ids), None)
                .await
                .map_err(|e| JmapError::server_fail(e.to_string()))?;
            let emails_in_mailbox: Vec<Email> = fetched
                .into_iter()
                .filter(|e| e.mailbox_ids.contains_key(&id))
                .collect();

            if !emails_in_mailbox.is_empty() {
                if !on_destroy_remove_emails {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        serde_json::to_value(SetError::new(SetErrorType::MailboxHasEmail))
                            .expect("SetError derives Serialize and is always serializable"),
                    );
                    continue;
                }

                // onDestroyRemoveEmails=true: cascade.
                for email in &emails_in_mailbox {
                    if email.mailbox_ids.len() == 1 {
                        // Only mailbox — destroy the email entirely.
                        match backend
                            .destroy_object::<Email>(&account_id, &email.id)
                            .await
                        {
                            Ok(()) => {}
                            Err(BackendSetError::SetError(_)) => {}
                            Err(BackendSetError::Other(e)) => {
                                return Err(JmapError::server_fail(e.to_string()));
                            }
                        }
                    } else {
                        // Email is in other mailboxes — remove this mailbox from mailboxIds.
                        let mut patch = serde_json::Map::new();
                        let key = format!("mailboxIds/{}", id.as_ref());
                        patch.insert(key, Value::Null);
                        match backend
                            .update_object::<Email>(&account_id, &email.id, Value::Object(patch))
                            .await
                        {
                            Ok(_) => {}
                            Err(BackendSetError::SetError(_)) => {}
                            Err(BackendSetError::Other(e)) => {
                                return Err(JmapError::server_fail(e.to_string()));
                            }
                        }
                    }
                }
            }

            // Destroy the mailbox itself.
            match backend.destroy_object::<Mailbox>(&account_id, &id).await {
                Ok(()) => {
                    destroyed.push(id_str.to_owned());
                }
                Err(BackendSetError::SetError(se)) => {
                    not_destroyed.insert(
                        id_str.to_owned(),
                        serde_json::to_value(se)
                            .expect("type derives Serialize and is always serializable"),
                    );
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

    // Fetch new state after all mutations.
    let new_state = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let created_out = if created.is_empty() {
        Value::Null
    } else {
        Value::Object(created)
    };
    let not_created_out = if not_created.is_empty() {
        Value::Null
    } else {
        Value::Object(not_created)
    };
    let updated_out = if updated.is_empty() {
        Value::Null
    } else {
        Value::Object(updated)
    };
    let not_updated_out = if not_updated.is_empty() {
        Value::Null
    } else {
        Value::Object(not_updated)
    };
    let destroyed_out = if destroyed.is_empty() {
        Value::Null
    } else {
        Value::Array(destroyed.into_iter().map(Value::String).collect())
    };
    let not_destroyed_out = if not_destroyed.is_empty() {
        Value::Null
    } else {
        Value::Object(not_destroyed)
    };

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldState": current_state.as_ref(),
            "newState": new_state.as_ref(),
            "created": created_out,
            "notCreated": not_created_out,
            "updated": updated_out,
            "notUpdated": not_updated_out,
            "destroyed": destroyed_out,
            "notDestroyed": not_destroyed_out,
        }),
        vec![],
    ))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a [`Mailbox`] from a JSON create-properties object.
///
/// Returns an error value suitable for `notCreated` on failure.
fn build_mailbox_from_props(props: &Value) -> Result<Mailbox, Value> {
    let name = match props.get("name").and_then(|v| v.as_str()) {
        Some(s) => s.to_owned(),
        None => {
            return Err(serde_json::to_value(
                SetError::new(SetErrorType::InvalidProperties)
                    .with_properties(["name"]),
            )
            .expect("SetError derives Serialize and is always serializable"));
        }
    };

    let sort_order: u32 = match props.get("sortOrder") {
        None | Some(Value::Null) => 0,
        Some(v) => match v.as_u64() {
            Some(n) if n <= u32::MAX as u64 => n as u32,
            _ => {
                return Err(serde_json::to_value(
                    SetError::new(SetErrorType::InvalidProperties)
                        .with_properties(["sortOrder"])
                        .with_description("sortOrder must be a non-negative integer ≤ 4294967295"),
                )
                .expect("SetError is always serializable"));
            }
        },
    };

    let is_subscribed: bool = props
        .get("isSubscribed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Use a placeholder id; the backend will assign the real one.
    let mut mailbox = Mailbox::new(
        Id::from("placeholder"),
        name,
        sort_order,
        0,
        0,
        0,
        0,
        jmap_mail_types::MailboxRights::default(),
        is_subscribed,
    );

    if let Some(parent_id_val) = props.get("parentId") {
        if let Some(s) = parent_id_val.as_str() {
            mailbox.parent_id = Some(Id::from(s));
        }
    }

    if let Some(role_val) = props.get("role") {
        if let Some(s) = role_val.as_str() {
            let role: jmap_mail_types::MailboxRole =
                serde_json::from_value(Value::String(s.to_owned())).map_err(|_| {
                    serde_json::to_value(
                        SetError::new(SetErrorType::InvalidProperties)
                            .with_properties(["role"]),
                    )
                    .expect("SetError derives Serialize and is always serializable")
                })?;
            mailbox.role = Some(role);
        }
    }

    Ok(mailbox)
}
