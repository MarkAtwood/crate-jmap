//! CalendarEvent/parse method (draft-ietf-jmap-calendars-26 §5.13).

use jmap_types::Id;

use super::{CalendarEventParseResponse, SessionClient, CALL_ID, USING_PARSE};
use jmap_base_client::ClientError;

impl SessionClient {
    /// Parse calendar event blobs into `CalendarEvent` objects
    /// (draft-ietf-jmap-calendars-26 §5.13 — CalendarEvent/parse).
    ///
    /// # Errors
    /// Returns `ClientError::InvalidArgument` if `blob_ids` is an empty slice.
    pub async fn calendar_event_parse(
        &self,
        blob_ids: &[Id],
        properties: Option<&[&str]>,
    ) -> Result<CalendarEventParseResponse, ClientError> {
        if blob_ids.is_empty() {
            return Err(ClientError::InvalidArgument(
                "calendar_event_parse: blob_ids must not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "blobIds": serde_json::to_value(blob_ids)
                .expect("Id slice Serialize is infallible"),
        });
        if let Some(props) = properties {
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("CalendarEvent/parse", args, USING_PARSE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, CALL_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use serde_json::json;

    // The InvalidArgument guard for empty blob_ids lives in CalendarEvent/parse
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    // blob_ids_wire_name_is_camel_case and properties_none_is_absent_from_request
    // were vacuous: they hand-built args and fed them to build_request,
    // never exercising the production calendar_event_parse builder.
    // Deleted in JMAP-231o.8. Real coverage:
    // tests/calendar_smoke_tests.rs::calendar_event_parse_smoke.

    /// Oracle: USING_PARSE contains all 3 capability URIs including the parse extension.
    /// This test legitimately exercises the USING_PARSE constant directly,
    /// not pretending to test calendar_event_parse via build_request.
    #[test]
    fn using_parse_contains_parse_capability() {
        let req = build_request(
            "CalendarEvent/parse",
            json!({"accountId":"acc"}),
            USING_PARSE,
        );
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using array");
        let has_parse = using
            .iter()
            .any(|u| u.as_str() == Some("urn:ietf:params:jmap:calendars:parse"));
        assert!(has_parse, "USING_PARSE must contain calendars:parse URI");
    }
}
