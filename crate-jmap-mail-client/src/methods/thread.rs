//! JMAP Mail — Thread/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_MAIL)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse};

impl super::SessionClient {
    /// Fetch Thread objects by IDs (RFC 8621 §3.1 — Thread/get).
    ///
    /// If `ids` is `None`, the server returns all Threads for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn thread_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_mail_types::Thread>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `email_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("Thread/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Thread objects since `since_state` (RFC 8621 §3.2 — Thread/changes).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    pub async fn thread_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: `State::new_validated` rejects empty strings, but
        // `State::from` does not. Guard against pathological constructions.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "thread_changes: since_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("Thread/changes", args, super::USING_MAIL);
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

    // thread_get_empty_id_returns_invalid_argument was deleted in JMAP-6by7.2
    // (typed-Id refactor): under `Option<&[Id]>` the empty-Id case becomes
    // impossible to express through the typed API.

    // The InvalidArgument guard for empty since_state lives in thread_changes
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    // Deleted in JMAP-tco1.5 as Pattern E (vacuous inline tests):
    //   - thread_get_request_shape
    //   - thread_changes_request_includes_since_state
    // Each hand-built `args = json!({...})` and fed it to `build_request`,
    // never invoking the `thread_get` / `thread_changes` production builders.
    // Real production-path coverage for these methods is tracked as a
    // wiremock-smoke gap under JMAP-uuoi (no `tests/thread_*.rs` smoke
    // file exists yet).
    //
    // `build_request`, `CALL_ID`, and `USING_MAIL` themselves have their
    // own focused tests in `methods/mod.rs`.

    /// Oracle: Thread deserialization from RFC 8621 §3 shape.
    /// RFC 8621 §3.1: Thread has id and emailIds fields.
    #[test]
    fn thread_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s5",
            "list": [
                {
                    "id": "t1",
                    "emailIds": ["e1", "e2", "e3"]
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Thread> =
            serde_json::from_value(json).expect("must deserialize Thread GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].id.as_ref(), "t1");
        assert_eq!(resp.list[0].email_ids.len(), 3);
    }
}
