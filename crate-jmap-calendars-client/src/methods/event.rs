// JMAP Calendars — CalendarEvent/* method implementations on SessionClient.
//
// Note: CalendarEvent/copy lives in event_copy.rs.

use std::collections::HashMap;

use jmap_calendars_types::{CalendarEventComparator, CalendarEventFilterCondition};
use jmap_types::{Id, PatchObject};

use super::{
    CalendarEventGetParams, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse,
    SetResponse,
};

impl super::SessionClient {
    /// Fetch CalendarEvent objects by IDs (draft-ietf-jmap-calendars-26 §5.4).
    ///
    /// Pass `ids: None` to fetch all events. `params` carries
    /// CalendarEvent-specific extra arguments:
    /// - `expand_recurrences`: expand recurring events to instances.
    /// - `reduced_participants`: hide participants other than the user.
    /// - `fetch_calendars`: include Calendar objects in implicit fetch.
    pub async fn calendar_event_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
        params: Option<CalendarEventGetParams>,
    ) -> Result<GetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_event_get: ids element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "ids": ids,
            "properties": properties,
        });
        if let Some(p) = params {
            if let Some(v) = p.expand_recurrences {
                args["expandRecurrences"] = v.into();
            }
            if let Some(v) = p.reduced_participants {
                args["reducedParticipants"] = v.into();
            }
            if let Some(v) = p.fetch_calendars {
                args["fetchCalendars"] = v.into();
            }
        }
        let req = super::build_request("CalendarEvent/get", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to CalendarEvent objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §5.5).
    pub async fn calendar_event_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
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

    /// Create, update, or destroy CalendarEvent objects
    /// (draft-ietf-jmap-calendars-26 §5.9).
    ///
    /// - `create`: map of creation id → typed
    ///   [`CalendarEvent`](jmap_calendars_types::CalendarEvent) to create.
    /// - `update`: map of existing CalendarEvent id → [`PatchObject`]
    ///   (RFC 8620 §5.3). Wire format is unchanged from a plain JSON
    ///   object because [`PatchObject`] is `#[serde(transparent)]`; the
    ///   typed parameter exists to bind the JSON Pointer key + null-leaf
    ///   removal contract to the type system. Patch keys may carry
    ///   `/`-separated paths into `recurrenceOverrides` etc.; the
    ///   server interprets them per the patched object's schema.
    /// - `destroy`: list of CalendarEvent ids to destroy.
    pub async fn calendar_event_set(
        &self,
        create: Option<HashMap<String, jmap_calendars_types::CalendarEvent>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[&str]>,
    ) -> Result<SetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        if let Some(ref m) = create {
            for k in m.keys() {
                if k.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_event_set: create map key (creation id) may not be empty".into(),
                    ));
                }
            }
        }
        if let Some(ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_event_set: destroy element may not be empty".into(),
                    ));
                }
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
            args["destroy"] =
                serde_json::Value::Array(d.iter().copied().map(serde_json::Value::from).collect());
        }
        let req = super::build_request("CalendarEvent/set", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query CalendarEvent IDs with optional filter and sort
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

    /// Fetch query-result changes for CalendarEvent since `since_query_state`
    /// (draft-ietf-jmap-calendars-26 §5.12).
    pub async fn calendar_event_query_changes(
        &self,
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("CalendarEvent/queryChanges", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CalendarEventGetParams, CALL_ID, USING_CALENDARS};
    use serde_json::json;

    // The end-to-end InvalidArgument guard for empty `ids` slice elements
    // lives in tests/calendar_smoke_tests.rs as a wiremock-backed test
    // (calendar_event_get_empty_id_returns_invalid_argument). The previous
    // inline unit test was a vacuous re-assertion of `"".is_empty()` and
    // never exercised the production guard (JMAP-231o.7).

    /// Oracle: CalendarEvent/get with expandRecurrences sends the flag in args.
    /// Expected field name is "expandRecurrences" per draft §5.4.
    #[test]
    fn calendar_event_get_params_expand_recurrences_in_args() {
        let params = CalendarEventGetParams {
            expand_recurrences: Some(true),
            reduced_participants: None,
            fetch_calendars: None,
        };
        let mut args = json!({ "accountId": "acc1", "ids": null, "properties": null });
        if let Some(v) = params.expand_recurrences {
            args["expandRecurrences"] = v.into();
        }
        let req = build_request("CalendarEvent/get", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["expandRecurrences"], json!(true));
    }

    /// Oracle: CalendarEvent/query with expandRecurrences=true sends the flag.
    /// Expected field name is "expandRecurrences" per draft §5.11.
    #[test]
    fn calendar_event_query_expand_recurrences_in_args() {
        let mut args = json!({ "accountId": "acc1" });
        args["expandRecurrences"] = true.into();
        let req = build_request("CalendarEvent/query", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("CalendarEvent/query"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
        assert_eq!(calls[0][1]["expandRecurrences"], json!(true));
    }

    // The InvalidArgument guards for empty since_state and since_query_state
    // live in calendar_event_changes / calendar_event_query_changes production
    // code; testing them requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.
}
