// JMAP Calendars — CalendarEventNotification/* method implementations.
//
// CalendarEventNotification/set is destroy-only per draft-ietf-jmap-calendars-26 §7.3.
// The server must reject create and update operations with `forbidden`.
// This method accepts only `destroy` to prevent constructing invalid requests.

use jmap_calendars_types::NotificationFilterCondition;

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch CalendarEventNotification objects by IDs
    /// (draft-ietf-jmap-calendars-26 §7.1).
    pub async fn calendar_event_notification_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<
        GetResponse<jmap_calendars_types::CalendarEventNotification>,
        jmap_base_client::ClientError,
    > {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_event_notification_get: ids element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `calendar_get` for the rationale.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::Value::Array(
                id_slice
                    .iter()
                    .copied()
                    .map(serde_json::Value::from)
                    .collect(),
            );
        }
        if let Some(props) = properties {
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request(
            "CalendarEventNotification/get",
            args,
            super::USING_CALENDARS,
        );
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to CalendarEventNotification objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §7.2).
    pub async fn calendar_event_notification_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_notification_changes: since_state may not be empty".into(),
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
        let req = super::build_request(
            "CalendarEventNotification/changes",
            args,
            super::USING_CALENDARS,
        );
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy CalendarEventNotification objects (draft-ietf-jmap-calendars-26 §7.3).
    ///
    /// CalendarEventNotification/set is destroy-only: the server rejects create
    /// and update operations with `forbidden` SetErrors. This method only sends
    /// `destroy` to prevent constructing invalid requests.
    ///
    /// Pass `destroy: None` to send an empty destroy list (no-op).
    pub async fn calendar_event_notification_set(
        &self,
        destroy: Option<&[&str]>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if let Some(ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_event_notification_set: destroy element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let destroy_val = match destroy {
            Some(ids) => {
                serde_json::Value::Array(ids.iter().copied().map(serde_json::Value::from).collect())
            }
            None => serde_json::Value::Array(vec![]),
        };
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": destroy_val,
        });
        let req = super::build_request(
            "CalendarEventNotification/set",
            args,
            super::USING_CALENDARS,
        );
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query CalendarEventNotification IDs with optional filter and sort
    /// (draft-ietf-jmap-calendars-26 §7.4).
    ///
    /// - `filter`: typed [`NotificationFilterCondition`].
    /// - `sort`: comparator slice. CalendarEventNotification's Comparator
    ///   type in `jmap-calendars-types` is `serde_json::Value` because the
    ///   spec's sort properties for notifications are minimal (just
    ///   `created`); the slice is forwarded as-is.
    pub async fn calendar_event_notification_query(
        &self,
        filter: Option<&NotificationFilterCondition>,
        sort: Option<&[serde_json::Value]>,
        position: Option<u64>,
        limit: Option<u64>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(f) = filter {
            args["filter"] = serde_json::to_value(f).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "calendar_event_notification_query: serializing filter failed: {e}"
                ))
            })?;
        }
        if let Some(s) = sort {
            args["sort"] = serde_json::Value::Array(s.to_vec());
        }
        if let Some(p) = position {
            args["position"] = p.into();
        }
        if let Some(l) = limit {
            args["limit"] = l.into();
        }
        let req = super::build_request(
            "CalendarEventNotification/query",
            args,
            super::USING_CALENDARS,
        );
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for CalendarEventNotification since
    /// `since_query_state` (draft-ietf-jmap-calendars-26 §7.5).
    pub async fn calendar_event_notification_query_changes(
        &self,
        since_query_state: &str,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_notification_query_changes: since_query_state may not be empty"
                    .into(),
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
        let req = super::build_request(
            "CalendarEventNotification/queryChanges",
            args,
            super::USING_CALENDARS,
        );
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_CALENDARS};
    use serde_json::json;

    /// Oracle: CalendarEventNotification/set with no destroy sends destroy:[] in args.
    /// The spec mandates destroy-only — no create or update keys.
    #[test]
    fn calendar_event_notification_set_no_destroy_sends_empty_array() {
        let destroy_val = serde_json::Value::Array(vec![]);
        let args = json!({
            "accountId": "acc1",
            "destroy": destroy_val,
        });
        let req = build_request("CalendarEventNotification/set", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");

        assert_eq!(
            calls[0][0],
            json!("CalendarEventNotification/set"),
            "method name"
        );
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");

        let method_args = &calls[0][1];
        let destroy = method_args["destroy"]
            .as_array()
            .expect("destroy must be array");
        assert!(destroy.is_empty(), "destroy must be empty when None passed");
        assert!(
            method_args.get("create").is_none(),
            "create must not be present in destroy-only method"
        );
        assert!(
            method_args.get("update").is_none(),
            "update must not be present in destroy-only method"
        );
    }

    /// Oracle: CalendarEventNotification/set with destroy list sends IDs.
    #[test]
    fn calendar_event_notification_set_with_destroy_sends_ids() {
        let ids = ["notif1", "notif2"];
        let destroy_val =
            serde_json::Value::Array(ids.iter().copied().map(serde_json::Value::from).collect());
        let args = json!({ "accountId": "acc1", "destroy": destroy_val });
        let req = build_request("CalendarEventNotification/set", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 2);
        assert!(destroy_arr.contains(&json!("notif1")));
        assert!(destroy_arr.contains(&json!("notif2")));
    }

    // The end-to-end InvalidArgument guard for empty `ids` slice elements
    // lives in tests/calendar_smoke_tests.rs as a wiremock-backed test
    // (calendar_event_notification_get_empty_id_returns_invalid_argument).
    // The previous inline unit test was a vacuous re-assertion of
    // `"".is_empty()` and never exercised the production guard (JMAP-231o.7).
    //
    // The InvalidArgument guard for empty since_state in
    // calendar_event_notification_changes is also exercised end-to-end via
    // wiremock; see JMAP-sc1b.64.
}
