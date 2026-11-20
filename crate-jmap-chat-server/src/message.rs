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
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`: `stateMismatch`;
//! - `/changes`, `/queryChanges`: `cannotCalculateChanges`;
//! - `/queryChanges`: `tooManyChanges`;
//! - `/query`, `/queryChanges`: `unsupportedFilter`;
//! - `/query`: `unsupportedSort`, `anchorNotFound`.
//!
//! Per-target failures inside `/set` (including the `slow_mode_check`
//! `rateLimit` and reaction-patch `invalidPatch` paths) surface in the
//! `notCreated` / `notUpdated` / `notDestroyed` maps within `Ok((Value, ...))`,
//! not as `Err`.

use jmap_chat_types::{
    Attachment, AttachmentCreate, BROADCAST_MENTION_SCOPES, BroadcastMention, Chat, DeliveryState,
    Mention, Message, MessageAction, PresenceStatus, ReadPosition, Space,
};
use jmap_chat_types::{Filter, MessageComparator, MessageFilter, MessageFilterCondition};
use jmap_types::{Id, JmapError, PatchObject, UTCDate};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::chat::record_unread_count_changes;
use crate::outbound::{OutboundEvent, ReactionAction};
use crate::{ChatBackend, ChatLimits};
use jmap_server::{BackendSetError, SetError, SetErrorType};
use std::collections::{BTreeMap, HashMap, HashSet};
use subtle::ConstantTimeEq;

use crate::helpers::{
    PropType, check_metadata_entry, create_metadata, is_metadata_key, mistyped_create_props,
};
use crate::helpers::{
    SetAccumulators, default_sort, enforce_max_objects_in_get, enforce_max_objects_in_set,
    extract_account_id, filter_properties, finalize_set_response, iso8601_before, not_found_json,
    now_utc_string, optional_arg, refusal_value, reject_unsupported_sort, serialize_value,
    strip_read_only_patch,
};
use crate::helpers::{parents_first, resolve_entry_refs, resolved_create};
use jmap_server::{
    HandlerOutput, MetadataProjection, QueryParams, backend_set_error_value, bool_arg, filter_arg,
    is_client_updatable, query_anchor_args, reject_server_set, server_fail_from_backend,
    server_fail_value_from_backend,
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
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]). When the backend lists Message for
/// draft-ietf-jmap-metadata-05 ([`JmapAccounts::metadata_info`](jmap_server::JmapAccounts::metadata_info)),
/// `metadata` is returned as an object, by default only with
/// `urn:ietf:params:jmap:metadata` in `using`, and with that capability
/// `properties` may hold `metadata/<namespace>` subselectors (-05 §2,
/// §3.1); see
/// [`jmap_server::MetadataProjection`].
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_get<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
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

    let projection = MetadataProjection::new(
        properties.as_deref(),
        true,
        using,
        backend
            .metadata_info::<Message>(caller, &account_id)
            .as_ref(),
    )?;

    let ids_slice = ids.as_deref();
    let (fetched, mut not_found) = backend
        .get_objects::<Message>(caller, &account_id, ids_slice, properties.as_deref())
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // draft-atwood-jmap-chat-00 §Message/query (jmap-chat-spec 77c3474
    // L1301): "messages in channels the caller cannot view SHOULD be
    // omitted" from `Message/get`; done by choice. A hidden
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

    let prop_set: Option<HashSet<&str>> = properties.as_ref().map(|props| {
        let mut set: HashSet<&str> = props.iter().map(|s| s.as_str()).collect();
        set.insert("id");
        set
    });
    let list_json: Vec<Value> = list
        .iter()
        .map(|obj| {
            let val = serialize_value(obj)?;
            let mut out = match &prop_set {
                Some(set) => filter_properties(&val, set),
                None => val.clone(),
            };
            projection.apply(&val, &mut out);
            Ok(out)
        })
        .collect::<Result<Vec<_>, JmapError>>()?;

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
/// `lastReadMessageId`, or always when that is absent, and the owner did
/// not send it (`senderId` is not the accountId), as for
/// `Chat.unreadCount`.
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
                .filter(|r| r.sender_id != *account_id)
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
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); with
/// `urn:ietf:params:jmap:metadata` the request may set
/// draft-ietf-jmap-metadata-05 §3.3 `ignoreMetadataOnlyChanges`, and
/// without it metadata-only changes are left out (-05 §3).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Message, B>(backend, caller, args, using).await
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
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-05 §3.6
/// filter condition is `unsupportedFilter` (-05 §3). A JMAP Search
/// `where` (crate-jmap docs/specs/jmap-search.md) needs
/// `urn:ietf:params:jmap:search` in `using` and `"Message"` in
/// [`ChatSettings::search_capability`](crate::ChatSettings::search_capability), else `unsupportedFilter`; it is
/// checked with [`jmap_search::check_where_filter`] and makes
/// `canCalculateChanges` `false`. It does not lift the `chatId` /
/// `hasMention: true` rule.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_query<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::reject_metadata_filter_arg(
        &args,
        jmap_chat_types::MessageFilterCondition::has_metadata_condition,
        using,
    )?;
    let (account_id, mut args) = extract_account_id(args)?;

    // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let searched =
        check_message_search(backend, caller, &account_id, args.get("filter"), using).await?;

    let calculate_total: bool = bool_arg(&args, "calculateTotal", false);

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

    // draft-atwood-jmap-chat-00 §Message/query (jmap-chat-spec 77c3474
    // L1301): "messages in channels the caller cannot view SHOULD be
    // omitted"; done by choice. A query scoped to one Chat the
    // caller cannot view matches nothing; a query without `chatId` relies
    // on the backend (see `ChatAccessPolicy::may_view_chat`).
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
        "canCalculateChanges": result.can_calculate_changes && !searched,
        "position": position,
        "ids": ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total && let Some(t) = total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

/// Check the JMAP Search `where` of the undecoded Message `filter`
/// against `using` and [`ChatSettings::search_capability`](crate::ChatSettings::search_capability), and return
/// whether there was one (crate-jmap docs/specs/jmap-search.md §Errors,
/// §Opting In a Data Type). The capability is read only for a filter with
/// `where`.
///
/// # Errors
///
/// As [`jmap_search::check_where_filter`] for the `"Message"` data type,
/// and `serverFail` when the backend fails.
async fn check_message_search<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    filter: Option<&Value>,
    using: &[String],
) -> Result<bool, JmapError> {
    if !filter.is_some_and(jmap_search::filter_uses_where) {
        return Ok(false);
    }
    let cap = backend
        .search_capability(caller, account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    jmap_search::check_where_filter(filter, "Message", using, cap.as_ref())
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
    let unsupported = |description: &str| {
        let mut err = JmapError::unsupported_filter();
        err.description = Some(description.to_owned());
        err
    };
    let Some(Filter::Condition(c)) = filter else {
        return Err(unsupported(
            "the filter must be a single condition with chatId or hasMention: true",
        ));
    };
    if c.chat_id.is_none() && c.has_mention != Some(true) {
        return Err(unsupported(
            "the filter must set chatId or hasMention: true",
        ));
    }
    if c.text.is_some() {
        return Err(unsupported("full-text search (text) is not supported"));
    }
    if c.thread_root_id.is_some() && !supports_threads {
        return Err(unsupported(
            "threadRootId is valid only when supportsThreads is true",
        ));
    }
    Ok(c)
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
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-05 §3.6
/// filter condition is `unsupportedFilter` (-05 §3). A filter with a
/// JMAP Search `where` is checked as for [`handle_message_query`] and then
/// answered with `cannotCalculateChanges` (jmap-search §Query Changes).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_message_query_changes<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    mut args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::reject_metadata_filter_arg(
        &args,
        jmap_chat_types::MessageFilterCondition::has_metadata_condition,
        using,
    )?;
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
        // RFC 8620 §3.6.2 accountNotFound (bd:JMAP-dtpe), before the
        // may_view_chat read below.
        if !backend
            .account_exists(caller, account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            return Err(JmapError::account_not_found());
        }
        let mut probe = serde_json::Map::new();
        if let Some(f) = args.get("filter") {
            probe.insert("filter".into(), f.clone());
        }
        let filter: Option<MessageFilter> = filter_arg(&mut probe)?;
        let supports_threads = backend.limits(caller, account_id).supports_threads;
        check_message_filter(filter.as_ref(), supports_threads)?;
        // jmap-search §Query Changes: "Servers MAY return
        // `cannotCalculateChanges` for any `/queryChanges` call whose
        // filter uses `where`".
        if check_message_search(backend, caller, account_id, args.get("filter"), using).await? {
            return Err(JmapError::cannot_calculate_changes());
        }
    }

    // §Message/query (jmap-chat-spec 77c3474 L1301): "messages in channels
    // the caller cannot view SHOULD be omitted"; by choice, changes to such
    // a Chat's messages are not reported.
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
/// SetError." §Fields Inside the Ciphertext (jmap-chat-spec 77c3474
/// L1823): "Servers SHOULD reject a `Message/set` create or update that has
/// an encrypted `bodyType` and any of these fields non-empty"; this crate
/// performs the recommended rejection by choice. The field rule runs on the Message as it
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
/// entry with `filenodeId` goes to [`ChatResolvers::filenode_attachment`](crate::ChatResolvers::filenode_attachment)
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
                Ok(Err(set_err)) => return Err(serde_json::Value::from(&set_err)),
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
/// checked against [`ChatResolvers::attachment_blob`](crate::ChatResolvers::attachment_blob); a mismatch is
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
            Ok(Err(set_err)) => return Some(serde_json::Value::from(&set_err)),
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
/// contract on `ChatStore::create_object`).
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
/// [`ChatStore::create_object`](crate::ChatStore::create_object). It sets every other field, including
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
/// [`ChatStore::create_object`](crate::ChatStore::create_object), unchanged.
// Same error type as `ChatStore::create_object`, passed through unboxed.
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

/// A Mention `id`, a ChatContact id (draft-atwood-jmap-chat-00 §Mention),
/// as `(property, entry field, property named on failure)` for
/// [`resolve_entry_refs`].
const MENTION_ID: (&str, &str, &str) = ("mentions", "id", "mentions");

/// An Attachment `filenodeId`, a FileNode id; failed validation names
/// `filenodeId` (draft-atwood-jmap-chat-filenode-00 §filenodeId).
const ATTACHMENT_FILENODE_ID: (&str, &str, &str) = ("attachments", "filenodeId", "filenodeId");

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
/// 8620 §5.3). So does a Mention `id` in a create or an edit, and an
/// Attachment `filenodeId` in a create. An unknown reference is
/// `invalidProperties` naming the property (`filenodeId` for an
/// Attachment).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]): a write to `metadata` or
/// `privateMetadata` without `urn:ietf:params:jmap:metadata` is
/// `invalidProperties` (draft-ietf-jmap-metadata-05 §3), as is `null` for
/// either in a create (-05 §3.2). The backend validates the rest.
pub async fn handle_message_set<B: ChatBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    mut args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    // A `#creationId` blob reference names a blob created earlier in the
    // request (RFC 8620 §5.3; draft-ietf-jmap-blobext-03 §8); an unknown one
    // is left for the Id checks below.
    jmap_server::resolve_blob_refs(&mut args, &["blobId", "thumbnailBlobId"], created_ids);
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

    let limits = backend.limits(caller, &account_id);
    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
        created_ids,
        using,
        limits: &limits,
    };
    let mut acc = SetAccumulators::default();
    let mut mutated = set_create(&ctx, &mut args, &mut acc).await?;
    // RFC 8620 §5.3: update keys and destroy ids may name this call's creates.
    jmap_server::resolve_set_id_refs(&mut args, created_ids, &acc.created);
    mutated |= set_update(&ctx, &mut args, &mut acc).await?;
    mutated |= set_destroy(&ctx, &mut args, &mut acc).await?;

    finalize_set_response::<B, Message>(backend, caller, &account_id, old_state, mutated, acc).await
}

