//! Email/get, Email/changes, Email/query, Email/queryChanges, Email/set,
//! Email/copy, Email/import, Email/parse method handlers (RFC 8621 §4).
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3, `/copy` → §5.4,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by RFC 8621 §4. The returned `Value` is the
//! corresponding method-response object per the same section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3).
//! For the standard `/get`, `/changes`, `/query`, `/queryChanges`,
//! `/set`, `/import`, and `/parse` handlers in this module the vector
//! is **always empty**. `handle_email_copy` is the one exception: it MAY
//! emit an `Email/set` follow-up invocation when `onSuccessDestroyOriginal`
//! is true (RFC 8620 §5.4).
//!
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`, `/import`, `/copy`: `stateMismatch`;
//! - `/copy`: `fromAccountNotFound`;
//! - `/changes`, `/queryChanges`: `cannotCalculateChanges`;
//! - `/queryChanges`: `tooManyChanges`;
//! - `/query`, `/queryChanges`: `unsupportedFilter`;
//! - `/query`: `unsupportedSort`, `anchorNotFound`.
//!
//! Per-target failures inside a `/set` or `/copy` call surface in the
//! `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use std::collections::{BTreeMap, HashMap, HashSet};

use jmap_mail_types::{Email, Keyword};
use jmap_mime::{ComposeError, EmailDraft, compose_email};
use jmap_types::{Date, Id, Invocation, JmapError, PatchObject, State, UTCDate};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::MailBackend;
use crate::helpers::{
    MetadataProjection, SetAccumulators, check_metadata_entry, create_metadata,
    enforce_max_objects_in_get, enforce_max_objects_in_set, extract_account_id,
    finalize_set_response, not_found_json, reject_metadata_filter_without_capability,
    resolve_blob_ref, resolve_blob_refs, resolve_ref, uses_metadata,
};
use crate::project::{
    DEFAULT_EMAIL_GET_PROPERTIES, DEFAULT_EMAIL_PARSE_PROPERTIES, EmailProjection,
    is_email_get_property, parse_header_property, take_body_args, validate_header_form,
};
use jmap_server::{
    HandlerOutput, QueryChangesParams, QueryParams, backend_set_error_value, bool_arg,
    check_read_only_patch, filter_arg, limit_or_max, reject_server_set, resolve_query_offset,
    resolve_query_start, server_fail_from_backend, server_fail_value_from_backend, take_bool_arg,
};
use jmap_server::{SetError, SetErrorType};

// ---------------------------------------------------------------------------
// Email/get (RFC 8621 §4.2)
// ---------------------------------------------------------------------------

/// Handle an `Email/get` method call (RFC 8621 §4.2).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`), augmented with the RFC 8621 §4.2
/// Email-specific arguments (`bodyProperties`, `fetchTextBodyValues`,
/// `fetchHTMLBodyValues`, `fetchAllBodyValues`, `maxBodyValueBytes`); the
/// returned `Value` is the §5.1 `/get` response shape (`accountId`,
/// `state`, `list`, `notFound`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]). When the backend lists Email for
/// draft-ietf-jmap-metadata-05 ([`JmapAccounts::metadata_info`](jmap_server::JmapAccounts::metadata_info)), a
/// requested `metadata` is returned as an object, and with
/// `urn:ietf:params:jmap:metadata` in `using` `properties` may hold
/// `metadata/<namespace>` subselectors; the default property list is
/// unchanged (-05 §2, §3.1).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_get<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    email_get_impl(
        backend,
        caller,
        args,
        using,
        EmailSource::<std::future::Ready<Result<HistoryRows, JmapError>>>::Current,
    )
    .await
}

/// Where [`email_get_impl`] reads the Emails from.
pub(crate) enum EmailSource<'a, F> {
    /// The current Emails (`MailBackend::get_objects`).
    Current,
    /// History entries (draft-ietf-jmap-object-history §3): `fetch` gets
    /// the account id and `ids` and returns (Email, objectHistory) pairs
    /// and the ids not found. The per-Email projection is the same; each
    /// entry gains `objectHistory`.
    History(&'a (dyn Fn(Id, Option<Vec<Id>>) -> F + Sync)),
}

type HistoryRows = (Vec<(Email, Value)>, Vec<Id>);

/// The body of [`handle_email_get`] and the history-aware Email/get.
pub(crate) async fn email_get_impl<B, F>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
    source: EmailSource<'_, F>,
) -> Result<HandlerOutput, JmapError>
where
    B: MailBackend,
    F: std::future::Future<Output = Result<HistoryRows, JmapError>>,
{
    let (account_id, mut args) = extract_account_id(args)?;
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

    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    // RFC 8620 §5.1: "If an invalid property is requested, the call MUST
    // be rejected with an "invalidArguments" error."
    if let Some(props) = &properties {
        let metadata = backend
            .metadata_info::<Email>(caller, &account_id)
            .is_some();
        if let Some(bad) = props.iter().find(|p| !is_email_get_property(p, metadata)) {
            return Err(JmapError::invalid_arguments(format!(
                "properties: '{bad}' is not an Email property"
            )));
        }
    }

    // RFC 8621 §4.2 — body-value fetch arguments (all default to false / 0 / default list).
    let (body_properties, body_fetch_args) = take_body_args(&mut args)?;
    // RFC 8621 §4.1.3 header: properties are validated here, before
    // touching the backend.
    let email_projection = EmailProjection::new(
        properties.as_deref(),
        DEFAULT_EMAIL_GET_PROPERTIES,
        true,
        &body_properties,
        body_fetch_args,
    )?;

    let projection = MetadataProjection::new(
        properties.as_deref(),
        false,
        using,
        backend.metadata_info::<Email>(caller, &account_id).as_ref(),
    )?;

    let (list, not_found): (Vec<(Email, Option<Value>)>, Vec<Id>) = match source {
        EmailSource::Current => {
            let (list, not_found) = backend
                .get_objects::<Email>(caller, &account_id, ids.as_deref(), None)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;
            (list.into_iter().map(|e| (e, None)).collect(), not_found)
        }
        EmailSource::History(fetch) => {
            let (rows, not_found) = fetch(account_id.clone(), ids).await?;
            (
                rows.into_iter().map(|(e, info)| (e, Some(info))).collect(),
                not_found,
            )
        }
    };

    let state = backend
        .get_state::<Email>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    let list_json: Vec<Value> = list
        .iter()
        .map(|(email, object_history)| {
            let mut obj = email_projection.project(email, |val, obj| projection.apply(val, obj))?;
            if let (Some(info), Value::Object(map)) = (object_history, &mut obj) {
                jmap_server::attach_object_history(map, info.clone(), None);
            }
            Ok(obj)
        })
        .collect::<Result<Vec<_>, JmapError>>()?;

    let resp = json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list_json,
        "notFound": not_found_json(&not_found),
    });

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// Email/changes (RFC 8621 §4.3)
// ---------------------------------------------------------------------------

/// Handle an `Email/changes` method call (RFC 8621 §4.3).
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
pub async fn handle_email_changes<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    jmap_server::handle_changes::<Email, B>(backend, caller, args, using).await
}

// ---------------------------------------------------------------------------
// Email/query (RFC 8621 §4.4)
// ---------------------------------------------------------------------------

/// Decode the optional `sort` argument of `Email/query` and
/// `Email/queryChanges`.
///
/// RFC 8621 §4.4.2 lists the Email comparator properties; any other property
/// decodes as [`jmap_mail_types::ComparatorProperty::Other`] and is
/// unsupportedSort (RFC 8620 §5.5). Strings compare byte-wise, so `i;octet`
/// (RFC 4790 §9.3) is the only collation accepted; §5.5 makes "a collation
/// method it does not recognise" unsupportedSort too. A malformed comparator
/// is invalidArguments. An absent or null `sort` yields `None`.
fn email_sort_arg(
    args: &mut serde_json::Map<String, Value>,
) -> Result<Option<Vec<jmap_mail_types::EmailComparator>>, JmapError> {
    let sort: Vec<jmap_mail_types::EmailComparator> = match args.remove("sort") {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => serde_json::from_value(v)
            .map_err(|e| JmapError::invalid_arguments(format!("sort: {e}")))?,
    };
    if sort.iter().any(|c| {
        matches!(c.property, jmap_mail_types::ComparatorProperty::Other(_))
            || c.collation.as_deref().is_some_and(|k| k != "i;octet")
    }) {
        return Err(JmapError::unsupported_sort());
    }
    Ok(Some(sort))
}

/// Check every JMAP Search `where` in the undecoded Email `filter`
/// against `using` and [`MailBackend::search_capability`], and return
/// whether there was one (crate-jmap docs/specs/jmap-search.md §Errors,
/// §Opting In a Data Type). The capability is read only for a filter with
/// `where`.
///
/// # Errors
///
/// As [`jmap_search::check_where_filter`] for the `"Email"` data type, and
/// `serverFail` when the backend fails.
async fn check_email_search<B: MailBackend>(
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
    jmap_search::check_where_filter(filter, "Email", using, cap.as_ref())
}

