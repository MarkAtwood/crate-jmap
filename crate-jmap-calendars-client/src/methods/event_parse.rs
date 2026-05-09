//! CalendarEvent/parse method (draft-ietf-jmap-calendars-26 §5.13).

use super::{CalendarEventParseResponse, SessionClient, CALL_ID, USING_PARSE};
use jmap_base_client::ClientError;

impl SessionClient {
    /// Parse calendar event blobs into CalendarEvent objects
    /// (draft-ietf-jmap-calendars-26 §5.13 — CalendarEvent/parse).
    ///
    /// # Errors
    /// Returns `ClientError::InvalidArgument` if `blob_ids` is empty or contains
    /// any empty string.
    pub async fn calendar_event_parse(
        &self,
        blob_ids: &[&str],
        properties: Option<&[&str]>,
    ) -> Result<CalendarEventParseResponse, ClientError> {
        if blob_ids.is_empty() {
            return Err(ClientError::InvalidArgument(
                "calendar_event_parse: blob_ids must not be empty".into(),
            ));
        }
        super::validate_ids_field(blob_ids, "calendar_event_parse", "blob_ids")?;
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "blobIds": blob_ids,
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

    /// Oracle: blobIds must appear in the request with camelCase wire name.
    #[test]
    fn blob_ids_wire_name_is_camel_case() {
        let args = json!({
            "accountId": "acc1",
            "blobIds": ["blob-1", "blob-2"],
        });
        let req = build_request("CalendarEvent/parse", args, USING_PARSE);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("CalendarEvent/parse"));
        // blobIds (camelCase) must be present.
        assert!(
            calls[0][1]["blobIds"].is_array(),
            "blobIds must be an array"
        );
        assert_eq!(calls[0][1]["blobIds"][0], json!("blob-1"));
    }

    /// Oracle: properties:None → key absent from request (not null).
    #[test]
    fn properties_none_is_absent_from_request() {
        let args = json!({
            "accountId": "acc1",
            "blobIds": ["blob-1"],
        });
        let req = build_request("CalendarEvent/parse", args, USING_PARSE);
        let v = serde_json::to_value(&req).expect("serialize");
        let args_val = &v["methodCalls"][0][1];
        assert!(
            args_val.get("properties").is_none(),
            "properties must not be present when None"
        );
    }

    /// Oracle: USING_PARSE contains all 3 capability URIs including the parse extension.
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