/// What every `Message/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: ChatBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    /// The request-wide `#creationId` map (RFC 8620 §5.3).
    created_ids: &'a BTreeMap<Id, Id>,
    /// The request's `using` capabilities.
    using: &'a [String],
    /// The account's chat limits.
    limits: &'a ChatLimits,
}

/// What the `Message/set` create loop carries from one entry to the next.
struct CreateRun {
    /// The request-wide `#creationId` map plus this call's creates so
    /// far (RFC 8620 §5.3: creates of this call join the request-wide map
    /// so later creates in this call can reference them).
    created_ids: BTreeMap<Id, Id>,
    /// The Chats of the created Messages, for the ReadPosition and
    /// `unreadCount` bookkeeping after the loop.
    delivered_chat_ids: Vec<Id>,
}

/// The `create` part of `Message/set`. Returns whether the backend was
/// written.
async fn set_create<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        let mut run = CreateRun {
            created_ids: ctx.created_ids.clone(),
            delivered_chat_ids: Vec::new(),
        };
        for (create_id, obj_val) in parents_first(create_map, &["replyTo", "threadRootId"]) {
            mutated |= create_one(ctx, create_id, obj_val, acc, &mut run).await?;
        }
        let delivered_chat_ids = run.delivered_chat_ids;
        ensure_read_positions(
            ctx.backend,
            ctx.caller,
            ctx.account_id,
            delivered_chat_ids.clone(),
        )
        .await;
        record_unread_count_changes(ctx.backend, ctx.caller, ctx.account_id, delivered_chat_ids)
            .await;
    }
    Ok(mutated)
}

/// One `create` entry of `Message/set`. Returns whether the backend was
/// written.
async fn create_one<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    create_id: &str,
    obj_val: &Value,
    acc: &mut SetAccumulators,
    run: &mut CreateRun,
) -> Result<bool, JmapError> {
    let build = match message_builder(ctx, obj_val, &run.created_ids).await {
        Ok(build) => build,
        Err(err) => {
            acc.not_created.insert(create_id.to_owned(), err);
            return Ok(false);
        }
    };
    // draft-atwood-jmap-chat-00 §Message/set: the server sets the
    // server-assigned fields "then enqueues the message for
    // outbound delivery"; `create_message` does both as one unit.
    match ctx
        .backend
        .create_message(ctx.caller, ctx.account_id, create_id, build)
        .await
    {
        Ok((server_id, created_obj)) => {
            run.created_ids.insert(Id::from(create_id), server_id);
            run.delivered_chat_ids.push(created_obj.chat_id.clone());
            acc.created.insert(
                create_id.to_owned(),
                jmap_server::serialize_value(&created_obj)?,
            );
            Ok(true)
        }
        Err(e) => {
            acc.not_created
                .insert(create_id.to_owned(), backend_set_error_value(e));
            Ok(false)
        }
    }
}