/// Handle an `Email/query` method call (RFC 8621 §4.4).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`),
/// augmented with the RFC 8621 §4.4.3 Email-specific `collapseThreads`
/// argument; the returned `Value` is the §5.5 `/query` response shape
/// (`accountId`, `queryState`, `canCalculateChanges`, `position`, `ids`,
/// optional `total`, optional `limit`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-05 §3.6
/// filter condition is `unsupportedFilter` (-05 §3). A JMAP Search
/// `where` (crate-jmap docs/specs/jmap-search.md) needs
/// `urn:ietf:params:jmap:search` in `using` and `"Email"` in
/// [`MailBackend::search_capability`], else `unsupportedFilter`; it is
/// checked with [`jmap_search::check_where_filter`] and makes
/// `canCalculateChanges` `false`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_query<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let searched =
        check_email_search(backend, caller, &account_id, args.get("filter"), using).await?;
    let filter: Option<jmap_mail_types::EmailFilter> = filter_arg(&mut args)?;
    reject_metadata_filter_without_capability(
        filter.as_ref(),
        jmap_mail_types::EmailFilterCondition::has_metadata_condition,
        using,
    )?;

    let sort = email_sort_arg(&mut args)?;

    // effective_limit is always a concrete u64 after parsing (default 256 when absent).
    // requested_limit tracks what the client sent so we know when to echo limit back.
    let (effective_limit, requested_limit): (u64, Option<u64>) = match args.remove("limit") {
        None | Some(Value::Null) => (256, None),
        Some(v) => match v.as_u64() {
            Some(n) => (n, Some(n)),
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

    let collapse_threads: bool = take_bool_arg(&mut args, "collapseThreads", false);

    let calculate_total: bool = take_bool_arg(&mut args, "calculateTotal", false);

    // RFC 8620 §5.5: anchor-based pagination overrides position.
    let anchor: Option<Id> = match args.remove("anchor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            Id::new_validated(s.as_str())
                .map_err(|e| JmapError::invalid_arguments(format!("anchor: {e}")))?,
        ),
        Some(v) => {
            return Err(JmapError::invalid_arguments(format!(
                "anchor: expected an Id string or null, got {}",
                jmap_server::json_type_name(&v)
            )));
        }
    };
    let anchor_offset: i64 = match args.remove("anchorOffset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_i64().ok_or_else(|| {
            JmapError::invalid_arguments(format!(
                "anchorOffset: expected an integer, got {}",
                jmap_server::json_type_name(&v)
            ))
        })?,
    };

    let sort_slice = sort.as_deref();

    // collapseThreads needs the result set in-process. Without it,
    // position, anchor and limit go to the backend, which pages exactly
    // and reports anchorNotFound (RFC 8620 §5.5) over the whole result.
    let (ids, total, query_state, can_calculate_changes, reported_position) = if collapse_threads {
        // Fast path: effective_limit=0, no total and no anchor to
        // resolve (anchorNotFound must still be reported) — skip the
        // full fetch; a zero-limit query gives queryState and
        // canCalculateChanges.
        if effective_limit == 0 && !calculate_total && anchor.is_none() {
            let empty = backend
                .query_objects::<Email>(
                    caller,
                    &account_id,
                    QueryParams::new()
                        .filter(filter.as_ref())
                        .sort(sort_slice)
                        .limit(Some(0)),
                )
                .await
                .map_err(JmapError::from)?;
            return Ok(HandlerOutput::new(serde_json::json!({
                "accountId": account_id.as_ref(),
                "queryState": empty.query_state.as_ref(),
                "canCalculateChanges": empty.can_calculate_changes && !searched,
                "position": 0u64,
                "ids": [],
            })));
        }

        // Fetch cap+1 to detect whether the backend had more results than
        // the cap.  If more than cap items come back the result was
        // truncated; truncate to cap and report total as an approximation.
        let collapse_cap = backend.max_collapse_threads_emails(caller, &account_id);
        let all = backend
            .query_objects::<Email>(
                caller,
                &account_id,
                QueryParams::new()
                    .filter(filter.as_ref())
                    .sort(sort_slice)
                    .limit(Some(collapse_cap as u64 + 1)),
            )
            .await
            .map_err(JmapError::from)?;
        let fetched_count = all.ids.len();
        let query_state = all.query_state.clone();
        let can_calculate_changes = all.can_calculate_changes;

        // We requested cap+1 to detect overflow: more than cap returned means
        // the result set was larger than the cap and we must truncate.  Exactly
        // cap returned means we did NOT overflow (the +1 "probe" came back empty).
        let was_capped = fetched_count > collapse_cap;
        let ids_for_collapse: Vec<Id> = if was_capped {
            all.ids.into_iter().take(collapse_cap).collect()
        } else {
            all.ids
        };

        let all_ids = if collapse_threads {
            collapse_by_thread(backend, caller, &account_id, ids_for_collapse)
                .await
                .map_err(|e| server_fail_from_backend(&e))?
        } else {
            ids_for_collapse
        };

        // When not capped, total is exact. When capped it is the number
        // of collapsed Threads among the first `collapse_cap` Emails, a
        // lower bound (RFC 8620 §5.5 requires total when calculateTotal
        // is true). Paging and anchors also see only those Emails.
        let total: Option<u64> = Some(all_ids.len() as u64);

        // Resolve start position: anchor overrides position.
        let start = if let Some(ref anchor_id) = anchor {
            resolve_query_start(&all_ids, position, Some(anchor_id), anchor_offset)?
        } else {
            // bd:JMAP-qz9v.48 — centralized i64-to-usize bounds /
            // i64::MIN handling in jmap_server::resolve_query_offset.
            resolve_query_offset(position, all_ids.len())
        };

        let page: Vec<Id> = all_ids
            .into_iter()
            .skip(start)
            .take(limit_or_max(Some(effective_limit)))
            .collect();
        (
            page,
            total,
            query_state,
            can_calculate_changes,
            start as u64,
        )
    } else {
        let result = backend
            .query_objects::<Email>(
                caller,
                &account_id,
                QueryParams::new()
                    .filter(filter.as_ref())
                    .sort(sort_slice)
                    .limit(Some(effective_limit))
                    .position(position)
                    .anchor(anchor.as_ref())
                    .anchor_offset(anchor_offset),
            )
            .await
            .map_err(JmapError::from)?;
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

    // RFC 8620 §5.5: total MUST be omitted when calculateTotal is false (default).
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "queryState": query_state.as_ref(),
        "canCalculateChanges": can_calculate_changes && !searched,
        "position": reported_position,
        "ids": ids.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
    });
    if calculate_total && let Some(t) = total {
        resp["total"] = json!(t);
    }
    // RFC 8620 §5.5: return limit if server applied a cap different from what the client sent.
    if requested_limit != Some(effective_limit) {
        resp["limit"] = json!(effective_limit);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// Email/queryChanges (RFC 8621 §4.5)
// ---------------------------------------------------------------------------

/// Handle an `Email/queryChanges` method call (RFC 8621 §4.5).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`),
/// augmented with the RFC 8621 §4.4.3 Email-specific `collapseThreads`
/// argument; the returned `Value` is the §5.6 `/queryChanges` response
/// shape (`accountId`, `oldQueryState`, `newQueryState`, optional
/// `total`, `removed`, `added`).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]); without
/// `urn:ietf:params:jmap:metadata` a draft-ietf-jmap-metadata-05 §3.6
/// filter condition is `unsupportedFilter` (-05 §3). A filter with a
/// JMAP Search `where` is checked as for [`handle_email_query`] and then
/// answered with `cannotCalculateChanges` (jmap-search §Query Changes).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_query_changes<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let since_query_state: State = match args.remove("sinceQueryState") {
        Some(Value::String(s)) => State::from(s.as_str()),
        _ => return Err(JmapError::invalid_arguments("sinceQueryState is required")),
    };

    let searched =
        check_email_search(backend, caller, &account_id, args.get("filter"), using).await?;
    let filter: Option<jmap_mail_types::EmailFilter> = filter_arg(&mut args)?;
    reject_metadata_filter_without_capability(
        filter.as_ref(),
        jmap_mail_types::EmailFilterCondition::has_metadata_condition,
        using,
    )?;

    let sort = email_sort_arg(&mut args)?;
    // jmap-search §Query Changes: "Servers MAY return
    // `cannotCalculateChanges` for any `/queryChanges` call whose filter
    // uses `where`".
    if searched {
        return Err(JmapError::cannot_calculate_changes());
    }

    let max_changes = jmap_server::parse_max_changes(args.remove("maxChanges").as_ref())?;

    let up_to_id: Option<Id> = match args.remove("upToId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            Id::new_validated(s.as_str())
                .map_err(|e| JmapError::invalid_arguments(format!("upToId: {e}")))?,
        ),
        Some(_) => {
            return Err(JmapError::invalid_arguments(
                "upToId must be a string Id or null",
            ));
        }
    };

    // RFC 8621 §4.5: collapseThreads mirrors the argument from the original
    // Email/query that produced the sinceQueryState. Backends that track
    // per-query result sets use it to return thread-collapsed deltas.
    let collapse_threads: bool = take_bool_arg(&mut args, "collapseThreads", false);

    let calculate_total: bool = take_bool_arg(&mut args, "calculateTotal", false);

    let sort_slice = sort.as_deref();
    let result = backend
        .query_changes::<Email>(
            caller,
            &account_id,
            QueryChangesParams::new(&since_query_state)
                .filter(filter.as_ref())
                .sort(sort_slice)
                .max_changes(max_changes)
                .up_to_id(up_to_id.as_ref())
                .collapse_threads(collapse_threads),
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
        .map(|id| Value::String(id.as_ref().to_owned()))
        .collect();

    // RFC 8620 §5.6: total MUST be omitted unless calculateTotal is true.
    let mut resp = json!({
        "accountId": account_id.as_ref(),
        "oldQueryState": result.old_query_state.as_ref(),
        "newQueryState": result.new_query_state.as_ref(),
        "removed": removed_json,
        "added": added_json,
    });
    if calculate_total && let Some(t) = result.total {
        resp["total"] = json!(t);
    }

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// Email/set (RFC 8621 §4.6)
// ---------------------------------------------------------------------------

/// Handle an `Email/set` method call (RFC 8621 §4.6).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps); the
/// returned `Value` is the §5.3 `/set` response shape (`accountId`,
/// `oldState`, `newState`, plus the per-operation `created` /
/// `notCreated` / `updated` / `notUpdated` / `destroyed` / `notDestroyed`
/// maps).
///
/// `#creationId` references among the `mailboxIds` keys of a create, a
/// `mailboxIds` patch value or a `mailboxIds/<id>` patch path resolve
/// against `created_ids` (pass [`jmap_server::CallContext::created_ids`];
/// RFC 8620 §5.3).
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]): a write to `metadata` or
/// `privateMetadata` without `urn:ietf:params:jmap:metadata` is
/// `invalidProperties` (draft-ietf-jmap-metadata-05 §3), as is `null` for
/// either in a create (-05 §3.2). The backend validates the rest.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_set<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.2). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<Email>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let ctx = SetCtx {
        backend,
        caller,
        account_id: &account_id,
        created_ids,
        using,
    };
    let mut acc = SetAccumulators::default();
    let mut mutated = set_create(&ctx, &mut args, &mut acc).await?;
    jmap_server::resolve_set_id_refs(&mut args, created_ids, &acc.created);
    mutated |= set_update(&ctx, &mut args, &mut acc).await?;
    mutated |= set_destroy(&ctx, &mut args, &mut acc).await?;

    finalize_set_response::<B, Email>(backend, caller, &account_id, old_state, mutated, acc).await
}

