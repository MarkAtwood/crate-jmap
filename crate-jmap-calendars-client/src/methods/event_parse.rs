//! CalendarEvent/parse method (draft-ietf-jmap-calendars-26 §5.13).

use jmap_types::Id;

use super::{CalendarEventParseParams, CalendarEventParseResponse, SessionClient};
use jmap_base_client::ClientError;

impl SessionClient {
    /// Parse calendar event blobs into `CalendarEvent` objects
    /// (draft-ietf-jmap-calendars-26 §5.13 — CalendarEvent/parse).
    ///
    /// `params` lets the caller pin the `properties` selector and thread
    /// vendor / site extension fields through the wire request via the
    /// struct's `extra` flatten map. Pass `None` to omit all optional
    /// arguments.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`] if `blob_ids` is empty, or if
    ///   `params` is `Some` and serializing it to JSON fails
    ///   (pathological conditions only — allocation failure, or a vendor
    ///   value in `params.extra` that itself fails to serialize).
    /// - [`ClientError::InvalidSession`] if the bound session has no
    ///   primary account for `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](ClientError::Http),
    ///   [`Parse`](ClientError::Parse),
    ///   [`AuthFailed`](ClientError::AuthFailed),
    ///   [`MethodError`](ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`),
    ///   [`MethodNotFound`](ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](ClientError::UnexpectedResponse).
    pub async fn calendar_event_parse(
        &self,
        blob_ids: &[Id],
        params: Option<CalendarEventParseParams>,
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
        if let Some(p) = params {
            let pv = serde_json::to_value(&p).map_err(|e| {
                ClientError::InvalidArgument(format!(
                    "calendar_event_parse: failed to serialize params: {e}"
                ))
            })?;
            if let serde_json::Value::Object(map) = pv {
                // Use `entry().or_insert()` so a caller who put a typed
                // wire key (e.g. "accountId", "blobIds", "properties")
                // into `params.extra` cannot silently clobber the typed
                // args. Typed wins on collision.
                let args_obj = args
                    .as_object_mut()
                    .expect("calendar_event_parse: args is constructed as Object");
                for (k, v) in map {
                    args_obj.entry(k).or_insert(v);
                }
            }
        }
        let req = super::build_request("CalendarEvent/parse", args, super::USING_PARSE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use serde_json::json;

    // The InvalidArgument guard for empty blob_ids is unreachable through
    // the typed `&[Id]` API: `Id::new_validated("")` returns Err at the
    // caller's construction site. The guard is kept as defense in depth
    // (see calendar_event_parse body). End-to-end production-path
    // coverage lives in
    // tests/calendar_smoke_tests.rs::calendar_event_parse_smoke.

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