/// Check one `Message/set` create object, with its `#creationId`
/// references resolved against `created_ids`, and return the builder for
/// the stored Message, or the SetError value for `notCreated`.
async fn message_builder<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    obj_val: &Value,
    created_ids: &BTreeMap<Id, Id>,
) -> Result<impl FnOnce(&Id) -> Message + Send + use<B>, Value> {
    let resolved = resolved_create(obj_val, MESSAGE_ID_PROPERTIES, created_ids)
        .and_then(|mut v| {
            resolve_entry_refs(&mut v, &[MENTION_ID, ATTACHMENT_FILENODE_ID], created_ids)
                .map(|()| v)
        })
        .map_err(|err| serde_json::Value::from(&*err))?;
    let obj_val = &resolved;
    let fields = create_fields(obj_val, ctx.using, ctx.limits)?;
    let chat = create_target_chat(ctx, &fields.chat_id).await?;
    let send_error = create_send_error(ctx, &chat, &fields.chat_id, &fields.broadcast_mentions);
    if let Some(err) = send_error.await {
        return Err(err);
    }
    let (reply_to, thread_root_id) = create_thread(ctx, obj_val, &fields.chat_id).await?;
    let (sender_expires_at, burn_on_read) = create_expiry(obj_val)?;

    let now_str = now_utc_string();
    let received_at: UTCDate = UTCDate::from(now_str.as_ref());

    let (attachments, apply_optionals) = create_payload(ctx, &chat, obj_val).await?;
    if let Some(err) = slow_mode_error(ctx, &fields.chat_id).await {
        return Err(err);
    }

    // draft-atwood-jmap-chat-00 §Message `senderMsgId`: "Equals
    // `id` for messages composed by the owner."
    let sender_id = ctx.account_id.clone();
    Ok(move |id: &Id| {
        let mut msg = Message::new(
            id.clone(),
            id.clone(),
            sender_id,
            fields.chat_id,
            fields.body,
            fields.body_type,
            fields.sent_at,
            received_at,
            DeliveryState::Pending,
        );
        msg.reply_to = reply_to;
        msg.thread_root_id = thread_root_id;
        msg.sender_expires_at = sender_expires_at;
        msg.burn_on_read = burn_on_read;
        msg.attachments = attachments;
        msg.metadata = fields.metadata;
        msg.private_metadata = fields.private_metadata;
        apply_optionals(&mut msg);
        msg
    })
}

/// The client-supplied core of a `Message/set` create, checked.
struct CreateFields {
    metadata: Option<Map<String, Value>>,
    private_metadata: Option<Map<String, Value>>,
    chat_id: Id,
    body: String,
    sent_at: UTCDate,
    body_type: String,
    broadcast_mentions: Vec<Value>,
}

/// Check the properties of one `Message/set` create object that need no
/// backend read and return them, or the SetError value for `notCreated`.
fn create_fields(
    obj_val: &Value,
    using: &[String],
    limits: &ChatLimits,
) -> Result<CreateFields, Value> {
    if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Message>) {
        return Err(serde_json::Value::from(&err));
    }
    if let Some(Err(err)) = obj_val
        .as_object()
        .map(|o| check_metadata_entry(o, true, using))
    {
        return Err(serde_json::Value::from(&*err));
    }
    // draft-ietf-jmap-metadata-05 §3.2: the create sets the
    // metadata properties; the backend validates them.
    let (metadata, private_metadata) = create_metadata(obj_val)?;
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
        return Err(err);
    }
    let (chat_id, body, sent_at) = create_chat_body(obj_val, limits)?;
    let body_type = create_body_type(obj_val, limits)?;

    // BroadcastMention structural validation (§4.4.1 / §4.16).
    let broadcast_mentions: Vec<Value> = obj_val
        .get("broadcastMentions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if let Some(err) = validate_broadcast_mentions(&broadcast_mentions) {
        return Err(err);
    }
    Ok(CreateFields {
        metadata,
        private_metadata,
        chat_id,
        body,
        sent_at,
        body_type,
        broadcast_mentions,
    })
}

/// The `chatId`, `body` and `sentAt` of one `Message/set` create object,
/// checked, or the SetError value for `notCreated`.
fn create_chat_body(obj_val: &Value, limits: &ChatLimits) -> Result<(Id, String, UTCDate), Value> {
    let Some(chat_id_str) = obj_val.get("chatId").and_then(|v| v.as_str()) else {
        return Err(json!({ "type": "invalidProperties", "properties": ["chatId"] }));
    };
    let chat_id = jmap_server::parse_wire_id(chat_id_str, "chatId")
        .map_err(|err| serde_json::Value::from(&*err))?;

    let Some(body) = obj_val
        .get("body")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
    else {
        return Err(json!({ "type": "invalidProperties", "properties": ["body"] }));
    };
    // §Account-Level Capability Object `maxBodyBytes`: "Servers
    // MUST reject messages exceeding this limit; in `Message/set`
    // the rejection is a `tooLarge` SetError."
    if u64::try_from(body.len()).map_or(true, |n| n > limits.max_body_bytes) {
        return Err(json!({
            "type": "tooLarge",
            "description": format!("body exceeds maxBodyBytes ({})", limits.max_body_bytes),
        }));
    }

    // sentAt is a UTCDate per RFC 8620 §1.4 (20-char
    // YYYY-MM-DDTHH:MM:SSZ). Validate the wire shape via
    // UTCDate::new_validated; a malformed value produces
    // invalidProperties rather than silently flowing through
    // to storage where downstream lex-compares and slice ops
    // (helpers::iso8601_before) assume a validated 20-byte
    // ASCII shape.
    let Some(sent_at_str) = obj_val.get("sentAt").and_then(|v| v.as_str()) else {
        return Err(json!({ "type": "invalidProperties", "properties": ["sentAt"] }));
    };
    let Ok(sent_at) = UTCDate::new_validated(sent_at_str) else {
        return Err(json!({ "type": "invalidProperties", "properties": ["sentAt"] }));
    };
    Ok((chat_id, body, sent_at))
}

/// The `bodyType` of one `Message/set` create object, checked against the
/// supported, server-reserved and encrypted body types, or the SetError
/// value for `notCreated`.
fn create_body_type(obj_val: &Value, limits: &ChatLimits) -> Result<String, Value> {
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
        return Err(json!({ "type": "invalidProperties", "properties": ["bodyType"] }));
    };
    // draft-atwood-jmap-chat-mimi-00 §application/mimi-fanout, the
    // one reserving companion: "the server MUST reject a
    // `Message/set` create with this body type with an
    // `invalidProperties` SetError naming `bodyType`."
    if limits.server_reserved_body_types.contains(&body_type) {
        return Err(json!({
            "type": "invalidProperties",
            "properties": ["bodyType"],
            "description": format!("bodyType {body_type} is reserved for server-created Messages")
        }));
    }
    let body_type = body_type.to_owned();

    // §Fields Inside the Ciphertext: "an `invalidProperties`
    // SetError whose `properties` lists each such field".
    let conflicts = body_type_field_conflicts(obj_val, &body_type, limits.encrypted_body_types);
    if !conflicts.is_empty() {
        return Err(json!({
            "type": "invalidProperties",
            "properties": conflicts,
            "description": format!("must be empty or absent when bodyType is {body_type}"),
        }));
    }
    Ok(body_type)
}