/// What every `Email/set` sub-function needs from the request
/// (docs/design/set-handler-split.md).
struct SetCtx<'a, B: MailBackend> {
    backend: &'a B,
    caller: &'a B::CallerCtx,
    account_id: &'a Id,
    /// The request-wide `#creationId` map (RFC 8620 §5.3).
    created_ids: &'a BTreeMap<Id, Id>,
    /// The request's `using` capabilities.
    using: &'a [String],
}

/// The `create` part of `Email/set`. Returns whether the backend was
/// written.
async fn set_create<B: MailBackend>(
    ctx: &SetCtx<'_, B>,
    args: &mut serde_json::Map<String, Value>,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    let mut mutated = false;
    if let Some(create_map) = args.get("create").and_then(|v| v.as_object()) {
        for (create_id, obj_val) in create_map {
            mutated |= create_one(ctx, create_id, obj_val, acc).await?;
        }
    }
    Ok(mutated)
}

/// One `create` entry of `Email/set`. Returns whether the backend was
/// written.
async fn create_one<B: MailBackend>(
    ctx: &SetCtx<'_, B>,
    create_id: &str,
    obj_val: &Value,
    acc: &mut SetAccumulators,
) -> Result<bool, JmapError> {
    if let Some(err) = obj_val.as_object().and_then(reject_server_set::<Email>) {
        acc.not_created
            .insert(create_id.to_owned(), serde_json::Value::from(&err));
        return Ok(false);
    }
    if let Some(Err(err)) = obj_val
        .as_object()
        .map(|o| check_metadata_entry(o, true, ctx.using))
    {
        acc.not_created
            .insert(create_id.to_owned(), serde_json::Value::from(&*err));
        return Ok(false);
    }
    let mut obj_val = obj_val.clone();
    if let Err(err) = resolve_mailbox_id_refs(&mut obj_val, ctx.created_ids) {
        acc.not_created
            .insert(create_id.to_owned(), serde_json::Value::from(&*err));
        return Ok(false);
    }
    resolve_blob_refs(&mut obj_val, &["blobId"], ctx.created_ids);
    let obj_val = &obj_val;
    // Validate: at least one mailboxId with value true is required (RFC 8621 §5.5.3).
    let mailbox_ids_ok = obj_val
        .get("mailboxIds")
        .and_then(|v| v.as_object())
        .map(|m| m.values().any(|v| v.as_bool() == Some(true)))
        .unwrap_or(false);

    if !mailbox_ids_ok {
        acc.not_created.insert(
            create_id.to_owned(),
            json!({
                "type": "invalidProperties",
                "properties": ["mailboxIds"],
            }),
        );
        return Ok(false);
    }

    // Validate client-supplied receivedAt up-front so malformed
    // values surface as invalidProperties rather than silently
    // truncating inside build_email_from_create's UTCDate::from.
    // Helper is shared with Email/import and Email/copy
    // (`bd:JMAP-j7pa.4`).
    if let Err(err) = parse_received_at_field(obj_val) {
        acc.not_created.insert(create_id.to_owned(), err);
        return Ok(false);
    }

    // Compose the message and the builder for the stored Email.
    let (message, build) =
        match build_email_from_create(obj_val, ctx.account_id, ctx.backend, ctx.caller).await {
            Ok(b) => b,
            Err(err) => {
                acc.not_created.insert(create_id.to_owned(), err);
                return Ok(false);
            }
        };

    match ctx
        .backend
        .create_email(ctx.caller, ctx.account_id, create_id, message, build)
        .await
    {
        Ok((server_id, created_obj)) => {
            // RFC 8621 §5.5: created map contains only server-set fields.
            acc.created.insert(
                create_id.to_owned(),
                json!({
                    "id": server_id.as_ref(),
                    "blobId": created_obj.blob_id.as_ref(),
                    "threadId": created_obj.thread_id.as_ref(),
                    "size": created_obj.size,
                }),
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

/// The `update` part of `Email/set`. Returns whether the backend was
/// written.
async fn set_update<B: MailBackend>(
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

/// One `update` entry of `Email/set`. Returns whether the backend was
/// written.
async fn update_one<B: MailBackend>(
    ctx: &SetCtx<'_, B>,
    id_str: String,
    mut patch_val: Value,
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
    if let Err(err) = resolve_mailbox_id_refs(&mut patch_val, ctx.created_ids) {
        acc.not_updated
            .insert(id_str, serde_json::Value::from(&*err));
        return Ok(false);
    }

    // Convert wire-format Value into a typed PatchObject. RFC 8620
    // §5.3 mandates a PatchObject is a JSON Object; non-object
    // values produce an `invalidPatch` SetError.
    let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
        Ok(p) => p,
        Err(e) => {
            acc.not_updated.insert(
                id_str,
                json!({ "type": "invalidPatch", "description": e.to_string() }),
            );
            return Ok(false);
        }
    };

    // A changed server-set or immutable value is rejected, an
    // identical one dropped (RFC 8620 §5.3); nothing left is a
    // no-op success.
    match check_read_only_patch::<_, Email>(
        ctx.backend,
        ctx.caller,
        ctx.account_id,
        &id,
        patch.as_map_mut(),
    )
    .await
    {
        Ok(false) => {}
        Ok(true) => {
            acc.updated.insert(id_str, Value::Null);
            return Ok(false);
        }
        Err(err) => {
            acc.not_updated.insert(id_str, err);
            return Ok(false);
        }
    }
    if let Err(err) = check_metadata_entry(patch.as_map(), false, ctx.using) {
        acc.not_updated
            .insert(id_str, serde_json::Value::from(&*err));
        return Ok(false);
    }
    let shape_error = match check_email_patch(patch.as_map_mut()) {
        Ok(false) => None,
        Ok(true) => last_mailbox_removed(ctx, &id, &patch).await?,
        Err(err) => Some(err),
    };
    if let Some(err) = shape_error {
        acc.not_updated.insert(id_str, err);
        return Ok(false);
    }

    match ctx
        .backend
        .update_object::<Email>(ctx.caller, ctx.account_id, &id, patch)
        .await
    {
        Ok(Some(obj)) => {
            acc.updated
                .insert(id_str, jmap_server::serialize_value(&obj)?);
            Ok(true)
        }
        Ok(None) => {
            acc.updated.insert(id_str, Value::Null);
            Ok(true)
        }
        Err(e) => {
            acc.not_updated.insert(id_str, backend_set_error_value(e));
            Ok(false)
        }
    }
}

/// For a `patch` that removes a Mailbox by path, the `invalidProperties`
/// SetError value when no Mailbox of Email `id` would remain, else `None`.
///
/// # Errors
///
/// `serverFail` when the backend cannot read the Email.
async fn last_mailbox_removed<B: MailBackend>(
    ctx: &SetCtx<'_, B>,
    id: &Id,
    patch: &PatchObject,
) -> Result<Option<Value>, JmapError> {
    // A Mailbox is removed by path: one must remain.
    let (current, _) = ctx
        .backend
        .get_objects::<Email>(
            ctx.caller,
            ctx.account_id,
            Some(std::slice::from_ref(id)),
            None,
        )
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let Some(email) = current.first() else {
        return Ok(None);
    };
    let remaining = email.mailbox_ids.keys().any(|m| {
        !matches!(
            patch.as_map().get(&format!("mailboxIds/{}", m.as_ref())),
            Some(Value::Null)
        )
    }) || patch
        .as_map()
        .iter()
        .any(|(k, v)| k.starts_with("mailboxIds/") && v == &Value::Bool(true));
    if remaining {
        return Ok(None);
    }
    Ok(Some(
        json!({ "type": "invalidProperties", "properties": ["mailboxIds"],
                "description": "an Email must belong to at least one Mailbox" }),
    ))
}

/// The `destroy` part of `Email/set`. Returns whether the backend was
/// written.
///
/// # Errors
///
/// `invalidArguments` when an element of `destroy` is not a string.
async fn set_destroy<B: MailBackend>(
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
                .destroy_object::<Email>(ctx.caller, ctx.account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    acc.destroyed.push(Value::String(id_str.to_owned()));
                }
                Err(e) => {
                    acc.not_destroyed
                        .insert(id_str.to_owned(), backend_set_error_value(e));
                }
            }
        }
    }
    Ok(mutated)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const FORBIDDEN_KEYWORD_CHARS: &[u8] = b"(){]%*\"\\";
const MAX_KEYWORD_LEN: usize = 255;

/// Validate and normalize a raw keyword map from the wire format.
///
/// RFC 8621 §4.1.1 rules:
/// - Each keyword must be 1–255 bytes long.
/// - All bytes must be in printable ASCII range 0x21–0x7e.
/// - Must not contain: `( ) { ] % * " \`
/// - Keywords are normalized to ASCII lowercase before storage.
/// - False-valued entries are silently dropped (keyword is absent).
///
/// Returns the validated, normalized map or a descriptive error string for
/// use in an `invalidProperties` SetError.
fn validate_and_normalize_keywords(
    raw: BTreeMap<String, bool>,
) -> Result<BTreeMap<Keyword, bool>, String> {
    raw.into_iter()
        .filter(|(_, v)| *v) // false-valued keyword means absent — skip
        .map(|(kw, _)| Ok((Keyword::from(normalize_keyword(&kw)?), true)))
        .collect()
}

/// `kw` checked against RFC 8621 §4.1.1 and lowercased ("servers MUST
/// return keywords in lowercase").
fn normalize_keyword(kw: &str) -> Result<String, String> {
    if kw.is_empty() || kw.len() > MAX_KEYWORD_LEN {
        return Err(format!(
            "keyword '{kw}' has invalid length (must be 1–255 bytes)"
        ));
    }
    if let Some(b) = kw
        .bytes()
        .find(|&b| !(0x21..=0x7e).contains(&b) || FORBIDDEN_KEYWORD_CHARS.contains(&b))
    {
        return Err(format!(
            "keyword '{kw}' contains forbidden character 0x{b:02x}"
        ));
    }
    Ok(kw.to_ascii_lowercase())
}

/// Check and normalize the `keywords` and `mailboxIds` parts of an
/// Email/set update patch (RFC 8621 §4.1.1): each keyword valid and
/// lowercased, every value `true` (a `keywords/...` or `mailboxIds/...`
/// path may also be `null`, which removes it), and a whole `mailboxIds`
/// non-empty. Returns whether the patch removes a Mailbox by path, so the
/// caller must check that one remains ("An Email in the mail store MUST
/// belong to one or more Mailboxes at all times").
///
/// # Errors
///
/// The `invalidProperties` SetError naming the property.
fn check_email_patch(patch: &mut serde_json::Map<String, Value>) -> Result<bool, Value> {
    let invalid = |prop: &str, why: String| json!({ "type": "invalidProperties", "properties": [prop], "description": why });
    let mut removes_mailbox = false;
    let mut renamed: Vec<(String, String)> = Vec::new();
    for (key, value) in patch.iter_mut() {
        if key == "keywords" {
            let Some(map) = value.as_object() else {
                return Err(invalid("keywords", "keywords must be an object".to_owned()));
            };
            let mut out = serde_json::Map::new();
            for (kw, v) in map {
                if v != &Value::Bool(true) {
                    return Err(invalid(
                        "keywords",
                        format!("keyword '{kw}' must map to true"),
                    ));
                }
                out.insert(
                    normalize_keyword(kw).map_err(|e| invalid("keywords", e))?,
                    Value::Bool(true),
                );
            }
            *value = Value::Object(out);
        } else if let Some(kw) = key.strip_prefix("keywords/") {
            if !matches!(value, Value::Bool(true) | Value::Null) {
                return Err(invalid(
                    "keywords",
                    format!("keyword '{kw}' must be set to true or null"),
                ));
            }
            let plain = kw.replace("~1", "/").replace("~0", "~");
            normalize_keyword(&plain).map_err(|e| invalid("keywords", e))?;
            let lower = format!("keywords/{}", kw.to_ascii_lowercase());
            if &lower != key {
                renamed.push((key.clone(), lower));
            }
        } else if key == "mailboxIds" {
            let ok = value
                .as_object()
                .is_some_and(|m| !m.is_empty() && m.values().all(|v| v == &Value::Bool(true)));
            if !ok {
                return Err(invalid(
                    "mailboxIds",
                    "mailboxIds must be a non-empty object of Ids mapped to true".to_owned(),
                ));
            }
        } else if key.starts_with("mailboxIds/") {
            match value {
                Value::Bool(true) => {}
                Value::Null => removes_mailbox = true,
                _ => {
                    return Err(invalid(
                        "mailboxIds",
                        format!("{key} must be set to true or null"),
                    ));
                }
            }
        }
    }
    for (old, new) in renamed {
        if let Some(v) = patch.remove(&old) {
            patch.insert(new, v);
        }
    }
    Ok(removes_mailbox && !patch.contains_key("mailboxIds"))
}

/// Replace `#creationId` mailbox references in an Email create or patch
/// with the ids they name (RFC 8620 §5.3): the keys of a `mailboxIds`
/// object and the id segment of a `mailboxIds/<id>` patch path. A literal
/// key or path segment that is not an RFC 8620 §1.2 Id is rejected.
///
/// # Errors
///
/// An `invalidProperties` [`jmap_server::SetError`] naming `mailboxIds` when a
/// literal is not a valid Id or a reference matches no record created
/// earlier in the request.
pub(crate) fn resolve_mailbox_id_refs(
    obj: &mut Value,
    created_ids: &BTreeMap<Id, Id>,
) -> Result<(), Box<jmap_server::SetError>> {
    let Some(obj) = obj.as_object_mut() else {
        return Ok(());
    };
    if let Some(Value::Object(mailbox_ids)) = obj.get_mut("mailboxIds") {
        let mut resolved = serde_json::Map::with_capacity(mailbox_ids.len());
        for (key, value) in std::mem::take(mailbox_ids) {
            let id = resolve_ref(&key, "mailboxIds", created_ids)?;
            resolved.insert(id.as_ref().to_owned(), value);
        }
        *mailbox_ids = resolved;
    }
    let ref_paths: Vec<String> = obj
        .keys()
        .filter(|k| k.starts_with("mailboxIds/"))
        .cloned()
        .collect();
    for path in ref_paths {
        let id = resolve_ref(&path["mailboxIds/".len()..], "mailboxIds", created_ids)?;
        if let Some(value) = obj.remove(&path) {
            obj.insert(format!("mailboxIds/{}", id.as_ref()), value);
        }
    }
    Ok(())
}

/// Parse and validate `mailboxIds` from an Email/import or Email/copy entry.
///
/// Returns the list of mailbox IDs (entries with `true` value), or an
/// `invalidProperties` error `Value` suitable for inserting into `notCreated`.
///
/// The `rfc_ref` string is embedded in the "required" error description so
/// callers can cite the correct RFC section (§5.7 for import, §6.1 for copy).
fn parse_mailbox_ids(entry: &Value, rfc_ref: &str) -> Result<Vec<Id>, Value> {
    let mailbox_ids: Vec<Id> = match entry.get("mailboxIds").and_then(|v| v.as_object()) {
        Some(m) => m
            .iter()
            .filter(|(_, v)| v.as_bool() == Some(true))
            .map(|(k, _)| {
                jmap_server::parse_wire_id(k, "mailboxIds")
                    .map_err(|e| serde_json::Value::from(&*e))
            })
            .collect::<Result<_, _>>()?,
        None => {
            return Err(json!({"type": "invalidProperties", "properties": ["mailboxIds"]}));
        }
    };
    if mailbox_ids.is_empty() {
        return Err(json!({
            "type": "invalidProperties",
            "properties": ["mailboxIds"],
            "description": format!("at least one mailboxId is required ({rfc_ref})")
        }));
    }
    Ok(mailbox_ids)
}

/// Parse and validate the optional `receivedAt` field of an Email/set
/// create, Email/import, or Email/copy entry.
///
/// `receivedAt` is a client-supplied RFC 8620 §1.4 UTCDate. A malformed
/// value MUST produce `invalidProperties` rather than flowing into the
/// stored object — `UTCDate::from` is infallible and would silently
/// truncate / pad a malformed string, breaking sort order downstream.
///
/// Returns `Ok(Some(parsed))` for a valid date, `Ok(None)` for an absent
/// or null field, or `Err(value)` where `value` is the SetError-shaped
/// JSON suitable for inserting into `notCreated` / `notImported` /
/// `notCopied`. Single source of truth for the three call sites in
/// Email/set, Email/import, and Email/copy (`bd:JMAP-j7pa.4`).
fn parse_received_at_field(entry: &Value) -> Result<Option<UTCDate>, Value> {
    match entry.get("receivedAt") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_str() {
            Some(s) => match UTCDate::new_validated(s) {
                Ok(d) => Ok(Some(d)),
                Err(_) => Err(json!({
                    "type": "invalidProperties",
                    "properties": ["receivedAt"],
                })),
            },
            None => Err(json!({
                "type": "invalidProperties",
                "properties": ["receivedAt"],
            })),
        },
    }
}

/// Parse and validate `keywords` from an Email/import or Email/copy entry.
///
/// Returns the normalized keyword list, or an `invalidProperties` error
/// `Value` suitable for inserting into `notCreated`.
fn parse_keywords_field(entry: &Value) -> Result<Vec<Keyword>, Value> {
    let raw_keywords_map: BTreeMap<String, bool> = match entry.get("keywords") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(v) => match BTreeMap::<String, bool>::deserialize(v) {
            Ok(kws) => kws,
            Err(_) => {
                return Err(json!({"type": "invalidProperties", "properties": ["keywords"]}));
            }
        },
    };
    match validate_and_normalize_keywords(raw_keywords_map) {
        Ok(kws) => Ok(kws.into_keys().collect()),
        Err(desc) => Err(json!({
            "type": "invalidProperties",
            "properties": ["keywords"],
            "description": desc
        })),
    }
}

