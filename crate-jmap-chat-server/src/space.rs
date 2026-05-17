//! Space/* method handlers (draft-atwood-jmap-chat-00 §Space).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-atwood-jmap-chat-00 §Space. The returned
//! `Value` is the corresponding method-response object per the same
//! section refs. `Space/join` (draft §Space) is a Chat-specific method
//! with its own request/response shape (`accountId`, `inviteCode` →
//! `accountId`, `spaceId`).
//!
//! The returned `Vec<Invocation>` carries any back-reference invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set`
//! (including the structural-mutation pipeline rejections —
//! `addRoles`/`removeRoles`/`addMembers` etc. — and the
//! position-0 / cap-exceeded enforcement) surface in the `notCreated`
//! / `notUpdated` / `notDestroyed` maps within `Ok((Value, ...))`, not
//! as `Err`.

use jmap_chat_types::space_set::{
    CategoryPatch, ChannelCreate, ChannelPatch, MemberPatch, RolePatch,
};
use jmap_chat_types::{Category, Space, SpaceInvite, SpaceRole};
use jmap_types::{Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;

use crate::backend::{
    BackendSetError, ChatBackend, ChatLimits, SetError, SetErrorType, SpacePatchOp,
};
use crate::helpers::{
    count_add_ops, enforce_max_objects_in_set, extract_account_id, finalize_set_response,
    iso8601_before, not_found_json, now_utc_string, serialize_value, set_error_value,
    SetAccumulators,
};
use jmap_server::{server_fail_from_backend, server_fail_value_from_backend};

// ---------------------------------------------------------------------------
// Space/set structural-mutation parsing
// ---------------------------------------------------------------------------

/// The 12 structural mutation keys on a Space patch
/// (draft-atwood-jmap-chat-00 §Space/set). Each variant maps one-to-one
/// to a family of [`SpacePatchOp`] values.
///
/// Used to type-dispatch wire-key classification and per-key parsing
/// without stringly-typed matches in the hot path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuralKey {
    AddRoles,
    UpdateRoles,
    RemoveRoles,
    AddMembers,
    UpdateMembers,
    RemoveMembers,
    AddChannels,
    UpdateChannels,
    RemoveChannels,
    AddCategories,
    UpdateCategories,
    RemoveCategories,
}

impl StructuralKey {
    /// Parse a wire-format key string into a [`StructuralKey`]. Returns
    /// `None` if the key is not one of the 12 structural keys (callers
    /// then classify it as metadata or unknown).
    fn from_wire(key: &str) -> Option<Self> {
        Some(match key {
            "addRoles" => Self::AddRoles,
            "updateRoles" => Self::UpdateRoles,
            "removeRoles" => Self::RemoveRoles,
            "addMembers" => Self::AddMembers,
            "updateMembers" => Self::UpdateMembers,
            "removeMembers" => Self::RemoveMembers,
            "addChannels" => Self::AddChannels,
            "updateChannels" => Self::UpdateChannels,
            "removeChannels" => Self::RemoveChannels,
            "addCategories" => Self::AddCategories,
            "updateCategories" => Self::UpdateCategories,
            "removeCategories" => Self::RemoveCategories,
            _ => return None,
        })
    }

    /// The canonical wire-format spelling of this key, for use in error
    /// messages and `invalidProperties.properties` payloads.
    fn wire_name(self) -> &'static str {
        match self {
            Self::AddRoles => "addRoles",
            Self::UpdateRoles => "updateRoles",
            Self::RemoveRoles => "removeRoles",
            Self::AddMembers => "addMembers",
            Self::UpdateMembers => "updateMembers",
            Self::RemoveMembers => "removeMembers",
            Self::AddChannels => "addChannels",
            Self::UpdateChannels => "updateChannels",
            Self::RemoveChannels => "removeChannels",
            Self::AddCategories => "addCategories",
            Self::UpdateCategories => "updateCategories",
            Self::RemoveCategories => "removeCategories",
        }
    }
}

/// Parse one structural wire key's payload
/// (draft-atwood-jmap-chat-00 §Space/set) into a `Vec<SpacePatchOp>`.
///
/// The 12 structural keys are pluralized arrays whose elements have a
/// per-key shape; this helper handles all of them.
///
/// Returns an error string describing why parsing failed, suitable for use
/// as the `description` field of an `invalidProperties` SetError. The
/// returned error is fatal for the containing update target — the handler
/// inserts the failing wire key into `notUpdated[id].properties` and skips
/// any remaining keys.
fn parse_structural_entries(key: StructuralKey, value: Value) -> Result<Vec<SpacePatchOp>, String> {
    let canonical = key.wire_name();
    let arr = match value {
        Value::Array(a) => a,
        other => {
            return Err(format!(
                "{canonical} must be a JSON array, got {}",
                json_value_kind(&other)
            ));
        }
    };

    let mut out = Vec::with_capacity(arr.len());
    for (idx, entry) in arr.into_iter().enumerate() {
        let op = match key {
            StructuralKey::AddRoles => {
                SpacePatchOp::AddRole(parse_entry::<SpaceRole>(canonical, idx, entry)?)
            }
            StructuralKey::RemoveRoles => {
                SpacePatchOp::RemoveRole(parse_id_entry(canonical, idx, entry)?)
            }
            StructuralKey::UpdateRoles => {
                let (id, patch) = parse_update_entry::<RolePatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateRole { id, patch }
            }
            StructuralKey::AddMembers => {
                let (user_id, role_ids) = parse_add_member_entry(canonical, idx, entry)?;
                SpacePatchOp::AddMember(jmap_chat_types::MemberCreate::new(user_id, role_ids))
            }
            StructuralKey::RemoveMembers => {
                SpacePatchOp::RemoveMember(parse_id_entry(canonical, idx, entry)?)
            }
            StructuralKey::UpdateMembers => {
                let (id, patch) = parse_update_entry::<MemberPatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateMember { user_id: id, patch }
            }
            StructuralKey::AddChannels => {
                SpacePatchOp::AddChannel(parse_entry::<ChannelCreate>(canonical, idx, entry)?)
            }
            StructuralKey::RemoveChannels => {
                SpacePatchOp::RemoveChannel(parse_id_entry(canonical, idx, entry)?)
            }
            StructuralKey::UpdateChannels => {
                let (id, patch) = parse_update_entry::<ChannelPatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateChannel { id, patch }
            }
            StructuralKey::AddCategories => {
                SpacePatchOp::AddCategory(parse_entry::<Category>(canonical, idx, entry)?)
            }
            StructuralKey::RemoveCategories => {
                SpacePatchOp::RemoveCategory(parse_id_entry(canonical, idx, entry)?)
            }
            StructuralKey::UpdateCategories => {
                let (id, patch) = parse_update_entry::<CategoryPatch>(canonical, idx, entry)?;
                SpacePatchOp::UpdateCategory { id, patch }
            }
        };
        out.push(op);
    }
    Ok(out)
}

