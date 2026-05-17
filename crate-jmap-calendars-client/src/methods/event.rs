//! JMAP Calendars — CalendarEvent/* method implementations on SessionClient.
//!
//! Note: CalendarEvent/copy lives in event_copy.rs.

use std::collections::HashMap;

use jmap_calendars_types::{CalendarEventComparator, CalendarEventFilterCondition};
use jmap_types::{Id, PatchObject, State};

use super::{
    CalendarEventGetParams, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse,
    SetResponse,
};

impl super::SessionClient {
    /// Fetch `CalendarEvent` objects by IDs (draft-ietf-jmap-calendars-26 §5.4).
    ///
    /// Pass `ids: None` to fetch all events. `params` carries
    /// CalendarEvent-specific extra arguments:
    /// - `expand_recurrences`: expand recurring events to instances.
    /// - `reduced_participants`: hide participants other than the user.
    /// - `fetch_calendars`: include Calendar objects in implicit fetch.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `params` is `Some` and serializing it to JSON fails
    ///   (pathological conditions only — allocation failure, or a vendor
    ///   value in `params.extra` that itself fails to serialize).
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
    pub async fn calendar_event_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        params: Option<CalendarEventGetParams>,
    ) -> Result<GetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `calendar_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        if let Some(p) = params {
            let pv = serde_json::to_value(&p).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "calendar_event_get: failed to serialize params: {e}"
                ))
            })?;
            if let serde_json::Value::Object(map) = pv {
                // Use `entry().or_insert()` so a caller who put a typed
                // wire key (e.g. "accountId", "ids", "properties") into
                // `params.extra` cannot silently clobber the typed args.
                // Typed wins on collision.
                let args_obj = args
                    .as_object_mut()
                    .expect("calendar_event_get: args is constructed as Object");
                for (k, v) in map {
                    args_obj.entry(k).or_insert(v);
                }
            }
        }
        let req = super::build_request("CalendarEvent/get", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to `CalendarEvent` objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §5.5).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth —
    ///   `State` constructed via [`State::from`](jmap_types::State::from)
    ///   accepts empty strings, but an empty `sinceState` is never
    ///   useful and would otherwise generate a wasted round-trip).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::calendar_event_get`].
    pub async fn calendar_event_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: `State::new_validated` rejects empty strings, but
        // `State::from` does not. Guard against pathological constructions.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_changes: since_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("CalendarEvent/changes", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy `CalendarEvent` objects
    /// (draft-ietf-jmap-calendars-26 §5.9).
    ///
    /// - `create`: map of creation id → typed
    ///   [`CalendarEvent`](jmap_calendars_types::CalendarEvent) to create.
    /// - `update`: map of existing `CalendarEvent` id → [`PatchObject`]
    ///   (RFC 8620 §5.3). Wire format is unchanged from a plain JSON
    ///   object because [`PatchObject`] is `#[serde(transparent)]`; the
    ///   typed parameter exists to bind the JSON Pointer key + null-leaf
    ///   removal contract to the type system. Patch keys may carry
    ///   `/`-separated paths into `recurrenceOverrides` etc.; the
    ///   server interprets them per the patched object's schema.
    /// - `destroy`: list of `CalendarEvent` ids to destroy.
    /// - `if_in_state`: optional optimistic-concurrency guard per RFC 8620
    ///   §5.3. If supplied, the value must equal the current `CalendarEvent`
    ///   state on the server or the method rejects with `stateMismatch`.
    ///   Pass the `newState` returned by a prior /get or /set response.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if any key in `create` is the empty string (caller-precondition
    ///   guard — RFC 8620 §5.3 requires non-empty creation ids), or if
    ///   `serde_json::to_value` fails on the `create` or `update` map
    ///   (pathological conditions only; see [`Self::calendar_set`] for
    ///   the memory-cost discussion that applies identically here).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::calendar_event_get`].
    pub async fn calendar_event_set(
        &self,
        create: Option<HashMap<String, jmap_calendars_types::CalendarEvent>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[Id]>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        if let Some(m) = &create {
            if m.keys().any(|k| k.is_empty()) {
                return Err(jmap_base_client::ClientError::InvalidArgument(
                    "calendar_event_set: create map key (creation id) may not be empty".into(),
                ));
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(c) = create {
            args["create"] = serde_json::to_value(&c).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "calendar_event_set: serializing create map failed: {e}"
                ))
            })?;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "calendar_event_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(d).expect("Id slice Serialize is infallible");
        }
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("CalendarEvent/set", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query `CalendarEvent` IDs with optional filter and sort
    /// (draft-ietf-jmap-calendars-26 §5.11).
    ///
    /// - `filter`: typed [`CalendarEventFilterCondition`].
    ///   Pass `None` to omit the `filter` argument.
    /// - `sort`: typed comparator slice. Pass `None` to omit the `sort`
    ///   argument.
    /// - `expand_recurrences`: if `true`, include individual recurrence
    ///   instances in the result set, each with a synthetic instance id.
    ///   When `true`, `filter` MUST be `Some(_)` with both `before` and
    ///   `after` set; otherwise the server returns `invalidArguments`
    ///   (validated server-side per §5.11).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `serde_json::to_value` fails on the typed `filter` or `sort`
    ///   (pathological conditions only — these are typed wire shapes
    ///   defined by the workspace, so serde failure indicates allocation
    ///   pressure rather than malformed input).
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::calendar_event_get`].
    ///   RFC 8620 §5.5 defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn calendar_event_query(
        &self,
        filter: Option<&CalendarEventFilterCondition>,
        sort: Option<&[CalendarEventComparator]>,
        position: Option<u64>,
        limit: Option<u64>,
        expand_recurrences: Option<bool>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(f) = filter {
            args["filter"] = serde_json::to_value(f).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "calendar_event_query: serializing filter failed: {e}"
                ))
            })?;
        }
        if let Some(s) = sort {
            args["sort"] = serde_json::to_value(s).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "calendar_event_query: serializing sort failed: {e}"
                ))
            })?;
        }
        if let Some(p) = position {
            args["position"] = p.into();
        }
        if let Some(l) = limit {
            args["limit"] = l.into();
        }
        if let Some(er) = expand_recurrences {
            args["expandRecurrences"] = er.into();
        }
        let req = super::build_request("CalendarEvent/query", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for `CalendarEvent` since `since_query_state`
    /// (draft-ietf-jmap-calendars-26 §5.12).
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `CalendarEvent/query` call that returned `since_query_state`
    /// — RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set.
    ///
    /// `up_to_id` is the highest-index id the client has cached;
    /// `calculate_total` requests the new total result count.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::calendar_event_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::calendar_event_get`].
    ///   RFC 8620 §5.6 also defines `cannotCalculateChanges` (returned
    ///   when the server cannot honour the request given the supplied
    ///   filter / sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn calendar_event_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `calendar_event_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(uti) = up_to_id {
            args["upToId"] = serde_json::to_value(uti).expect("Id Serialize is infallible");
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        let req = super::build_request("CalendarEvent/queryChanges", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests — see tests/calendar_smoke_tests.rs (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// Previous inline `mod tests` deleted in JMAP-231o.8: vacuous tests that
// hand-built `args` and fed them to `build_request` without exercising
// the production `calendar_event_*` builders.
//
// Production-path coverage:
//   - calendar_event_set_smoke (success path for /set,
//     tests/calendar_smoke_tests.rs)
//   - calendar_event_get_params_all_three_passthrough,
//     calendar_event_get_no_params_omits_all_three_keys,
//     calendar_event_changes_since_state_and_max_changes_passthrough,
//     calendar_event_query_filter_sort_expand_recurrences_passthrough,
//     calendar_event_query_no_args_omits_optional_keys,
//     calendar_event_query_changes_since_state_passthrough
//     (specific-flag passthrough, tests/event_smoke_tests.rs)
// Added under JMAP-uuoi.1.