/// RFC 8621 §4.6 — validate a single EmailBodyPart on creation.
///
/// Rules enforced:
/// - `partId` and `blobId` are mutually exclusive.
/// - If `partId` is present it MUST exist as a key in `body_values`.
/// - The corresponding `EmailBodyValue` MUST NOT have `isEncodingProblem` or
///   `isTruncated` set to `true`.
/// - A part without `subParts` MUST give `partId` or `blobId`: RFC 8621
///   §4.1.4 says each "is null if, and only if, the part is of type
///   "multipart/*"", and `compose_email` has no content for such a part.
/// - Sub-parts are validated recursively.
fn validate_body_part(
    part: &Value,
    body_values: Option<&serde_json::Map<String, Value>>,
) -> Result<(), String> {
    validate_create_header_properties(part).map_err(|(_, description)| description)?;
    let given = |name: &str| part.get(name).is_some_and(|v| !v.is_null());
    let (has_part_id, has_blob_id) = (given("partId"), given("blobId"));
    if has_part_id && has_blob_id {
        return Err("EmailBodyPart must not specify both partId and blobId".into());
    }
    if !has_part_id && !has_blob_id && !given("subParts") {
        return Err("EmailBodyPart has no partId, blobId or subParts".into());
    }
    if has_part_id {
        let part_id = part["partId"].as_str().unwrap_or("");
        match body_values {
            None => {
                return Err(format!(
                    "bodyValues required but missing for partId '{part_id}'"
                ));
            }
            Some(bv) => {
                if !bv.contains_key(part_id) {
                    return Err(format!("bodyValues missing entry for partId '{part_id}'"));
                }
                let bv_entry = &bv[part_id];
                if bv_entry
                    .get("isEncodingProblem")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Err(format!(
                        "bodyValues['{part_id}'].isEncodingProblem must not be true on create"
                    ));
                }
                if bv_entry
                    .get("isTruncated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Err(format!(
                        "bodyValues['{part_id}'].isTruncated must not be true on create"
                    ));
                }
            }
        }
    }
    // Recurse into sub-parts.
    if let Some(sub_parts) = part.get("subParts").and_then(|v| v.as_array()) {
        for sub in sub_parts {
            validate_body_part(sub, body_values)?;
        }
    }
    Ok(())
}

