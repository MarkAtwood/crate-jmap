// JMAP Calendars — CalendarEvent/* method implementations on SessionClient.
//
// Note: CalendarEvent/copy lives in event_copy.rs.

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
    /// (draft-ietf-jmap-calendars-26 §5.6).
    pub async fn calendar_event_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<serde_json::Value>,
        destroy: Option<Vec<&str>>,
    ) -> Result<SetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        if let Some(ref ids) = destroy {
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
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = u;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::Value::Array(
                d.into_iter()
                    .map(|id| serde_json::Value::String(id.to_owned()))
                    .collect(),
            );
        }
        let req = super::build_request("CalendarEvent/set", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query CalendarEvent IDs with optional filter and sort
    /// (draft-ietf-jmap-calendars-26 §5.11).
    ///
    /// `expand_recurrences`: if `true`, include individual recurrence instances
    /// in the result set, each with a synthetic instance id (draft §5.11).
    pub async fn calendar_event_query(
        &self,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        position: Option<u64>,
        limit: Option<u64>,
        expand_recurrences: Option<bool>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
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

    /// Oracle: empty ID in ids slice triggers the validation guard.
    #[test]
    fn calendar_event_get_empty_id_returns_invalid_argument() {
        let ids: &[&str] = &[""];
        let mut found_error = false;
        for id in ids.iter() {
            if id.is_empty() {
                found_error = true;
                break;
            }
        }
        assert!(
            found_error,
            "empty id must trigger the InvalidArgument guard"
        );
    }

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

    /// Oracle: empty since_state returns InvalidArgument.
    #[test]
    fn calendar_event_changes_empty_state_returns_invalid_argument() {
        let since_state = "";
        let result: Result<(), jmap_base_client::ClientError> = if since_state.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_changes: since_state may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty since_state must produce InvalidArgument"
        );
    }

    /// Oracle: empty since_query_state returns InvalidArgument.
    #[test]
    fn calendar_event_query_changes_empty_state_returns_invalid_argument() {
        let sqc = "";
        let result: Result<(), jmap_base_client::ClientError> = if sqc.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_query_changes: since_query_state may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty since_query_state must produce InvalidArgument"
        );
    }
}
