//! JMAP Calendars — CalendarEventNotification/* method implementations.
//!
//! CalendarEventNotification/set is destroy-only per draft-ietf-jmap-calendars-26 §7.3.
//! The server must reject create and update operations with `forbidden`.
//! This method accepts only `destroy` to prevent constructing invalid requests.

use jmap_calendars_types::NotificationFilterCondition;
use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch `CalendarEventNotification` objects by IDs
    /// (draft-ietf-jmap-calendars-26 §7.1).
    pub async fn calendar_event_notification_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<
        GetResponse<jmap_calendars_types::CalendarEventNotification>,
        jmap_base_client::ClientError,
    > {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `calendar_get` for the rationale.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
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

    /// Fetch changes to `CalendarEventNotification` objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §7.2).
    pub async fn calendar_event_notification_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `calendar_event_changes`.
        if since_state.as_ref().is_empty() {
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

    /// Destroy `CalendarEventNotification` objects (draft-ietf-jmap-calendars-26 §7.3).
    ///
    /// CalendarEventNotification/set is destroy-only: the server rejects create
    /// and update operations with `forbidden` `SetErrors`. This method only sends
    /// `destroy` to prevent constructing invalid requests.
    ///
    /// **Network call is unconditional.** Both `destroy: None` and
    /// `destroy: Some(&[])` produce a wire request with `"destroy": []` and
    /// always make an HTTP round-trip to the server. The response will
    /// trivially have `oldState == newState` and empty `destroyed` /
    /// `notDestroyed` maps — an uninteresting round-trip that still costs
    /// latency and counts against any rate limit.
    ///
    /// **If you want to avoid the round-trip, filter your list and skip
    /// this call entirely** when there is nothing to destroy:
    ///
    /// ```ignore
    /// if !ids.is_empty() {
    ///     let _ = sc.calendar_event_notification_set(Some(&ids)).await?;
    /// }
    /// ```
    ///
    /// (`ids` here is `Vec<jmap_types::Id>`.)
    ///
    /// Rationale (bd:JMAP-231o.9): the alternative — short-circuiting and
    /// synthesizing an empty `SetResponse` client-side without a network
    /// call — would require fabricating `oldState`/`newState` tokens with
    /// no way to know the current value, which would be wrong and
    /// confusing if the caller then used the synthesized state for
    /// optimistic concurrency. Keeping the call unconditional preserves
    /// state-token correctness; the caller is in the best position to
    /// decide whether to skip the call entirely.
    pub async fn calendar_event_notification_set(
        &self,
        destroy: Option<&[Id]>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let destroy_val = match destroy {
            Some(ids) => serde_json::to_value(ids).expect("Id slice Serialize is infallible"),
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

    /// Query `CalendarEventNotification` IDs with optional filter and sort
    /// (draft-ietf-jmap-calendars-26 §7.4).
    ///
    /// - `filter`: typed [`NotificationFilterCondition`].
    /// - `sort`: comparator slice. `CalendarEventNotification`'s Comparator
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

    /// Fetch query-result changes for `CalendarEventNotification` since
    /// `since_query_state` (draft-ietf-jmap-calendars-26 §7.5).
    pub async fn calendar_event_notification_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `calendar_event_changes`.
        if since_query_state.as_ref().is_empty() {
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

// calendar_event_notification_set_no_destroy_sends_empty_array and
// calendar_event_notification_set_with_destroy_sends_ids were vacuous:
// they hand-built args and fed them to build_request, never exercising
// the production calendar_event_notification_set builder. Deleted in
// JMAP-231o.8. The destroy-only enforcement (no create/update keys)
// and the destroy IDs passthrough are now covered by
// calendar_event_notification_set_destroy_only_with_ids and
// calendar_event_notification_set_destroy_none_sends_empty_array
// in tests/event_notification_smoke_tests.rs (JMAP-uuoi.1).
//
// The end-to-end InvalidArgument guard for empty `ids` slice elements
// lives in tests/calendar_smoke_tests.rs as a wiremock-backed test
// (calendar_event_notification_get_empty_id_returns_invalid_argument).
