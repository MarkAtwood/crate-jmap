//! Principal/getAvailability method (draft-ietf-jmap-calendars-26 §2.2).

use jmap_types::Id;

use super::{PrincipalGetAvailabilityResponse, SessionClient, CALL_ID, USING_AVAILABILITY};
use jmap_base_client::ClientError;

impl SessionClient {
    /// Fetch availability data for a principal
    /// (draft-ietf-jmap-calendars-26 §2.2 — Principal/getAvailability).
    ///
    /// The wire key for `principal_id` is `"id"` (not `"principalId"`) per §2.2.
    ///
    /// `utc_start` and `utc_end` remain `&str` here pending the workspace-wide
    /// `UTCDate` newtype migration tracked under `bd:JMAP-g7wu.9`.
    ///
    /// # Errors
    /// Returns `ClientError::InvalidArgument` if `utc_start` or `utc_end` is empty.
    pub async fn principal_get_availability(
        &self,
        principal_id: &Id,
        utc_start: &str,
        utc_end: &str,
        show_details: Option<bool>,
        event_properties: Option<&[&str]>,
    ) -> Result<PrincipalGetAvailabilityResponse, ClientError> {
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
            "id": principal_id.as_ref(),  // §2.2 uses "id" not "principalId"
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

    // The InvalidArgument guard for empty principal_id is unreachable
    // through the typed `&Id` API: `Id::new_validated("")` returns Err at
    // the caller's construction site, and a caller who bypasses validation
    // via `Id::from("")` is explicitly opting out of the type-system
    // guarantee. The guard is kept as defense in depth (see
    // principal_get_availability body), but the wiremock-backed
    // production-path coverage lives in
    // tests/availability_tests.rs::principal_get_availability_round_trip.

    // request_uses_id_key_not_principal_id and
    // show_details_none_is_absent_from_request were vacuous: they hand-built
    // args and fed them to build_request, never exercising the production
    // principal_get_availability builder. Deleted in JMAP-231o.8. Real
    // production-path coverage lives in
    // tests/availability_tests.rs::principal_get_availability_round_trip.

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
