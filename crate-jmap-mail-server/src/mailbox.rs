//! Mailbox/* method handlers (RFC 8621 §2).

use std::collections::HashSet;

use jmap_mail_types::{Email, EmailFilter, EmailFilterCondition, Mailbox, MailboxFilterCondition};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State};
use serde_json::{json, Value};

use crate::backend::{BackendSetError, MailBackend, SetError, SetErrorType};
use crate::helpers::{extract_account_id, filter_properties, not_found_json, ser, set_error_value};

// ---------------------------------------------------------------------------
// Mailbox/get (RFC 8621 §2.1)
// ---------------------------------------------------------------------------

/// Handle a `Mailbox/get` method call (RFC 8621 §2.1).
pub async fn handle_mailbox_get<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let account_id = extract_account_id(&args)?;
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
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

    // RFC 8620 §5.1: when `properties` is specified return only those fields
    // (plus `id` which is always included). `None` means return all fields.
    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, not_found) = backend
        .get_objects::<Mailbox>(&account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let state = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let list_json: Vec<Value> = if let Some(ref props) = properties {
        // Build the effective property set once; always include "id" per RFC 8620 §5.1.
        let mut prop_set: HashSet<&str> = props.iter().map(|s| s.as_str()).collect();
        prop_set.insert("id");
        list.iter()
            .map(|obj| {
                let val = ser(obj)?;
                Ok(filter_properties(&val, &prop_set))
            })
            .collect::<Result<Vec<_>, JmapError>>()?
    } else {
        list.iter().map(ser).collect::<Result<Vec<_>, _>>()?
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
// Mailbox/changes (RFC 8621 §2.2)
// ---------------------------------------------------------------------------

/// Handle a `Mailbox/changes` method call (RFC 8621 §2.2).
pub async fn handle_mailbox_changes<B: MailBackend>(
    backend: &B,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Mailbox, B>(backend, args).await
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
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

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
            JmapError::invalid_arguments(format!("anchorOffset: expected an integer, got {v}"))
        })?,
    };

    // Parse sort comparators. RFC 8621 §2.3 defines "name" and "sortOrder" as valid
    // Mailbox comparator properties. Reject unknown properties with unsupportedSort
    // (RFC 8620 §5.5). Each entry is (property, is_ascending).
    let sort_comparators: Vec<(String, bool)> = match args.get("sort") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(arr)) => {
            let mut comps = Vec::with_capacity(arr.len());
            for comp in arr {
                let prop = comp
                    .get("property")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        JmapError::invalid_arguments("sort comparator missing 'property'")
                    })?;
                match prop {
                    "name" | "sortOrder" => {}
                    _ => return Err(JmapError::unsupported_sort()),
                }
                let asc = comp
                    .get("isAscending")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                comps.push((prop.to_owned(), asc));
            }
            comps
        }
        _ => {
            return Err(JmapError::invalid_arguments(
                "sort must be an array or null",
            ))
        }
    };

    // sortAsTree=true changes result ordering semantics (RFC 8621 §2.3):
    // children must immediately follow their parent in the result list.
    // Reject explicitly rather than silently ignoring — silently returning flat
    // results would produce semantically wrong data for clients relying on
    // tree-mode ordering.
    if args
        .get("sortAsTree")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err(JmapError::unsupported_sort());
    }
    // filterAsTree=true changes child-mailbox inclusion semantics (RFC 8621 §2.3):
    // a mailbox passes the filter only if it or one of its ancestors matches.
    // Reject explicitly rather than silently ignoring — filterAsTree affects which
    // mailboxes appear in the result, so silently returning flat results would
    // produce semantically wrong data for clients relying on tree-mode semantics.
    if args
        .get("filterAsTree")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err(JmapError::unsupported_filter());
    }

    // O(n): fetches all mailboxes and filters in-process. Acceptable for typical account sizes.
    // For very large accounts (IMAP migration), push filter/sort into the backend query.
    let (all_mailboxes, _) = backend
        .get_objects::<Mailbox>(&account_id, None, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    let query_state = backend
        .get_state::<Mailbox>(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // Reject unknown filter condition keys (RFC 8620 §5.5 requires unsupportedFilter).
    //
    // MAINTENANCE: This list must match all fields of MailboxFilterCondition
    // (jmap-mail-types). If a new field is added to MailboxFilterCondition,
    // add its wire name here. Drift causes filter conditions to be rejected
    // with unsupportedFilter instead of being applied — protocol misbehavior
    // that is not a compile error.
    const KNOWN_FILTER_KEYS: &[&str] = &["parentId", "name", "role", "hasAnyRole", "isSubscribed"];
    if let Some(filter_obj) = args.get("filter").and_then(|v| v.as_object()) {
        for key in filter_obj.keys() {
            if !KNOWN_FILTER_KEYS.contains(&key.as_str()) {
                return Err(JmapError::unsupported_filter());
            }
        }
    }

    // Extract parentId from raw filter JSON to preserve three-way semantics
    // (absent / null / string). Serde cannot distinguish absent from null for
    // Option<T>, so we extract before consuming the filter value.
    let filter_parent_id: Option<Value> = args
        .get("filter")
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.get("parentId").cloned());

    // Parse filter into MailboxFilterCondition so the struct fields drive the
    // in-process filter directly, eliminating a duplicate field list.
    let filter: Option<MailboxFilterCondition> = match args.remove("filter") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|e| JmapError::invalid_arguments(format!("filter: {e}")))?,
        ),
    };

    // Pre-compute the wire-format role string once outside the filter closure to
    // avoid calling to_wire_str on every mailbox for every iteration.
    let filter_role_wire: Option<&str> = filter.as_ref().and_then(|f| f.role.as_deref());

    let mut matching: Vec<Mailbox> = all_mailboxes
        .into_iter()
        .filter(|m| {
            // parentId: three-way — absent = no filter; null = top-level only; string = specific parent.
            if let Some(ref pv) = filter_parent_id {
                match pv {
                    Value::Null if m.parent_id.is_some() => {
                        return false;
                    }
                    Value::String(id_str)
                        if m.parent_id.as_ref().map(|p| p.as_ref()) != Some(id_str.as_str()) =>
                    {
                        return false;
                    }
                    _ => {}
                }
            }
            let Some(ref f) = filter else { return true };
            if let Some(ref name_substr) = f.name {
                if !m.name.contains(name_substr.as_str()) {
                    return false;
                }
            }
            if let Some(role_str) = filter_role_wire {
                match &m.role {
                    Some(r) => {
                        if r.to_wire_str() != role_str {
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
        .collect();

    // Sort by client comparators, falling back to id for stable pagination.
    matching.sort_by(|a, b| {
        let mut ord = std::cmp::Ordering::Equal;
        for (prop, asc) in &sort_comparators {
            if ord != std::cmp::Ordering::Equal {
                break;
            }
            let cmp = match prop.as_str() {
                "name" => a.name.cmp(&b.name),
                "sortOrder" => a.sort_order.cmp(&b.sort_order),
                _ => std::cmp::Ordering::Equal,
            };
            ord = if *asc { cmp } else { cmp.reverse() };
        }
        if ord == std::cmp::Ordering::Equal {
            a.id.as_ref().cmp(b.id.as_ref())
        } else {
            ord
        }
    });
    let matching: Vec<Id> = matching.into_iter().map(|m| m.id).collect();

    let total = matching.len() as u64;

    // Resolve start position: anchor overrides position.
    let start = if let Some(ref anchor_id) = anchor {
        let anchor_idx = matching
            .iter()
            .position(|id| id == anchor_id)
            .ok_or_else(JmapError::anchor_not_found)?;
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
        "canCalculateChanges": backend.can_calculate_mailbox_query_changes(&account_id),
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
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }

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
    if !backend
        .account_exists(&account_id)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?
    {
        return Err(JmapError::account_not_found());
    }
    let Value::Object(mut args) = args else {
        return Err(JmapError::invalid_arguments("args must be an object"));
    };

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

    // Fetch all existing mailboxes before any mutations. This snapshot is used
    // in the create loop to check role uniqueness (no two mailboxes may share a
    // role within an account) and in the update loop for the same check. A
    // second fetch after mutations covers the destroy loop's child-mailbox check,
    // because newly created or reparented children must be visible at destroy time.
    //
    // RFC 8620 §5.3 requires create/update/destroy to all operate on the
    // pre-mutation state, so a request that destroys mailbox A (holding role X)
    // and creates mailbox B (with role X) will correctly reject the create:
    // the snapshot still shows A holding role X at the time creates are processed.
    // To swap a role, use two sequential requests.
    let (all_mailboxes, _) = backend
        .get_objects::<Mailbox>(&account_id, None, None)
        .await
        .map_err(|e| JmapError::server_fail(e.to_string()))?;

    // -----------------------------------------------------------------------
    // Pre-compute roles that will be vacated by updates in this request.
    //
    // The create loop runs before the update loop (RFC 8620 §5.3 ordering), so
    // `roles_successfully_vacated` is not yet populated when create role-uniqueness
    // is checked. To allow a single request that clears a role via update and
    // then assigns it via create, we scan the update patches up-front and build
    // the set of role strings that are being set to null.  We only include a
    // role if the patch actually targets an existing mailbox that currently
    // holds that role.
    // -----------------------------------------------------------------------
    let roles_intended_to_vacate: std::collections::HashSet<String> = {
        let mut vacated = std::collections::HashSet::new();
        if let Some(Value::Object(updates)) = args.get("update") {
            for (id_str, patch) in updates {
                let sets_role_null = patch
                    .as_object()
                    .and_then(|o| o.get("role"))
                    .is_some_and(|v| v.is_null());
                if sets_role_null {
                    let id = Id::from(id_str.as_str());
                    if let Some(role) = all_mailboxes
                        .iter()
                        .find(|m| m.id == id)
                        .and_then(|m| m.role.as_ref())
                        .map(|r| r.to_wire_str().to_owned())
                    {
                        vacated.insert(role);
                    }
                }
            }
        }
        vacated
    };

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
                    set_error_value(
                        &SetError::new(SetErrorType::InvalidProperties).with_properties(["name"]),
                    ),
                );
                continue;
            }

            // Role uniqueness check.
            if let Some(role_val) = props.get("role").filter(|v| !v.is_null()) {
                if let Some(role_str) = role_val.as_str() {
                    // Exclude roles that are being vacated by an update in this
                    // same request (pre-computed above).  No mailbox id to exclude:
                    // we are creating a new object, not updating an existing one.
                    let role_taken = role_already_held(
                        role_str,
                        &all_mailboxes,
                        &roles_intended_to_vacate,
                        None,
                    );
                    // Also check what we already successfully created in this request.
                    // MailboxRole always serializes as a bare JSON string (via
                    // impl_string_enum!), so as_str() is always correct here.
                    let role_just_created = created
                        .values()
                        .any(|v| v.get("role").and_then(|r| r.as_str()) == Some(role_str));
                    if role_taken || role_just_created {
                        not_created.insert(
                            create_id.clone(),
                            set_error_value(
                                &SetError::new(SetErrorType::InvalidProperties)
                                    .with_properties(["role"]),
                            ),
                        );
                        continue;
                    }
                }
            }

            // Duplicate name+parentId check (RFC 8621 §2.5 — alreadyExists).
            //
            // Two mailboxes under the same parent may not share a name. The
            // parentId is compared so that null (top-level) and absent (also
            // top-level) are treated identically.
            //
            // The check also covers mailboxes created earlier in this same
            // request: `created` already holds successfully-created entries
            // whose serialised form includes the assigned name and parentId.
            if let Some(proposed_name) = props.get("name").and_then(|v| v.as_str()) {
                // Normalise the proposed parentId: absent/null → None, string → Some(str).
                let proposed_parent: Option<&str> = match props.get("parentId") {
                    Some(serde_json::Value::String(s)) => Some(s.as_str()),
                    _ => None,
                };

                let name_taken_existing = all_mailboxes.iter().any(|m| {
                    m.name == proposed_name
                        && m.parent_id.as_ref().map(|p| p.as_ref()) == proposed_parent
                });

                // Also check mailboxes created earlier in this same request.
                let name_taken_this_request = created.values().any(|v| {
                    v.get("name").and_then(|n| n.as_str()) == Some(proposed_name)
                        && v.get("parentId").and_then(|p| p.as_str()) == proposed_parent
                });

                if name_taken_existing || name_taken_this_request {
                    not_created.insert(
                        create_id.clone(),
                        set_error_value(&SetError::new(SetErrorType::AlreadyExists)),
                    );
                    continue;
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
                            let obj_val = serde_json::to_value(&obj).unwrap_or_else(
                                |e| json!({ "type": "serverFail", "description": e.to_string() }),
                            );
                            created.insert(create_id.clone(), obj_val);
                        }
                        Err(BackendSetError::SetError(se)) => {
                            not_created.insert(create_id.clone(), set_error_value(&se));
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
    // Roles claimed by non-vacating updates in this request — prevents
    // two updates in the same request from both claiming the same role.
    let mut roles_claimed_this_request: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    // Roles freed by successful vacating updates (pass 1).  Built only from
    // updates that actually succeeded, so a failed vacate does NOT release
    // the role and a same-request claim against it is correctly rejected.
    let mut roles_successfully_vacated: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Server-set fields that may not appear in a Mailbox/set update patch.
    //
    // MAINTENANCE: This list must match all server-set fields of the Mailbox
    // struct (jmap-mail-types) as defined in RFC 8621 §2.1. If a new
    // server-set field is added to Mailbox, add its wire name here. Drift
    // allows clients to overwrite server-managed fields — protocol misbehavior
    // that is not a compile error.
    const SERVER_SET: &[&str] = &[
        "totalEmails",
        "unreadEmails",
        "totalThreads",
        "unreadThreads",
        "myRights",
        "id",
    ];

    if let Some(Value::Object(updates)) = args.remove("update") {
        // Two-pass update loop — do NOT collapse into a single pass.
        //
        // Two invariants govern the role-swap logic across the whole update loop:
        //
        //   roles_successfully_vacated — roles freed by pass-1 updates that
        //     actually succeeded. Only a successful vacate releases the role;
        //     a backend error on the vacate keeps the role occupied so that
        //     any same-request claim against it is correctly rejected.
        //
        //   roles_claimed_this_request — roles claimed by pass-2 updates so
        //     far, preventing two non-vacating updates in the same request
        //     from both claiming the same role.
        //
        // Pass 1 runs every patch that sets role: null (vacating a role).
        // Pass 2 runs everything else (including role-claiming patches).
        //
        // Two-pass: vacating patches (role: null) run first so that a single
        // request can atomically swap roles — e.g. A vacates inbox, B claims
        // inbox. Without the two passes, HashMap iteration order is undefined
        // so either pass could run first and reject the claim. RFC 8621 §2.5.
        // Pass 1 = role-vacating (role: null); pass 2 = everything else.
        // This ordering allows a single request to swap roles atomically.
        let (vacating, non_vacating): (Vec<_>, Vec<_>) = updates.into_iter().partition(|(_, v)| {
            v.as_object()
                .and_then(|o| o.get("role"))
                .is_some_and(|v| v.is_null())
        });

        // Returns the InvalidProperties error value if `patch` contains any
        // server-set field, or None if the patch is clean. Used in both passes
        // to avoid duplicating the check block.
        let server_set_error = |patch: &PatchObject| -> Option<Value> {
            let obj = patch.as_map();
            let bad_props: Vec<String> = SERVER_SET
                .iter()
                .filter(|&&field| obj.contains_key(field))
                .map(|&s| s.to_owned())
                .collect();
            if bad_props.is_empty() {
                None
            } else {
                Some(set_error_value(
                    &SetError::new(SetErrorType::InvalidProperties).with_properties(bad_props),
                ))
            }
        };

        // --- Pass 1: role-vacating updates (patch sets role: null) ---
        for (id_str, patch_val) in vacating {
            let id = Id::from(id_str.as_str());

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

            // Reject patches that touch server-set fields.
            if let Some(err) = server_set_error(&patch) {
                not_updated.insert(id_str, err);
                continue;
            }

            // Capture the role currently held by this mailbox before the
            // update runs; needed to know which role to record as vacated.
            let current_role: Option<String> = all_mailboxes
                .iter()
                .find(|m| m.id == id)
                .and_then(|m| m.role.as_ref())
                .map(|r| r.to_wire_str().to_owned());

            match backend
                .update_object::<Mailbox>(&account_id, &id, patch)
                .await
            {
                Ok(maybe_obj) => {
                    // RFC 8620 §5.3: if the backend modified server-set fields
                    // beyond the patch, echo them in the updated map entry.
                    let entry = maybe_obj
                        .as_ref()
                        .map(|o| serde_json::to_value(o).unwrap_or_else(
                            |e| serde_json::json!({ "type": "serverFail", "description": e.to_string() })
                        ))
                        .unwrap_or(Value::Null);
                    updated.insert(id_str, entry);
                    if let Some(role) = current_role {
                        roles_successfully_vacated.insert(role);
                    }
                }
                Err(BackendSetError::SetError(se)) => {
                    not_updated.insert(id_str, set_error_value(&se));
                }
                Err(BackendSetError::Other(e)) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "serverFail", "description": e.to_string() }),
                    );
                }
            }
        }

        // --- Pass 2: non-vacating updates ---
        for (id_str, patch_val) in non_vacating {
            let id = Id::from(id_str.as_str());

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

            // Reject patches that touch server-set fields.
            if let Some(err) = server_set_error(&patch) {
                not_updated.insert(id_str, err);
                continue;
            }

            // Role uniqueness: check against pre-request state minus
            // roles freed by successful pass-1 vacates, plus any role
            // already claimed by an earlier update in this pass, plus
            // any role claimed by the create loop.
            let obj = patch.as_map();
            if let Some(role_val) = obj.get("role").filter(|v| !v.is_null()) {
                if let Some(role_str) = role_val.as_str() {
                    // Exclude the mailbox being updated (id) — it may
                    // already hold this role and is about to change it.
                    let role_taken = role_already_held(
                        role_str,
                        &all_mailboxes,
                        &roles_successfully_vacated,
                        Some(&id),
                    );
                    let role_just_claimed = roles_claimed_this_request.contains(role_str);
                    // MailboxRole always serializes as a bare JSON string (via
                    // impl_string_enum!), so as_str() is always correct here.
                    let role_just_created = created
                        .values()
                        .any(|v| v.get("role").and_then(|r| r.as_str()) == Some(role_str));
                    if role_taken || role_just_claimed || role_just_created {
                        not_updated.insert(
                            id_str,
                            set_error_value(
                                &SetError::new(SetErrorType::InvalidProperties)
                                    .with_properties(["role"]),
                            ),
                        );
                        continue;
                    }
                    roles_claimed_this_request.insert(role_str.to_owned());
                }
            }

            match backend
                .update_object::<Mailbox>(&account_id, &id, patch)
                .await
            {
                Ok(maybe_obj) => {
                    // RFC 8620 §5.3: if the backend modified server-set fields
                    // beyond the patch, echo them in the updated map entry.
                    let entry = maybe_obj
                        .as_ref()
                        .map(|o| serde_json::to_value(o).unwrap_or_else(
                            |e| serde_json::json!({ "type": "serverFail", "description": e.to_string() })
                        ))
                        .unwrap_or(Value::Null);
                    updated.insert(id_str, entry);
                }
                Err(BackendSetError::SetError(se)) => {
                    not_updated.insert(id_str, set_error_value(&se));
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
    // Destroy
    // -----------------------------------------------------------------------

    let mut destroyed: Vec<String> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();

    if let Some(Value::Array(destroy_ids)) = args.get("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        if let Some(bad) = destroy_ids.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {bad}"
            )));
        }

        // Re-fetch mailboxes after create/update: RFC 8620 §5.3 requires
        // create/update/destroy to operate in sequence, so a mailbox created
        // earlier in this request could be a child of the mailbox being
        // destroyed. Using the pre-mutation snapshot (all_mailboxes) would miss
        // that child.
        let (mut mailboxes_after_mutations, _) = backend
            .get_objects::<Mailbox>(&account_id, None, None)
            .await
            .map_err(|e| JmapError::server_fail(e.to_string()))?;

        for id_val in destroy_ids {
            let id_str = match id_val.as_str() {
                Some(s) => s,
                None => continue, // unreachable: validated above
            };
            let id = Id::from(id_str);

            // Check for child mailboxes using the post-create snapshot.
            let has_child = mailboxes_after_mutations
                .iter()
                .any(|m| m.parent_id.as_ref() == Some(&id));
            if has_child {
                not_destroyed.insert(
                    id_str.to_owned(),
                    set_error_value(&SetError::new(SetErrorType::MailboxHasChild)),
                );
                continue;
            }

            // Fetch emails in this mailbox. The backend MUST respect the
            // inMailbox filter in query_objects; if it does not, this may fetch
            // all emails and cause OOM on large accounts. The secondary
            // .filter() below is a correctness safety net for backends that
            // return false positives, not a substitute for proper backend
            // filtering.
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
                        set_error_value(&SetError::new(SetErrorType::MailboxHasEmail)),
                    );
                    continue;
                }

                // onDestroyRemoveEmails=true: cascade.
                // Emails with only this mailbox are batch-destroyed in a single backend call.
                // Emails in multiple mailboxes still need individual update_object calls to
                // patch mailboxIds (no batch-update API exists yet).
                let ids_to_destroy: Vec<Id> = emails_in_mailbox
                    .iter()
                    .filter(|e| e.mailbox_ids.len() == 1)
                    .map(|e| e.id.clone())
                    .collect();

                if !ids_to_destroy.is_empty() {
                    let destroy_results = backend
                        .batch_destroy_emails(&account_id, &ids_to_destroy)
                        .await;
                    for (email_id, err) in destroy_results {
                        match err {
                            None => {}
                            // Semantic error (e.g. already deleted by a concurrent request).
                            // Best-effort cascade: the email is gone or unreachable, so
                            // proceed with mailbox destruction rather than aborting.
                            Some(BackendSetError::SetError(_)) => {}
                            Some(BackendSetError::Other(e)) => {
                                return Err(JmapError::server_fail(format!(
                                    "batch_destroy_emails {email_id}: {e}"
                                )));
                            }
                        }
                    }
                }

                for email in &emails_in_mailbox {
                    if email.mailbox_ids.len() > 1 {
                        // Email is in other mailboxes — remove this mailbox from mailboxIds.
                        let mut patch = serde_json::Map::new();
                        let key = format!("mailboxIds/{}", id.as_ref());
                        patch.insert(key, Value::Null);
                        match backend
                            .update_object::<Email>(
                                &account_id,
                                &email.id,
                                PatchObject::from_map(patch),
                            )
                            .await
                        {
                            Ok(_) => {}
                            // Semantic error (e.g. email deleted concurrently). Best-effort:
                            // proceed with mailbox destruction.
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
                    // RFC 8620 §5.3: each destroy takes effect sequentially.
                    // Remove the just-destroyed mailbox from the snapshot so
                    // subsequent has_child checks do not see it as a child.
                    mailboxes_after_mutations.retain(|m| m.id != id);
                }
                Err(BackendSetError::SetError(se)) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&se));
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

/// Returns `true` if `role_str` is already held by some mailbox in `all`,
/// taking into account roles known to be vacated by the current request.
///
/// `exclude_id` is `Some(id)` when called from the update loop (skip the
/// mailbox being updated itself) and `None` when called from the create loop
/// (no existing mailbox to exclude).
///
/// This encapsulates the `all_mailboxes.iter().any()` check that appears in
/// both the create and the update loops, ensuring the predicate stays in sync
/// if the Mailbox role rules ever change.
fn role_already_held(
    role_str: &str,
    all: &[jmap_mail_types::Mailbox],
    vacated: &std::collections::HashSet<String>,
    exclude_id: Option<&jmap_types::Id>,
) -> bool {
    all.iter().any(|m| {
        // Skip the mailbox being updated (only applicable in the update loop).
        if exclude_id.is_some_and(|exc| *exc == m.id) {
            return false;
        }
        // The role is only still occupied if it was NOT vacated by this request.
        !vacated.contains(role_str) && m.role.as_ref().is_some_and(|r| r.to_wire_str() == role_str)
    })
}

/// Build a [`Mailbox`] from a JSON create-properties object.
///
/// Returns an error value suitable for `notCreated` on failure.
fn build_mailbox_from_props(props: &Value) -> Result<Mailbox, Value> {
    let name = match props.get("name").and_then(|v| v.as_str()) {
        Some(s) => s.to_owned(),
        None => {
            return Err(set_error_value(
                &SetError::new(SetErrorType::InvalidProperties).with_properties(["name"]),
            ));
        }
    };

    let sort_order: u32 = match props.get("sortOrder") {
        None | Some(Value::Null) => 0,
        Some(v) => match v.as_u64() {
            Some(n) if n <= u32::MAX as u64 => n as u32,
            _ => {
                return Err(set_error_value(
                    &SetError::new(SetErrorType::InvalidProperties)
                        .with_properties(["sortOrder"])
                        .with_description("sortOrder must be a non-negative integer ≤ 4294967295"),
                ));
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
                    set_error_value(
                        &SetError::new(SetErrorType::InvalidProperties).with_properties(["role"]),
                    )
                })?;
            mailbox.role = Some(role);
        }
    }

    Ok(mailbox)
}