/// The Chat a `Message/set` create posts to, once it accepts the create,
/// or the SetError value for `notCreated`.
async fn create_target_chat<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    chat_id: &Id,
) -> Result<Chat, Value> {
    // draft-atwood-jmap-chat-00 §Input Validation: "`chatId`:
    // verify per {{chat-id}} — confirm match with stored id"; a
    // failed check is "an `invalidProperties` SetError ... whose
    // `properties` names the offending property".
    let chat = match ctx
        .backend
        .get_objects::<Chat>(
            ctx.caller,
            ctx.account_id,
            Some(std::slice::from_ref(chat_id)),
            None,
        )
        .await
    {
        Ok((found, _)) => match found.into_iter().next() {
            Some(chat) => chat,
            None => {
                return Err(json!({
                    "type": "invalidProperties",
                    "properties": ["chatId"],
                    "description": format!("no Chat with id {chat_id}"),
                }));
            }
        },
        Err(e) => return Err(server_fail_value_from_backend(&e)),
    };

    let outcome = ctx
        .backend
        .may_create_message(ctx.caller, ctx.account_id, &chat)
        .await;
    if let Some(err) = refusal_value(outcome) {
        return Err(err);
    }

    // draft-atwood-jmap-chat-00 §Chat `closedAt`: "After it is
    // set, the server MUST reject with a `forbidden` SetError
    // every `Message/set` create in this Chat".
    if chat.closed_at.is_some() {
        return Err(json!({ "type": "forbidden", "description": "the group is closed" }));
    }

    // draft-atwood-jmap-chat-00 §Space `closedAt`: "After it is
    // set, the Space's channels are read-only: the server MUST
    // reject every `Message/set` create in them with a
    // `forbidden` SetError."
    if let Some(sid) = &chat.space_id {
        match ctx
            .backend
            .get_objects::<Space>(
                ctx.caller,
                ctx.account_id,
                Some(std::slice::from_ref(sid)),
                None,
            )
            .await
        {
            Ok((spaces, _)) if spaces.first().is_some_and(|s| s.closed_at.is_some()) => {
                return Err(json!({ "type": "forbidden", "description": "the Space is closed" }));
            }
            Ok(_) => {}
            Err(e) => return Err(server_fail_value_from_backend(&e)),
        }
    }
    Ok(chat)
}

/// The SetError value when the sender may not post to `chat` (the Chat
/// `chat_id`), or may not send its `broadcast_mentions` there; `None` to
/// go on.
async fn create_send_error<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    chat: &Chat,
    chat_id: &Id,
    broadcast_mentions: &[Value],
) -> Option<Value> {
    // draft-atwood-jmap-chat-00 §Message/set: the sender MUST be a
    // current member of the target Chat and, in a channel, hold
    // `"send"`; otherwise `forbidden`.
    let outcome = ctx
        .backend
        .may_send_message(ctx.caller, ctx.account_id, chat_id)
        .await;
    if let Some(err) = refusal_value(outcome) {
        return Some(err);
    }

    // BroadcastMention permission check (§4.4.1).
    // Non-empty broadcastMentions in a Space-backed channel
    // requires mention_broadcast permission; rich-body spans are
    // display-only and never consulted (§Rich Body Format).
    // Group/direct chats (no spaceId) are unrestricted per spec.
    if let Some(sid) = chat
        .space_id
        .as_ref()
        .filter(|_| !broadcast_mentions.is_empty())
    {
        let outcome = ctx
            .backend
            .may_broadcast_mention(ctx.caller, ctx.account_id, sid, chat_id)
            .await;
        return refusal_value(outcome);
    }
    None
}

/// The `replyTo` and the thread root of one `Message/set` create in Chat
/// `chat_id`, checked, or the SetError value for `notCreated`.
async fn create_thread<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    obj_val: &Value,
    chat_id: &Id,
) -> Result<(Option<Id>, Option<Id>), Value> {
    let reply_to: Option<Id> = match obj_val.get("replyTo").and_then(|v| v.as_str()) {
        Some(s) => Some(
            jmap_server::parse_wire_id(s, "replyTo")
                .map_err(|err| serde_json::Value::from(&*err))?,
        ),
        None => None,
    };

    let supplied_root: Option<Id> = match obj_val.get("threadRootId").and_then(|v| v.as_str()) {
        Some(s) => Some(
            jmap_server::parse_wire_id(s, "threadRootId")
                .map_err(|err| serde_json::Value::from(&*err))?,
        ),
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
            let parent = match ctx
                .backend
                .get_objects::<Message>(
                    ctx.caller,
                    ctx.account_id,
                    Some(std::slice::from_ref(parent_id)),
                    None,
                )
                .await
            {
                Ok((found, _)) => found.into_iter().find(|m| m.chat_id == *chat_id),
                Err(e) => return Err(server_fail_value_from_backend(&e)),
            };
            let Some(parent) = parent else {
                return Err(json!({
                    "type": "invalidProperties",
                    "properties": ["replyTo"],
                    "description": "replyTo is not a Message in this Chat",
                }));
            };
            Some(parent.thread_root_id.unwrap_or(parent.id))
        }
    };
    if supplied_root.is_some() && supplied_root != expected_root {
        return Err(json!({
            "type": "invalidProperties",
            "properties": ["threadRootId"],
            "description": "threadRootId does not match the thread of replyTo",
        }));
    }
    Ok((reply_to, expected_root))
}

/// The `senderExpiresAt` and `burnOnRead` of one `Message/set` create
/// object, checked, or the SetError value for `notCreated`.
fn create_expiry(obj_val: &Value) -> Result<(Option<UTCDate>, Option<bool>), Value> {
    // senderExpiresAt is a UTCDate per RFC 8620 §1.4 (20-char
    // YYYY-MM-DDTHH:MM:SSZ). Validate the wire shape via
    // UTCDate::new_validated; a malformed value produces
    // invalidProperties rather than silently flowing through to
    // a downstream string compare with undefined ordering.
    let sender_expires_at: Option<UTCDate> =
        if let Some(s) = obj_val.get("senderExpiresAt").and_then(|v| v.as_str()) {
            let Ok(d) = UTCDate::new_validated(s) else {
                return Err(json!({
                    "type": "invalidProperties",
                    "properties": ["senderExpiresAt"],
                }));
            };
            Some(d)
        } else {
            None
        };

    let burn_on_read: Option<bool> = obj_val.get("burnOnRead").and_then(|v| v.as_bool());

    if let Some(ref expires_at) = sender_expires_at {
        let now = now_utc_string();
        if !iso8601_before(&now, expires_at) {
            return Err(json!({ "type": "invalidProperties", "properties": ["senderExpiresAt"] }));
        }
    }
    Ok((sender_expires_at, burn_on_read))
}

/// The attachments of one `Message/set` create in `chat`, and the function
/// that copies its other optional properties onto the Message, checked,
/// or the SetError value for `notCreated`.
async fn create_payload<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    chat: &Chat,
    obj_val: &Value,
) -> Result<(Vec<Attachment>, impl FnOnce(&mut Message) + Send + use<B>), Value> {
    let mut attachments = Vec::new();
    if let Some(v) = obj_val.get("attachments").filter(|v| !v.is_null()) {
        attachments =
            create_attachments(ctx.backend, ctx.caller, ctx.account_id, ctx.limits, chat, v)
                .await?;
    }
    let apply_optionals = apply_create_optionals(obj_val)
        .map_err(|property| json!({ "type": "invalidProperties", "properties": [property] }))?;
    if let Some(err) = check_attachments(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        ctx.limits,
        &attachments,
    )
    .await
    {
        return Err(err);
    }
    Ok((attachments, apply_optionals))
}

