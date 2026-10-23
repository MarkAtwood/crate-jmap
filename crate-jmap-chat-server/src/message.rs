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
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures
//! (`accountNotFound`, `invalidArguments`, `stateMismatch`, `serverFail`,
//! `unsupportedFilter`, `unsupportedSort`, `cannotCalculateChanges` —
//! per RFC 8620 §3.6 and §5). Per-target failures inside `/set`
//! (including the `slow_mode_check` `rateLimit` and reaction-patch
//! `invalidPatch` paths) surface in the `notCreated` / `notUpdated` /
//! `notDestroyed` maps within `Ok((Value, ...))`, not as `Err`.

use jmap_chat_types::{
    Attachment, AttachmentCreate, BROADCAST_MENTION_SCOPES, BroadcastMention, Chat, DeliveryState,
    Mention, Message, MessageAction, PresenceStatus, ReadPosition, Space,
};
use jmap_chat_types::{Filter, MessageComparator, MessageFilter, MessageFilterCondition};
use jmap_types::{Id, JmapError, PatchObject, UTCDate};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::chat::record_unread_count_changes;
use crate::outbound::{OutboundEvent, ReactionAction};
use crate::{BackendSetError, ChatBackend, ChatLimits, SetError, SetErrorType};
use std::collections::{BTreeMap, HashMap, HashSet};
use subtle::ConstantTimeEq;

use crate::helpers::{PropType, mistyped_create_props};
use crate::helpers::{
    SetAccumulators, default_sort, enforce_max_objects_in_get, enforce_max_objects_in_set,
    extract_account_id, filter_properties, finalize_set_response, iso8601_before, not_found_json,
    now_utc_string, optional_arg, reject_unsupported_sort, serialize_value, set_error_value,
};
use crate::helpers::{parents_first, resolved_create};
use jmap_server::{
    HandlerOutput, QueryParams, backend_set_error_value, filter_arg, is_client_updatable,
    query_anchor_args, reject_server_set, server_fail_from_backend, server_fail_value_from_backend,
};

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
    let (fetched, mut not_found) = backend
        .get_objects::<Message>(caller, &account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // draft-atwood-jmap-chat-00 §Message/query: "messages in channels the
    // caller cannot view MUST be omitted" from `Message/get`. A hidden
    // message is reported like a missing one: a requested id goes to
    // notFound, and a full fetch drops it (RFC 8620 §5.1 notFound lists
    // only requested ids).
    let mut viewable: HashMap<Id, bool> = HashMap::new();
    let mut list = Vec::with_capacity(fetched.len());
    for msg in fetched {
        let visible = match viewable.get(&msg.chat_id) {
            Some(&v) => v,
            None => {
                let v = backend
                    .may_view_chat(caller, &account_id, &msg.chat_id)
                    .await
                    .map_err(|e| server_fail_from_backend(&e))?;
                viewable.insert(msg.chat_id.clone(), v);
                v
            }
        };
        if visible {
            list.push(msg);
        } else if ids_slice.is_some() {
            not_found.push(msg.id);
        }
    }

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

    let wants_counts = properties.as_ref().is_none_or(|p| {
        p.iter()
            .any(|k| k == "replyCount" || k == "unreadReplyCount")
    });
    if wants_counts && backend.limits(caller, &account_id).supports_threads {
        fill_reply_counts(backend, caller, &account_id, &mut list).await?;
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

    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    })))
}

