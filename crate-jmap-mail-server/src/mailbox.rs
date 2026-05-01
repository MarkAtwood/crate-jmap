//! Mailbox/* method handlers (RFC 8621 §2).

use jmap_mail_types::{Email, Mailbox};
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

    let ids: Option<Vec<Id>> = match args.get("ids") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v.clone())
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
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
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

    let limit: Option<u64> = args.get("limit").and_then(|v| v.as_u64());
    let position: i64 = args.get("position").and_then(|v| v.as_i64()).unwrap_or(0);

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

    // Extract mailbox-specific filter fields from args.
    let filter_parent_id: Option<Option<Id>> = match args.get("filter") {
        Some(f) => match f.get("parentId") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => v.as_str().map(|s| Some(Id::from(s))),
        },
        None => None,
    };
    let filter_name: Option<&str> = args
        .get("filter")
        .and_then(|f| f.get("name"))
        .and_then(|v| v.as_str());
    let filter_role: Option<&str> = args
        .get("filter")
        .and_then(|f| f.get("role"))
        .and_then(|v| v.as_str());
    let filter_has_any_role: Option<bool> = args
        .get("filter")
        .and_then(|f| f.get("hasAnyRole"))
        .and_then(|v| v.as_bool());
    let filter_is_subscribed: Option<bool> = args
        .get("filter")
        .and_then(|f| f.get("isSubscribed"))
        .and_then(|v| v.as_bool());

    let mut matching: Vec<Id> = all_mailboxes
        .into_iter()
        .filter(|m| {
            if let Some(ref wanted_parent) = filter_parent_id {
                if &m.parent_id != wanted_parent {
                    return false;
                }
            }
            if let Some(name_substr) = filter_name {
                if !m.name.contains(name_substr) {
                    return false;
                }
            }
            if let Some(role_str) = filter_role {
                match &m.role {
                    Some(r) => {
                        if r.to_string() != role_str {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            if let Some(want_any_role) = filter_has_any_role {
                let has_role = m.role.is_some();
                if has_role != want_any_role {
                    return false;
                }
            }
            if let Some(want_subscribed) = filter_is_subscribed {
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
    let start = if position >= 0 {
        (position as usize).min(matching.len())
    } else {
        let neg = (-position) as usize;
        matching.len().saturating_sub(neg)
    };

    let page: Vec<&str> = matching[start..]
        .iter()
        .take(limit.map_or(usize::MAX, |n| n as usize))
        .map(|id| id.as_ref())
        .collect();

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "queryState": query_state.as_ref(),
            "canCalculateChanges": true,
            "position": start as i64,
            "ids": page,
            "total": total,
        }),
        vec![],
    ))
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
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            JmapError::invalid_arguments("maxChanges must be a positive integer")
        })?),
    };

    let up_to_id: Option<Id> = args.get("upToId").and_then(|v| v.as_str()).map(Id::from);

    let result = backend
        .query_changes::<Mailbox>(
            &account_id,
            &since_query_state,
            None,
            None,
            max_changes,
            up_to_id.as_ref(),
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

    Ok((
        json!({
            "accountId": account_id.as_ref(),
            "oldQueryState": result.old_query_state.as_ref(),
            "newQueryState": result.new_query_state.as_ref(),
            "removed": removed,
            "added": added,
        }),
        vec![],
    ))
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
                            .with_properties(vec!["name".to_owned()]),
                    )
                    .expect("SetError derives Serialize and is always serializable"),
                );
                continue;
            }

            // Role uniqueness check.
            if let Some(role_val) = props.get("role").filter(|v| !v.is_null()) {
                if let Some(role_str) = role_val.as_str() {
                    let role_taken = all_mailboxes.iter().any(|m| {
                        m.role.as_ref().map(|r| r.to_string()) == Some(role_str.to_owned())
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
                                    .with_properties(vec!["role".to_owned()]),
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
                            return Err(JmapError::server_fail(e.to_string()));
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

                // Role uniqueness on update: check against pre-request state and
                // any role already assigned by an earlier update in this request.
                if let Some(role_val) = obj.get("role").filter(|v| !v.is_null()) {
                    if let Some(role_str) = role_val.as_str() {
                        let role_taken = all_mailboxes.iter().any(|m| {
                            m.id != id
                                && m.role.as_ref().map(|r| r.to_string())
                                    == Some(role_str.to_owned())
                        });
                        let role_just_updated = roles_updated_this_request.contains(role_str);
                        if role_taken || role_just_updated {
                            not_updated.insert(
                                id_str.clone(),
                                serde_json::to_value(
                                    SetError::new(SetErrorType::InvalidProperties)
                                        .with_properties(vec!["role".to_owned()]),
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
                    return Err(JmapError::server_fail(e.to_string()));
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
        // Fetch all emails once before the loop to avoid O(N) backend scans.
        let (all_emails, _) = backend
            .get_objects::<Email>(&account_id, None, None)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        for id_val in destroy_ids {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue,
            };
            let id = Id::from(id_str);

            // Check for child mailboxes.
            let has_child = all_mailboxes
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

            let emails_in_mailbox: Vec<&Email> = all_emails
                .iter()
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
                    return Err(JmapError::server_fail(e.to_string()));
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
                    .with_properties(vec!["name".to_owned()]),
            )
            .expect("SetError derives Serialize and is always serializable"));
        }
    };

    let sort_order: u32 = props
        .get("sortOrder")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(0);

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
                            .with_properties(vec!["role".to_owned()]),
                    )
                    .expect("SetError derives Serialize and is always serializable")
                })?;
            mailbox.role = Some(role);
        }
    }

    Ok(mailbox)
}
