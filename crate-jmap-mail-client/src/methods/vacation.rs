//! JMAP Mail — VacationResponse/get and VacationResponse/set implementations
//! on SessionClient.
//!
//! VacationResponse is a singleton object per account (RFC 8621 §8). Its `id`
//! is always `"singleton"`. `VacationResponse/get` ignores the `ids` argument
//! and always returns the single object; `VacationResponse/set` does not
//! support `create` or `destroy` — only `update`.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (empty-string guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_VACATION)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject};

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
        let req = super::build_request("VacationResponse/get", args, super::USING_VACATION);
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
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). The
    /// usual shape is `{"singleton": <patch>}`. Wire format is unchanged
    /// from a plain JSON object because [`PatchObject`] is
    /// `#[serde(transparent)]`.
    pub async fn vacation_response_set(
        &self,
        update: Option<HashMap<Id, PatchObject>>,
    ) -> Result<SetResponse<jmap_mail_types::VacationResponse>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "vacation_response_set: serializing update map failed: {e}"
                ))
            })?;
        }
        let req = super::build_request("VacationResponse/set", args, super::USING_VACATION);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    // Deleted in JMAP-tco1.5 as Pattern E (vacuous inline tests):
    //   - vacation_response_get_request_shape
    //   - vacation_response_set_request_shape
    //   - vacation_response_set_no_update_sends_account_id_only
    // Each hand-built `args = json!({...})` and fed it to `build_request`,
    // never invoking the `vacation_response_get` / `vacation_response_set`
    // production builders. The third was a "None field is absent" tautology
    // on hand-built args. Real production-path coverage for these methods
    // is tracked as a wiremock-smoke gap under JMAP-uuoi (no
    // `tests/vacation_*.rs` smoke file exists yet). The singleton-id-passing
    // and no-create/no-destroy invariants of `VacationResponse/{get,set}`
    // (RFC 8621 §8) are tracked under JMAP-uuoi for a follow-up wiremock
    // smoke test.
    //
    // `build_request`, `CALL_ID`, and `USING_VACATION` themselves have their
    // own focused tests in `methods/mod.rs`.

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