/// The `rateLimit` SetError value when slow mode holds the sender back in
/// Chat `chat_id`, or `None`.
///
/// Slow-mode rate-limit gate (draft-atwood-jmap-chat-00
/// §Chat `slowModeSeconds` + spec commit `de60acb`).
///
/// Runs AFTER wire-format validation so malformed requests
/// don't consume rate-tracker slots, and BEFORE the
/// `create_object` call so a throttled sender never sees the
/// message touch storage. On a Throttled return the backend
/// tells us when the caller may retry; we surface that
/// verbatim as the `serverRetryAfter` extra field on the
/// `rateLimit` SetError, built via the typed
/// `SetError::with_extra` builder added in bd:JMAP-dha0.
/// The spec requires "a `rateLimit` SetError ({{RFC8620,
/// Section 5.3}}) on the offending `Message/set` create",
/// not the method-level `rateLimited` (which is reserved
/// for the Peer/* methods). `serverRetryAfter` is a
/// workspace extension field. The reference `MemoryBackend`
/// never returns an Err here.
async fn slow_mode_error<B: ChatBackend>(ctx: &SetCtx<'_, B>, chat_id: &Id) -> Option<Value> {
    let slow_err = ctx
        .backend
        .slow_mode_check(ctx.caller, ctx.account_id, chat_id)
        .await
        .err()?;
    let retry_after_str: String = slow_err.retry_after.as_ref().to_owned();
    let set_err = SetError::new(SetErrorType::RateLimit)
        .with_description("Slow mode is active for this chat")
        .with_extra("serverRetryAfter", json!(retry_after_str));
    Some(serde_json::Value::from(&set_err))
}

/// The `update` part of `Message/set`. Returns whether the backend was
/// written.
async fn set_update<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            mutated |= update_one(ctx, id_str, patch_val, acc).await?;
        }
    }
    Ok(mutated)
}

/// The SetError value when `ChatRestrictions::may_update_message` refuses
/// a `Message/set` update of `stored` in `chat`, or `None`. A Message
/// whose Chat does not resolve is not gated.
///
/// Per-Chat restrictions of extension-bound Chats; see
/// `ChatRestrictions::may_update_message`. It sees only the keys
/// the update may change.
async fn restriction_error<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    chat: Option<&Chat>,
    stored: &Message,
    patch_val: &Value,
) -> Option<Value> {
    let chat = chat?;
    let patch_keys = defined_message_keys(patch_val);
    let outcome = ctx
        .backend
        .may_update_message(ctx.caller, ctx.account_id, chat, stored, &patch_keys)
        .await;
    refusal_value(outcome)
}

/// One `update` entry of `Message/set`. Returns whether the backend was
/// written.
async fn update_one<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    id_str: String,
    patch_val: Value,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let id = match jmap_server::parse_wire_id(&id_str, "id") {
        Ok(id) => id,
        Err(err) => {
            acc.not_updated
                .insert(id_str, serde_json::Value::from(&*err));
            return Ok(false);
        }
    };
    let checked = match update_patch_error(patch_val, ctx.created_ids, ctx.using) {
        Ok(patch_val) => update_targets(ctx, &id)
            .await
            .map(|targets| (patch_val, targets)),
        Err(err) => Err(err),
    };
    let (mut patch_val, (stored, stored_chat)) = match checked {
        Ok(checked) => checked,
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(false);
        }
    };

    // Existence and permission come before shape (DECISIONS.md 44):
    // the shape errors are held in `shape_err` and reported only after
    // the `forbidden` checks, which look at patch keys and values the
    // read-only check does not touch.
    let shape_err = update_shape_error(&mut patch_val, &stored, ctx.limits);

    if let Some(err) = restriction_error(ctx, stored_chat.as_ref(), &stored, &patch_val).await {
        acc.not_updated.insert(id_str, err);
        return Ok(false);
    }

    let deletes_for_all = patch_val.get("deletedForAll").and_then(|v| v.as_bool()) == Some(true);
    let edits = ["body", "bodyType", "mentions", "broadcastMentions"]
        .iter()
        .any(|&field| patch_val.get(field).is_some());
    // Every `forbidden` check has run: report a held shape error.
    let refused = edit_forbidden(&stored, ctx.account_id, deletes_for_all, edits)
        .or(shape_err)
        .or_else(|| {
            (deletes_for_all || edits)
                .then(|| edit_limit_error(&stored, &patch_val, ctx.limits))
                .flatten()
        });
    if let Some(err) = refused {
        acc.not_updated.insert(id_str, err);
        return Ok(false);
    }
    // The event for `ChatEvents::on_outbound_event`, built from the
    // stored Message once the checks above pass.
    let outbound = (deletes_for_all || edits).then(|| edit_event(&stored, deletes_for_all));

    // Nothing left once identical values are dropped (or a literal
    // `{}`): a no-op success with no write (RFC 8620 §5.3).
    if patch_val.as_object().is_some_and(Map::is_empty) {
        acc.updated.insert(id_str, Value::Null);
        return Ok(false);
    }

    let (mut augmented, deletes) = augment_patch(patch_val, deletes_for_all);
    let reaction_events = match reaction_events(&mut augmented, &stored, ctx.account_id) {
        Ok(events) => events,
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(false);
        }
    };

    if edits
        && let Some(err) =
            broadcast_edit_error(ctx, &stored, stored_chat.as_ref(), &augmented).await
    {
        acc.not_updated.insert(id_str, err);
        return Ok(false);
    }

    let (receipt, burns) = read_outcome(&stored, &augmented, ctx.account_id);
    let events = UpdateEvents {
        outbound,
        reaction_events,
        receipt,
        deletes,
        burns,
    };
    write_update(ctx, id_str, &id, augmented, events, acc).await
}

/// The `Peer/receipt` event of a `Message/set` update of `stored`, and
/// whether the update burns it.
fn read_outcome(
    stored: &Message,
    augmented: &Value,
    account_id: &Id,
) -> (Option<OutboundEvent>, bool) {
    // Detect a non-null readAt assignment in the patch. A
    // `readAt: null` is a PatchObject clear (RFC 8620 §5.3), not a
    // "mark as read" event, so it does not trigger burn-on-read.
    // draft-atwood-jmap-chat-00 §Message `burnOnRead` (jmap-chat-spec
    // 77c3474 L750): the receiving server SHOULD hard-delete
    // immediately after setting readAt on a message whose
    // `burnOnRead` is `true`; this crate does so by choice.
    let patch_sets_read_at = augmented.get("readAt").is_some_and(|v| !v.is_null());

    // If the patch sets readAt, the stored Message decides whether
    // it is burn-on-read. We deliberately consult the pre-patch
    // state: a recipient setting readAt for the first time should
    // fire the burn. draft-atwood-jmap-chat-00 §Message
    // `burnOnRead`: "Applies only to the receiving mailbox; the
    // sender's own copy is not affected."
    let pre_patch_read: Option<Message> = patch_sets_read_at.then(|| stored.clone());
    let pre_patch_burn_on_read = pre_patch_read
        .as_ref()
        .is_some_and(|m| m.burn_on_read == Some(true) && m.sender_id != *account_id);
    let receipt = read_receipt(pre_patch_read, augmented, account_id);
    (receipt, patch_sets_read_at && pre_patch_burn_on_read)
}

