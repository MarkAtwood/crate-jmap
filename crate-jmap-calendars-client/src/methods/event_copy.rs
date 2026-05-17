//! JMAP Calendars — CalendarEvent/copy method implementation on SessionClient.
//!
//! CalendarEvent/copy copies events between accounts (draft-ietf-jmap-calendars-26 §5.10).

use std::collections::HashMap;

use jmap_types::Id;

use super::SetResponse;

impl super::SessionClient {
    /// Copy `CalendarEvent` objects from one account to another
    /// (draft-ietf-jmap-calendars-26 §5.10).
    ///
    /// - `from_account_id`: the source account containing the events to copy.
    /// - `create`: map of creation id → typed
    ///   [`CalendarEvent`](jmap_calendars_types::CalendarEvent) describing
    ///   what to copy and any modifications to apply. Each event MUST carry
    ///   the source `id` field (RFC 8620 §5.4 — `id` is the source record).
    ///
    /// The target account is the primary Calendars account from the session.
    pub async fn calendar_event_copy(
        &self,
        from_account_id: &Id,
        create: HashMap<String, jmap_calendars_types::CalendarEvent>,
    ) -> Result<SetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        if create.keys().any(|k| k.is_empty()) {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_copy: create map key (creation id) may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let create_val = serde_json::to_value(&create).map_err(|e| {
            jmap_base_client::ClientError::InvalidArgument(format!(
                "calendar_event_copy: serializing create map failed: {e}"
            ))
        })?;
        let args = serde_json::json!({
            "fromAccountId": from_account_id,
            "accountId": account_id,
            "create": create_val,
        });
        let req = super::build_request("CalendarEvent/copy", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// calendar_event_copy_request_shape was vacuous: it hand-built args and
// fed them to build_request, never exercising the production
// calendar_event_copy builder. Deleted in JMAP-231o.8.
//
// Real production-path coverage:
//   - calendar_event_copy_empty_creation_id_returns_invalid_argument
//     (guard, tests/calendar_smoke_tests.rs)
//   - calendar_event_copy_success_passthrough
//     (cross-account success path, tests/event_smoke_tests.rs)
// Added under JMAP-uuoi.1.