/// Compose the RFC 5322 message an `Email/set` create (`obj_val`)
/// describes, and return it with the builder that
/// [`MailBackend::create_email`] calls to make the stored [`Email`]
/// (RFC 8621 §4.6).
///
/// The builder parses the stored message, so the Email's header and body
/// properties are the ones the message carries, as for `Email/import`.
/// `mailboxIds`, `keywords` and `receivedAt` come from the create; the
/// thread id comes from a stored Email whose `messageId` matches an id in
/// the composed message's In-Reply-To or References header field, however
/// the create set it, else a fresh one. When the create gives
/// no `messageId` or `sentAt`, the message gets a generated Message-ID and
/// a Date equal to `receivedAt`: RFC 8621 §4.6 "If not included, the
/// server MUST generate and set a Message-ID header field ... and a Date
/// header field".
///
/// On failure returns the SetError for `notCreated`: `blobNotFound` with
/// `notFound` listing every missing `blobId`, `invalidProperties` for a
/// malformed create, or `serverFail`.
async fn build_email_from_create<B: MailBackend>(
    obj_val: &Value,
    account_id: &Id,
    backend: &B,
    caller: &B::CallerCtx,
) -> Result<
    (
        Vec<u8>,
        impl FnOnce(&Id, &Id, &[u8]) -> Email + Send + use<B>,
    ),
    Value,
