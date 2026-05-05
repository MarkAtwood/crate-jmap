// JMAP Mail — VacationResponse/get and VacationResponse/set implementations
// on SessionClient.
//
// VacationResponse is a singleton object per account (RFC 8621 §8). Its `id`
// is always `"singleton"`. `VacationResponse/get` ignores the `ids` argument
// and always returns the single object; `VacationResponse/set` does not
// support `create` or `destroy` — only `update`.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_MAIL)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use super::{GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch the VacationResponse singleton for the account (RFC 8621 §8).
    ///
    /// The server always returns a single `VacationResponse` object whose `id`
    /// is `"singleton"`. There is no need to pass ids.
    pub async fn vacation_response_get(
        &self,
    ) -> Result<GetResponse<jmap_mail_types::VacationResponse>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": ["singleton"],
        });
        let req = super::build_request("VacationResponse/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Update the VacationResponse singleton (RFC 8621 §8).
    ///
    /// `update` should be a JSON object of the form:
    /// ```json
    /// { "singleton": { "isEnabled": true, "subject": "Out of office" } }
    /// ```
    ///
    /// `create` and `destroy` are not supported by `VacationResponse/set`.
    pub async fn vacation_response_set(
        &self,
        update: Option<serde_json::Value>,
    ) -> Result<SetResponse<jmap_mail_types::VacationResponse>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(u) = update {
            args["update"] = u;
        }
        let req = super::build_request("VacationResponse/set", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_MAIL};
    use serde_json::json;

    /// Oracle: VacationResponse/get request uses ids:["singleton"] per RFC 8621 §8.
    /// The singleton pattern means there is always exactly one VacationResponse.
    #[test]
    fn vacation_response_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": ["singleton"],
        });
        let req = build_request("VacationResponse/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("VacationResponse/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
        let ids = calls[0][1]["ids"].as_array().expect("ids array");
        assert_eq!(ids.len(), 1, "must request exactly one id");
        assert_eq!(ids[0], json!("singleton"), "must request singleton id");
    }

    /// Oracle: VacationResponse/set request includes update but no create/destroy.
    /// RFC 8621 §8 — only update is supported.
    #[test]
    fn vacation_response_set_request_shape() {
        let update = json!({
            "singleton": {
                "isEnabled": true,
                "subject": "Out of office"
            }
        });
        let mut args = json!({ "accountId": "acc1" });
        args["update"] = update;

        let req = build_request("VacationResponse/set", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("VacationResponse/set"), "method name");
        assert_eq!(
            calls[0][1]["update"]["singleton"]["isEnabled"],
            json!(true),
            "isEnabled must be in update"
        );
        assert!(
            calls[0][1].get("create").is_none() || calls[0][1]["create"].is_null(),
            "create must not be present"
        );
        assert!(
            calls[0][1].get("destroy").is_none() || calls[0][1]["destroy"].is_null(),
            "destroy must not be present"
        );
    }

    /// Oracle: VacationResponse/set with no update sends only accountId.
    #[test]
    fn vacation_response_set_no_update_sends_account_id_only() {
        let args = json!({ "accountId": "acc1" });
        let req = build_request("VacationResponse/set", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][1]["accountId"], json!("acc1"));
        assert!(
            calls[0][1].get("update").is_none() || calls[0][1]["update"].is_null(),
            "update must not be present when None passed"
        );
    }

    /// Oracle: VacationResponse deserialization from RFC 8621 §8 shape.
    #[test]
    fn vacation_response_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s1",
            "list": [
                {
                    "id": "singleton",
                    "isEnabled": false
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::VacationResponse> =
            serde_json::from_value(json).expect("must deserialize VacationResponse GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].id.as_ref(), "singleton");
        assert!(!resp.list[0].is_enabled);
    }
}