/// Deserialize one Add* entry into the per-key payload type.
fn parse_entry<T: serde::de::DeserializeOwned>(
    canonical: &'static str,
    idx: usize,
    entry: Value,
) -> Result<T, String> {
    serde_json::from_value(entry)
        .map_err(|e| format!("{canonical}[{idx}]: failed to parse entry: {e}"))
}

/// Remove duplicate member entries whose `id` matches `caller_identity`,
/// keeping the first such entry. Other members are passed through unchanged.
///
/// Used by [`handle_space_join`]'s TOCTOU recovery when two concurrent
/// joins from the same caller both wrote member entries. Per RFC 8620
/// §1.6's forward-compat posture, the join is idempotent: repeated writes
/// that converge on the same state are not failures. After dedup the
/// caller appears exactly once in the members list, which is the join's
/// contract.
fn dedup_caller_members(members: Vec<Value>, caller_identity: &str) -> Vec<Value> {
    let mut seen_ours = false;
    members
        .into_iter()
        .filter(|m| {
            if m.get("id").and_then(|v| v.as_str()) == Some(caller_identity) {
                if seen_ours {
                    false
                } else {
                    seen_ours = true;
                    true
                }
            } else {
                true
            }
        })
        .collect()
}

/// Parse one Remove* entry — a bare string id.
fn parse_id_entry(canonical: &'static str, idx: usize, entry: Value) -> Result<Id, String> {
    match entry {
        Value::String(s) => Ok(Id::from(s.as_str())),
        other => Err(format!(
            "{canonical}[{idx}] must be a string Id, got {}",
            json_value_kind(&other)
        )),
    }
}

