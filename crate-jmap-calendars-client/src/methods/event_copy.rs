// JMAP Calendars — CalendarEvent/copy method implementation on SessionClient.
//
// CalendarEvent/copy copies events between accounts (draft-ietf-jmap-calendars-26 §5.10).

use std::collections::HashMap;

use super::SetResponse;

impl super::SessionClient {
    /// Copy CalendarEvent objects from one account to another
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
        from_account_id: &str,
        create: HashMap<String, jmap_calendars_types::CalendarEvent>,
    ) -> Result<SetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        super::validate_id_field(from_account_id, "calendar_event_copy: from_account_id")?;
        for k in create.keys() {
            if k.is_empty() {
                return Err(jmap_base_client::ClientError::InvalidArgument(
                    "calendar_event_copy: create map key (creation id) may not be empty".into(),
                ));
            }
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

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_CALENDARS};
    use serde_json::json;

    /// Oracle: CalendarEvent/copy request has correct method name, call_id, and
    /// fromAccountId in args. Expected values from draft §5.7.
    #[test]
    fn calendar_event_copy_request_shape() {
        let args = json!({
            "fromAccountId": "src_acc",
            "accountId": "dst_acc",
            "create": {},
        });
        let req = build_request("CalendarEvent/copy", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("CalendarEvent/copy"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
        assert_eq!(calls[0][1]["fromAccountId"], json!("src_acc"));
        assert_eq!(calls[0][1]["accountId"], json!("dst_acc"));
    }

    // The InvalidArgument guard for empty from_account_id lives in
    // calendar_event_copy production code; testing it requires a
    // wiremock-backed async harness. See JMAP-sc1b.64.
}
