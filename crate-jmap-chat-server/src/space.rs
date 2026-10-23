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
//! The returned `HandlerOutput::extra_invocations` carries any invocations
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

use jmap_chat_types::SPEC_PERMISSION_NAMES;
use jmap_chat_types::{
    CategoryCreate, CategoryPatch, ChannelCreate, ChannelPatch, MemberPatch, RolePatch,
};
use jmap_chat_types::{
    Chat, ChatKind, RoleCreate, Space, SpaceBan, SpaceInvite, SpaceMember, SpaceRole,
};
use jmap_chat_types::{SpaceComparator, SpaceFilter};
use jmap_types::{Id, JmapError, PatchObject, SetObject, UTCDate};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

use crate::helpers::{PropType, mistyped_create_props};
use crate::helpers::{
    SetAccumulators, count_add_ops, default_sort, enforce_max_objects_in_get,
    enforce_max_objects_in_set, extract_account_id, finalize_set_response, iso8601_before,
    not_found_json, now_utc_string, optional_arg, reject_unsupported_sort, serialize_value,
    set_error_value,
};
use crate::{ChatBackend, ChatLimits, InviteJoinRefusal, SetError, SetErrorType, SpacePatchOp};
use jmap_server::{
    HandlerOutput, QueryParams, backend_set_error_value, filter_arg, is_client_updatable,
    query_anchor_args, reject_read_only_patch, reject_server_set, server_fail_from_backend,
    server_fail_value_from_backend,
};

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
                jmap_server::json_type_name(&other)
            ));
        }
    };

    let mut out = Vec::with_capacity(arr.len());
    for (idx, entry) in arr.into_iter().enumerate() {
        let op = match key {
            StructuralKey::AddRoles => {
                SpacePatchOp::AddRole(parse_entry::<RoleCreate>(canonical, idx, entry)?)
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
                SpacePatchOp::AddCategory(parse_entry::<CategoryCreate>(canonical, idx, entry)?)
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
        Value::String(s) => Id::new_validated(s).map_err(|e| format!("{canonical}[{idx}]: {e}")),
        other => Err(format!(
            "{canonical}[{idx}] must be a string Id, got {}",
            jmap_server::json_type_name(&other)
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
                jmap_server::json_type_name(&other)
            ));
        }
    };
    let id_val = obj
        .remove("id")
        .ok_or_else(|| format!("{canonical}[{idx}] missing required \"id\" property"))?;
    let id = match id_val {
        Value::String(s) => {
            Id::new_validated(s).map_err(|e| format!("{canonical}[{idx}].id: {e}"))?
        }
        other => {
            return Err(format!(
                "{canonical}[{idx}].id must be a string, got {}",
                jmap_server::json_type_name(&other)
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
                jmap_server::json_type_name(&other)
            ));
        }
    };
    let id_val = obj
        .remove("id")
        .ok_or_else(|| format!("{canonical}[{idx}] missing required \"id\" property"))?;
    let user_id = match id_val {
        Value::String(s) => {
            Id::new_validated(s).map_err(|e| format!("{canonical}[{idx}].id: {e}"))?
        }
        other => {
            return Err(format!(
                "{canonical}[{idx}].id must be a string, got {}",
                jmap_server::json_type_name(&other)
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

    // Parse the `properties` request argument per RFC 8620 §5.1. The
    // reference `MemoryBackend` returns full Space objects regardless;
    // this handler applies the projection post-hoc (which is also
    // where the non-member field trim layers on, per
    // draft-atwood-jmap-chat-00 §Space/get + bd:JMAP-v9py.4).
    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
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

    // Resolve the caller's member id once for the whole request. The
    // membership check fires per Space; the per-Space cost is a
    // linear scan over `members` (small in the reference impl).
    // The caller is the owner, whose member entry is the accountId
    // (draft-atwood-jmap-chat-00 §Terminology, §SpaceMember.id). This
    // holds whether or not `principal_id` resolves a caller identity
    // (bd:JMAP-yt1w.36).
    let caller_member_id: &Id = &account_id;

    // First pass: classify each backend-returned Space against the
    // non-member-vs-non-previewable rule (bd:JMAP-v9py.20). Spaces
    // that fall in that bucket are lifted out of `list` into
    // `not_found` so a non-member caller cannot distinguish "Space
    // exists but is not publicly previewable" from "Space does not
    // exist" — both produce an identical `notFound` entry per
    // draft-atwood-jmap-chat-00 §Space/get.
    let mut visible_list: Vec<&Space> = Vec::with_capacity(list.len());
    for space in &list {
        if non_member_non_previewable(space, caller_member_id) {
            not_found.push(space.id.clone());
        } else {
            visible_list.push(space);
        }
    }

    let list_json: Vec<Value> = visible_list
        .iter()
        .map(|space| project_space_for_caller(space, caller_member_id, properties.as_deref()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    })))
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
/// `caller_member_id` is the caller's member entry id: the accountId,
/// since the caller is the owner (draft-atwood-jmap-chat-00
/// §Terminology). The rule fires when no entry in the Space's
/// `members` carries that id AND `isPubliclyPreviewable: false`.
fn non_member_non_previewable(space: &Space, caller_member_id: &Id) -> bool {
    let is_member = space
        .members
        .iter()
        .any(|m| m.id.as_ref() == caller_member_id.as_ref());
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
    "closedAt",
];

/// Apply the JMAP `/get` `properties` filter plus the
/// non-member-previewable field trim to a single Space object before
/// it lands in the response `list`.
///
/// Three cases (draft-atwood-jmap-chat-00 §Space/get):
///
/// 1. **Member caller** (`caller_member_id`, the accountId per
///    §Terminology, matches a `members[i].id`) → return the full Space, intersected with
///    `properties` if specified. No restricted-view trim.
/// 2. **Non-member caller AND `isPubliclyPreviewable: true`** →
///    return the 8-field restricted view, intersected with
///    `properties` if specified. Fields outside
///    [`NON_MEMBER_PREVIEWABLE_FIELDS`] are omitted even when the
///    caller asked for them.
/// 3. **Non-member caller AND `isPubliclyPreviewable: false`** —
///    handled BEFORE this function: `handle_space_get` lifts the
///    Space out of `list` and into `not_found` via
///    [`non_member_non_previewable`]. By the time
///    a Space reaches this projection function, it has already
///    been confirmed visible to the caller. This branch is
///    therefore unreachable in normal flow; if it WERE reached
///    (e.g. via a direct call from outside `handle_space_get`),
///    the conservative fallback is "no field cap, no trim" — same
///    as a member. Production callers SHOULD route through
///    `handle_space_get` rather than calling this function
///    directly so the notFound classification fires.
fn project_space_for_caller(
    space: &Space,
    caller_member_id: &Id,
    properties: Option<&[String]>,
) -> Result<Value, JmapError> {
    let full = serialize_value(space)?;

    // Compute the field set the caller is allowed to see.
    let is_member = space
        .members
        .iter()
        .any(|m| m.id.as_ref() == caller_member_id.as_ref());
    let allowed_fields: Option<Vec<&str>> = if is_member {
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
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Space, B>(backend, caller, args).await
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
/// Unknown filter clauses and sort properties are refused with
/// `unsupportedFilter` / `unsupportedSort` (RFC 8620 §5.5).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
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
                    "limit: expected a non-negative integer, got {}",
                    jmap_server::json_type_name(&v)
                )));
            }
        },
    };

    let position: i64 = match args.remove("position") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "position: expected an integer, got {}",
                jmap_server::json_type_name(&v)
            ))
        })?,
    };

    let (anchor, anchor_offset) = query_anchor_args(&args)?;

    // §Space/query: "Default sort: `name` ascending."
    default_sort(&mut args, json!([{ "property": "name" }]));
    reject_unsupported_sort::<SpaceComparator>(args.get("sort"))?;
    let filter: Option<SpaceFilter> = filter_arg(&mut args)?;

    let sort: Option<Vec<SpaceComparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    let result = backend
        .query_objects::<Space>(
            caller,
            &account_id,
            QueryParams::new()
                .filter(filter.as_ref())
                .sort(sort.as_deref())
                .limit(limit)
                .position(position)
                .anchor(anchor.as_ref())
                .anchor_offset(anchor_offset),
        )
        .await
        .map_err(JmapError::from)?;

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": result.position,
        "ids": result.ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
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
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // RFC 8620 §5.6: the `filter` and `sort` are those of the original
    // query, so the §Space/query default sort applies here too.
    if let Value::Object(map) = &mut args {
        default_sort(map, json!([{ "property": "name" }]));
    }
    reject_unsupported_sort::<SpaceComparator>(args.get("sort"))?;
    jmap_server::handle_query_changes::<Space, B>(backend, caller, args).await
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
    //
    // Per-aggregate static description — names which aggregate was
    // exceeded without disclosing the per-Space count or the per-account
    // cap value. See bd:JMAP-x2gd.107: leaking either through an error
    // description back-channels deployment policy and membership counts
    // to any caller who can propose a /set patch. The aggregate name
    // alone is enough for a client to know which collection to retry
    // without; numeric counts and caps are deployment-private. Byte-
    // identical to the backend-canonical description in
    // [`crate::memory::check_count_caps`] for response-shape parity.
    let exceeded =
        |description: &'static str, current: u32, add: u32, cap: u32| -> Option<SetError> {
            if add == 0 {
                return None;
            }
            if current.saturating_add(add) > cap {
                Some(SetError::new(SetErrorType::OverQuota).with_description(description))
            } else {
                None
            }
        };

    // First cap to trip determines the SetError. `find_map` expresses
    // that ordered "first hit wins" semantic in one combinator chain
    // rather than four sequential `if-let-Some` early-returns. Mirrors
    // the backend-canonical sibling [`crate::memory::check_count_caps`].
    Ok([
        (
            "too many roles",
            cur_roles,
            add_roles,
            limits.max_roles_per_space,
        ),
        (
            "too many members",
            cur_members,
            add_members,
            limits.max_space_members,
        ),
        (
            "too many channels",
            cur_channels,
            add_channels,
            limits.max_channels_per_space,
        ),
        (
            "too many categories",
            cur_categories,
            add_categories,
            limits.max_categories_per_space,
        ),
    ]
    .into_iter()
    .find_map(|(description, current, add, cap)| exceeded(description, current, add, cap)))
}