/// Parse one Update* entry into (id, patch). The wire form is an object
/// with an `id` property plus the patch fields at the top level; we split
/// `id` off and deserialize the remainder as the typed patch.
fn parse_update_entry<P: serde::de::DeserializeOwned>(
    canonical: &'static str,
    idx: usize,
    entry: Value,
) -> Result<(Id, P), String> {
    let mut obj = match entry {
        Value::Object(o) => o,
        other => {
            return Err(format!(
                "{canonical}[{idx}] must be a JSON object, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let id_val = obj
        .remove("id")
        .ok_or_else(|| format!("{canonical}[{idx}] missing required \"id\" property"))?;
    let id = match id_val {
        Value::String(s) => Id::from(s.as_str()),
        other => {
            return Err(format!(
                "{canonical}[{idx}].id must be a string, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let patch: P = serde_json::from_value(Value::Object(obj))
        .map_err(|e| format!("{canonical}[{idx}]: failed to parse patch fields: {e}"))?;
    Ok((id, patch))
}

/// Parse one `addMembers` entry. Wire form:
/// `{"id": "<ChatContact.id>", "roleIds": ["<RoleId>", …] (optional)}`.
fn parse_add_member_entry(
    canonical: &'static str,
    idx: usize,
    entry: Value,
) -> Result<(Id, Vec<Id>), String> {
    let mut obj = match entry {
        Value::Object(o) => o,
        other => {
            return Err(format!(
                "{canonical}[{idx}] must be a JSON object, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let id_val = obj
        .remove("id")
        .ok_or_else(|| format!("{canonical}[{idx}] missing required \"id\" property"))?;
    let user_id = match id_val {
        Value::String(s) => Id::from(s.as_str()),
        other => {
            return Err(format!(
                "{canonical}[{idx}].id must be a string, got {}",
                json_value_kind(&other)
            ));
        }
    };
    let role_ids: Vec<Id> = match obj.remove("roleIds") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| format!("{canonical}[{idx}].roleIds must be a string array: {e}"))?,
    };
    if let Some(extra) = obj.keys().next() {
        return Err(format!(
            "{canonical}[{idx}] has unexpected property \"{extra}\""
        ));
    }
    Ok((user_id, role_ids))
}

/// Stringify a `serde_json::Value`'s top-level kind for error messages.
fn json_value_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// Space/get
// ---------------------------------------------------------------------------

/// Handle a `Space/get` method call (draft-atwood-jmap-chat-00 §Space).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`); the returned `Value` is the §5.1
/// `/get` response shape (`accountId`, `state`, `list`, `notFound`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_get<B: ChatBackend>(
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

    // Parse the `properties` request argument per RFC 8620 §5.1. The
    // reference `MemoryBackend` returns full Space objects regardless;
    // this handler applies the projection post-hoc (which is also
    // where the non-member field trim layers on, per
    // draft-atwood-jmap-chat-00 §Space/get + bd:JMAP-v9py.4).
    let properties: Option<Vec<String>> = match args.remove("properties").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    let ids_slice = ids.as_deref();
    let (list, mut not_found) = backend
        .get_objects::<Space>(caller, &account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let state = backend
        .get_state::<Space>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // Resolve the caller's identity once for the whole request. The
    // membership check fires per Space; the per-Space cost is a
    // linear scan over `members` (small in the reference impl).
    // When `principal_id` returns `None` the backend has not wired
    // identity (single-user mode) — treat every Space as if the
    // caller were a member, which keeps the kit's no-identity
    // posture intact and preserves prior behavior for existing
    // single-user tests.
    let caller_principal: Option<&Id> = B::principal_id(caller);

    // First pass: classify each backend-returned Space against the
    // non-member-vs-non-previewable rule (bd:JMAP-v9py.20). Spaces
    // that fall in that bucket are lifted out of `list` into
    // `not_found` so a non-member caller cannot distinguish "Space
    // exists but is not publicly previewable" from "Space does not
    // exist" — both produce an identical `notFound` entry per
    // draft-atwood-jmap-chat-00 §Space/get.
    let mut visible_list: Vec<&Space> = Vec::with_capacity(list.len());
    for space in &list {
        if non_member_non_previewable(space, caller_principal) {
            not_found.push(space.id.clone());
        } else {
            visible_list.push(space);
        }
    }

    let list_json: Vec<Value> = visible_list
        .iter()
        .map(|space| project_space_for_caller(space, caller_principal, properties.as_deref()))
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

/// Returns `true` when the caller is a non-member of `space` AND the
/// Space is not publicly previewable.
///
/// Per draft-atwood-jmap-chat-00 §Space/get, such Spaces MUST be
/// classified as `notFound` for the caller — the
/// kit's `handle_space_get` lifts them out of `list` and into
/// `not_found` before the response is shaped. The response is then
/// indistinguishable from the "Space does not exist" outcome, so a
/// non-member caller cannot probe for the existence of a private
/// Space.
///
/// Anonymous callers (`caller_principal == None` — single-user
/// mode) are treated as members and so cannot trip this rule. The
/// rule fires only when caller identity is wired AND the caller is
/// not in the Space's `members` AND `isPubliclyPreviewable: false`.
fn non_member_non_previewable(space: &Space, caller_principal: Option<&Id>) -> bool {
    let Some(principal) = caller_principal else {
        return false;
    };
    let is_member = space
        .members
        .iter()
        .any(|m| m.id.as_ref() == principal.as_ref());
    !is_member && !space.is_publicly_previewable
}

/// Fields a non-member caller may see on a publicly-previewable Space,
/// per draft-atwood-jmap-chat-00 §Space/get.
///
/// The list is exhaustive — any field NOT named here MUST be omitted
/// from the returned object even when the caller explicitly requests
/// it via the `/get` `properties` argument. The handler treats the
/// list as a hard cap, intersected against any `properties` filter.
const NON_MEMBER_PREVIEWABLE_FIELDS: &[&str] = &[
    "id",
    "name",
    "description",
    "iconBlobId",
    "memberCount",
    "createdAt",
    "isPublic",
    "isPubliclyPreviewable",
];

/// Apply the JMAP `/get` `properties` filter plus the
/// non-member-previewable field trim to a single Space object before
/// it lands in the response `list`.
///
/// Three cases (draft-atwood-jmap-chat-00 §Space/get):
///
/// 1. **Member caller** (the caller's principal id matches a
///    `members[i].id`) → return the full Space, intersected with
///    `properties` if specified. No restricted-view trim.
/// 2. **Anonymous caller** (`principal_id` returned `None`, i.e.
///    single-user mode where the backend has not wired identity) →
///    same as the member case. The kit's no-identity posture treats
///    every caller as fully authorized; multi-user deployments
///    override `principal_id` to opt out.
/// 3. **Non-member caller AND `isPubliclyPreviewable: true`** →
///    return the 8-field restricted view, intersected with
///    `properties` if specified. Fields outside
///    [`NON_MEMBER_PREVIEWABLE_FIELDS`] are omitted even when the
///    caller asked for them.
/// 4. **Non-member caller AND `isPubliclyPreviewable: false`** —
///    handled BEFORE this function: `handle_space_get` lifts the
///    Space out of `list` and into `not_found` via
///    [`non_member_non_previewable`]. By the time
///    a Space reaches this projection function, it has already
///    been confirmed visible to the caller. This match arm is
///    therefore unreachable in normal flow; if it WERE reached
///    (e.g. via a direct call from outside `handle_space_get`),
///    the conservative fallback is "no field cap, no trim" — same
///    as a member. Production callers SHOULD route through
///    `handle_space_get` rather than calling this function
///    directly so the notFound classification fires.
fn project_space_for_caller(
    space: &Space,
    caller_principal: Option<&Id>,
    properties: Option<&[String]>,
) -> Result<Value, JmapError> {
    let full = serialize_value(space)?;

    // Compute the field set the caller is allowed to see.
    let allowed_fields: Option<Vec<&str>> = match caller_principal {
        // Anonymous (no-identity mode) — full visibility.
        None => None,
        Some(principal) => {
            let is_member = space
                .members
                .iter()
                .any(|m| m.id.as_ref() == principal.as_ref());
            if is_member {
                None
            } else if space.is_publicly_previewable {
                // Non-member, but the Space is publicly
                // previewable. Apply the 8-field restricted view.
                Some(NON_MEMBER_PREVIEWABLE_FIELDS.to_vec())
            } else {
                // Non-member of a non-previewable Space — the
                // outer handler has already lifted this Space
                // into `not_found` before calling us; reaching
                // here implies a non-handler call site. Fall back
                // to full visibility to keep `project_space_for_caller`
                // a pure projection function with no
                // notFound side-effect; the caller is responsible
                // for the visibility decision.
                None
            }
        }
    };

    // Intersect the allowed set with the request `properties`
    // filter, then apply both. Either filter being `None` means
    // "no constraint from this layer".
    let request_filter: Option<&[String]> = properties;
    match (allowed_fields, request_filter) {
        (None, None) => Ok(full),
        (None, Some(props)) => Ok(filter_object_fields(full, |k| props.iter().any(|p| p == k))),
        (Some(cap), None) => Ok(filter_object_fields(full, |k| cap.contains(&k))),
        (Some(cap), Some(props)) => Ok(filter_object_fields(full, |k| {
            cap.contains(&k) && props.iter().any(|p| p == k)
        })),
    }
}

/// Retain only the top-level fields of `value` for which `keep`
/// returns true. If `value` is not a JSON object, return it
/// unchanged.
///
/// The filter operates on TOP-LEVEL keys only. Nested objects
/// (e.g. inside `roles[].permissions`) are unaffected. This matches
/// the RFC 8620 `/get` `properties` semantics and the spec's
/// restricted-view rule (both reference top-level Space fields).
fn filter_object_fields<F: Fn(&str) -> bool>(value: Value, keep: F) -> Value {
    match value {
        Value::Object(map) => {
            Value::Object(map.into_iter().filter(|(k, _)| keep(k.as_str())).collect())
        }
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Space/changes
// ---------------------------------------------------------------------------

/// Handle a `Space/changes` method call (draft-atwood-jmap-chat-00 §Space).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_changes::<Space, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Space/query
// ---------------------------------------------------------------------------

/// Handle a `Space/query` method call (draft-atwood-jmap-chat-00 §Space).
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
pub async fn handle_space_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    let calculate_total: bool = args
        .get("calculateTotal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let limit: Option<u64> = match args.remove("limit").unwrap_or(Value::Null) {
        Value::Null => None,
        v => match v.as_u64() {
            Some(n) => Some(n),
            None => {
                return Err(JmapError::invalid_arguments(format!(
                    "limit: expected a non-negative integer, got {v}"
                )))
            }
        },
    };

    let position: i64 = match args.remove("position").unwrap_or(Value::Null) {
        Value::Null => 0,
        v => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!("position: expected an integer, got {v}"))
        })?,
    };

    let filter: Option<serde_json::Value> = match args.remove("filter").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(v),
    };

    let sort: Option<Vec<serde_json::Value>> = match args.remove("sort").unwrap_or(Value::Null) {
        Value::Null => None,
        v => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("sort must be an array"))?,
        ),
    };

    let result = backend
        .query_objects::<Space>(
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
// Space/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Space/queryChanges` method call (draft-atwood-jmap-chat-00 §Space).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_query_changes<B: ChatBackend>(
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
        .query_changes::<Space>(
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
// Space/set count-limit enforcement (bd:JMAP-g7wu.2.4.8, bd:JMAP-x2gd.44)
// ---------------------------------------------------------------------------

// `count_add_ops` is shared with the reference `MemoryBackend`'s
// backend-canonical cap enforcement via `crate::helpers::count_add_ops`.

/// Enforce per-Space count limits before dispatching structural ops to
/// [`ChatBackend::apply_space_patch`].
///
/// Per draft-atwood-jmap-chat-00 §Space/set (spec commit `80d5e11`,
/// 2026-05-11), each of the four `add*` ops MUST return an `overQuota`
/// SetError (RFC 8620 §5.3) when the resulting count would exceed a
/// server-defined limit. The handler queries the backend's
/// [`ChatBackend::limits`] for the cap values, fetches the current
/// Space to count existing roles/members/channels/categories, then
/// compares `existing + add` against the cap for each affected
/// collection.
///
/// Atomicity: if any aggregate would exceed its cap, the whole update
/// target is rejected with one `overQuota` SetError — matching
/// RFC 8620 §5.3 `/set` semantics at the target level. The handler
/// surfaces the failure in `notUpdated[id]` and skips the
/// `apply_space_patch` call entirely.
///
/// # Returns
///
/// - `Ok(None)` — no Add* ops in the patch, or all caps satisfied.
/// - `Ok(Some(SetError))` — one or more caps would be exceeded; the
///   SetError is `overQuota` with a description naming the offending
///   collection.
/// - `Err(JmapError)` — the backend read failed; the caller propagates
///   as a `serverFail` for the whole `Space/set` request.
///
/// If `get_objects` returns the Space as missing (e.g. the id was
/// destroyed since `get_state` was read), this helper returns
/// `Ok(None)` and lets `apply_space_patch` return the canonical
/// `notFound` SetError for consistency with the existing not-found
/// path.
async fn check_space_count_limits<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    space_id: &Id,
    ops: &[SpacePatchOp],
    limits: &ChatLimits,
) -> Result<Option<SetError>, JmapError> {
    let (add_roles, add_members, add_channels, add_categories) = count_add_ops(ops);
    if add_roles == 0 && add_members == 0 && add_channels == 0 && add_categories == 0 {
        return Ok(None);
    }

    let (found, _not_found) = backend
        .get_objects::<Space>(
            caller,
            account_id,
            Some(std::slice::from_ref(space_id)),
            None,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // If the Space is missing, let apply_space_patch surface notFound
    // through its existing path. The cap check has nothing to do.
    let Some(space) = found.into_iter().next() else {
        return Ok(None);
    };

    let cur_roles = u32::try_from(space.roles.len()).unwrap_or(u32::MAX);
    let cur_members = u32::try_from(space.members.len()).unwrap_or(u32::MAX);
    let cur_categories = u32::try_from(space.categories.len()).unwrap_or(u32::MAX);
    let cur_channels = u32::try_from(
        space.uncategorized_channel_ids.len()
            + space
                .categories
                .iter()
                .map(|c| c.channel_ids.len())
                .sum::<usize>(),
    )
    .unwrap_or(u32::MAX);

    // Build an overQuota SetError naming the first offending collection.
    // The handler emits a single error per target, so we surface the
    // first cap to trip; a client retry after fixing that collection
    // would expose any second offender on the next request.
    let exceeded = |label: &'static str, current: u32, add: u32, cap: u32| -> Option<SetError> {
        if add == 0 {
            return None;
        }
        let proposed = current.saturating_add(add);
        if proposed > cap {
            Some(
                SetError::new(SetErrorType::OverQuota).with_description(format!(
                    "{label}: would have {proposed} after adding {add} (existing {current}, cap {cap})"
                )),
            )
        } else {
            None
        }
    };

    if let Some(e) = exceeded("roles", cur_roles, add_roles, limits.max_roles_per_space) {
        return Ok(Some(e));
    }
    if let Some(e) = exceeded(
        "members",
        cur_members,
        add_members,
        limits.max_space_members,
    ) {
        return Ok(Some(e));
    }
    if let Some(e) = exceeded(
        "channels",
        cur_channels,
        add_channels,
        limits.max_channels_per_space,
    ) {
        return Ok(Some(e));
    }
    if let Some(e) = exceeded(
        "categories",
        cur_categories,
        add_categories,
        limits.max_categories_per_space,
    ) {
        return Ok(Some(e));
    }

    Ok(None)
}

// ---------------------------------------------------------------------------
// Space/set
// ---------------------------------------------------------------------------

/// Handle a `Space/set` method call (draft-atwood-jmap-chat-00 §Space).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps), augmented
/// with the JMAP Chat structural-mutation pipeline (addRoles / removeRoles
/// / updateRoles / addMembers / removeMembers / updateMembers / addCategories
/// / removeCategories / updateCategories / addChannels / removeChannels /
/// updateChannels / metadata-patch surface); the returned `Value` is the
/// §5.3 `/set` response shape (`accountId`, `oldState`, `newState`, plus
/// the per-operation `created` / `notCreated` / `updated` / `notUpdated`
/// / `destroyed` / `notDestroyed` maps).
///
/// Validation enforced here (not in the backend):
/// - `name` is required on create.
/// - `id`, `createdAt`, `memberCount` are server-set and rejected in updates.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<Space>(caller, &account_id)
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
            let name = match obj_val.get("name").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_owned(),
                _ => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": ["name"] }),
                    );
                    continue;
                }
            };
            if name.len() > 256 {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["name"] }),
                );
                continue;
            }

            let is_public = obj_val
                .get("isPublic")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_publicly_previewable = obj_val
                .get("isPubliclyPreviewable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let now_str = now_utc_string();
            let now: UTCDate = UTCDate::from(now_str.as_ref());

            let mut space = Space::new(
                Id::from("placeholder"),
                name,
                vec![],
                vec![],
                vec![],
                vec![],
                now,
                is_public,
                is_publicly_previewable,
                0,
            );

            if let Some(desc) = obj_val.get("description").and_then(|v| v.as_str()) {
                space.description = Some(desc.to_owned());
            }

            match backend
                .create_object::<Space>(caller, &account_id, create_id, space)
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
    //
    // Query per-Space content limits once for the whole request. The
    // values are implementation-defined (bd:JMAP-g7wu.2.4.8 / workspace
    // AGENTS.md "Backend caps and limits") and the trait method is a
    // sync default that backends can override per-account. Querying
    // once per request (rather than once per target) is correct because
    // the limits are scoped to the account, not the Space.
    let space_limits = backend.limits(caller, &account_id);

    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = Id::from(id_str.as_str());

            // Reject patches that include server-set or directly-overwritable fields.
            // `roles`, `members`, `categories`, and `uncategorizedChannelIds` are
            // managed through named semantic mutations (addRoles/removeRoles, etc.)
            // and must never be overwritten directly via a JSON Merge Patch.
            const SPACE_READONLY: &[&str] = &[
                "id",
                "createdAt",
                "memberCount",
                "roles",
                "members",
                "categories",
                "uncategorizedChannelIds",
            ];
            let bad_props: Vec<&str> = SPACE_READONLY
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

            // The patch must be a JSON object. Reject `null`, arrays, and
            // scalars up front so the rest of the handler can assume a map.
            let Value::Object(mut patch_map) = patch_val else {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch must be a JSON object" }),
                );
                continue;
            };

            // Allowed metadata fields (RFC 8620 §5.3 partial update on
            // server-managed properties). These reach `update_object` via a
            // JSON Merge Patch.
            const METADATA_FIELDS: &[&str] = &[
                "name",
                "description",
                "iconBlobId",
                "isPublic",
                "isPubliclyPreviewable",
            ];

            // Walk every key on the patch object and bucket it as:
            //   - structural (parsed into SpacePatchOp values via the
            //     StructuralKey enum, defined at module top),
            //   - metadata (forwarded to update_object),
            //   - unknown (rejected as invalidProperties).
            // Any parse error on a structural entry is fatal for this target.
            //
            // Note: per draft §Space/set the wire-level apply order across
            // keys is implementation-defined. The backend's apply_space_patch
            // (not this loop) is canonical for ordering semantics.
            let mut ops: Vec<SpacePatchOp> = Vec::new();
            let mut clean_patch = serde_json::Map::new();
            let mut unknown_keys: Vec<String> = Vec::new();
            let mut bad_structural_key: Option<(&'static str, String)> = None;

            for (key, value) in std::mem::take(&mut patch_map) {
                if let Some(structural) = StructuralKey::from_wire(&key) {
                    match parse_structural_entries(structural, value) {
                        Ok(parsed) => ops.extend(parsed),
                        Err(reason) => {
                            bad_structural_key = Some((structural.wire_name(), reason));
                            break;
                        }
                    }
                } else if METADATA_FIELDS.contains(&key.as_str()) {
                    clean_patch.insert(key, value);
                } else {
                    unknown_keys.push(key);
                }
            }

            if let Some((canonical, reason)) = bad_structural_key {
                not_updated.insert(
                    id_str,
                    json!({
                        "type": "invalidProperties",
                        "properties": [canonical],
                        "description": reason,
                    }),
                );
                continue;
            }

            if !unknown_keys.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": unknown_keys }),
                );
                continue;
            }

            if ops.is_empty() && clean_patch.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidPatch", "description": "patch contains no valid fields" }),
                );
                continue;
            }

            // Reject SpaceRole.position == 0 per draft-atwood-jmap-chat-00
            // §SpaceRole commit `c3ea5d9` ("harden position 0 / @everyone
            // reservation"): position 0 is reserved for the implicit
            // @everyone role, which every member of a Space holds and
            // which serves as the permission floor. Defined SpaceRoles
            // MUST have position > 0. The reference permissions resolver
            // uses position: 0 internally for the synthetic @everyone
            // role, so accepting it on the wire would create real
            // conflicts.
            //
            // Per RFC 8620 §5.3 the rejection is per-target atomic, so a
            // single position-0 violation in any addRoles or updateRoles
            // entry rejects the whole update target. The wire shape is
            // `invalidProperties` with `properties: ["position"]` to
            // match the existing per-field-rejection convention in this
            // crate; the bead's "invalidArguments" text was an internal
            // slip (RFC 8620's `invalidArguments` SetError does not
            // carry a `properties` field).
            if ops.iter().any(|op| match op {
                SpacePatchOp::AddRole(role) => role.position == 0,
                SpacePatchOp::UpdateRole { patch, .. } => patch.position == Some(0),
                _ => false,
            }) {
                not_updated.insert(
                    id_str,
                    json!({
                        "type": "invalidProperties",
                        "properties": ["position"],
                        "description":
                            "SpaceRole.position 0 is reserved for the implicit @everyone role; defined roles MUST have position > 0 (draft-atwood-jmap-chat-00 §SpaceRole)",
                    }),
                );
                continue;
            }

            // Enforce per-Space count limits before dispatching structural
            // ops to the backend (bd:JMAP-g7wu.2.4.8). If any aggregate
            // would exceed its cap, reject the whole update target with
            // a single `overQuota` SetError per RFC 8620 §5.3 atomicity
            // at the target level. The cap values come from the
            // backend's `ChatBackend::limits` (queried once per request
            // above the loop); the current per-collection counts come
            // from a `get_objects::<Space>` read of the target Space.
            if !ops.is_empty() {
                match check_space_count_limits(
                    backend,
                    caller,
                    &account_id,
                    &id,
                    &ops,
                    &space_limits,
                )
                .await
                {
                    Ok(Some(err)) => {
                        not_updated.insert(id_str, set_error_value(&err));
                        continue;
                    }
                    Ok(None) => {}
                    Err(je) => return Err(je),
                }
            }

            // Apply structural ops first. If any op fails, surface the first
            // failure as a `notUpdated` entry and skip the metadata write —
            // RFC 8620 §5.3 requires each update target to land in exactly
            // one of `updated` / `notUpdated`, and a partial half-applied
            // outcome would be misleading.
            if !ops.is_empty() {
                match backend
                    .apply_space_patch(caller, &account_id, &id, ops)
                    .await
                {
                    Ok(op_results) => {
                        if let Some(first_err) =
                            op_results.iter().find_map(|r| r.outcome.as_ref().err())
                        {
                            not_updated.insert(id_str, set_error_value(first_err));
                            continue;
                        }
                    }
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
                }
            }

            // Apply metadata (if any) via the dedicated chat-server
            // entry point. Routing through
            // `ChatBackend::apply_space_metadata_patch` (rather than
            // the generic `update_object::<Space>`) gives the backend
            // the type and identity context it needs to apply the
            // `manage_space` permission gate atomically with the
            // mutation. The gate is backend-canonical per workspace
            // AGENTS.md "Caller identity (foundation seam)" — handler
            // does no permission check here. See bd:JMAP-g7wu.2.4.13.
            //
            // If there were no metadata fields, structural ops alone
            // count as a successful update — emit a null sentinel
            // into `updated`.
            if clean_patch.is_empty() {
                mutated = true;
                updated.insert(id_str, Value::Null);
                continue;
            }

            // Decode the wire patch's metadata-key subset into the typed
            // SpaceMetadataPatch shape (bd:JMAP-x2gd.39). The handler has
            // already filtered to METADATA_FIELDS, so unknown-key
            // rejection is impossible here; the remaining failure mode is
            // a wire `null` on a non-nullable field (`isPublic` /
            // `isPubliclyPreviewable`) which surfaces as
            // `invalidProperties` naming the offending key. The error
            // message names the field that failed to deserialize, which
            // is sufficient for a client to act on.
            let typed_patch: jmap_chat_types::SpaceMetadataPatch =
                match serde_json::from_value(Value::Object(clean_patch)) {
                    Ok(p) => p,
                    Err(e) => {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "invalidProperties",
                                "description": format!("metadata patch failed to deserialize: {e}"),
                            }),
                        );
                        continue;
                    }
                };

            match backend
                .apply_space_metadata_patch(caller, &account_id, &id, typed_patch)
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
                .destroy_object::<Space>(caller, &account_id, &id)
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

    finalize_set_response::<B, Space>(
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
// Space/join
// ---------------------------------------------------------------------------

/// Handle a `Space/join` method call (draft-atwood-jmap-chat-00 §Space).
///
/// `args` is the draft §Space `Space/join` request shape (`accountId`,
/// exactly one of `inviteCode` or `spaceId`); the returned `Value` is
/// the §Space response shape (`accountId`, `spaceId`).
///
/// Accepts exactly one of `inviteCode` or `spaceId`. Validates the invite or
/// space, adds the caller as a member, and returns `{ "accountId": ..., "spaceId": ... }`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_join<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    // Resolve the caller's identity via the foundation seam
    // (`JmapBackend::principal_id`) and use it for the
    // `SpaceMember.id` we write. draft-atwood-jmap-chat-00
    // §SpaceMember.id requires that field to carry the participant's
    // `ChatContact.id` — i.e. the caller's authenticated userId, not
    // the JMAP `accountId`. The reader-side membership checks in
    // `non_member_non_previewable` and `project_space_for_caller`
    // already compare against `principal_id`; writing `account_id`
    // here desynchronizes the writer from the reader and is invisible
    // only in single-user deployments where `account_id ==
    // principal_id` collapses both into the same value.
    //
    // Single-user posture per workspace AGENTS.md "Caller identity
    // (foundation seam)": a `None` return from `principal_id` means
    // the backend has not wired identity; fall back to `account_id`
    // so the kit's no-identity test fixtures and the testjig keep
    // their existing behavior (the user IS the account in that
    // posture). Multi-user production backends override `principal_id`
    // and get spec-correct semantics.
    let caller_identity: Id = B::principal_id(caller)
        .cloned()
        .unwrap_or_else(|| account_id.clone());

    let invite_code = args
        .get("inviteCode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let space_id_str = args
        .get("spaceId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    // Validate the invite or space and collect the space_id, current members, and
    // (for the invite path) the pending invite-uses increment deferred until after
    // the already_member check.  Each branch produces
    // (space_id: Id, current_members: Vec<Value>, invite_update: Option<(Id, u64)>).
    let (space_id, current_members, invite_update): (Id, Vec<Value>, Option<(Id, u64)>) =
        match (invite_code, space_id_str) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(JmapError::invalid_arguments(
                    "exactly one of inviteCode or spaceId must be provided",
                ));
            }
            (Some(code), None) => {
                // NOTE: The MemoryBackend stores objects per-account, so invite code lookup
                // works only when the caller's account created the invite. A production backend
                // must maintain a global invite code index. This is a known architectural
                // limitation of the test backend.

                // Invite code path: scan all invites for matching code.
                let (invites, _) = backend
                    .get_objects::<SpaceInvite>(caller, &account_id, None, None)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;

                // Constant-time invite-code compare (bd:JMAP-sc1b.89).
                //
                // `SpaceInvite.code` is an unguessable CSPRNG-derived credential
                // (see `ChatBackend::generate_invite_code` in `backend.rs`). A
                // plain `String == String` short-circuits at the first mismatched
                // byte, exposing a byte-by-byte timing oracle to any caller that
                // can issue `Space/join` requests. `ConstantTimeEq::ct_eq`
                // compares whole equal-length byte slices in constant time.
                //
                // Length-discrimination note: `ct_eq` returns `Choice(0)` cheaply
                // when lengths differ, so an attacker can learn whether their
                // supplied code matches the stored length. The canonical
                // generator emits fixed-length (32 hex chars) codes, so the
                // length is effectively public — only the content needs
                // constant-time protection.
                let invite = invites
                    .into_iter()
                    .find(|inv| inv.code.as_bytes().ct_eq(code.as_bytes()).into())
                    .ok_or_else(|| JmapError::invalid_arguments("invite code not found"))?;

                // Check expiry using second-precision prefix (see iso8601_before).
                // Pure lexicographic comparison on the full string is incorrect for
                // fractional-second timestamps ('.' < 'Z' in ASCII).
                if let Some(expires_at) = &invite.expires_at {
                    let now = now_utc_string();
                    if !iso8601_before(now.as_ref(), expires_at.as_ref()) {
                        return Err(JmapError::invalid_arguments("invite has expired"));
                    }
                }

                // Check maxUses: if set, uses must be strictly less.
                if let Some(max) = invite.max_uses {
                    if invite.uses >= max {
                        return Err(JmapError::invalid_arguments(
                            "invite has reached its maximum number of uses",
                        ));
                    }
                }

                let invite_id = invite.id.clone();
                let new_uses = invite.uses.saturating_add(1);
                let space_id = invite.space_id.clone();

                // Do NOT increment uses yet — defer until after the already_member check
                // so that a failed rejoin attempt does not silently exhaust invite uses.

                // Fetch the space to get the current members list.
                let (spaces, _) = backend
                    .get_objects::<Space>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(&space_id)),
                        None,
                    )
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;
                let members: Vec<Value> = spaces
                    .into_iter()
                    .next()
                    .map(|s| {
                        s.members
                            .into_iter()
                            .map(serde_json::to_value)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .unwrap_or(Ok(vec![]))
                    .map_err(|e| server_fail_from_backend(&e))?;

                (space_id, members, Some((invite_id, new_uses)))
            }
            (None, Some(sid)) => {
                // Public space path: fetch the space by id and verify is_public.
                // The spec requires notPermitted when the space is not found or isPublic is false.
                // JmapError has no notPermitted constructor; forbidden() is the closest standard
                // equivalent and is what the existing tests expect.
                let space_id_typed = Id::from(sid.as_str());
                let (spaces, _) = backend
                    .get_objects::<Space>(caller, &account_id, Some(&[space_id_typed]), None)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;

                let space = spaces
                    .into_iter()
                    .next()
                    .filter(|s| s.is_public)
                    .ok_or_else(JmapError::forbidden)?;

                let space_id = space.id.clone();
                let members: Vec<Value> = space
                    .members
                    .into_iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| server_fail_from_backend(&e))?;

                (space_id, members, None)
            }
        };

    // Add the calling user as a Space member.
    // This bypasses the SPACE_READONLY guard in handle_space_set — Space/join calling
    // update_object directly is correct: it is an atomic server operation, not a client patch.
    let now_str = now_utc_string();
    let mut new_members = current_members;

    // Pre-check: reject if already a member. This check is NOT atomic with the
    // write below — two concurrent requests can both pass and create duplicate
    // member entries. The post-write duplicate detection below catches this in
    // the common case. Storage-layer unique constraints are the authoritative guard.
    //
    // Identity: compare against the caller's resolved identity (see top of
    // function). Writer and reader must agree on which value identifies the
    // member, or two distinct join paths can both succeed and the
    // duplicate-detection logic below will misclassify.
    let already_member = new_members
        .iter()
        .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(caller_identity.as_ref()));
    if already_member {
        return Err(JmapError::invalid_arguments(
            "caller is already a member of this space",
        ));
    }

    new_members.push(json!({
        "id": caller_identity.as_ref(),
        "roleIds": [],
        "joinedAt": now_str,
    }));
    // Concurrency note: this is a read-modify-write on the members array.
    // Two concurrent Space/join calls for different accounts can both read
    // the same stale members list and overwrite each other. Production backends
    // MUST implement this as an atomic array-append (e.g. via a transaction or
    // compare-and-swap) to prevent membership loss.
    let mut members_patch = serde_json::Map::new();
    members_patch.insert("members".to_owned(), json!(new_members));
    backend
        .update_object::<Space>(
            caller,
            &account_id,
            &space_id,
            PatchObject::from_map(members_patch),
        )
        .await
        // bd:JMAP-ic0j.70 — route through server_fail_from_backend so the
        // wire description is the static SERVER_FAIL_INTERNAL_DESC rather
        // than the backend BackendSetError Display output.
        .map_err(|e: jmap_server::BackendSetError<_>| server_fail_from_backend(&e))?;

    // TOCTOU guard: re-read members after write and detect duplicate entries.
    // Two concurrent join requests can both pass the pre-check above and both
    // succeed at the write layer. If that happened, exactly one racer must
    // undo its write. We detect the duplicate here and return an error;
    // the storage layer SHOULD enforce a unique constraint on (space_id, user_id)
    // as the authoritative guard — this code is best-effort self-healing.
    let (post_spaces, _) = backend
        .get_objects::<Space>(
            caller,
            &account_id,
            Some(std::slice::from_ref(&space_id)),
            None,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let post_members: Vec<Value> = post_spaces
        .into_iter()
        .next()
        .map(|s| {
            s.members
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or(Ok(vec![]))
        .map_err(|e| server_fail_from_backend(&e))?;
    let duplicate_count = post_members
        .iter()
        .filter(|m| m.get("id").and_then(|v| v.as_str()) == Some(caller_identity.as_ref()))
        .count();
    if duplicate_count > 1 {
        // Same-caller TOCTOU: two concurrent Space/join requests from this
        // caller both passed the pre-check (line 1474-1481) and both wrote a
        // member entry. The filter keys on caller_identity, so duplicate_count
        // > 1 strictly implies same-caller — different-account races can
        // never hit this branch.
        //
        // Both winning and losing writes inserted bit-identical entries
        // (same id, same roleIds, possibly-millisecond-coincident joinedAt).
        // The previous recovery filter matched on (id, joinedAt) and dropped
        // serverFail in the caller's face — but if the joinedAt strings
        // collided at millisecond resolution, the filter could match either
        // entry, and either way the user IS a member after dedup. The
        // serverFail return forced the caller into a confusing retry path
        // where the pre-check then rejected as already-a-member.
        //
        // Idempotent join: dedup the extras down to a single entry and
        // succeed. Per RFC 8620 §1.6's forward-compat posture, repeated
        // identical writes that converge on the same state are not failures.
        // The unique-key constraint at the storage layer remains the
        // authoritative guard (see write comment at line 1488); this code
        // is best-effort self-healing.
        let deduped = dedup_caller_members(post_members, caller_identity.as_ref());
        let mut deduped_patch = serde_json::Map::new();
        deduped_patch.insert("members".to_owned(), json!(deduped));
        backend
            .update_object::<Space>(
                caller,
                &account_id,
                &space_id,
                PatchObject::from_map(deduped_patch),
            )
            .await
            .map_err(|e: jmap_server::BackendSetError<_>| server_fail_from_backend(&e))?;
        // Fall through to the invite-uses increment and success return —
        // the caller is a member of the Space, which is the join's contract.
    }

    // Apply the invite uses increment only on the success path (after TOCTOU check passes).
    // Deferring here prevents a race-loss from silently consuming an invite use.
    if let Some((invite_id, new_uses)) = invite_update {
        let mut uses_patch = serde_json::Map::new();
        uses_patch.insert("uses".to_owned(), json!(new_uses));
        backend
            .update_object::<SpaceInvite>(
                caller,
                &account_id,
                &invite_id,
                PatchObject::from_map(uses_patch),
            )
            .await
            // bd:JMAP-ic0j.70 — route through server_fail_from_backend so the
            // wire description is the static SERVER_FAIL_INTERNAL_DESC rather
            // than the backend BackendSetError Display output.
            .map_err(|e: jmap_server::BackendSetError<_>| server_fail_from_backend(&e))?;
    }

    Ok((
        json!({ "accountId": account_id.as_ref(), "spaceId": space_id.as_ref() }),
        vec![],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Oracles in this module are hand-built per workspace test-integrity
    // policy: the assertion uses a wire shape independent of the dedup
    // helper under test. Each case asserts both the resulting Vec length
    // and the resulting member ids in order, matching a JMAP wire view
    // a real backend would produce.

    /// A single-entry list with the caller's id is untouched.
    #[test]
    fn dedup_caller_members_keeps_single_entry() {
        let input = vec![json!({"id": "a1", "joinedAt": "2026-01-01T00:00:00.000Z"})];
        let out = dedup_caller_members(input, "a1");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "a1");
    }

    /// Two entries with the caller's id collapse to one; the first is kept.
    /// This is the bug fix surface for bd:JMAP-x2gd.11.
    #[test]
    fn dedup_caller_members_collapses_duplicates() {
        let input = vec![
            json!({"id": "a1", "joinedAt": "2026-01-01T00:00:00.000Z"}),
            json!({"id": "a1", "joinedAt": "2026-01-01T00:00:00.001Z"}),
        ];
        let out = dedup_caller_members(input, "a1");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "a1");
        // First entry wins; the second's joinedAt is dropped.
        assert_eq!(out[0]["joinedAt"], "2026-01-01T00:00:00.000Z");
    }

    /// Three duplicates collapse to one — the kit's recovery is unbounded
    /// in the count of duplicates a backend may report.
    #[test]
    fn dedup_caller_members_collapses_three_duplicates() {
        let input = vec![
            json!({"id": "a1"}),
            json!({"id": "a1"}),
            json!({"id": "a1"}),
        ];
        let out = dedup_caller_members(input, "a1");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "a1");
    }

    /// Other members are preserved verbatim, in order. Only the caller's
    /// duplicates are deduplicated; bystander members are untouched.
    #[test]
    fn dedup_caller_members_preserves_other_members() {
        let input = vec![
            json!({"id": "u2"}),
            json!({"id": "a1"}),
            json!({"id": "u3"}),
            json!({"id": "a1"}),
            json!({"id": "u4"}),
        ];
        let out = dedup_caller_members(input, "a1");
        assert_eq!(out.len(), 4);
        let ids: Vec<&str> = out
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(ids, vec!["u2", "a1", "u3", "u4"]);
    }

    /// An empty members list is returned unchanged.
    #[test]
    fn dedup_caller_members_empty() {
        let out = dedup_caller_members(vec![], "a1");
        assert!(out.is_empty());
    }

    /// Members whose id field is missing or non-string don't match — they
    /// pass through to the result. This protects against malformed data
    /// in storage (which the kit should not produce but a misbehaving
    /// backend might).
    #[test]
    fn dedup_caller_members_skips_malformed_entries() {
        let input = vec![
            json!({"joinedAt": "2026-01-01T00:00:00.000Z"}), // no id field
            json!({"id": 42}),                               // non-string id
            json!({"id": "a1"}),
        ];
        let out = dedup_caller_members(input, "a1");
        assert_eq!(out.len(), 3);
    }
}