/// Set the server-computed thread counts on each Message in `list`
/// (draft-atwood-jmap-chat-00 §Message, used when `supportsThreads` is
/// `true`).
///
/// `replyCount`: "Count of messages in this chat with `replyTo` equal to
/// this message's `id`." `unreadReplyCount`, only on a thread root
/// (`threadRootId` absent): "Count of replies to this message received
/// after the owner's `ReadPosition.lastReadMessageId` in this Chat".
/// Message ids are ULIDs "lexicographically ordered by time" (§Message
/// IDs), so a reply counts as unread when its `id` sorts after
/// `lastReadMessageId`, or always when that is absent (as for
/// `Chat.unreadCount`).
// ponytail: scans every Message and ReadPosition in the account per
// call (O(messages) per Message/get); a backend with many messages
// needs a reply-count index behind a ChatBackend hook.
async fn fill_reply_counts<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    list: &mut [Message],
) -> Result<(), JmapError> {
    if list.is_empty() {
        return Ok(());
    }
    let (all, _) = backend
        .get_objects::<Message>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let (positions, _) = backend
        .get_objects::<ReadPosition>(caller, account_id, None, None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    for msg in list.iter_mut() {
        let replies = || {
            all.iter()
                .filter(|r| r.chat_id == msg.chat_id && r.reply_to.as_ref() == Some(&msg.id))
        };
        let reply_count = replies().count() as u64;
        msg.unread_reply_count = if msg.thread_root_id.is_none() {
            let last_read = positions
                .iter()
                .find(|p| p.chat_id == msg.chat_id)
                .and_then(|p| p.last_read_message_id.as_ref());
            let unread = replies()
                .filter(|r| last_read.is_none_or(|l| r.id.as_ref() > l.as_ref()))
                .count();
            Some(unread as u64)
        } else {
            None
        };
        msg.reply_count = Some(reply_count);
    }
    Ok(())
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
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Message, B>(backend, caller, args).await
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
/// Unknown filter clauses and sort properties are refused with
/// `unsupportedFilter` / `unsupportedSort` (RFC 8620 §5.5).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_query<B: ChatBackend>(
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

    reject_unsupported_sort::<MessageComparator>(args.get("sort"))?;
    let filter: Option<MessageFilter> = filter_arg(&mut args)?;

    let supports_threads = backend.limits(caller, &account_id).supports_threads;
    let top = check_message_filter(filter.as_ref(), supports_threads)?;

    // §Message/query: "Default sort: `receivedAt` ascending when `chatId`
    // is present; `receivedAt` descending when `chatId` is absent."
    let in_one_chat = top.chat_id.is_some();
    default_sort(
        &mut args,
        json!([{ "property": "receivedAt", "isAscending": in_one_chat }]),
    );
    let sort: Option<Vec<MessageComparator>> = optional_arg(&mut args, "sort", || {
        JmapError::invalid_arguments("sort must be an array")
    })?;

    // draft-atwood-jmap-chat-00 §Message/query: "messages in channels the
    // caller cannot view MUST be omitted". A query scoped to one Chat the
    // caller cannot view matches nothing; a query without `chatId` relies
    // on the backend (see `ChatBackend::may_view_chat`).
    let hidden = match top.chat_id.as_ref() {
        Some(chat_id) => !backend
            .may_view_chat(caller, &account_id, chat_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?,
        None => false,
    };

    let result = backend
        .query_objects::<Message>(
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
    let (ids, position, total) = if hidden {
        (Vec::new(), 0, Some(0))
    } else {
        (result.ids, result.position, result.total)
    };

    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": result.query_state.as_ref(),
        "canCalculateChanges": result.can_calculate_changes,
        "position": position,
        "ids": ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total && let Some(t) = total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

/// Apply the draft-atwood-jmap-chat-00 §Message/query `unsupportedFilter`
/// refusals and return the top-level filter condition.
///
/// Shared by `Message/query` and `Message/queryChanges`, whose filter is
/// that of the original query (RFC 8620 §5.6).
///
/// # Errors
///
/// `unsupportedFilter` when the filter is not a top-level condition with
/// `chatId` or `hasMention: true` ("Servers MUST return an
/// `unsupportedFilter` error for any request that omits `chatId` without
/// also including `hasMention: true`"), sets `text` ("Servers that do not
/// support full-text search MUST return `unsupportedFilter`"), or sets
/// `threadRootId` while `supports_threads` is `false` ("Valid only when
/// `supportsThreads` is `true`; otherwise servers MUST return
/// `unsupportedFilter`").
///
/// ponytail: no backend implements `text`, so it is refused for every
/// backend; a backend that adds it needs a ChatBackend capability hook to
/// lift the refusal.
fn check_message_filter(
    filter: Option<&MessageFilter>,
    supports_threads: bool,
) -> Result<&MessageFilterCondition, JmapError> {
    match filter {
        Some(Filter::Condition(c))
            if (c.chat_id.is_some() || c.has_mention == Some(true))
                && c.text.is_none()
                && (c.thread_root_id.is_none() || supports_threads) =>
        {
            Ok(c)
        }
        _ => Err(JmapError::unsupported_filter()),
    }
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
/// A query scoped to one Chat the caller cannot view fails with
/// `cannotCalculateChanges`, so the client re-runs `Message/query`, which
/// returns nothing for that Chat (see [`handle_message_query`]).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
) -> Result<HandlerOutput, JmapError> {
    // RFC 8620 §5.6: the `filter` and `sort` are those of the original
    // query, so the §Message/query default sort applies here too.
    let chat_id = args
        .get("filter")
        .and_then(|f| f.get("chatId"))
        .and_then(Value::as_str)
        .map(Id::from);
    if let Value::Object(map) = &mut args {
        default_sort(
            map,
            json!([{ "property": "receivedAt", "isAscending": chat_id.is_some() }]),
        );
    }
    reject_unsupported_sort::<MessageComparator>(args.get("sort"))?;

    // RFC 8620 §5.6: the filter is that of the original query, so the
    // §Message/query `unsupportedFilter` refusals apply here too.
    let account_id = args.get("accountId").and_then(Value::as_str).map(Id::from);
    if let Some(account_id) = &account_id {
        let mut probe = serde_json::Map::new();
        if let Some(f) = args.get("filter") {
            probe.insert("filter".into(), f.clone());
        }
        let filter: Option<MessageFilter> = filter_arg(&mut probe)?;
        let supports_threads = backend.limits(caller, account_id).supports_threads;
        check_message_filter(filter.as_ref(), supports_threads)?;
    }

    // §Message/query: "messages in channels the caller cannot view MUST be
    // omitted", so changes to such a Chat's messages are not reported.
    if let (Some(account_id), Some(chat_id)) = (account_id, chat_id)
        && !backend
            .may_view_chat(caller, &account_id, &chat_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::cannot_calculate_changes());
    }
    jmap_server::handle_query_changes::<Message, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// BroadcastMention validation
// ---------------------------------------------------------------------------

/// Validate broadcast mentions against spec §4.4.1 / §4.16 constraints.
///
/// Returns the SetError value on failure, None on success. §BroadcastMention:
/// "Servers MUST reject a BroadcastMention with an unrecognized `scope`
/// value; in `Message/set` the rejection is an `invalidProperties` SetError
/// naming `broadcastMentions`." A missing or non-string `scope` takes the
/// §Input Validation default, which is the same SetError.
fn validate_broadcast_mentions(broadcast_mentions: &[serde_json::Value]) -> Option<Value> {
    let invalid_properties = |desc: String| {
        json!({
            "type": "invalidProperties",
            "properties": ["broadcastMentions"],
            "description": desc,
        })
    };
    for (i, bm) in broadcast_mentions.iter().enumerate() {
        // scope validation
        let scope = match bm.get("scope").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return Some(invalid_properties(format!(
                    "broadcastMentions[{i}]: missing or non-string scope"
                )));
            }
        };
        if !BROADCAST_MENTION_SCOPES.contains(&scope) {
            return Some(invalid_properties(format!(
                "broadcastMentions[{i}]: unrecognized scope \"{scope}\""
            )));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Optional create properties
// ---------------------------------------------------------------------------

/// Default `bodyType` values that indicate an end-to-end encrypted body:
/// the one type draft-atwood-jmap-chat-00 §Message `body` names. A backend
/// adds others, such as `"application/mimi-fanout"`
/// (draft-atwood-jmap-chat-mimi-00 §MLS Messages), through
/// [`ChatLimits::encrypted_body_types`](crate::ChatLimits::encrypted_body_types).
pub const ENCRYPTED_BODY_TYPES: &[&str] = &["application/mls-ciphertext"];

/// Default `bodyType` values this server accepts, advertised as the
/// `supportedBodyTypes` capability (draft-atwood-jmap-chat-00
/// §Account-Level Capability Object). A backend lists other types through
/// [`ChatLimits::supported_body_types`](crate::ChatLimits::supported_body_types).
pub const SUPPORTED_BODY_TYPES: &[&str] = &[
    "text/plain",
    "text/markdown",
    "application/jmap-chat-rich",
    "application/mls-ciphertext",
];

/// draft-atwood-jmap-chat-00 §Fields Inside the Ciphertext: Message fields
/// that "MUST be empty or absent" when `bodyType` indicates an encrypted type.
const CIPHERTEXT_ONLY_FIELDS: [&str; 8] = [
    "mentions",
    "broadcastMentions",
    "attachments",
    "actions",
    "replyTo",
    "threadRootId",
    "topicId",
    "subject",
];

/// Check the create fields that `bodyType` requires to be empty: all of
/// [`CIPHERTEXT_ONLY_FIELDS`] when `body_type` is one of the account's
/// `encrypted` types (§Fields Inside the Ciphertext).
///
/// Returns every offending field, in [`CIPHERTEXT_ONLY_FIELDS`] order, for
/// an `invalidProperties` SetError "whose `properties` lists each such
/// field"; empty on success.
fn body_type_field_conflicts(
    obj: &Value,
    body_type: &str,
    encrypted: &[&str],
) -> Vec<&'static str> {
    if !encrypted.contains(&body_type) {
        return Vec::new();
    }
    let non_empty = |key: &str| {
        obj.get(key)
            .is_some_and(|v| !(v.is_null() || v == "" || v.as_array().is_some_and(Vec::is_empty)))
    };
    CIPHERTEXT_ONLY_FIELDS
        .into_iter()
        .filter(|&k| non_empty(k))
        .collect()
}

/// Check a `Message/set` edit `patch` of the stored `msg` against
/// `maxBodyBytes` and the encrypted-`bodyType` empty-field rule, which
/// apply to updates as well as creates.
///
/// draft-atwood-jmap-chat-00 §Account-Level Capability Object
/// `maxBodyBytes`: "in `Message/set` the rejection is a `tooLarge`
/// SetError." §Fields Inside the Ciphertext: "Servers MUST reject a
/// `Message/set` create or update that has an encrypted `bodyType` and any
/// of these fields non-empty". The field rule runs on the Message as it
/// would be after the patch, so a `bodyType`-only patch to an encrypted
/// type is rejected when the stored Message keeps such a field.
///
/// Returns the SetError, or `None` when the edit passes.
fn edit_limit_error(msg: &Message, patch: &Value, limits: &ChatLimits) -> Option<Value> {
    if let Some(body) = patch.get("body").and_then(Value::as_str)
        && u64::try_from(body.len()).map_or(true, |n| n > limits.max_body_bytes)
    {
        return Some(json!({
            "type": "tooLarge",
            "description": format!("body exceeds maxBodyBytes ({})", limits.max_body_bytes),
        }));
    }
    let mut merged = serde_json::to_value(msg).ok()?;
    if let (Some(dst), Some(src)) = (merged.as_object_mut(), patch.as_object()) {
        dst.extend(src.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    let body_type = merged.get("bodyType").and_then(Value::as_str)?.to_owned();
    let conflicts = body_type_field_conflicts(&merged, &body_type, limits.encrypted_body_types);
    (!conflicts.is_empty()).then(|| {
        json!({
            "type": "invalidProperties",
            "properties": conflicts,
            "description": format!("must be empty or absent when bodyType is {body_type}"),
        })
    })
}

/// Implementation-defined maximum byte length of `subject`
/// (draft-atwood-jmap-chat-00 §Input Validation).
const MAX_SUBJECT_BYTES: usize = 1024;

/// RFC 8620 §1.3 UnsignedInt upper bound, 2^53 - 1.
const MAX_UNSIGNED_INT: u64 = (1 << 53) - 1;

/// Parse the optional `Message/set` create properties `mentions`,
/// `broadcastMentions`, `actions`, `topicId`, `subject` and
/// `expiresAfterReadSeconds` from `obj`, applying the
/// draft-atwood-jmap-chat-00 §Input Validation checks, and return a
/// function that copies them onto a Message. An absent or `null`
/// property keeps the default. `broadcastMentions` must already have
/// passed [`validate_broadcast_mentions`].
///
/// On error, returns the wire name of the first offending property, for an
/// `invalidProperties` SetError.
fn apply_create_optionals(
    obj: &Value,
) -> Result<impl FnOnce(&mut Message) + Send + use<>, &'static str> {
    let get = |key: &str| obj.get(key).filter(|v| !v.is_null());
    let mentions = get("mentions")
        .map(|v| Vec::<Mention>::deserialize(v).map_err(|_| "mentions"))
        .transpose()?;
    let broadcast_mentions = get("broadcastMentions")
        .map(|v| Vec::<BroadcastMention>::deserialize(v).map_err(|_| "broadcastMentions"))
        .transpose()?;
    // §MessageAction: "Servers MUST store and forward MessageAction
    // records without inspection or transformation."
    let actions = get("actions")
        .map(|v| Vec::<MessageAction>::deserialize(v).map_err(|_| "actions"))
        .transpose()?;
    let topic_id = match get("topicId") {
        None => None,
        Some(v) => match v.as_str() {
            Some(s) if !s.is_empty() && s.len() <= 255 => Some(s.to_owned()),
            _ => return Err("topicId"),
        },
    };
    let subject = match get("subject") {
        None => None,
        Some(v) => match v.as_str() {
            Some(s) if topic_id.is_some() && s.len() <= MAX_SUBJECT_BYTES => Some(s.to_owned()),
            _ => return Err("subject"),
        },
    };
    let expires_after_read_seconds = match get("expiresAfterReadSeconds") {
        None => None,
        Some(v) => match v.as_u64() {
            Some(n) if n <= MAX_UNSIGNED_INT => Some(n),
            _ => return Err("expiresAfterReadSeconds"),
        },
    };
    Ok(move |msg: &mut Message| {
        if let Some(v) = mentions {
            msg.mentions = v;
        }
        if let Some(v) = broadcast_mentions {
            msg.broadcast_mentions = v;
        }
        if let Some(v) = actions {
            msg.actions = v;
        }
        if topic_id.is_some() {
            msg.topic_id = topic_id;
        }
        if subject.is_some() {
            msg.subject = subject;
        }
        if expires_after_read_seconds.is_some() {
            msg.expires_after_read_seconds = expires_after_read_seconds;
        }
    })
}

/// Build a `Message/set` create's `attachments` from its wire value `v`
/// for target Chat `chat`; `Err` holds the SetError value.
///
/// draft-atwood-jmap-chat-00 §Denial of Service: "Enforce
/// `maxAttachmentsPerMessage` at creation and update time"; it is checked
/// first, so no entry is resolved past the cap, and is `tooLarge`, the
/// error §Account-Level Capability Object names for `maxBodyBytes`. An
/// entry with `filenodeId` goes to [`ChatBackend::filenode_attachment`]
/// (draft-atwood-jmap-chat-filenode-00 §filenodeId). Any other entry, or
/// one the hook leaves unresolved, must carry every Attachment field. Each
/// result must pass [`valid_attachment`]. A malformed entry is
/// `invalidProperties` naming `attachments`.
async fn create_attachments<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    limits: &ChatLimits,
    chat: &Chat,
    v: &Value,
) -> Result<Vec<Attachment>, Value> {
    let invalid = || json!({ "type": "invalidProperties", "properties": ["attachments"] });
    let entries = v.as_array().ok_or_else(invalid)?;
    if u64::try_from(entries.len()).map_or(true, |n| n > limits.max_attachments_per_message) {
        return Err(json!({
            "type": "tooLarge",
            "description": format!(
                "attachments exceed maxAttachmentsPerMessage ({})",
                limits.max_attachments_per_message
            ),
        }));
    }
    let mut attachments = Vec::with_capacity(entries.len());
    for entry in entries {
        let requested = AttachmentCreate::deserialize(entry).map_err(|_| invalid())?;
        let filled = if requested.filenode_id.is_some() {
            match backend
                .filenode_attachment(caller, account_id, chat, &requested)
                .await
            {
                Ok(Ok(filled)) => filled,
                Ok(Err(set_err)) => return Err(set_error_value(&set_err)),
                Err(e) => return Err(server_fail_value_from_backend(&e)),
            }
        } else {
            None
        };
        let attachment = match filled {
            Some(a) => a,
            None => Attachment::deserialize(entry).map_err(|_| invalid())?,
        };
        if !valid_attachment(&attachment) {
            return Err(invalid());
        }
        attachments.push(attachment);
    }
    Ok(attachments)
}

/// draft-atwood-jmap-chat-00 §Attachment and §Input Validation:
/// `filename` holds no `/`, `\` or null byte and `contentType` is a
/// syntactically valid MIME type. `sha256` is lowercase hex by its type.
///
/// `size` and `sha256` are checked against the blob in [`check_attachments`].
fn valid_attachment(a: &Attachment) -> bool {
    !a.filename.contains(['/', '\\', '\0']) && valid_media_type(&a.content_type)
}

/// Enforce the size limit and blob checks on a `Message/set` create's
/// `attachments` from [`create_attachments`]; `None` on success, else the
/// SetError value.
///
/// draft-atwood-jmap-chat-00 §Denial of Service: "Enforce `maxBodyBytes`
/// and `maxAttachmentBytes` at parse time, before any fetch or storage."
/// The limit is `tooLarge`, the error §Account-Level Capability Object
/// names for `maxBodyBytes`. Then each Attachment's `size` and `sha256` are
/// checked against [`ChatBackend::attachment_blob`]; a mismatch is
/// `invalidProperties` naming `attachments` (§Input Validation).
async fn check_attachments<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    limits: &ChatLimits,
    attachments: &[Attachment],
) -> Option<Value> {
    if attachments
        .iter()
        .any(|a| a.size > limits.max_attachment_bytes)
    {
        return Some(json!({
            "type": "tooLarge",
            "description": format!(
                "an attachment exceeds maxAttachmentBytes ({})",
                limits.max_attachment_bytes
            ),
        }));
    }
    for a in attachments {
        match backend
            .attachment_blob(caller, account_id, &a.blob_id)
            .await
        {
            Ok(Ok(None)) => {}
            Ok(Ok(Some(blob))) => {
                // Constant-time digest comparison (compliance tests §14.13:
                // "Verify that blob hash comparison uses constant-time
                // comparison").
                if blob.size != a.size || !bool::from(a.sha256.to_bytes().ct_eq(&blob.sha256)) {
                    return Some(json!({
                        "type": "invalidProperties",
                        "properties": ["attachments"],
                        "description": format!(
                            "attachment {} does not match the blob's size or sha256",
                            a.blob_id
                        ),
                    }));
                }
            }
            Ok(Err(set_err)) => return Some(set_error_value(&set_err)),
            Err(e) => return Some(server_fail_value_from_backend(&e)),
        }
    }
    None
}

/// RFC 2045 §5.1 `type "/" subtype`, each an RFC 2045 `token`.
///
/// ponytail: parameters after `;` are not parsed; add an RFC 2045
/// parameter parser if they ever need validating.
fn valid_media_type(s: &str) -> bool {
    let is_token = |t: &str| {
        !t.is_empty()
            && t.bytes()
                .all(|b| b.is_ascii_graphic() && !b"()<>@,;:\\\"/[]?=".contains(&b))
    };
    let essence = s.split(';').next().unwrap_or_default().trim();
    essence
        .split_once('/')
        .is_some_and(|(t, sub)| is_token(t) && is_token(sub))
}

// ---------------------------------------------------------------------------
// Message/set
// ---------------------------------------------------------------------------

/// Create the owner's ReadPosition for each Chat in `chat_ids` that has
/// none yet. draft-atwood-jmap-chat-00 §ReadPosition: "For direct and
/// group Chats, a ReadPosition is created when the first message is
/// delivered. For channel Chats, a ReadPosition is created when the
/// owner joins the containing Space." Called by `Message/set` and by
/// `Space/join` ([`crate::space::handle_space_join`]).
///
/// Best effort: the Messages are already stored, so a backend failure
/// here must not become a method-level error (RFC 8620 §3.6.2: "the
/// externally visible state of the server MUST NOT have changed if an
/// error is returned at the method level"). A record missed here is
/// created by the next delivery to that Chat. `alreadyExists` means a
/// concurrent delivery created it first (see the ReadPosition uniqueness
/// contract on `ChatBackend::create_object`).
///
/// ponytail: does not look up the Chat's kind, so `Message/set` also
/// fills in a missing channel position (for example a channel added
/// after the owner joined). One full ReadPosition scan per `Message/set` call that created a
/// message; a backend with a `(account, chatId)` index can replace it.
pub(crate) async fn ensure_read_positions<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    mut chat_ids: Vec<Id>,
) {
    if chat_ids.is_empty() {
        return;
    }
    let Ok((existing, _)) = backend
        .get_objects::<ReadPosition>(caller, account_id, None, None)
        .await
    else {
        return;
    };
    chat_ids.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    chat_ids.dedup();
    chat_ids.retain(|c| !existing.iter().any(|p| p.chat_id == *c));
    for chat_id in chat_ids {
        let _ = backend
            .create_object(caller, account_id, chat_id.as_ref(), |id: &Id| {
                ReadPosition::new(id.clone(), chat_id.clone())
            })
            .await;
    }
}

/// Find the Message in `chat_id` whose `senderMsgId` is `sender_msg_id`.
///
/// draft-atwood-jmap-chat-00 §Message: "Servers MUST maintain a durable
/// index of `senderMsgId` values per chat to support idempotent
/// delivery, `Peer/retract` lookup, and resolution of `replyTo` /
/// `threadRootId` references." The same `senderMsgId` in another Chat is
/// a different Message.
///
/// ponytail: one full Message scan of the account per call, not an
/// index. A backend with a `(chatId, senderMsgId)` index can serve the
/// same contract from it once this becomes a hot path.
///
/// # Errors
///
/// Returns the backend error if reading the account's Messages fails.
pub async fn find_message_by_sender_msg_id<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    chat_id: &Id,
    sender_msg_id: &Id,
) -> Result<Option<Message>, B::Error> {
    let (messages, _) = backend
        .get_objects::<Message>(caller, account_id, None, None)
        .await?;
    Ok(messages
        .into_iter()
        .find(|m| m.chat_id == *chat_id && m.sender_msg_id == *sender_msg_id))
}

/// The result of [`store_server_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreOutcome {
    /// The Message was stored under this server-assigned id.
    Stored(Id),
    /// The Chat already holds a Message with the same `senderMsgId`; this
    /// is its id, and nothing was written.
    Duplicate(Id),
}