// ---------------------------------------------------------------------------
// Space/set
// ---------------------------------------------------------------------------

/// Id of the bootstrap `"Admin"` role [`handle_space_set`] gives every
/// newly created Space (bd:JMAP-yt1w.34).
const BOOTSTRAP_ADMIN_ROLE_ID: &str = "admin";

/// Position of the bootstrap `"Admin"` role: 2^53-1, the largest JMAP
/// `UnsignedInt` (RFC 8620 §1.3), so no later role can outrank it.
const BOOTSTRAP_ADMIN_ROLE_POSITION: u64 = 9_007_199_254_740_991;

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
/// On create the Space is born with one role, `"Admin"` (id `"admin"`,
/// every permission in [`SPEC_PERMISSION_NAMES`], position 2^53-1), and
/// one member, the owner (id = `accountId`) holding that role, per
/// draft-atwood-jmap-chat-00 §Space/set "Creating a Space".
///
/// Validation enforced here (not in the backend):
/// - `name` is required on create.
/// - `id`, `createdAt`, `memberCount` are server-set and rejected in updates.
/// - Keys listed by [`ChatBackend::space_extension_properties`] are
///   accepted on create and update, except a core Space property or a
///   structural patch key, which keeps its core meaning (`closedAt` in an
///   update is `invalidProperties`); any other unknown update key is
///   `invalidProperties`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-fo38).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.3). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<Space>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;
    // `ChatBackend::space_extension_properties`: "A listed key that is
    // already a core Space property or a structural patch key keeps its
    // core meaning", so such a key is dropped here and never reaches
    // `extra` on create or `apply_space_metadata_patch` on update.
    let extension_props: Vec<&str> = backend
        .space_extension_properties(caller, &account_id)
        .iter()
        .copied()
        .filter(|&key| {
            !(Space::SERVER_SET.contains(&key)
                || Space::CLIENT_SETTABLE.contains(&key)
                || StructuralKey::from_wire(key).is_some())
        })
        .collect();

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Space>) {
                not_created.insert(create_id.clone(), set_error_value(&err));
                continue;
            }
            // Optional properties: a wrongly-typed value is invalidProperties
            // (RFC 8620 §5.3), not a silent absence.
            if let Some(err) = mistyped_create_props(
                obj_val,
                &[
                    ("description", PropType::Str),
                    ("isPublic", PropType::Bool),
                    ("isPubliclyPreviewable", PropType::Bool),
                ],
            ) {
                not_created.insert(create_id.clone(), err);
                continue;
            }
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

            // draft-atwood-jmap-chat-00 §Space/set "Creating a Space": the
            // server "adds the caller to `members` with at least one role
            // granting permissions sufficient to administer the Space". The
            // caller acts as the account owner, whose member entry is the
            // accountId (§SpaceMember `id`). Bootstrap configuration
            // (bd:JMAP-yt1w.34): one "Admin" role holding every spec
            // permission at the highest UnsignedInt position. Role ids are
            // scoped to their Space, so the fixed id cannot collide with a
            // role in another Space.
            // ponytail: one fixed bootstrap policy for every backend, though
            // the draft calls it server-defined. If a consumer needs another
            // policy, add a ChatBackend hook returning the bootstrap role,
            // defaulting to this one.
            let admin_role: SpaceRole = serde_json::from_value(json!({
                "id": BOOTSTRAP_ADMIN_ROLE_ID,
                "name": "Admin",
                "permissions": SPEC_PERMISSION_NAMES,
                "position": BOOTSTRAP_ADMIN_ROLE_POSITION,
            }))
            .expect("bootstrap role literal matches SpaceRole");
            let owner: SpaceMember = serde_json::from_value(json!({
                "id": account_id,
                "roleIds": [BOOTSTRAP_ADMIN_ROLE_ID],
                "joinedAt": now_str,
            }))
            .expect("bootstrap member literal matches SpaceMember");

            let build = |id: &Id| {
                let mut space = Space::new(
                    id.clone(),
                    name,
                    vec![admin_role],
                    vec![owner],
                    vec![],
                    vec![],
                    now,
                    is_public,
                    is_publicly_previewable,
                    1,
                );
                if let Some(desc) = obj_val.get("description").and_then(|v| v.as_str()) {
                    space.description = Some(desc.to_owned());
                }
                for &key in &extension_props {
                    match obj_val.get(key) {
                        None | Some(Value::Null) => {}
                        Some(v) => {
                            space.extra.insert(key.to_owned(), v.clone());
                        }
                    }
                }
                space
            };

            match backend
                .create_object::<Space, _>(caller, &account_id, create_id, build)
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
                Err(e) => {
                    not_created.insert(create_id.clone(), backend_set_error_value(e));
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
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };

            // Reject patches to server-set Space properties, including
            // `roles`, `members`, `categories` and `uncategorizedChannelIds`,
            // which change only through the structural keys.
            if let Some(err) = patch_val
                .as_object()
                .and_then(|m| reject_read_only_patch::<Space>(m, None))
            {
                not_updated.insert(id_str, set_error_value(&err));
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

            // Walk every key on the patch object and bucket it as:
            //   - structural (parsed into SpacePatchOp values via the
            //     StructuralKey enum, defined at module top),
            //   - metadata or a backend-registered extension property
            //     (`ChatBackend::space_extension_properties`), forwarded
            //     to apply_space_metadata_patch,
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
                } else if is_client_updatable::<Space>(&key)
                    || extension_props.contains(&key.as_str())
                {
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

            // draft-atwood-jmap-chat-00 §Space `closedAt`: "The server
            // MUST also reject ... every `Space/set` update to the Space
            // with a `forbidden` SetError, except a member removing
            // themselves via `removeMembers`." The owner removes themselves
            // by the accountId (§Terminology).
            let self_removal = clean_patch.is_empty()
                && matches!(ops.as_slice(), [SpacePatchOp::RemoveMember(m)] if *m == account_id);
            if !self_removal {
                match backend
                    .get_objects::<Space>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(&id)),
                        None,
                    )
                    .await
                {
                    Ok((spaces, _)) if spaces.first().is_some_and(|s| s.closed_at.is_some()) => {
                        not_updated.insert(
                            id_str,
                            json!({ "type": "forbidden", "description": "the Space is closed" }),
                        );
                        continue;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
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
                let removed_channels: Vec<Id> = ops
                    .iter()
                    .filter_map(|op| match op {
                        SpacePatchOp::RemoveChannel(chat_id) => Some(chat_id.clone()),
                        _ => None,
                    })
                    .collect();
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
                        for chat_id in &removed_channels {
                            backend
                                .on_chat_destroyed(caller, &account_id, chat_id)
                                .await;
                        }
                    }
                    Err(e) => {
                        not_updated.insert(id_str, backend_set_error_value(e));
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
            // already filtered to METADATA_FIELDS plus the backend's
            // extension properties (which land in `extra`), so unknown-key
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
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
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
        let id_strs: Vec<&str> = destroy_arr
            .iter()
            .map(|v| {
                v.as_str().ok_or_else(|| {
                    JmapError::invalid_arguments(format!(
                        "destroy: every element must be a string Id; got {}",
                        jmap_server::json_type_name(v)
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        for id_str in id_strs {
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&err));
                    continue;
                }
            };

            // The backend cascades the destroy to the Space's channel
            // Chats (§Destroying a Space), so read their ids first for
            // `ChatBackend::on_chat_destroyed`.
            // ponytail: a channel added by a concurrent request between
            // this read and the destroy gets no callback. The upgrade
            // path is a destroy method that returns the cascaded ids.
            let channel_ids = match space_channel_ids(backend, caller, &account_id, &id).await {
                Ok(ids) => ids,
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), server_fail_value_from_backend(&e));
                    continue;
                }
            };

            match backend
                .destroy_object::<Space>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str.to_owned()));
                    for chat_id in &channel_ids {
                        backend
                            .on_chat_destroyed(caller, &account_id, chat_id)
                            .await;
                    }
                    backend.on_space_destroyed(caller, &account_id, &id).await;
                }
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), backend_set_error_value(e));
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
/// `inviteCode` resolves only within the request's `accountId`, which
/// names the account hosting the Space; invite links therefore carry
/// both `accountId` and the code. A code created in another account is
/// reported as `invalidArguments` "invite code not found", exactly like
/// an unknown code.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_space_join<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, args) = extract_account_id(args)?;

    // The joining participant is the owner, referenced by the accountId:
    // draft-atwood-jmap-chat-00 §SpaceMember.id is "The participant's
    // ChatContact.id, or the accountId for the owner", and the owner's
    // SpaceBan carries the accountId as its `userId` (§Terminology).

    let invite_code = args
        .get("inviteCode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let space_id_str = args
        .get("spaceId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    // Validate the invite or space and collect the space_id, current members
    // (public path only; the invite path's membership write is the backend's
    // atomic redemption), the invite to redeem, and whether the Space is closed.
    let (space_id, current_members, invite_id, closed) = match (invite_code, space_id_str) {
        (Some(_), Some(_)) | (None, None) => {
            return Err(JmapError::invalid_arguments(
                "exactly one of inviteCode or spaceId must be provided",
            ));
        }
        (Some(code), None) => {
            // Invite code path: scan the request account's invites for a
            // matching code. Resolution is scoped to `accountId` by design
            // (see the function docs); there is no server-wide index.
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
                if !iso8601_before(&now, expires_at) {
                    return Err(JmapError::invalid_arguments("invite has expired"));
                }
            }

            // Check maxUses: if set, uses must be strictly less. Kept here for
            // the draft's check order; `join_space_with_invite` re-checks it
            // atomically with the increment.
            if let Some(max) = invite.max_uses
                && invite.uses >= max
            {
                return Err(JmapError::invalid_arguments(
                    "invite has reached its maximum number of uses",
                ));
            }

            let space_id = invite.space_id.clone();

            // Fetch the space for its `closedAt`.
            let (spaces, _) = backend
                .get_objects::<Space>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&space_id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            let closed = spaces.first().is_some_and(|s| s.closed_at.is_some());

            (space_id, Vec::new(), Some(invite.id), closed)
        }
        (None, Some(sid)) => {
            // Public space path: fetch the space by id and verify is_public.
            // draft-atwood-jmap-chat-00 §Space/join: "If the Space does not exist or has
            // `isPublic: false`, the server MUST return a `notPermitted` method error."
            let space_id_typed = Id::new_validated(sid.as_str())
                .map_err(|e| JmapError::invalid_arguments(format!("spaceId: {e}")))?;
            let (spaces, _) = backend
                .get_objects::<Space>(caller, &account_id, Some(&[space_id_typed]), None)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            let space = spaces
                .into_iter()
                .next()
                .filter(|s| s.is_public)
                .ok_or_else(|| JmapError::custom("notPermitted"))?;

            let space_id = space.id.clone();
            let closed = space.closed_at.is_some();
            let members: Vec<Value> = space
                .members
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| server_fail_from_backend(&e))?;

            (space_id, members, None, closed)
        }
    };

    // draft-atwood-jmap-chat-00 §Space/join: "The server MUST verify that no
    // active SpaceBan exists for the requesting account's owner in the target
    // Space." Evaluated after the argument and Space-resolution checks above,
    // per the draft's check order. A ban whose `expiresAt` has passed is not
    // active.
    let (bans, _) = backend
        .get_objects::<SpaceBan>(caller, &account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let now = now_utc_string();
    if bans.iter().any(|b| {
        b.space_id == space_id
            && b.user_id == account_id
            && b.expires_at
                .as_ref()
                .is_none_or(|e| iso8601_before(&now, e))
    }) {
        return Err(JmapError::forbidden());
    }

    // draft-atwood-jmap-chat-00 §Space/join: "If the target Space has
    // `closedAt` set ({{space}}), the server MUST reject the request with
    // a `forbidden` method error, whether the caller supplied
    // `inviteCode` or `spaceId`." Evaluated with the ban check, after
    // Space resolution.
    if closed {
        return Err(JmapError::forbidden());
    }

    let joined = || {
        Ok(HandlerOutput::new(
            json!({ "accountId": account_id.as_ref(), "spaceId": space_id.as_ref() }),
        ))
    };

    // draft-atwood-jmap-chat-00 §Space/join: "The `uses` increment and
    // membership insertion MUST be performed within a single atomic
    // operation so that concurrent redemptions cannot exceed `maxUses`."
    if let Some(invite_id) = invite_id {
        let joined_at = UTCDate::from(now_utc_string().as_ref());
        return match backend
            .join_space_with_invite(caller, &account_id, &invite_id, &joined_at)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            Ok(()) => {
                create_channel_read_positions(backend, caller, &account_id, &space_id).await;
                joined()
            }
            Err(InviteJoinRefusal::NotFound) => {
                Err(JmapError::invalid_arguments("invite code not found"))
            }
            Err(InviteJoinRefusal::Exhausted) => Err(JmapError::invalid_arguments(
                "invite has reached its maximum number of uses",
            )),
            Err(InviteJoinRefusal::AlreadyMember) => Err(JmapError::invalid_arguments(
                "caller is already a member of this space",
            )),
        };
    }

    // Add the calling user as a Space member.
    // This bypasses the read-only check in handle_space_set — Space/join calling
    // update_object directly is correct: it is an atomic server operation, not a client patch.
    let now_str = now_utc_string();
    let mut new_members = current_members;

    // Pre-check: reject if already a member. This check is NOT atomic with the
    // write below — two concurrent requests can both pass and create duplicate
    // member entries. The post-write duplicate detection below catches this in
    // the common case. Storage-layer unique constraints are the authoritative guard.
    //
    // Identity: compare against the owner's accountId (see top of
    // function). Writer and reader must agree on which value identifies the
    // member, or two distinct join paths can both succeed and the
    // duplicate-detection logic below will misclassify.
    let already_member = new_members
        .iter()
        .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(account_id.as_ref()));
    if already_member {
        return Err(JmapError::invalid_arguments(
            "caller is already a member of this space",
        ));
    }

    new_members.push(json!({
        "id": account_id.as_ref(),
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
        .transpose()
        .map_err(|e| server_fail_from_backend(&e))?
        .unwrap_or_default();
    let duplicate_count = post_members
        .iter()
        .filter(|m| m.get("id").and_then(|v| v.as_str()) == Some(account_id.as_ref()))
        .count();
    if duplicate_count > 1 {
        // Same-caller TOCTOU: two concurrent Space/join requests from this
        // caller both passed the pre-check (line 1474-1481) and both wrote a
        // member entry. The filter keys on account_id, so duplicate_count
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
        let deduped = dedup_caller_members(post_members, account_id.as_ref());
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
        // Fall through to the success return — the caller is a member of
        // the Space, which is the join's contract.
    }

    create_channel_read_positions(backend, caller, &account_id, &space_id).await;
    joined()
}

/// Create the owner's ReadPosition for every channel Chat of `space_id`.
///
/// draft-atwood-jmap-chat-00 §ReadPosition: "For channel Chats, a
/// ReadPosition is created when the owner joins the containing Space."
/// Best effort, like [`crate::message::ensure_read_positions`]: the join
/// is already stored, so a failure here is not a method-level error; a
/// missed record is created by the next delivery to that channel.
async fn create_channel_read_positions<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) {
    let Ok(channel_ids) = space_channel_ids(backend, caller, account_id, space_id).await else {
        return;
    };
    crate::message::ensure_read_positions(backend, caller, account_id, channel_ids).await;
}

/// Ids of every channel Chat whose `spaceId` is `space_id`.
async fn space_channel_ids<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    space_id: &Id,
) -> Result<Vec<Id>, B::Error> {
    let (chats, _) = backend
        .get_objects::<Chat>(caller, account_id, None, None)
        .await?;
    Ok(chats
        .into_iter()
        .filter(|c| c.kind == ChatKind::Channel && c.space_id.as_ref() == Some(space_id))
        .map(|c| c.id)
        .collect())
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
