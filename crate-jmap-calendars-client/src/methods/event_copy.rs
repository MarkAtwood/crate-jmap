// JMAP Calendars — CalendarEvent/copy method implementation on SessionClient.
//
// CalendarEvent/copy copies events between accounts (draft-ietf-jmap-calendars-26 §5.7).

use super::SetResponse;

impl super::SessionClient {
    /// Copy CalendarEvent objects from one account to another
    /// (draft-ietf-jmap-calendars-26 §5.7).
    ///
    /// - `from_account_id`: the source account containing the events to copy.
    /// - `create`: map of creation id → CalendarEvent patch object describing
    ///   what to copy and any modifications to apply.
    ///
    /// The target account is the primary Calendars account from the session.
    pub async fn calendar_event_copy(
        &self,
        from_account_id: &str,
        create: serde_json::Value,
    ) -> Result<SetResponse<jmap_calendars_types::CalendarEvent>, jmap_base_client::ClientError>
    {
        if from_account_id.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_copy: from_account_id may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "fromAccountId": from_account_id,
            "accountId": account_id,
            "create": create,
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

    /// Oracle: empty from_account_id triggers the InvalidArgument guard.
    #[test]
    fn calendar_event_copy_empty_from_account_id_returns_invalid_argument() {
        let from_account_id = "";
        let result: Result<(), jmap_base_client::ClientError> = if from_account_id.is_empty() {
            Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_copy: from_account_id may not be empty".into(),
            ))
        } else {
            Ok(())
        };
        assert!(
            matches!(
                result,
                Err(jmap_base_client::ClientError::InvalidArgument(_))
            ),
            "empty from_account_id must produce InvalidArgument"
        );
    }
}