/// Store a Message that the server, not a client, creates: for example a
/// MIMI fanout Message (draft-atwood-jmap-chat-mimi-00
/// §application/mimi-fanout) or a federation `Peer/deliver` Message.
///
/// `build` makes the Message for the id the backend assigns, as for
/// [`ChatBackend::create_object`]. It sets every other field, including
/// the server-set `senderMsgId`, `senderId`, `sentAt` and `receivedAt` and
/// a body type that [`ChatLimits::server_reserved_body_types`] keeps out
/// of `Message/set`. Its `chatId` and `senderMsgId` are then overwritten
/// with `chat_id` and `sender_msg_id`, so the stored Message always matches
/// the duplicate check. None of the `Message/set` client-input checks run.
///
/// Before storing, `chat_id` is searched for `sender_msg_id` with
/// [`find_message_by_sender_msg_id`]. A hit returns
/// [`StoreOutcome::Duplicate`] and writes nothing. The mimi draft
/// requires this: "The server MUST NOT store a FanoutMessage whose
/// `senderMsgId` it has already stored in the Chat". After a store, the
/// owner's ReadPosition for the Chat is created if missing and the
/// Chat's `unreadCount` is recomputed, as for `Message/set`
/// (draft-atwood-jmap-chat-00 §ReadPosition: "For direct and group
/// Chats, a ReadPosition is created when the first message is
/// delivered.").
///
/// ponytail: the duplicate check and the create are separate backend
/// calls. Two concurrent calls with one `senderMsgId` can both store, so
/// a caller that can race (such as parallel `/notify` deliveries)
/// serializes calls per Chat.
///
/// # Errors
///
/// Returns the backend's error from the lookup (as
/// [`BackendSetError::Other`]) or from
/// [`ChatBackend::create_object`], unchanged.
// Same error type as `ChatBackend::create_object`, passed through unboxed.
pub async fn store_server_message<B, F>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    chat_id: &Id,
    sender_msg_id: &Id,
    build: F,
) -> Result<StoreOutcome, BackendSetError<B::Error>>
where
    B: ChatBackend,
    F: FnOnce(&Id) -> Message + Send,
{
    if let Some(existing) =
        find_message_by_sender_msg_id(backend, caller, account_id, chat_id, sender_msg_id)
            .await
            .map_err(BackendSetError::Other)?
    {
        return Ok(StoreOutcome::Duplicate(existing.id));
    }
    let (chat, smid) = (chat_id.clone(), sender_msg_id.clone());
    let (id, stored) = backend
        .create_object(
            caller,
            account_id,
            sender_msg_id.as_ref(),
            move |id: &Id| {
                let mut m = build(id);
                m.chat_id = chat;
                m.sender_msg_id = smid;
                m
            },
        )
        .await?;
    ensure_read_positions(backend, caller, account_id, vec![stored.chat_id.clone()]).await;
    record_unread_count_changes(backend, caller, account_id, vec![stored.chat_id]).await;
    Ok(StoreOutcome::Stored(id))
}