/// Resolve the `#creationId` references of one `Message/set` update patch
/// and run the checks that need only the request, returning the patch or
/// the SetError value for `notUpdated`.
fn update_patch_error(
    mut patch_val: Value,
    created_ids: &BTreeMap<Id, Id>,
    using: &[String],
) -> Result<Value, Value> {
    // An edit may replace `mentions`; `attachments` is immutable.
    if let Err(err) = resolve_entry_refs(&mut patch_val, &[MENTION_ID], created_ids) {
        return Err(serde_json::Value::from(&*err));
    }

    // RFC 8620 §5.3: "There MUST NOT be two patches in the
    // PatchObject where the pointer of one is the prefix of the
    // pointer of the other" or the update "MUST be rejected with an
    // "invalidPatch" error".
    if patch_val.get("reactions").is_some()
        && patch_val
            .as_object()
            .is_some_and(|obj| obj.keys().any(|k| k.starts_with("reactions/")))
    {
        return Err(json!({
            "type": "invalidPatch",
            "description":
                "patch must not combine top-level `reactions` with `reactions/{id}` entries",
        }));
    }

    // draft-ietf-jmap-metadata-05 §3.2: patch errors "that can be
    // determined from the request alone ... MUST be reported with
    // "invalidPatch"" before `notFound`.
    if let Some(Err(err)) = patch_val
        .as_object()
        .map(|o| check_metadata_entry(o, false, using))
    {
        return Err(serde_json::Value::from(&*err));
    }
    if let Some(Err(e)) = patch_val.as_object().map(jmap_server::check_patch_paths) {
        return Err(json!({ "type": "invalidPatch", "description": e.to_string() }));
    }
    Ok(patch_val)
}

/// The stored Message `id` a `Message/set` update targets and its Chat,
/// read once for every check, or the SetError value for `notUpdated`.
async fn update_targets<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    id: &Id,
) -> Result<(Message, Option<Chat>), Value> {
    let stored = match ctx
        .backend
        .get_objects::<Message>(
            ctx.caller,
            ctx.account_id,
            Some(std::slice::from_ref(id)),
            None,
        )
        .await
    {
        Ok((found, _)) => found.into_iter().next(),
        Err(e) => return Err(server_fail_value_from_backend(&e)),
    };
    let Some(stored) = stored else {
        return Err(Value::from(&SetError::new(SetErrorType::NotFound)));
    };
    let stored_chat = match ctx
        .backend
        .get_objects::<Chat>(
            ctx.caller,
            ctx.account_id,
            Some(std::slice::from_ref(&stored.chat_id)),
            None,
        )
        .await
    {
        Ok((found, _)) => found.into_iter().next(),
        Err(e) => return Err(server_fail_value_from_backend(&e)),
    };
    Ok((stored, stored_chat))
}

/// Whether `k` is a `Message/set` update key the draft defines.
///
/// draft-atwood-jmap-chat-00 §Message/set defines the only
/// client updates: Editing (`body`, `bodyType`, `mentions`,
/// `broadcastMentions`), Adding and Removing Reactions
/// (`reactions/<senderReactionId>` JSON Pointer keys), Deleting
/// (`deletedAt`, `deletedForAll`) and Marking as Read (`readAt`,
/// `readDisposition`): the client-settable Message properties
/// that are not immutable, with `reactions` changed only per
/// entry. Every other key names a server-set or immutable
/// property, so it is rejected with RFC 8620 §5.3
/// `invalidProperties`. draft-ietf-jmap-metadata-05 §3.2 adds
/// `metadata` and `privateMetadata`, written whole or by patch
/// path.
fn is_defined_message_key(k: &str) -> bool {
    k.starts_with("reactions/")
        || is_metadata_key(k)
        || (k != "reactions" && is_client_updatable::<Message>(k))
}

/// The keys of a `Message/set` update patch that the draft defines.
fn defined_message_keys(patch_val: &Value) -> Vec<&str> {
    patch_val
        .as_object()
        .map(|obj| {
            obj.keys()
                .map(String::as_str)
                .filter(|k| is_defined_message_key(k))
                .collect()
        })
        .unwrap_or_default()
}

/// Drop the read-only values (and a whole `reactions`) of a `Message/set`
/// update patch that are identical to `stored`, and return the first
/// shape error the patch still has.
///
/// RFC 8620 §5.3: "Any server-set properties MAY be included in
/// the patch if their value is identical to the current server
/// value ... Otherwise, the update MUST be rejected with an
/// "invalidProperties" SetError." Identical values are dropped
/// here, so the checks below see only the client changes.
fn update_shape_error(
    patch_val: &mut Value,
    stored: &Message,
    limits: &ChatLimits,
) -> Option<Value> {
    let mut shape_err: Option<Value> = None;
    if let Value::Object(map) = &mut *patch_val
        && let Err(err) = strip_read_only_patch::<Message>(map, stored)
    {
        shape_err = Some(err);
    }
    // A whole `reactions` identical to the stored map changes
    // nothing: drop it, as an identical server-set value is
    // (RFC 8620 §5.3), so only a real whole-map write is rejected
    // below (bd:JMAP-t8ej.32 item 4).
    if let Value::Object(map) = &mut *patch_val
        && map.get("reactions").is_some_and(|r| {
            serde_json::to_value(stored)
                .ok()
                .is_some_and(|s| s.get("reactions").unwrap_or(&Value::Null) == r)
        })
    {
        map.remove("reactions");
    }
    if shape_err.is_some() {
        return shape_err;
    }

    // Only the keys the draft defines (`is_defined_message_key`).
    let bad_props: Vec<&str> = patch_val
        .as_object()
        .map(|obj| {
            obj.keys()
                .map(String::as_str)
                .filter(|k| !is_defined_message_key(k))
                .collect()
        })
        .unwrap_or_default();
    if !bad_props.is_empty() {
        return Some(json!({ "type": "invalidProperties", "properties": bad_props }));
    }

    // draft-atwood-jmap-chat-00 §Message `readDisposition`: "the
    // server MUST reject an update that sets `readDisposition`
    // without `readAt` with an `invalidProperties` SetError naming
    // `readDisposition`." A null `readAt` clears it, not sets it.
    let sets = |field: &str| patch_val.get(field).is_some_and(|v| !v.is_null());
    if sets("readDisposition") && !sets("readAt") {
        return Some(json!({ "type": "invalidProperties", "properties": ["readDisposition"] }));
    }

    // §Input Validation: "`bodyType`: reject a value not listed in
    // `supportedBodyTypes`".
    if patch_val.get("bodyType").is_some_and(|v| {
        !v.as_str()
            .is_some_and(|s| limits.supported_body_types.contains(&s))
    }) {
        return Some(json!({ "type": "invalidProperties", "properties": ["bodyType"] }));
    }
    None
}