> {
    fn invalid(property: &str, description: impl std::fmt::Display) -> Value {
        json!({
            "type": "invalidProperties",
            "properties": [property],
            "description": format!("{property}: {description}"),
        })
    }
    fn field<T: serde::de::DeserializeOwned>(obj: &Value, name: &str) -> Result<Option<T>, Value> {
        match obj.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => T::deserialize(v).map(Some).map_err(|e| invalid(name, e)),
        }
    }

    // mailboxIds: required (already validated non-empty by caller).
    // RFC 8621 §5.5.3: values MUST be true; false means absent — filter out false entries,
    // same as keywords, so the stored object never has false mailboxId entries.
    let mailbox_ids: BTreeMap<Id, bool> = obj_val
        .get("mailboxIds")
        .and_then(|v| BTreeMap::<Id, bool>::deserialize(v).ok())
        .map(|m| m.into_iter().filter(|(_, v)| *v).collect())
        .unwrap_or_default();

    // keywords: optional; validate RFC 8621 §4.1.1 syntax, normalize to lowercase,
    // filter to true entries only.
    let raw_keywords: BTreeMap<String, bool> = match obj_val.get("keywords") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(v) => BTreeMap::<String, bool>::deserialize(v)
            .map_err(|_| invalid("keywords", "invalid keyword or format"))?,
    };
    let keywords =
        validate_and_normalize_keywords(raw_keywords).map_err(|e| invalid("keywords", e))?;

    validate_email_body(obj_val)?;

    // draft-ietf-jmap-metadata-05 §3.2: the create sets the metadata
    // properties; the backend validates them.
    let (metadata, private_metadata) = obj_val
        .as_object()
        .map(create_metadata)
        .transpose()?
        .unwrap_or_default();

    let mut draft = EmailDraft::default();
    draft.from = field(obj_val, "from")?;
    draft.sender = field(obj_val, "sender")?;
    draft.reply_to = field(obj_val, "replyTo")?;
    draft.to = field(obj_val, "to")?;
    draft.cc = field(obj_val, "cc")?;
    draft.bcc = field(obj_val, "bcc")?;
    draft.subject = field(obj_val, "subject")?;
    draft.sent_at = field(obj_val, "sentAt")?;
    draft.message_id = field(obj_val, "messageId")?;
    draft.in_reply_to = field(obj_val, "inReplyTo")?;
    draft.references = field(obj_val, "references")?;
    draft.text_body = field(obj_val, "textBody")?.unwrap_or_default();
    draft.html_body = field(obj_val, "htmlBody")?.unwrap_or_default();
    draft.attachments = field(obj_val, "attachments")?.unwrap_or_default();
    draft.body_structure = field(obj_val, "bodyStructure")?;
    draft.body_values = field(obj_val, "bodyValues")?.unwrap_or_default();
    draft.header_properties = obj_val
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| k.starts_with("header:"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // Whether a non-null `header:` property sets `field`; the server then
    // leaves that field to the client.
    let header_given = |field: &str| {
        draft.header_properties.iter().any(|(k, v)| {
            !v.is_null()
                && k["header:".len()..]
                    .split(':')
                    .next()
                    .is_some_and(|n| n.eq_ignore_ascii_case(field))
        })
    };
    let (date_given, message_id_given) = (header_given("Date"), header_given("Message-ID"));

    // receivedAt: use provided value or now (RFC 8621 §5.5.3).
    // The caller is required to have run `parse_received_at_field` to
    // reject malformed values up-front (see handle_email_set), so this
    // helper only sees either a valid UTCDate string or no field at all.
    let received_at: UTCDate = obj_val
        .get("receivedAt")
        .and_then(|v| v.as_str())
        .map(UTCDate::from)
        .unwrap_or_else(|| UTCDate::from(crate::helpers::now_utc_string().as_ref()));

    if draft.sent_at.is_none() && !date_given {
        draft.sent_at = Some(Date::from(received_at.as_ref()));
    }
    if draft.message_id.is_none() && !message_id_given {
        // RFC 5322 §3.6.4: a unique id-left at a domain; the sender's
        // domain when the create names one.
        let domain = draft
            .from
            .iter()
            .flatten()
            .find_map(|a| a.email.rsplit_once('@').map(|(_, d)| d.to_owned()))
            .unwrap_or_else(|| "jmap.invalid".to_owned());
        draft.message_id = Some(vec![format!("{}@{domain}", next_id())]);
    }

    let mut blobs = BTreeMap::new();
    let mut missing = Vec::new();
    for blob_id in draft.blob_ids() {
        match backend
            .read_blob(caller, account_id, blob_id)
            .await
            .map_err(|e| server_fail_value_from_backend(&e))?
        {
            Some(bytes) => {
                blobs.insert(blob_id.clone(), bytes);
            }
            None => missing.push(blob_id.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(serde_json::Value::from(
            &SetError::new(SetErrorType::BlobNotFound).with_not_found(missing),
        ));
    }

    let message = compose_email(&draft, &blobs).map_err(|e| match e {
        ComposeError::InvalidDate(_) => invalid("sentAt", e),
        ComposeError::InvalidHeaderProperty { property, reason } => invalid(&property, reason),
        _ => json!({ "type": "invalidProperties", "description": e.to_string() }),
    })?;

    // RFC 8621 §3 threads by the message's own In-Reply-To and References,
    // so read them from the composed message: a create may set them via
    // the `inReplyTo`/`references` properties or any `header:` form.
    let parsed = crate::parse::parse_rfc5322_headers(&message);
    let thread_id = assign_thread(
        backend,
        caller,
        account_id,
        &parsed.in_reply_to,
        &parsed.references,
    )
    .await
    .map_err(|e| server_fail_value_from_backend(&e))?;

    Ok((message, move |id: &Id, blob_id: &Id, bytes: &[u8]| {
        let mut email = Email::new(
            id.clone(),
            blob_id.clone(),
            thread_id,
            mailbox_ids,
            bytes.len() as u64,
            received_at,
        );
        email.keywords = keywords;
        let parsed = crate::parse::parse_rfc5322_headers(bytes);
        crate::parse::fill_from_message(&mut email, parsed, bytes, blob_id);
        email.metadata = metadata;
        email.private_metadata = private_metadata;
        email
    }))
}

/// Check the header properties of one Email or EmailBodyPart in a create
/// (RFC 8621 §4.6): "The "headers" property MUST NOT be given ... the
/// client must set each header field as an individual property", and
/// "Header fields MUST NOT be specified in parsed forms that are
/// forbidden for that particular field." `compose_email` checks the
/// other header constraints.
///
/// On failure returns the offending property (`headers` or the `header:`
/// key) and a description.
fn validate_create_header_properties(obj: &Value) -> Result<(), (String, String)> {
    if obj.get("headers").is_some_and(|v| !v.is_null()) {
        return Err((
            "headers".into(),
            "headers must not be given on create; set header: properties instead".into(),
        ));
    }
    for key in obj
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .filter(|k| k.starts_with("header:"))
    {
        let invalid = |e| (key.clone(), format!("{key}: {e}"));
        let req = parse_header_property(key).map_err(invalid)?;
        validate_header_form(&req.name_lower, &req.form).map_err(invalid)?;
    }
    Ok(())
}

/// Validate the body-related fields of an Email creation payload (RFC 8621 §4.6).
///
/// Checks:
/// - `bodyStructure` is mutually exclusive with `textBody`, `htmlBody`, and `attachments`.
/// - `textBody` must be exactly one part of type `text/plain`.
/// - `htmlBody` must be exactly one part of type `text/html`.
/// - All body parts satisfy `validate_body_part`: `partId`/`blobId` rules, and
///   `bodyValues` entries must not have `isEncodingProblem` or `isTruncated` true.
///
/// On failure returns an `invalidProperties` SetError whose `properties`
/// names the top-level property that failed (RFC 8620 §5.3).
fn validate_email_body(obj_val: &Value) -> Result<(), Value> {
    fn invalid(properties: &[&str], description: impl std::fmt::Display) -> Value {
        json!({
            "type": "invalidProperties",
            "properties": properties,
            "description": description.to_string(),
        })
    }
    let given = |name: &str| obj_val.get(name).is_some_and(|v| !v.is_null());
    validate_create_header_properties(obj_val)
        .map_err(|(property, description)| invalid(&[&property], description))?;
    // bodyStructure is mutually exclusive with textBody, htmlBody, and attachments.
    let conflicting: Vec<&str> = ["textBody", "htmlBody", "attachments"]
        .into_iter()
        .filter(|name| given(name))
        .collect();
    if given("bodyStructure") && !conflicting.is_empty() {
        return Err(invalid(
            &[&["bodyStructure"], conflicting.as_slice()].concat(),
            "bodyStructure is mutually exclusive with textBody, htmlBody, and attachments",
        ));
    }

    // textBody / htmlBody: must be exactly one part of the matching type.
    for (property, media_type) in [("textBody", "text/plain"), ("htmlBody", "text/html")] {
        if !given(property) {
            continue;
        }
        let parts = obj_val[property]
            .as_array()
            .ok_or_else(|| invalid(&[property], format!("{property} must be an array")))?;
        if parts.len() != 1 {
            return Err(invalid(
                &[property],
                format!("{property} must contain exactly one body part"),
            ));
        }
        if parts[0].get("type").and_then(|v| v.as_str()) != Some(media_type) {
            return Err(invalid(
                &[property],
                format!("{property} part must be of type {media_type}"),
            ));
        }
    }

    let body_values = obj_val.get("bodyValues").and_then(|v| v.as_object());

    // Validate body parts: partId XOR blobId; partId must be in bodyValues;
    // bodyValues entries must not have isEncodingProblem or isTruncated true.
    let check = |property: &str, part: &Value| {
        validate_body_part(part, body_values)
            .map_err(|e| invalid(&[property], format!("{property}: {e}")))
    };
    for property in ["textBody", "htmlBody", "attachments"] {
        for part in obj_val
            .get(property)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            check(property, part)?;
        }
    }
    if given("bodyStructure") {
        check("bodyStructure", &obj_val["bodyStructure"])?;
    }

    Ok(())
}

/// Assign a thread id for a new email.
///
/// Calls [`MailBackend::find_thread_by_message_ids`] with the union of
/// `in_reply_to` and `references` tokens. Returns the matching thread id if
/// found, a freshly generated id if no existing email references these tokens,
/// or propagates the backend error so the caller can surface it.
async fn assign_thread<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
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
        .find_thread_by_message_ids(caller, account_id, &refs)
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
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static BASE: OnceLock<u64> = OnceLock::new();

    let base = *BASE.get_or_init(|| {
        // bd:JMAP-qz9v.47 — `Duration::as_nanos()` returns u128.
        // `u64::try_from` makes the year-2554 narrowing explicit and
        // falls back to the same sentinel as a `now()` failure rather
        // than silently wrapping.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_nanos()).ok())
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
    caller: &B::CallerCtx,
    account_id: &Id,
    ids: Vec<Id>,
) -> Result<Vec<Id>, B::Error> {
    // Fetch only the query-result emails (not all emails) to get their thread ids.
    // Pass a properties hint so backends with column stores can skip body data.
    let thread_props = vec!["id".to_owned(), "threadId".to_owned()];
    let (emails, _) = backend
        .get_objects::<Email>(caller, account_id, Some(&ids), Some(&thread_props))
        .await?;
    let thread_map: HashMap<Id, Id> = emails.into_iter().map(|e| (e.id, e.thread_id)).collect();

    let mut seen_threads: HashSet<Id> = HashSet::new();
    let mut result = Vec::with_capacity(ids.len());

    for id in ids {
        match thread_map.get(&id) {
            Some(tid) if seen_threads.insert(tid.clone()) => {
                result.push(id);
            }
            Some(_) => {
                // Thread already seen — drop this id from the collapsed view.
            }
            None => {
                // Email absent from thread map (concurrent delete). Skip it: without
                // thread info we cannot safely deduplicate, and including it risks
                // surfacing two emails from the same thread.
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Email/import (RFC 8621 §4.8)
// ---------------------------------------------------------------------------

/// Handle an `Email/import` method call (RFC 8621 §4.8).
///
/// `args` is the RFC 8621 §4.8 `Email/import` request shape (`accountId`,
/// optional `ifInState`, `emails` map of creationId → EmailImport object —
/// each carrying a `blobId`, `mailboxIds`, optional `keywords`, optional
/// `receivedAt`); the returned `Value` is the §4.8 response shape
/// (`accountId`, `oldState`, `newState`, `created` / `notCreated` maps).
///
/// Each entry in `emails` must name a blob already uploaded to the account.
/// The backend parses the raw bytes, assigns a thread, and stores the new email.
///
/// `mailboxIds` keys may be `#creationId` references (RFC 8620 §5.3),
/// resolved against `created_ids` (pass
/// [`jmap_server::CallContext::created_ids`]); an unknown reference fails
/// that entry with `invalidProperties` naming `mailboxIds`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_email_import<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let Some(Value::Object(emails)) = args.remove("emails") else {
        return Err(JmapError::invalid_arguments("emails is required"));
    };

    let old_state = backend
        .get_state::<Email>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();

    for (import_id, mut entry) in emails {
        if let Err(err) = resolve_mailbox_id_refs(&mut entry, created_ids) {
            not_created.insert(import_id, Value::from(&*err));
            continue;
        }
        let Some(s) = entry.get("blobId").and_then(|v| v.as_str()) else {
            not_created.insert(
                import_id,
                json!({"type": "invalidProperties", "properties": ["blobId"]}),
            );
            continue;
        };
        let blob_id: Id =
            match jmap_server::parse_wire_id(&resolve_blob_ref(s, created_ids), "blobId") {
                Ok(id) => id,
                Err(err) => {
                    not_created.insert(import_id, serde_json::Value::from(&*err));
                    continue;
                }
            };

        // Only include mailboxIds whose value is true (RFC 8621 §5.7 requires at least one).
        let mailbox_ids = match parse_mailbox_ids(&entry, "RFC 8621 §5.7") {
            Ok(ids) => ids,
            Err(err) => {
                not_created.insert(import_id, err);
                continue;
            }
        };

        // keywords: String[Boolean] wire format — validate RFC 8621 §4.1.1 syntax.
        let keywords: Vec<jmap_mail_types::Keyword> = match parse_keywords_field(&entry) {
            Ok(kws) => kws,
            Err(err) => {
                not_created.insert(import_id, err);
                continue;
            }
        };

        // receivedAt: client-supplied (RFC 8621 §4.8). Shared helper
        // with Email/set create and Email/copy (`bd:JMAP-j7pa.4`).
        let received_at: Option<UTCDate> = match parse_received_at_field(&entry) {
            Ok(v) => v,
            Err(err) => {
                not_created.insert(import_id, err);
                continue;
            }
        };

        match backend
            .import_email(
                caller,
                &account_id,
                &blob_id,
                &mailbox_ids,
                &keywords,
                received_at.as_ref(),
            )
            .await
        {
            Ok((server_id, email)) => {
                // RFC 8621 §4.8: created entries contain only these 4 server-set fields.
                let obj = json!({
                    "id": server_id.as_ref(),
                    "blobId": email.blob_id.as_ref(),
                    "threadId": email.thread_id.as_ref(),
                    "size": email.size,
                });
                created.insert(import_id, obj);
            }
            Err(e) => {
                not_created.insert(import_id, backend_set_error_value(e));
            }
        }
    }

    let new_state = if created.is_empty() {
        // No successful imports: state has not changed.
        old_state.clone()
    } else {
        backend
            .get_state::<Email>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    };

    let resp = json!({
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// Email/parse (RFC 8621 §4.9)
// ---------------------------------------------------------------------------

/// Handle an `Email/parse` method call (RFC 8621 §4.9).
///
/// `args` is the RFC 8621 §4.9 `Email/parse` request shape (`accountId`,
/// `blobIds`, optional `properties`, optional `bodyProperties`, optional
/// `fetchTextBodyValues` / `fetchHTMLBodyValues` / `fetchAllBodyValues`,
/// optional `maxBodyValueBytes`); the returned `Value` is the §4.9
/// response shape (`accountId`, `parsed` map, `notParsable` Id[],
/// `notFound` Id[]).
///
/// Parses the blobs identified by `blobIds` and returns Email objects without
/// storing them.
///
/// Blobs that exist but cannot be parsed → `notParsable`.
/// Blobs that do not exist → `notFound`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
///
/// A `#creationId` in `blobIds` needs the request's creation ids, which this
/// function does not take: [`register_mail_handlers`](crate::register_mail_handlers)
/// resolves them (RFC 8620 §5.3).
pub async fn handle_email_parse<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    email_parse(backend, caller, &BTreeMap::new(), args).await
}

/// [`handle_email_parse`], resolving each `#creationId` in `blobIds` against
/// `created_ids` ([`jmap_server::CallContext::created_ids`]; RFC 8620 §5.3).
///
/// The response names a blob by the `blobIds` entry as sent, so a resolved
/// reference keys `parsed` (or appears in `notParsable`/`notFound`) as
/// `"#c1"` and the client can match it to its request. A reference that
/// matches no earlier creation is not a valid Id, as before:
/// `invalidArguments`.
pub(crate) async fn email_parse<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    let not_ids = || JmapError::invalid_arguments("blobIds must be an Id array");
    let raw_blob_ids: Vec<String> = match args.remove("blobIds") {
        Some(v) => serde_json::from_value(v).map_err(|_| not_ids())?,
        None => return Err(JmapError::invalid_arguments("blobIds is required")),
    };
    // (as sent, resolved)
    let blob_ids: Vec<(String, Id)> = raw_blob_ids
        .into_iter()
        .map(|raw| {
            let id = Id::new_validated(resolve_blob_ref(&raw, created_ids).into_owned())
                .map_err(|_| not_ids())?;
            Ok((raw, id))
        })
        .collect::<Result<_, JmapError>>()?;

    let properties: Option<Vec<String>> = match args.remove("properties") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            serde_json::from_value(v)
                .map_err(|_| JmapError::invalid_arguments("properties must be a string array"))?,
        ),
    };

    // RFC 8620 §5.1: "If an invalid property is requested, the call MUST
    // be rejected with an "invalidArguments" error." Email/parse takes
    // Email/get's properties (RFC 8621 §4.9).
    if let Some(props) = &properties {
        let metadata = backend
            .metadata_info::<Email>(caller, &account_id)
            .is_some();
        if let Some(bad) = props.iter().find(|p| !is_email_get_property(p, metadata)) {
            return Err(JmapError::invalid_arguments(format!(
                "properties: '{bad}' is not an Email property"
            )));
        }
    }

    // RFC 8621 §4.9 — body-value fetch arguments (all default to false / 0 / default list).
    let (body_properties, body_fetch_args) = take_body_args(&mut args)?;
    // RFC 8621 §4.1.3 header: properties are validated before touching the
    // backend; the §4.9 default list applies when `properties` is null.
    // Unlike /get, `id` is not added when not requested: a parsed Email
    // has none.
    let email_projection = EmailProjection::new(
        properties.as_deref(),
        DEFAULT_EMAIL_PARSE_PROPERTIES,
        false,
        &body_properties,
        body_fetch_args,
    )?;

    let mut parsed = serde_json::Map::new();
    let mut not_parsable: Vec<Value> = Vec::new();
    let mut not_found: Vec<Value> = Vec::new();

    for (requested, blob_id) in &blob_ids {
        match backend.parse_email(caller, &account_id, blob_id).await {
            Ok(email) => {
                let mut obj = email_projection.project(&email, |_, _| {})?;
                // RFC 8621 §4.9: "The following metadata properties on the
                // Email objects will be null if requested": id, mailboxIds,
                // keywords, receivedAt; threadId too, as this server does
                // not compute the Thread a parsed Email would join. The
                // Email struct carries them as non-Option, so they are
                // forced to null here. blobId is the parsed blob.
                const PARSE_NULL_FIELDS: &[&str] =
                    &["id", "threadId", "mailboxIds", "keywords", "receivedAt"];
                if let Value::Object(ref mut map) = obj {
                    for &field in PARSE_NULL_FIELDS {
                        if map.contains_key(field) {
                            map.insert(field.to_owned(), Value::Null);
                        }
                    }
                    if map.contains_key("blobId") {
                        map.insert(
                            "blobId".to_owned(),
                            Value::String(blob_id.as_ref().to_owned()),
                        );
                    }
                }
                parsed.insert(requested.clone(), obj);
            }
            Err(_) => {
                // RFC 8621 §5.8: distinguish "blob not found" from "not parsable".
                // A transient backend error (Err) propagates as a top-level
                // serverFail so the client retries instead of silently
                // mis-classifying the blob as not-found.
                match backend.blob_exists(caller, &account_id, blob_id).await {
                    Ok(true) => not_parsable.push(Value::String(requested.clone())),
                    Ok(false) => not_found.push(Value::String(requested.clone())),
                    Err(e) => return Err(server_fail_from_backend(&e)),
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

    Ok(HandlerOutput::new(resp))
}

// ---------------------------------------------------------------------------
// Email/copy (RFC 8621 §4.7 / RFC 8620 §5.4)
// ---------------------------------------------------------------------------

/// Handle an `Email/copy` method call (RFC 8621 §4.7).
///
/// `args` is the RFC 8620 §5.4 `/copy` request shape (`fromAccountId`,
/// optional `ifFromInState`, `accountId`, optional `ifInState`, `create`
/// map of creationId → object, optional `onSuccessDestroyOriginal`,
/// optional `destroyFromIfInState`); the returned `Value` is the §5.4
/// `/copy` response shape (`fromAccountId`, `accountId`, `oldState`,
/// `newState`, `created` / `notCreated` maps). RFC 8621 §4.7 adds no
/// arguments: it is "a standard "/copy" method as described in \[RFC8620\],
/// Section 5.4".
///
/// Copies one or more emails from `fromAccountId` into the current account.
/// Supports `onSuccessDestroyOriginal` and `destroyFromIfInState`.
///
/// Returns `(response_args, extra_invocations)`. An extra invocation is
/// generated when `onSuccessDestroyOriginal` is true, per RFC 8620 §6.3. If `destroyFromIfInState` does not match
/// the source account's Email state, the single extra invocation is an
/// `error` of type `stateMismatch` and the originals are left untouched.
///
/// `using` is the request's capability list (pass
/// [`jmap_server::CallContext::using`]). draft-ietf-jmap-metadata-05 §3.4:
/// the copy keeps the source's `metadata`, and its `privateMetadata` only
/// with `urn:ietf:params:jmap:metadata` in `using` and when the
/// destination supports it; neither is copied when the destination does
/// not list Email ([`JmapAccounts::metadata_info`](jmap_server::JmapAccounts::metadata_info)). The create object
/// "MAY include metadata and privateMetadata to override the copied
/// values, with the same rules as a create in Foo/set": the handler
/// rejects them without the capability and rejects `null`, and
/// [`MailBackend::copy_object`] applies the rest.
///
/// `mailboxIds` keys may be `#creationId` references (RFC 8620 §5.3),
/// resolved against `created_ids` (pass
/// [`jmap_server::CallContext::created_ids`]) before the entry reaches
/// the backend; an unknown reference fails that entry with
/// `invalidProperties` naming `mailboxIds`.
pub async fn handle_email_copy<B: MailBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    created_ids: &BTreeMap<Id, Id>,
    args: Value,
    call_id: &str,
    using: &[String],
) -> Result<HandlerOutput, JmapError> {
    let (account_id, mut args) = extract_account_id(args)?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    let from_account_id: Id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::new_validated(s)
            .map_err(|e| JmapError::invalid_arguments(format!("fromAccountId: {e}")))?,
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };

    // RFC 8620 §5.4: fromAccountId MUST differ from accountId.
    if from_account_id == account_id {
        return Err(JmapError::invalid_arguments(
            "fromAccountId must be different from accountId",
        ));
    }
    if !backend
        .account_exists(caller, &from_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::from_account_not_found());
    }

    let Some(Value::Object(create)) = args.remove("create") else {
        return Err(JmapError::invalid_arguments("create is required"));
    };

    let on_success_destroy_original: bool = bool_arg(&args, "onSuccessDestroyOriginal", false);

    // ifFromInState: check source account state (RFC 8620 §5.4).
    if let Some(if_from_in_state) = args.get("ifFromInState").and_then(|v| v.as_str()) {
        let from_state = backend
            .get_state::<Email>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        if if_from_in_state != from_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let old_state = backend
        .get_state::<Email>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // ifInState: check destination account state (RFC 8620 §5.4).
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut copied_source_ids: Vec<(String, Id)> = Vec::new(); // (copy_id, source_id)

    for (copy_id, mut entry) in create {
        if let Err(err) = resolve_mailbox_id_refs(&mut entry, created_ids) {
            not_created.insert(copy_id, Value::from(&*err));
            continue;
        }
        let Some(s) = entry.get("id").and_then(|v| v.as_str()) else {
            not_created.insert(
                copy_id,
                json!({"type": "invalidProperties", "properties": ["id"]}),
            );
            continue;
        };
        let source_id: Id = match jmap_server::parse_wire_id(s, "id") {
            Ok(id) => id,
            Err(err) => {
                not_created.insert(copy_id, serde_json::Value::from(&*err));
                continue;
            }
        };

        // Only include mailboxIds whose value is true (RFC 8621 §6.1 requires at least one).
        let mailbox_ids = match parse_mailbox_ids(&entry, "RFC 8621 §6.1") {
            Ok(ids) => ids,
            Err(err) => {
                not_created.insert(copy_id, err);
                continue;
            }
        };

        // keywords: String[Boolean] wire format — validate RFC 8621 §4.1.1 syntax.
        // RFC 8620 §5.4: "any other properties included are used instead of
        // the current value", so an absent `keywords` keeps the source's.
        let keywords: Option<Vec<Keyword>> = match entry.get("keywords") {
            None => None,
            Some(_) => match parse_keywords_field(&entry) {
                Ok(kws) => Some(kws),
                Err(err) => {
                    not_created.insert(copy_id, err);
                    continue;
                }
            },
        };

        // receivedAt may be overridden during copy (RFC 8621 §4.7).
        // Shared helper with Email/set create and Email/import
        // (`bd:JMAP-j7pa.4`).
        let received_at: Option<UTCDate> = match parse_received_at_field(&entry) {
            Ok(v) => v,
            Err(err) => {
                not_created.insert(copy_id, err);
                continue;
            }
        };

        // draft-ietf-jmap-metadata-05 §3.4: metadata overrides follow the
        // Foo/set create rules (§3, §3.2).
        if let Some(Err(err)) = entry
            .as_object()
            .map(|o| check_metadata_entry(o, true, using))
        {
            not_created.insert(copy_id, Value::from(&*err));
            continue;
        }
        let (metadata, private_metadata) = match entry.as_object().map(create_metadata).transpose()
        {
            Ok(maps) => maps.unwrap_or_default(),
            Err(err) => {
                not_created.insert(copy_id, err);
                continue;
            }
        };

        let mut email = match backend
            .get_objects::<Email>(
                caller,
                &from_account_id,
                Some(std::slice::from_ref(&source_id)),
                None,
            )
            .await
        {
            Ok((mut found, _)) if !found.is_empty() => found.swap_remove(0),
            Ok(_) => {
                not_created.insert(copy_id, json!({ "type": "notFound" }));
                continue;
            }
            Err(e) => {
                not_created.insert(copy_id, server_fail_value_from_backend(&e));
                continue;
            }
        };
        email.mailbox_ids = mailbox_ids.into_iter().map(|id| (id, true)).collect();
        if let Some(keywords) = keywords {
            email.keywords = keywords.into_iter().map(|k| (k, true)).collect();
        }
        if let Some(received_at) = received_at {
            email.received_at = received_at;
        }
        // draft-ietf-jmap-metadata-05 §3.4: "When the request does not use
        // this capability, only metadata is copied", "privateMetadata is
        // not copied when the destination does not support it, and
        // neither property is copied when the data type is not listed in
        // the destination account".
        let info = backend.metadata_info::<Email>(caller, &account_id);
        if !uses_metadata(using) || !info.as_ref().is_some_and(|i| i.supports_private) {
            email.private_metadata = None;
        }
        if info.is_none() {
            email.metadata = None;
        }
        if metadata.is_some() {
            email.metadata = metadata;
        }
        if private_metadata.is_some() {
            email.private_metadata = private_metadata;
        }
        // The thread is per account (RFC 8621 §3): assign it in the
        // destination, as Email/set create does.
        email.thread_id = match assign_thread(
            backend,
            caller,
            &account_id,
            email.in_reply_to.as_deref().unwrap_or(&[]),
            email.references.as_deref().unwrap_or(&[]),
        )
        .await
        {
            Ok(thread_id) => thread_id,
            Err(e) => {
                not_created.insert(copy_id, server_fail_value_from_backend(&e));
                continue;
            }
        };

        match backend
            .copy_object(caller, &from_account_id, &account_id, email, &entry)
            .await
        {
            Ok((new_id, new_email)) => {
                // RFC 8621 §4.7: created entries contain only these 4 server-set fields.
                let obj = json!({
                    "id": new_id.as_ref(),
                    "blobId": new_email.blob_id.as_ref(),
                    "threadId": new_email.thread_id.as_ref(),
                    "size": new_email.size,
                });
                created.insert(copy_id.clone(), obj);
                copied_source_ids.push((copy_id, source_id));
            }
            Err(e) => {
                not_created.insert(copy_id, backend_set_error_value(e));
            }
        }
    }

    let new_state = if created.is_empty() {
        old_state.clone()
    } else {
        backend
            .get_state::<Email>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    };

    let resp = json!({
        "fromAccountId": from_account_id.as_ref(),
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created": if created.is_empty() { Value::Null } else { Value::Object(created) },
        "notCreated": if not_created.is_empty() { Value::Null } else { Value::Object(not_created) },
    });

    // Execute onSuccessDestroyOriginal and build a single implicit Email/set
    // response (RFC 8620 §6.3).
    //
    // The dispatcher appends extra invocations verbatim to methodResponses, so
    // we must build the full response object here — not request args.
    let mut extra: Vec<Invocation> = Vec::new();

    if on_success_destroy_original && !copied_source_ids.is_empty() {
        let email_old_state = backend
            .get_state::<Email>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        // RFC 8620 §5.4: `destroyFromIfInState` "is passed on as the
        // "ifInState" argument to the implicit "Foo/set" call"; per §5.3 a
        // mismatch aborts that whole call with a method-level stateMismatch
        // error in place of the Email/set response. Nothing is destroyed.
        if let Some(if_in_state) = args.get("destroyFromIfInState").and_then(Value::as_str)
            && if_in_state != email_old_state.as_ref()
        {
            let err = json!({ "type": "stateMismatch" });
            return Ok(HandlerOutput::new(resp).with_extra(vec![Invocation::new(
                "error",
                err,
                call_id.to_owned(),
            )]));
        }

        let mut email_destroyed: Vec<Value> = Vec::new();
        let mut email_not_destroyed = serde_json::Map::new();
        for (_, source_id) in &copied_source_ids {
            match backend
                .destroy_object::<Email>(caller, &from_account_id, source_id)
                .await
            {
                Ok(()) => {
                    email_destroyed.push(Value::String(source_id.as_ref().to_owned()));
                }
                Err(e) => {
                    email_not_destroyed
                        .insert(source_id.as_ref().to_owned(), backend_set_error_value(e));
                }
            }
        }

        let email_new_state = backend
            .get_state::<Email>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        // RFC 8620 §6.3: a single implicit Email/set response appended after
        // the Email/copy response.
        let set_resp = json!({
            "accountId": from_account_id.as_ref(),
            "oldState": email_old_state.as_ref(),
            "newState": email_new_state.as_ref(),
            "created": Value::Null,
            "updated": Value::Null,
            "destroyed": if email_destroyed.is_empty() { Value::Null } else { Value::Array(email_destroyed) },
            "notCreated": Value::Null,
            "notUpdated": Value::Null,
            "notDestroyed": if email_not_destroyed.is_empty() { Value::Null } else { Value::Object(email_not_destroyed) },
        });
        extra.push(Invocation::new("Email/set", set_resp, call_id.to_owned()));
    }

    Ok(HandlerOutput::new(resp).with_extra(extra))
}