/// The owner's effective receipt-sharing preference for `chat_id`.
///
/// draft-atwood-jmap-chat-federation-00 §Peer/receipt "Sender Behavior":
/// "if `Chat.receiptSharing` is present for the relevant Chat, that value
/// is used; otherwise the account-level `PresenceStatus.receiptSharing`
/// applies". A missing PresenceStatus takes the draft-atwood-jmap-chat-00
/// default, `true`. A failed lookup yields `false`: the read has already
/// been written, and a receipt not shown to be permitted is not sent.
async fn shares_receipts<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    chat_id: &Id,
) -> bool {
    let chat_override = match backend
        .get_objects::<Chat>(
            caller,
            account_id,
            Some(std::slice::from_ref(chat_id)),
            None,
        )
        .await
    {
        Ok((found, _not_found)) => found.first().and_then(|c| c.receipt_sharing),
        Err(_) => return false,
    };
    if let Some(shares) = chat_override {
        return shares;
    }
    match backend
        .get_objects::<PresenceStatus>(caller, account_id, None, None)
        .await
    {
        Ok((found, _not_found)) => found.first().is_none_or(|p| p.receipt_sharing),
        Err(_) => false,
    }
}

/// The Message create properties that hold the id of another record, which a
/// client may set to a `#creationId` reference (RFC 8620 §5.3).
const MESSAGE_ID_PROPERTIES: &[&str] = &["chatId", "replyTo", "threadRootId"];

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
///
/// A `chatId`, `replyTo` or `threadRootId` that is a `#creationId`
/// reference names a record created earlier in the request and resolves
/// against `created_ids` (pass [`jmap_server::CallContext::created_ids`]) or
/// against a create of this call, which runs replied-to messages first (RFC
/// 8620 §5.3). An unknown reference is `invalidProperties` naming the
/// property.
pub async fn handle_message_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
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
        .get_state::<Message>(caller, &account_id)
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
    let limits = backend.limits(caller, &account_id);

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        let mut delivered_chat_ids: Vec<Id> = Vec::new();
        // RFC 8620 §5.3: creates of this call join the request-wide map so
        // later creates in this call can reference them.
        let mut created_ids = created_ids.clone();
        for (create_id, obj_val) in parents_first(create_map, &["replyTo", "threadRootId"]) {
            let resolved = match resolved_create(obj_val, MESSAGE_ID_PROPERTIES, &created_ids) {
                Ok(v) => v,
                Err(err) => {
                    not_created.insert(create_id.clone(), set_error_value(&err));
                    continue;
                }
            };
            let obj_val = &resolved;
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Message>) {
                not_created.insert(create_id.clone(), set_error_value(&err));
                continue;
            }
            // Optional properties: a wrongly-typed value is invalidProperties
            // (RFC 8620 §5.3), not a silent absence.
            if let Some(err) = mistyped_create_props(
                obj_val,
                &[
                    ("broadcastMentions", PropType::Array),
                    ("replyTo", PropType::Str),
                    ("threadRootId", PropType::Str),
                    ("senderExpiresAt", PropType::Str),
                    ("burnOnRead", PropType::Bool),
                ],
            ) {
                not_created.insert(create_id.clone(), err);
                continue;
            }
            let Some(chat_id_str) = obj_val.get("chatId").and_then(|v| v.as_str()) else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["chatId"] }),
                );
                continue;
            };
            let chat_id = match jmap_server::parse_wire_id(chat_id_str, "chatId") {
                Ok(id) => id,
                Err(err) => {
                    not_created.insert(create_id.clone(), set_error_value(&err));
                    continue;
                }
            };

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
            // §Account-Level Capability Object `maxBodyBytes`: "Servers
            // MUST reject messages exceeding this limit; in `Message/set`
            // the rejection is a `tooLarge` SetError."
            if u64::try_from(body.len()).map_or(true, |n| n > limits.max_body_bytes) {
                not_created.insert(
                    create_id.clone(),
                    json!({
                        "type": "tooLarge",
                        "description": format!("body exceeds maxBodyBytes ({})", limits.max_body_bytes),
                    }),
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

            // §Input Validation: "`bodyType`: reject a value not listed in
            // `supportedBodyTypes`; in `Message/set` create, also reject a
            // type that a companion specification reserves for
            // server-created Messages."
            let Some(body_type) = obj_val
                .get("bodyType")
                .filter(|v| !v.is_null())
                .map_or(Some("text/plain"), Value::as_str)
                .filter(|s| limits.supported_body_types.contains(s))
            else {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "invalidProperties", "properties": ["bodyType"] }),
                );
                continue;
            };
            // draft-atwood-jmap-chat-mimi-00 §application/mimi-fanout, the
            // one reserving companion: "the server MUST reject a
            // `Message/set` create with this body type with an
            // `invalidProperties` SetError naming `bodyType`."
            if limits.server_reserved_body_types.contains(&body_type) {
                not_created.insert(
                    create_id.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": ["bodyType"],
                        "description": format!("bodyType {body_type} is reserved for server-created Messages")
                    }),
                );
                continue;
            }
            let body_type = body_type.to_owned();

            // §Fields Inside the Ciphertext: "an `invalidProperties`
            // SetError whose `properties` lists each such field".
            let conflicts =
                body_type_field_conflicts(obj_val, &body_type, limits.encrypted_body_types);
            if !conflicts.is_empty() {
                not_created.insert(
                    create_id.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": conflicts,
                        "description": format!("must be empty or absent when bodyType is {body_type}"),
                    }),
                );
                continue;
            }

            // BroadcastMention structural validation (§4.4.1 / §4.16).
            let broadcast_mentions_val: Vec<Value> = obj_val
                .get("broadcastMentions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if let Some(err) = validate_broadcast_mentions(&broadcast_mentions_val) {
                not_created.insert(create_id.clone(), err);
                continue;
            }

            // draft-atwood-jmap-chat-00 §Input Validation: "`chatId`:
            // verify per {{chat-id}} — confirm match with stored id"; a
            // failed check is "an `invalidProperties` SetError ... whose
            // `properties` names the offending property".
            let chat = match backend
                .get_objects::<Chat>(
                    caller,
                    &account_id,
                    Some(std::slice::from_ref(&chat_id)),
                    None,
                )
                .await
            {
                Ok((found, _)) => match found.into_iter().next() {
                    Some(chat) => chat,
                    None => {
                        not_created.insert(
                            create_id.clone(),
                            json!({
                                "type": "invalidProperties",
                                "properties": ["chatId"],
                                "description": format!("no Chat with id {chat_id}"),
                            }),
                        );
                        continue;
                    }
                },
                Err(e) => {
                    not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
                    continue;
                }
            };

            match backend.may_create_message(caller, &account_id, &chat).await {
                Ok(Ok(())) => {}
                Ok(Err(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                    continue;
                }
                Err(e) => {
                    not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
                    continue;
                }
            }

            // draft-atwood-jmap-chat-00 §Chat `closedAt`: "After it is
            // set, the server MUST reject with a `forbidden` SetError
            // every `Message/set` create in this Chat".
            if chat.closed_at.is_some() {
                not_created.insert(
                    create_id.clone(),
                    json!({ "type": "forbidden", "description": "the group is closed" }),
                );
                continue;
            }

            // draft-atwood-jmap-chat-00 §Space `closedAt`: "After it is
            // set, the Space's channels are read-only: the server MUST
            // reject every `Message/set` create in them with a
            // `forbidden` SetError."
            if let Some(sid) = &chat.space_id {
                match backend
                    .get_objects::<Space>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(sid)),
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
            }

            // draft-atwood-jmap-chat-00 §Message/set: the sender MUST be a
            // current member of the target Chat and, in a channel, hold
            // `"send"`; otherwise `forbidden`.
            match backend
                .may_send_message(caller, &account_id, &chat_id)
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(set_err)) => {
                    not_created.insert(create_id.clone(), set_error_value(&set_err));
                    continue;
                }
                Err(e) => {
                    not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
                    continue;
                }
            }

            // BroadcastMention permission check (§4.4.1).
            // Non-empty broadcastMentions in a Space-backed channel
            // requires mention_broadcast permission; rich-body spans are
            // display-only and never consulted (§Rich Body Format).
            // Group/direct chats (no spaceId) are unrestricted per spec.
            if let Some(sid) = chat
                .space_id
                .as_ref()
                .filter(|_| !broadcast_mentions_val.is_empty())
            {
                match backend
                    .may_broadcast_mention(caller, &account_id, sid, &chat_id)
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(set_err)) => {
                        not_created.insert(create_id.clone(), set_error_value(&set_err));
                        continue;
                    }
                    Err(e) => {
                        not_created.insert(create_id.clone(), server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            let reply_to: Option<Id> = match obj_val.get("replyTo").and_then(|v| v.as_str()) {
                Some(s) => match jmap_server::parse_wire_id(s, "replyTo") {
                    Ok(id) => Some(id),
                    Err(err) => {
                        not_created.insert(create_id.clone(), set_error_value(&err));
                        continue;
                    }
                },
                None => None,
            };

            let supplied_root: Option<Id> =
                match obj_val.get("threadRootId").and_then(|v| v.as_str()) {
                    Some(s) => match jmap_server::parse_wire_id(s, "threadRootId") {
                        Ok(id) => Some(id),
                        Err(err) => {
                            not_created.insert(create_id.clone(), set_error_value(&err));
                            continue;
                        }
                    },
                    None => None,
                };

            // §Message `replyTo`: "Servers MUST validate that this ID
            // exists in the same Chat before storing." §Thread Model:
            // the root of a reply is the referenced message's
            // `threadRootId`, or the referenced message itself when that
            // is absent; a message with no `replyTo` has no root.
            // "Servers SHOULD validate them and MAY correct
            // `threadRootId`": a wrong value is rejected and an omitted
            // one is filled in. §Input Validation names no other error,
            // so both failures are `invalidProperties`.
            let expected_root: Option<Id> = match reply_to.as_ref() {
                None => None,
                Some(parent_id) => {
                    let parent = match backend
                        .get_objects::<Message>(
                            caller,
                            &account_id,
                            Some(std::slice::from_ref(parent_id)),
                            None,
                        )
                        .await
                    {
                        Ok((found, _)) => found.into_iter().find(|m| m.chat_id == chat_id),
                        Err(e) => {
                            not_created
                                .insert(create_id.clone(), server_fail_value_from_backend(&e));
                            continue;
                        }
                    };
                    let Some(parent) = parent else {
                        not_created.insert(
                            create_id.clone(),
                            json!({
                                "type": "invalidProperties",
                                "properties": ["replyTo"],
                                "description": "replyTo is not a Message in this Chat",
                            }),
                        );
                        continue;
                    };
                    Some(parent.thread_root_id.unwrap_or(parent.id))
                }
            };
            if supplied_root.is_some() && supplied_root != expected_root {
                not_created.insert(
                    create_id.clone(),
                    json!({
                        "type": "invalidProperties",
                        "properties": ["threadRootId"],
                        "description": "threadRootId does not match the thread of replyTo",
                    }),
                );
                continue;
            }
            let thread_root_id = expected_root;

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

            let now_str = now_utc_string();
            let received_at: UTCDate = UTCDate::from(now_str.as_ref());

            let mut attachments = Vec::new();
            if let Some(v) = obj_val.get("attachments").filter(|v| !v.is_null()) {
                match create_attachments(backend, caller, &account_id, &limits, &chat, v).await {
                    Ok(a) => attachments = a,
                    Err(err) => {
                        not_created.insert(create_id.clone(), err);
                        continue;
                    }
                }
            }
            let apply_optionals = match apply_create_optionals(obj_val) {
                Ok(f) => f,
                Err(property) => {
                    not_created.insert(
                        create_id.clone(),
                        json!({ "type": "invalidProperties", "properties": [property] }),
                    );
                    continue;
                }
            };
            if let Some(err) =
                check_attachments(backend, caller, &account_id, &limits, &attachments).await
            {
                not_created.insert(create_id.clone(), err);
                continue;
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
            // `rateLimit` SetError, built via the typed
            // `SetError::with_extra` builder added in bd:JMAP-dha0.
            // The spec requires "a `rateLimit` SetError ({{RFC8620,
            // Section 5.3}}) on the offending `Message/set` create",
            // not the method-level `rateLimited` (which is reserved
            // for the Peer/* methods). `serverRetryAfter` is a
            // workspace extension field. The reference `MemoryBackend`
            // never returns an Err here.
            if let Err(slow_err) = backend.slow_mode_check(caller, &account_id, &chat_id).await {
                let retry_after_str: String = slow_err.retry_after.as_ref().to_owned();
                let set_err = SetError::new(SetErrorType::RateLimit)
                    .with_description("Slow mode is active for this chat")
                    .with_extra("serverRetryAfter", json!(retry_after_str));
                not_created.insert(create_id.clone(), set_error_value(&set_err));
                continue;
            }

            // draft-atwood-jmap-chat-00 §Message `senderMsgId`: "Equals
            // `id` for messages composed by the owner."
            let build = |id: &Id| {
                let mut msg = Message::new(
                    id.clone(),
                    id.clone(),
                    account_id.clone(),
                    chat_id,
                    body,
                    body_type,
                    sent_at,
                    received_at,
                    DeliveryState::Pending,
                );
                msg.reply_to = reply_to;
                msg.thread_root_id = thread_root_id;
                msg.sender_expires_at = sender_expires_at;
                msg.burn_on_read = burn_on_read;
                msg.attachments = attachments;
                apply_optionals(&mut msg);
                msg
            };
            match backend
                .create_object::<Message, _>(caller, &account_id, create_id, build)
                .await
            {
                Ok((server_id, created_obj)) => {
                    mutated = true;
                    created_ids.insert(Id::from(create_id.as_str()), server_id);
                    delivered_chat_ids.push(created_obj.chat_id.clone());
                    // draft-atwood-jmap-chat-00 §Message/set: the server
                    // sets the server-assigned fields "then enqueues the
                    // message for outbound delivery".
                    let event = OutboundEvent::NewMessage {
                        chat_id: created_obj.chat_id.clone(),
                        message_id: created_obj.id.clone(),
                        sender_msg_id: created_obj.sender_msg_id.clone(),
                    };
                    backend.on_outbound_event(caller, &account_id, &event).await;
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
        ensure_read_positions(backend, caller, &account_id, delivered_chat_ids.clone()).await;
        record_unread_count_changes(backend, caller, &account_id, delivered_chat_ids).await;
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };

            // RFC 8620 §5.3: "There MUST NOT be two patches in the
            // PatchObject where the pointer of one is the prefix of the
            // pointer of the other" or the update "MUST be rejected with an
            // "invalidPatch" error".
            if patch_val.get("reactions").is_some()
                && patch_val
                    .as_object()
                    .is_some_and(|obj| obj.keys().any(|k| k.starts_with("reactions/")))
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

            // draft-atwood-jmap-chat-00 §Message/set defines the only
            // client updates: Editing (`body`, `bodyType`, `mentions`,
            // `broadcastMentions`), Adding and Removing Reactions
            // (`reactions/<senderReactionId>` JSON Pointer keys), Deleting
            // (`deletedAt`, `deletedForAll`) and Marking as Read (`readAt`,
            // `readDisposition`): the client-settable Message properties
            // that are not immutable, with `reactions` changed only per
            // entry. Every other key names a server-set or immutable
            // property, so it is rejected with RFC 8620 §5.3
            // `invalidProperties`.
            let bad_props: Vec<&str> = patch_val
                .as_object()
                .map(|obj| {
                    obj.keys()
                        .map(String::as_str)
                        .filter(|&k| {
                            !(k.starts_with("reactions/")
                                || (k != "reactions" && is_client_updatable::<Message>(k)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !bad_props.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": bad_props }),
                );
                continue;
            }

            // draft-atwood-jmap-chat-00 §Message `readDisposition`: "the
            // server MUST reject an update that sets `readDisposition`
            // without `readAt` with an `invalidProperties` SetError naming
            // `readDisposition`." A null `readAt` clears it, not sets it.
            let sets = |field: &str| patch_val.get(field).is_some_and(|v| !v.is_null());
            if sets("readDisposition") && !sets("readAt") {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": ["readDisposition"] }),
                );
                continue;
            }

            // §Input Validation: "`bodyType`: reject a value not listed in
            // `supportedBodyTypes`".
            if patch_val.get("bodyType").is_some_and(|v| {
                !v.as_str()
                    .is_some_and(|s| limits.supported_body_types.contains(&s))
            }) {
                not_updated.insert(
                    id_str,
                    json!({ "type": "invalidProperties", "properties": ["bodyType"] }),
                );
                continue;
            }

            // The stored Message and its Chat, read once for every check
            // below. A Message or Chat that does not resolve is left for
            // update_object to surface `notFound`.
            let stored = match backend
                .get_objects::<Message>(caller, &account_id, Some(std::slice::from_ref(&id)), None)
                .await
            {
                Ok((found, _)) => found.into_iter().next(),
                Err(e) => {
                    not_updated.insert(id_str, server_fail_value_from_backend(&e));
                    continue;
                }
            };
            let stored_chat = match &stored {
                Some(msg) => match backend
                    .get_objects::<Chat>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(&msg.chat_id)),
                        None,
                    )
                    .await
                {
                    Ok((found, _)) => found.into_iter().next(),
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                },
                None => None,
            };

            // Per-Chat restrictions of extension-bound Chats; see
            // `ChatBackend::may_update_message`.
            if let (Some(msg), Some(chat)) = (&stored, &stored_chat) {
                let patch_keys: Vec<&str> = patch_val
                    .as_object()
                    .map(|obj| obj.keys().map(String::as_str).collect())
                    .unwrap_or_default();
                match backend
                    .may_update_message(caller, &account_id, chat, msg, &patch_keys)
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(set_err)) => {
                        not_updated.insert(id_str, set_error_value(&set_err));
                        continue;
                    }
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // draft-atwood-jmap-chat-00 §Deleting a Message: servers MUST
            // reject `deletedForAll: true` when `senderId` is not the
            // accountId. §Editing a Message: an edit of `body`, `bodyType`,
            // `mentions` or `broadcastMentions` is only permitted where
            // `senderId` is the accountId and `deletedAt` is absent; any other edit is
            // `forbidden`. A missing target falls through to the normal
            // `notFound` from `update_object`.
            let deletes_for_all =
                patch_val.get("deletedForAll").and_then(|v| v.as_bool()) == Some(true);
            let edits = ["body", "bodyType", "mentions", "broadcastMentions"]
                .iter()
                .any(|&field| patch_val.get(field).is_some());
            // The event for `ChatBackend::on_outbound_event`, built from
            // the stored Message once the checks below pass.
            let mut outbound: Option<OutboundEvent> = None;
            if deletes_for_all || edits {
                let description = stored.as_ref().and_then(|m| {
                    if deletes_for_all && m.sender_id != account_id {
                        Some("deletedForAll is only permitted on messages whose senderId is the accountId")
                    } else if edits && (m.sender_id != account_id || m.deleted_at.is_some()) {
                        Some("edits are only permitted on messages whose senderId is the accountId and deletedAt is absent")
                    } else {
                        None
                    }
                });
                if let Some(description) = description {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "forbidden", "description": description }),
                    );
                    continue;
                }
                if let Some(err) = stored
                    .as_ref()
                    .and_then(|m| edit_limit_error(m, &patch_val, &limits))
                {
                    not_updated.insert(id_str, err);
                    continue;
                }
                outbound = stored.as_ref().map(|m| {
                    let (chat_id, message_id, sender_msg_id) =
                        (m.chat_id.clone(), m.id.clone(), m.sender_msg_id.clone());
                    if deletes_for_all {
                        OutboundEvent::Retract {
                            chat_id,
                            message_id,
                            sender_msg_id,
                        }
                    } else {
                        OutboundEvent::Edit {
                            chat_id,
                            message_id,
                            sender_msg_id,
                        }
                    }
                });
            }

            // Build augmented patch with server-side timestamps.
            let mut augmented = patch_val;
            if augmented.get("body").is_some()
                && let Some(obj) = augmented.as_object_mut()
            {
                obj.insert("editedAt".to_owned(), json!(now_utc_string()));
            }
            // draft-atwood-jmap-chat-00 §Message deletedAt: a deleted
            // Message has an empty `body`, empty `attachments`, and no
            // `editHistory`, whether the deletion is local or for all. The
            // merge-patch null purges the stored revisions rather than
            // merely hiding them.
            let deletes =
                deletes_for_all || augmented.get("deletedAt").is_some_and(|v| !v.is_null());
            if deletes && let Some(obj) = augmented.as_object_mut() {
                if deletes_for_all {
                    obj.insert("deletedAt".to_owned(), json!(now_utc_string()));
                }
                obj.insert("body".to_owned(), json!(""));
                obj.insert("attachments".to_owned(), json!([]));
                obj.insert("editHistory".to_owned(), Value::Null);
            }
            // Spec §557 + §1029: when readAt is set without readDisposition, default to "displayed".
            // draft-atwood-jmap-chat-00 §Message `readDisposition`: "Absent
            // when `readAt` is not set." so clearing readAt clears it too.
            if let Some(obj) = augmented.as_object_mut() {
                match obj.get("readAt") {
                    Some(Value::Null) => {
                        obj.insert("readDisposition".to_owned(), Value::Null);
                    }
                    Some(_) if !obj.contains_key("readDisposition") => {
                        obj.insert("readDisposition".to_owned(), json!("displayed"));
                    }
                    _ => {}
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
            // The backend applies these paths into the stored
            // `reactions` map. Before forwarding, the handler:
            //
            // 1. Rejects `reactions/{id}` keys containing `/` or `~`
            //    (RFC 6901 escape characters — the chat-client also
            //    rejects them; this is defense-in-depth).
            // 2. Rejects non-null non-object values as `invalidPatch`.
            // 3. Pre-fetches the Message and rejects with `forbidden`
            //    if the patched key targets an existing reaction whose
            //    `senderId` is not the accountId — per spec, only the
            //    original sender may modify their own reactions.
            // 4. Server-overrides `senderId` to the accountId on every add
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

            // One `OutboundEvent::ReactionUpdate` per patched key, built
            // from the pre-fetched Message and reported after the write.
            let mut reaction_events: Vec<OutboundEvent> = Vec::new();
            if !reaction_pointer_keys.is_empty() {
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

                // Inspect the stored Message's existing reactions. An
                // update target that doesn't exist on the backend will
                // surface a normal `notFound` from the subsequent
                // `update_object` call — we don't short-circuit here.
                let mut forbidden: Vec<String> = Vec::new();
                if let Some(msg) = &stored {
                    for (raw_key, suffix) in &decoded {
                        let existing = msg.reactions.get(suffix);
                        if existing.is_some_and(|r| r.sender_id != account_id) {
                            forbidden.push(suffix.clone());
                        }
                        // A null on a key with no stored reaction
                        // removes nothing, so nothing propagates.
                        let action = match augmented.get(raw_key) {
                            Some(Value::Object(_)) => Some(ReactionAction::Add),
                            _ => existing.map(|r| ReactionAction::Remove { removed: r.clone() }),
                        };
                        if let Some(action) = action {
                            reaction_events.push(OutboundEvent::ReactionUpdate {
                                chat_id: msg.chat_id.clone(),
                                message_id: msg.id.clone(),
                                sender_msg_id: msg.sender_msg_id.clone(),
                                sender_reaction_id: suffix.clone(),
                                action,
                            });
                        }
                    }
                }
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

                // Server-override `senderId` and inject `sentAt` where
                // absent on every `reactions/{key}` add, so the stored
                // Reaction round-trips through the typed shape.
                let now_str = now_utc_string();
                if let Some(obj) = augmented.as_object_mut() {
                    for (raw_key, _) in &decoded {
                        if let Some(Value::Object(map)) = obj.get_mut(raw_key) {
                            // Spec MUST: server overrides senderId to
                            // the accountId on every add, regardless of
                            // what the client supplied.
                            map.insert("senderId".to_owned(), json!(account_id));
                            map.entry("sentAt").or_insert_with(|| json!(now_str));
                        }
                    }
                }
            }

            // Detect a non-null readAt assignment in the patch. A
            // `readAt: null` is a PatchObject clear (RFC 8620 §5.3), not a
            // "mark as read" event, so it does not trigger burn-on-read.
            // draft-atwood-jmap-chat-00 §Message `burnOnRead`: the
            // receiving server MUST hard-delete immediately after setting
            // readAt on a message whose `burnOnRead` is `true`.
            let patch_sets_read_at = augmented.get("readAt").is_some_and(|v| !v.is_null());

            // If the patch sets readAt, the stored Message decides whether
            // it is burn-on-read. We deliberately consult the pre-patch
            // state: a recipient setting readAt for the first time should
            // fire the burn. draft-atwood-jmap-chat-00 §Message
            // `burnOnRead`: "Applies only to the receiving mailbox; the
            // sender's own copy is not affected."
            let pre_patch_read: Option<Message> = if patch_sets_read_at {
                stored.clone()
            } else {
                None
            };
            let pre_patch_burn_on_read = pre_patch_read
                .as_ref()
                .is_some_and(|m| m.burn_on_read == Some(true) && m.sender_id != account_id);

            // BroadcastMention validation and permission check on edit
            // (§4.4.1 / §4.16). The post-patch broadcastMentions fall back
            // to the stored message where the patch leaves them unchanged,
            // so an edit that retains a broadcast mention is gated like one
            // that adds it.
            if edits {
                // A missing Message is left for update_object to surface
                // the normal notFound error downstream.
                let current_has_bm = stored
                    .as_ref()
                    .is_some_and(|msg| !msg.broadcast_mentions.is_empty());
                // Structural validation only when broadcastMentions is part
                // of the patch.
                let has_bm = match augmented.get("broadcastMentions") {
                    Some(bm) => {
                        let bm_arr: Vec<Value> = bm.as_array().cloned().unwrap_or_default();
                        if let Some(err) = validate_broadcast_mentions(&bm_arr) {
                            not_updated.insert(id_str, err);
                            continue;
                        }
                        !bm_arr.is_empty()
                    }
                    None => current_has_bm,
                };

                // BroadcastMention permission check on update (§4.4.1).
                // A post-patch broadcastMentions entry in a Space-backed
                // channel requires mention_broadcast permission; rich-body
                // spans are display-only and never consulted. Group/direct
                // chats (no spaceId) are unrestricted per spec.
                // A Message or Chat not found is left for update_object
                // to surface downstream.
                let channel = stored_chat
                    .as_ref()
                    .and_then(|c| Some((c.space_id.as_ref()?, &c.id)));
                if let (true, Some((sid, chat_id))) = (has_bm, channel) {
                    match backend
                        .may_broadcast_mention(caller, &account_id, sid, chat_id)
                        .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(set_err)) => {
                            not_updated.insert(id_str, set_error_value(&set_err));
                            continue;
                        }
                        Err(e) => {
                            not_updated.insert(id_str, server_fail_value_from_backend(&e));
                            continue;
                        }
                    }
                }
            }

            // draft-atwood-jmap-chat-federation-00 §Peer/receipt: the
            // owner read a Message another participant sent. The read
            // fields come from the patch, since a burned Message is gone.
            let receipt = pre_patch_read
                .filter(|m| m.sender_id != account_id)
                .and_then(|m| {
                    let read_at =
                        serde_json::from_value::<UTCDate>(augmented.get("readAt")?.clone()).ok()?;
                    let read_disposition = augmented
                        .get("readDisposition")
                        .and_then(|v| serde_json::from_value(v.clone()).ok());
                    Some(OutboundEvent::Receipt {
                        chat_id: m.chat_id,
                        message_id: m.id,
                        sender_msg_id: m.sender_msg_id,
                        read_at,
                        read_disposition,
                    })
                });
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
            // draft-atwood-jmap-chat-00 §Message `burnOnRead`: "MUST
            // permanently hard-delete this message (row removal, not
            // tombstone) immediately after setting `readAt`." One atomic
            // backend call, so a failure commits nothing and a retry
            // burns again.
            // draft-atwood-jmap-chat-federation-00 §Peer/retract "Sender
            // Behavior": send `Peer/retract` "before marking the local
            // record".
            if let Some(event @ OutboundEvent::Retract { .. }) = &outbound {
                backend.on_outbound_event(caller, &account_id, event).await;
            }
            let burns = patch_sets_read_at && pre_patch_burn_on_read;
            let update_outcome = if burns {
                backend
                    .read_and_burn_message(caller, &account_id, &id, patch)
                    .await
                    .map(|()| None)
            } else {
                backend
                    .update_object::<Message>(caller, &account_id, &id, patch)
                    .await
            };
            let maybe_updated_obj = match update_outcome {
                Ok(maybe_obj) => maybe_obj,
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
                    continue;
                }
            };

            // Update (or burn) succeeded.
            mutated = true;
            // draft-atwood-jmap-chat-00 §Editing a Message: "Send
            // `Peer/deliver` carrying an `edit` payload to all
            // recipients". A patch that also deletes or burns the Message
            // leaves nothing to deliver.
            if let Some(event @ OutboundEvent::Edit { .. }) = &outbound
                && !deletes
                && !burns
            {
                backend.on_outbound_event(caller, &account_id, event).await;
            }
            // draft-atwood-jmap-chat-00 §Adding and Removing Reactions:
            // propagate each add and removal "via `Peer/deliver`
            // `reactionUpdate` payload". As for edits, a patch that also
            // deletes or burns the Message leaves nothing to deliver.
            if !deletes && !burns {
                for event in &reaction_events {
                    backend.on_outbound_event(caller, &account_id, event).await;
                }
            }
            // §Peer/receipt "Sender Behavior": "If the effective preference
            // is `false`, the server MUST NOT invoke `Peer/receipt`". A
            // burned Message was still read, so its receipt is reported.
            if let Some(event @ OutboundEvent::Receipt { chat_id, .. }) = &receipt
                && shares_receipts(backend, caller, &account_id, chat_id).await
            {
                backend.on_outbound_event(caller, &account_id, event).await;
            }
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
        // The destroyed Messages' Chats, read before the destroy, for
        // `record_unread_count_changes` (§Chat `unreadCount`).
        // A malformed id is skipped here and rejected per entry below.
        let destroy_ids: Vec<Id> = destroy_arr
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|s| Id::new_validated(s).ok())
            .collect();
        let chat_of: HashMap<Id, Id> = backend
            .get_objects::<Message>(caller, &account_id, Some(&destroy_ids), None)
            .await
            .map(|(found, _)| found.into_iter().map(|m| (m.id, m.chat_id)).collect())
            .unwrap_or_default();
        let mut destroyed_chat_ids: Vec<Id> = Vec::new();
        for id_str in id_strs {
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str.to_owned(), set_error_value(&err));
                    continue;
                }
            };

            match backend
                .destroy_object::<Message>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_chat_ids.extend(chat_of.get(&id).cloned());
                    destroyed_list.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    not_destroyed.insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
        record_unread_count_changes(backend, caller, &account_id, destroyed_chat_ids).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryBackend;
    use serde_json::json;

    const ACCOUNT_ID: &str = "a1";

    // Oracles in this module are hand-built per workspace test-integrity
    // policy. Each test asserts the wire shape of a known-invalid or
    // known-valid broadcastMentions payload against the validation rules
    // from spec §4.4.1 / §4.16. The independent oracle is the spec text
    // itself; no assertion uses the code under test as its own reference.

    /// §BroadcastMention: "Servers MUST reject a BroadcastMention with an
    /// unrecognized `scope` value; in `Message/set` the rejection is an
    /// `invalidProperties` SetError naming `broadcastMentions`."
    #[tokio::test]
    async fn broadcast_mention_validation_rejects_unknown_scope() {
        let backend = MemoryBackend::new();
        backend.register_account(&Id::from(ACCOUNT_ID));
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": "c1",
                        "body": "Hello @channel",
                        "sentAt": "2024-01-01T00:00:00Z",
                        "broadcastMentions": [
                            { "scope": "channel", "offset": 6, "length": 8 }
                        ]
                    }
                }
            }),
        )
        .await
        .expect("handle_message_set");

        assert!(resp["notCreated"]["m0"].is_object());
        assert_eq!(resp["notCreated"]["m0"]["type"], "invalidProperties");
        assert_eq!(
            resp["notCreated"]["m0"]["properties"],
            json!(["broadcastMentions"])
        );
        let desc = resp["notCreated"]["m0"]["description"]
            .as_str()
            .expect("description");
        assert!(
            desc.contains("unrecognized scope"),
            "expected 'unrecognized scope' in description, got: {desc}"
        );
    }

    /// A valid @everyone mention at the correct body offset succeeds.
    #[tokio::test]
    async fn broadcast_mention_validation_accepts_valid() {
        let backend = MemoryBackend::new();
        let chat_id = make_group_chat(&backend).await;
        let body = "Hello @everyone!";
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": chat_id,
                        "body": body,
                        "sentAt": "2024-01-01T00:00:00Z",
                        "broadcastMentions": [
                            { "scope": "everyone", "offset": 6, "length": 9 }
                        ]
                    }
                }
            }),
        )
        .await
        .expect("handle_message_set");

        assert!(
            resp["created"]["m0"].is_object(),
            "expected created, got notCreated: {}",
            resp["notCreated"]["m0"]
        );
    }

    /// An empty broadcastMentions array is always valid.
    #[tokio::test]
    async fn broadcast_mention_validation_accepts_empty() {
        let backend = MemoryBackend::new();
        let chat_id = make_group_chat(&backend).await;
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": chat_id,
                        "body": "Hello, world!",
                        "sentAt": "2024-01-01T00:00:00Z",
                        "broadcastMentions": []
                    }
                }
            }),
        )
        .await
        .expect("handle_message_set");

        assert!(
            resp["created"]["m0"].is_object(),
            "expected created, got notCreated: {}",
            resp["notCreated"]["m0"]
        );
    }

    // -----------------------------------------------------------------------
    // BroadcastMention permission-check tests (§4.4.1 mention_broadcast)
    //
    // Oracle: the spec text itself. Group/direct chats without a spaceId
    // are unrestricted; channel chats with a spaceId must pass the
    // mention_broadcast gate. The channel fixture's owner role holds
    // mention_broadcast, so these tests verify the handler CODE PATH is
    // wired correctly and that the gate is skipped or invoked in the
    // right circumstances; tests/message_broadcast_permission_memory.rs
    // covers denial.
    // -----------------------------------------------------------------------

    /// Helper: create a group Chat (no spaceId) via handle_chat_set and
    /// return its server-assigned id.
    async fn make_group_chat(backend: &MemoryBackend) -> String {
        backend.register_account(&Id::from(ACCOUNT_ID));
        let HandlerOutput { primary: resp, .. } = crate::chat::handle_chat_set(
            backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "c0": { "kind": "group", "name": "test-group" } }
            }),
        )
        .await
        .expect("make_group_chat");
        resp["created"]["c0"]["id"]
            .as_str()
            .expect("group chat id")
            .to_owned()
    }

    /// Helper: seed a Space whose owner member (the accountId, per
    /// §SpaceMember `id`) holds `manage_channels` and `send` (§Message/set:
    /// "For channel Chats, the server MUST verify the sender holds the
    /// `"send"` permission"), then create a channel Chat inside it, and
    /// return the channel's server-assigned id.
    async fn make_channel_chat(backend: &MemoryBackend) -> String {
        let space_id = "s0";
        backend.insert_object_for_test(
            "Space",
            ACCOUNT_ID,
            space_id,
            json!({
                "id": space_id,
                "name": "test-space",
                "createdAt": "2026-01-01T00:00:00Z",
                "memberCount": 1,
                "categories": [],
                "uncategorizedChannelIds": [],
                "isPublic": false,
                "isPubliclyPreviewable": false,
                "roles": [{ "id": "r-owner", "name": "Owner", "permissions": ["manage_channels", "mention_broadcast", "send"], "position": 10 }],
                "members": [{ "id": ACCOUNT_ID, "roleIds": ["r-owner"], "joinedAt": "2026-01-01T00:00:00Z" }],
            }),
        );

        // Create a channel Chat in that Space. §Chat/set: "Channel Chats
        // are created exclusively via the `addChannels` patch key in
        // `Space/set`." §Space/set `addChannels`: "Requires
        // `"manage_channels"`."
        crate::space::handle_space_set(
            backend,
            &(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": { space_id: { "addChannels": [{ "name": "general" }] } }
            }),
        )
        .await
        .expect("make_channel_chat: addChannels");
        let HandlerOutput {
            primary: get_resp, ..
        } = crate::space::handle_space_get(
            backend,
            &(),
            json!({ "accountId": ACCOUNT_ID, "ids": [space_id] }),
        )
        .await
        .expect("make_channel_chat: Space/get");
        get_resp["list"][0]["uncategorizedChannelIds"][0]
            .as_str()
            .expect("channel chat id")
            .to_owned()
    }

    /// Create a Message with broadcastMentions in a group chat (no spaceId).
    /// The permission check must be SKIPPED (unrestricted per spec),
    /// and the message must succeed.
    #[tokio::test]
    async fn broadcast_mention_create_without_space_unrestricted() {
        let backend = MemoryBackend::new();
        let chat_id = make_group_chat(&backend).await;

        let body = "Hello @everyone!";
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": chat_id,
                        "body": body,
                        "sentAt": "2024-01-01T00:00:00Z",
                        "broadcastMentions": [
                            { "scope": "everyone", "offset": 6, "length": 9 }
                        ]
                    }
                }
            }),
        )
        .await
        .expect("handle_message_set");

        assert!(
            resp["created"]["m0"].is_object(),
            "expected created in group chat (no spaceId), got notCreated: {}",
            resp["notCreated"]["m0"]
        );
    }

    /// Create a Message with broadcastMentions in a channel chat (has spaceId).
    /// The owner holds mention_broadcast in the Space, so the message
    /// must succeed.
    #[tokio::test]
    async fn broadcast_mention_create_in_channel_succeeds() {
        let backend = MemoryBackend::new();
        let chat_id = make_channel_chat(&backend).await;

        let body = "Hello @everyone!";
        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": chat_id,
                        "body": body,
                        "sentAt": "2024-01-01T00:00:00Z",
                        "broadcastMentions": [
                            { "scope": "everyone", "offset": 6, "length": 9 }
                        ]
                    }
                }
            }),
        )
        .await
        .expect("handle_message_set");

        assert!(
            resp["created"]["m0"].is_object(),
            "expected created in channel chat, got notCreated: {}",
            resp["notCreated"]["m0"]
        );
    }

    /// Create a Message with empty broadcastMentions in a channel chat.
    /// The permission check must NOT fire (empty array skips the gate).
    #[tokio::test]
    async fn broadcast_mention_create_empty_skips_permission_check() {
        let backend = MemoryBackend::new();
        let chat_id = make_channel_chat(&backend).await;

        let HandlerOutput { primary: resp, .. } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": chat_id,
                        "body": "Hello, world!",
                        "sentAt": "2024-01-01T00:00:00Z",
                        "broadcastMentions": []
                    }
                }
            }),
        )
        .await
        .expect("handle_message_set");

        assert!(
            resp["created"]["m0"].is_object(),
            "expected created with empty broadcastMentions, got notCreated: {}",
            resp["notCreated"]["m0"]
        );
    }

    /// Update a Message to retain non-empty broadcastMentions in a channel chat.
    /// The owner holds mention_broadcast in the Space, so the update
    /// must succeed.
    #[tokio::test]
    async fn broadcast_mention_update_retaining_broadcast_succeeds() {
        let backend = MemoryBackend::new();
        let chat_id = make_channel_chat(&backend).await;

        // Create a message with broadcastMentions first.
        let body = "Hello @everyone!";
        let HandlerOutput {
            primary: create_resp,
            ..
        } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": {
                    "m0": {
                        "chatId": chat_id,
                        "body": body,
                        "sentAt": "2024-01-01T00:00:00Z",
                        "broadcastMentions": [
                            { "scope": "everyone", "offset": 6, "length": 9 }
                        ]
                    }
                }
            }),
        )
        .await
        .expect("create message");
        let msg_id = create_resp["created"]["m0"]["id"]
            .as_str()
            .expect("message id");

        // Update the message, retaining a non-empty broadcastMentions.
        let new_body = "Hey @here now!";
        let HandlerOutput {
            primary: update_resp,
            ..
        } = handle_message_set(
            &backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": {
                    (msg_id): {
                        "body": new_body,
                        "broadcastMentions": [
                            { "scope": "here", "offset": 4, "length": 5 }
                        ]
                    }
                }
            }),
        )
        .await
        .expect("update message");

        assert!(
            update_resp["updated"][msg_id].is_null() || update_resp["updated"][msg_id].is_object(),
            "expected updated, got notUpdated: {}",
            update_resp["notUpdated"][msg_id]
        );
        assert!(
            update_resp["notUpdated"].is_null()
                || !update_resp["notUpdated"]
                    .as_object()
                    .is_some_and(|m| m.contains_key(msg_id)),
            "message should not be in notUpdated"
        );
    }
}