/// The `forbidden` SetError value for a `Message/set` update that deletes
/// for all or edits a Message it may not, or `None`.
///
/// draft-atwood-jmap-chat-00 §Deleting a Message: servers MUST
/// reject `deletedForAll: true` when `senderId` is not the
/// accountId. §Editing a Message: an edit of `body`, `bodyType`,
/// `mentions` or `broadcastMentions` is only permitted where
/// `senderId` is the accountId and `deletedAt` is absent; any other edit is
/// `forbidden`.
fn edit_forbidden(
    stored: &Message,
    account_id: &Id,
    deletes_for_all: bool,
    edits: bool,
) -> Option<Value> {
    let description = if deletes_for_all && stored.sender_id != *account_id {
        "deletedForAll is only permitted on messages whose senderId is the accountId"
    } else if edits && (stored.sender_id != *account_id || stored.deleted_at.is_some()) {
        "edits are only permitted on messages whose senderId is the accountId and deletedAt is absent"
    } else {
        return None;
    };
    Some(json!({ "type": "forbidden", "description": description }))
}

/// The `Peer/retract` or `Peer/deliver` edit event for a `Message/set`
/// update of `stored` that deletes for all or edits it.
fn edit_event(stored: &Message, deletes_for_all: bool) -> OutboundEvent {
    let (chat_id, message_id, sender_msg_id) = (
        stored.chat_id.clone(),
        stored.id.clone(),
        stored.sender_msg_id.clone(),
    );
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
}

/// Add the server-side values to a checked `Message/set` update patch:
/// `editedAt`, the deletion purge, and the `readDisposition` default.
/// Returns the patch and whether it deletes the Message.
fn augment_patch(patch_val: Value, deletes_for_all: bool) -> (Value, bool) {
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
    let deletes = deletes_for_all || augmented.get("deletedAt").is_some_and(|v| !v.is_null());
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
    (augmented, deletes)
}

/// Check the `reactions/{senderReactionId}` keys of an augmented
/// `Message/set` update patch against `stored`, server-set their
/// `senderId` and `sentAt`, and return one `ReactionUpdate` event per
/// patched key, or the SetError value for `notUpdated`.
///
/// Reaction patch processing
/// (draft-atwood-jmap-chat-00 §Message/set, §Reaction).
///
/// Reactions are added/removed by patching
/// `reactions/{senderReactionId}` as RFC 8620 §5.3 JSON
/// Pointer entries:
///
/// - value is an object: add or modify the reaction
/// - value is `null`: remove the reaction
///
/// The backend applies these paths into the stored
/// `reactions` map. Before forwarding, the handler:
///
/// 1. Rejects `reactions/{id}` keys containing `/` or `~`
///    (RFC 6901 escape characters — the chat-client also
///    rejects them; this is defense-in-depth).
/// 2. Rejects non-null non-object values as `invalidPatch`.
/// 3. Pre-fetches the Message and rejects with `forbidden`
///    if the patched key targets an existing reaction whose
///    `senderId` is not the accountId — per spec, only the
///    original sender may modify their own reactions.
/// 4. Server-overrides `senderId` to the accountId on every add
///    (spec MUST; defense-in-depth).
/// 5. Server-injects `sentAt` to the current time on adds
///    that lack it, so the resulting `Reaction` round-trips
///    through the typed shape (which requires `sentAt`).
fn reaction_events(
    augmented: &mut Value,
    stored: &Message,
    account_id: &Id,
) -> Result<Vec<OutboundEvent>, Value> {
    // One `OutboundEvent::ReactionUpdate` per patched key, built
    // from the pre-fetched Message and reported after the write.
    let mut reaction_events: Vec<OutboundEvent> = Vec::new();
    let decoded = decode_reaction_keys(augmented)
        .map_err(|desc| json!({ "type": "invalidPatch", "description": desc }))?;
    if decoded.is_empty() {
        return Ok(reaction_events);
    }

    // Inspect the stored Message's existing reactions.
    let mut forbidden: Vec<String> = Vec::new();
    for (raw_key, suffix) in &decoded {
        let existing = stored.reactions.get(suffix);
        if existing.is_some_and(|r| r.sender_id != *account_id) {
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
                chat_id: stored.chat_id.clone(),
                message_id: stored.id.clone(),
                sender_msg_id: stored.sender_msg_id.clone(),
                sender_reaction_id: suffix.clone(),
                action,
            });
        }
    }
    if !forbidden.is_empty() {
        return Err(json!({
            "type": "forbidden",
            "description": format!(
                "cannot modify reactions authored by another sender: {}",
                forbidden.join(", "),
            ),
        }));
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
    Ok(reaction_events)
}

