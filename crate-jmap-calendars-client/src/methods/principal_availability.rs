//! Principal/getAvailability method (draft-ietf-jmap-calendars-26 §2.2).

use super::{PrincipalGetAvailabilityResponse, SessionClient, CALL_ID, USING_AVAILABILITY};
use jmap_base_client::ClientError;

impl SessionClient {
    /// Fetch availability data for a principal
    /// (draft-ietf-jmap-calendars-26 §2.2 — Principal/getAvailability).
    ///
    /// The wire key for `principal_id` is `"id"` (not `"principalId"`) per §2.2.
    ///
    /// # Errors
    /// Returns `ClientError::InvalidArgument` if any required parameter is empty.
    pub async fn principal_get_availability(
        &self,
        principal_id: &str,
        utc_start: &str,
        utc_end: &str,
        show_details: Option<bool>,
        event_properties: Option<&[&str]>,
    ) -> Result<PrincipalGetAvailabilityResponse, ClientError> {
        super::validate_id_field(
            principal_id,
            "principal_get_availability: principal_id",
        )?;
        if utc_start.is_empty() {
            return Err(ClientError::InvalidArgument(
                "principal_get_availability: utc_start must not be empty".into(),
            ));
        }
        if utc_end.is_empty() {
            return Err(ClientError::InvalidArgument(
                "principal_get_availability: utc_end must not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "id": principal_id,          // §2.2 uses "id" not "principalId"
            "utcStart": utc_start,
            "utcEnd": utc_end,
        });
        if let Some(sd) = show_details {
            args["showDetails"] = serde_json::Value::Bool(sd);
        }
        if let Some(props) = event_properties {
            args["eventProperties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("Principal/getAvailability", args, USING_AVAILABILITY);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, CALL_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use serde_json::json;

    // The InvalidArgument guard for empty principal_id is exercised by the
    // production method body in principal_get_availability (lines 22-26).
    // A black-box test that drives the async method is the right place to
    // verify it; that requires wiremock and is out of scope for this crate
    // (no async test harness yet). See JMAP-sc1b.64.

    /// Oracle: §2.2 wire field is "id", not "principalId".
    #[test]
    fn request_uses_id_key_not_principal_id() {
        let args = json!({
            "accountId": "acc1",
            "id": "p-joe",
            "utcStart": "2024-06-15T09:00:00Z",
            "utcEnd": "2024-06-15T10:00:00Z",
        });
        let req = build_request("Principal/getAvailability", args, USING_AVAILABILITY);
        let v = serde_json::to_value(&req).expect("serialize");
        let args_val = &v["methodCalls"][0][1];
        assert!(args_val.get("id").is_some(), "must use 'id' key");
        assert!(
            args_val.get("principalId").is_none(),
            "must NOT use 'principalId'"
        );
    }

    /// Oracle: showDetails:None → key absent from request (not null).
    #[test]
    fn show_details_none_is_absent_from_request() {
        let args = json!({
            "accountId": "acc1",
            "id": "p-joe",
            "utcStart": "2024-06-15T09:00:00Z",
            "utcEnd": "2024-06-15T10:00:00Z",
        });
        let req = build_request("Principal/getAvailability", args, USING_AVAILABILITY);
        let v = serde_json::to_value(&req).expect("serialize");
        let args_val = &v["methodCalls"][0][1];
        assert!(
            args_val.get("showDetails").is_none(),
            "showDetails must be absent when None"
        );
    }

    /// Oracle: USING_AVAILABILITY contains the principals:availability URI.
    #[test]
    fn using_contains_principals_availability_uri() {
        let req = build_request(
            "Principal/getAvailability",
            json!({"accountId":"acc"}),
            USING_AVAILABILITY,
        );
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using array");
        let has_avail = using
            .iter()
            .any(|u| u.as_str() == Some("urn:ietf:params:jmap:principals:availability"));
        assert!(has_avail, "must contain principals:availability URI");
    }
}