/// The `reactions/{id}` pointer keys of an augmented `Message/set` update
/// patch as `(key, senderReactionId)`, each checked for its suffix and
/// value shape, or the `invalidPatch` description of the first bad one.
fn decode_reaction_keys(augmented: &Value) -> Result<Vec<(String, String)>, String> {
    let reaction_pointer_keys: Vec<String> = augmented
        .as_object()
        .map(|obj| {
            obj.keys()
                .filter(|k| k.starts_with("reactions/"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    // Validate each pointer key + value shape. The
    // `.collect::<Result<Vec<_>, _>>()` form short-circuits
    // on the first Err and is the idiomatic Rust expression
    // of "validate-or-fail" over an iterator. A future
    // contributor adding a new validation rule appends an
    // `if ... { return Err(...) }` inside the closure; the
    // short-circuit behavior is structural, not a hand-coded
    // flag invariant.
    reaction_pointer_keys
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
        .collect()
}

/// The SetError value when an edit of `stored` keeps or adds a broadcast
/// mention it may not send, or `None`.
///
/// BroadcastMention validation and permission check on edit
/// (§4.4.1 / §4.16). The post-patch broadcastMentions fall back
/// to the stored message where the patch leaves them unchanged,
/// so an edit that retains a broadcast mention is gated like one
/// that adds it.
async fn broadcast_edit_error<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    stored: &Message,
    stored_chat: Option<&Chat>,
    augmented: &Value,
) -> Option<Value> {
    let current_has_bm = !stored.broadcast_mentions.is_empty();
    // Structural validation only when broadcastMentions is part
    // of the patch.
    let has_bm = match augmented.get("broadcastMentions") {
        Some(bm) => {
            let bm_arr: Vec<Value> = bm.as_array().cloned().unwrap_or_default();
            if let Some(err) = validate_broadcast_mentions(&bm_arr) {
                return Some(err);
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
    // A Chat not found is left for update_object to surface
    // downstream.
    let channel = stored_chat.and_then(|c| Some((c.space_id.as_ref()?, &c.id)));
    if let (true, Some((sid, chat_id))) = (has_bm, channel) {
        let outcome = ctx
            .backend
            .may_broadcast_mention(ctx.caller, ctx.account_id, sid, chat_id)
            .await;
        return refusal_value(outcome);
    }
    None
}

/// The `Peer/receipt` event for a `Message/set` update that marks as read
/// a Message another participant sent (`pre_patch_read`, the stored
/// Message when the patch sets `readAt`).
///
/// draft-atwood-jmap-chat-federation-00 §Peer/receipt: the
/// owner read a Message another participant sent. The read
/// fields come from the patch, since a burned Message is gone.
fn read_receipt(
    pre_patch_read: Option<Message>,
    augmented: &Value,
    account_id: &Id,
) -> Option<OutboundEvent> {
    pre_patch_read
        .filter(|m| m.sender_id != *account_id)
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
                sender_id: m.sender_id,
                read_at,
                read_disposition,
            })
        })
}

/// The events a checked `Message/set` update reports once it is written.
struct UpdateEvents {
    /// The `Peer/retract` or edit event, for a delete-for-all or an edit.
    outbound: Option<OutboundEvent>,
    /// One `ReactionUpdate` per patched reaction key.
    reaction_events: Vec<OutboundEvent>,
    /// The `Peer/receipt` event, for a read of another sender's Message.
    receipt: Option<OutboundEvent>,
    /// Whether the patch deletes the Message.
    deletes: bool,
    /// Whether the write hard-deletes a burn-on-read Message.
    burns: bool,
}

/// Write one checked `Message/set` update (or burn), then report its
/// `events`. Returns whether the backend was written.
async fn write_update<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    id_str: String,
    id: &Id,
    augmented: Value,
    events: UpdateEvents,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    // Convert the augmented wire-format Value into a typed
    // PatchObject (RFC 8620 §5.3). Non-object values yield
    // invalidPatch.
    let patch = match serde_json::from_value::<PatchObject>(augmented) {
        Ok(p) => p,
        Err(e) => {
            acc.not_updated.insert(
                id_str,
                json!({ "type": "invalidPatch", "description": e.to_string() }),
            );
            return Ok(false);
        }
    };
    // draft-atwood-jmap-chat-00 §Message `burnOnRead` (jmap-chat-spec
    // 77c3474 L750): "SHOULD permanently hard-delete this message
    // (row removal, not tombstone) immediately after setting
    // `readAt`." Done by choice, as one atomic
    // backend call, so a failure commits nothing and a retry
    // burns again.
    let update_outcome = if events.burns {
        ctx.backend
            .read_and_burn_message(ctx.caller, ctx.account_id, id, patch)
            .await
            .map(|()| None)
    } else {
        ctx.backend
            .update_object::<Message>(ctx.caller, ctx.account_id, id, patch)
            .await
    };
    let maybe_updated_obj = match update_outcome {
        Ok(maybe_obj) => maybe_obj,
        Err(e) => {
            acc.not_updated.insert(id_str, backend_set_error_value(e));
            return Ok(false);
        }
    };

    // Update (or burn) succeeded.
    report_update_events(ctx, &events).await;
    match maybe_updated_obj {
        Some(obj) => {
            acc.updated
                .insert(id_str, jmap_server::serialize_value(&obj)?);
        }
        None => {
            acc.updated.insert(id_str, Value::Null);
        }
    }
    Ok(true)
}

/// Report the `events` of a written `Message/set` update through
/// `ChatEvents::on_outbound_event`.
async fn report_update_events<B: ChatBackend>(ctx: &SetCtx<'_, B>, events: &UpdateEvents) {
    let UpdateEvents {
        outbound,
        reaction_events,
        receipt,
        deletes,
        burns,
    } = events;
    // draft-atwood-jmap-chat-federation-00 §Peer/retract "Sender
    // Behavior": an update that sets `deletedForAll: true` "MUST
    // send `Peer/retract` to the server of every other participant
    // in the Chat". Reported only once the update is written, so a
    // failed write retracts nothing on peers.
    if let Some(event @ OutboundEvent::Retract { .. }) = outbound {
        ctx.backend
            .on_outbound_event(ctx.caller, ctx.account_id, event)
            .await;
    }
    // draft-atwood-jmap-chat-00 §Editing a Message: "Send
    // `Peer/deliver` carrying an `edit` payload to all
    // recipients". A patch that also deletes or burns the Message
    // leaves nothing to deliver.
    if let Some(event @ OutboundEvent::Edit { .. }) = outbound
        && !deletes
        && !burns
    {
        ctx.backend
            .on_outbound_event(ctx.caller, ctx.account_id, event)
            .await;
    }
    // draft-atwood-jmap-chat-00 §Adding and Removing Reactions:
    // propagate each add and removal "via `Peer/deliver`
    // `reactionUpdate` payload". As for edits, a patch that also
    // deletes or burns the Message leaves nothing to deliver.
    if !deletes && !burns {
        for event in reaction_events {
            ctx.backend
                .on_outbound_event(ctx.caller, ctx.account_id, event)
                .await;
        }
    }
    // §Peer/receipt "Sender Behavior": "If the effective preference
    // is `false`, the server SHOULD NOT invoke `Peer/receipt`". A
    // burned Message was still read, so its receipt is reported.
    if let Some(event @ OutboundEvent::Receipt { chat_id, .. }) = receipt
        && shares_receipts(ctx.backend, ctx.caller, ctx.account_id, chat_id).await
    {
        ctx.backend
            .on_outbound_event(ctx.caller, ctx.account_id, event)
            .await;
    }
}

/// The `destroy` part of `Message/set`. Returns whether the backend was
/// written.
async fn set_destroy<B: ChatBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
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
        let chat_of: HashMap<Id, Id> = ctx
            .backend
            .get_objects::<Message>(ctx.caller, ctx.account_id, Some(&destroy_ids), None)
            .await
            .map(|(found, _)| found.into_iter().map(|m| (m.id, m.chat_id)).collect())
            .unwrap_or_default();
        let mut destroyed_chat_ids: Vec<Id> = Vec::new();
        for id_str in id_strs {
            let id = match jmap_server::parse_wire_id(id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    acc.not_destroyed
                        .insert(id_str.to_owned(), serde_json::Value::from(&*err));
                    continue;
                }
            };

            match ctx
                .backend
                .destroy_object::<Message>(ctx.caller, ctx.account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_chat_ids.extend(chat_of.get(&id).cloned());
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed
                        .insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
        record_unread_count_changes(ctx.backend, ctx.caller, ctx.account_id, destroyed_chat_ids)
            .await;
    }
    Ok(mutated)
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
        let backend = MemoryBackend::<()>::default();
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
            &[],
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
        let backend = MemoryBackend::<()>::default();
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
            &[],
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
        let backend = MemoryBackend::<()>::default();
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
            &[],
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
    async fn make_group_chat(backend: &MemoryBackend<()>) -> String {
        backend.register_account(&Id::from(ACCOUNT_ID));
        let HandlerOutput { primary: resp, .. } = crate::chat::handle_chat_set(
            backend,
            &(),
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "create": { "c0": { "kind": "group", "name": "test-group" } }
            }),
            &[],
        )
        .await
        .expect("make_group_chat");
        resp["created"]["c0"]["id"]
            .as_str()
            .expect("group chat id")
            .to_owned()
    }

    /// Helper: seed a Space whose owner member (the accountId, per
    /// §SpaceMember `id`) holds `manage_channels` and `send` (§Message/set,
    /// jmap-chat-spec 77c3474 L1220: "For channel Chats, the server SHOULD
    /// verify the sender holds the `"send"` permission"), then create a channel Chat inside it, and
    /// return the channel's server-assigned id.
    async fn make_channel_chat(backend: &MemoryBackend<()>) -> String {
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
            &std::collections::BTreeMap::new(),
            json!({
                "accountId": ACCOUNT_ID,
                "update": { space_id: { "addChannels": [{ "name": "general" }] } }
            }),
            &[],
        )
        .await
        .expect("make_channel_chat: addChannels");
        let HandlerOutput {
            primary: get_resp, ..
        } = crate::space::handle_space_get(
            backend,
            &(),
            json!({ "accountId": ACCOUNT_ID, "ids": [space_id] }),
            &[],
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
        let backend = MemoryBackend::<()>::default();
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
            &[],
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
        let backend = MemoryBackend::<()>::default();
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
            &[],
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
        let backend = MemoryBackend::<()>::default();
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
            &[],
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
        let backend = MemoryBackend::<()>::default();
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
            &[],
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
            &[],
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
