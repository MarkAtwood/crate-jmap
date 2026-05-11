//! JMAP Mail — EmailSubmission/* method implementations on SessionClient.
//!
//! Implements RFC 8621 §7.1-7.5.
//!
//! Each method follows the standard six-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_MAIL)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.
//!
//! Wire key notes (RFC 8621 §7):
//!   - Object field for submission creation time:     "sendAt"       (§7.1)
//!   - Sort property for /query:                      "sentAt"       (§7.3, line 4513)
//!   - Success hooks on /set:                         "onSuccessUpdateEmail",
//!     "onSuccessDestroyEmail" (§7.5)

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, EmailSubmissionSetParams, GetResponse, QueryChangesResponse, QueryResponse,
    SetResponse,
};

impl super::SessionClient {
    /// Fetch EmailSubmission objects by IDs (RFC 8621 §7.1 — EmailSubmission/get).
    ///
    /// If `ids` is `None`, the server returns all submissions for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn email_submission_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_mail_types::EmailSubmission>, jmap_base_client::ClientError> {
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
        let req = super::build_request("EmailSubmission/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to EmailSubmission objects since `since_state`
    /// (RFC 8621 §7.2 — EmailSubmission/changes).
    pub async fn email_submission_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_submission_changes: since_state may not be empty".into(),
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
        let req = super::build_request("EmailSubmission/changes", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query EmailSubmission IDs with optional filter and sort
    /// (RFC 8621 §7.3 — EmailSubmission/query).
    ///
    /// The sort property for this object type is `"sentAt"` (RFC 8621 §7.3, line 4513),
    /// not `"sendAt"` (which is an object field).  Callers constructing the sort
    /// argument should use `"sentAt"` as the property name.
    pub async fn email_submission_query(
        &self,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        position: Option<u64>,
        limit: Option<u64>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        if let Some(p) = position {
            args["position"] = p.into();
        }
        if let Some(l) = limit {
            args["limit"] = l.into();
        }
        let req = super::build_request("EmailSubmission/query", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for EmailSubmission since `since_query_state`
    /// (RFC 8621 §7.4 — EmailSubmission/queryChanges).
    pub async fn email_submission_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_submission_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        let req = super::build_request("EmailSubmission/queryChanges", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy EmailSubmission objects
    /// (RFC 8621 §7.5 — EmailSubmission/set).
    ///
    /// The optional `params` argument carries the two success-hook fields:
    ///
    /// - `on_success_update_email` — a `PatchObject` map (keyed by submission
    ///   creation key) of patches to apply to the associated Email when the
    ///   submission is created successfully (RFC 8621 §7.5).
    /// - `on_success_destroy_email` — IDs (or `#`-reference creation keys) of
    ///   Email objects to destroy when the submission is created successfully
    ///   (RFC 8621 §7.5).
    pub async fn email_submission_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
        params: Option<EmailSubmissionSetParams>,
    ) -> Result<SetResponse<jmap_mail_types::EmailSubmission>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        // Merge success-hook params into the top-level args object (RFC 8621 §7.5).
        // These are method-level arguments, not nested under a key.
        if let Some(p) = params {
            if let Some(v) = p.on_success_update_email {
                args["onSuccessUpdateEmail"] = serde_json::to_value(&v).map_err(|e| {
                    jmap_base_client::ClientError::InvalidArgument(format!(
                        "email_submission_set: serializing onSuccessUpdateEmail failed: {e}"
                    ))
                })?;
            }
            if let Some(v) = p.on_success_destroy_email {
                args["onSuccessDestroyEmail"] = serde_json::Value::Array(
                    v.into_iter().map(serde_json::Value::String).collect(),
                );
            }
        }
        if let Some(s) = if_in_state {
            args["ifInState"] = serde_json::Value::String(s.as_ref().to_owned());
        }
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "email_submission_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("EmailSubmission/set", args, super::USING_MAIL);
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

    // submission_get_empty_id_guard and submission_set_empty_destroy_id_guard
    // were deleted in JMAP-6by7.2 (typed-Id refactor): under `Option<&[Id]>`
    // and `Option<Vec<Id>>` the empty-Id case becomes impossible to express
    // through the typed API.

    // The InvalidArgument guards for empty since_state and since_query_state
    // live in email_submission_changes / email_submission_query_changes
    // production code; testing them requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    // Deleted in JMAP-tco1.5 as Pattern E (vacuous inline tests):
    //   - submission_get_request_shape
    //   - submission_changes_request_shape
    //   - submission_query_request_includes_filter
    //   - submission_query_changes_request_shape
    //   - submission_set_on_success_update_email_request_shape
    //   - submission_set_on_success_destroy_email_request_shape
    // Each hand-built `args = json!({...})` and fed it to `build_request`,
    // never invoking the `email_submission_get` / `email_submission_changes` /
    // `email_submission_query` / `email_submission_query_changes` /
    // `email_submission_set` production builders.
    //
    // Real production-path coverage:
    //   - tests/submission_get_changes.rs:
    //       email_submission_get_round_trip,
    //       email_submission_get_specific_ids,
    //       email_submission_changes_round_trip,
    //       email_submission_changes_no_max_changes
    //   - tests/submission_query.rs:
    //       email_submission_query_with_filter,
    //       email_submission_query_no_filter,
    //       email_submission_query_changes_round_trip,
    //       email_submission_query_changes_with_filter_and_sort
    //   - tests/submission_set.rs:
    //       email_submission_set_create_round_trip,
    //       email_submission_set_on_success_update_email,
    //       email_submission_set_no_on_success_when_none
    //
    // Specific-flag passthrough coverage that may be lost (`onSuccessDestroyEmail`)
    // is tracked under JMAP-uuoi for a follow-up wiremock smoke test —
    // there is no current wiremock test that asserts the
    // `onSuccessDestroyEmail` array field reaches the wire.
    //
    // `build_request`, `CALL_ID`, and `USING_MAIL` themselves have their
    // own focused tests in `methods/mod.rs`.

    // ── Response deserialization tests ───────────────────────────────────────

    /// Oracle: GetResponse<EmailSubmission> deserializes from RFC 8621 §7.1 response shape.
    /// JSON constructed from §7 field descriptions (not derived from code).
    #[test]
    fn submission_get_response_deserializes() {
        let json_val = json!({
            "accountId": "acc1",
            "state": "s5",
            "list": [
                {
                    "id": "sub1",
                    "identityId": "ident1",
                    "emailId": "eml1",
                    "threadId": "thr1",
                    "envelope": null,
                    "sendAt": "2024-06-15T10:00:00Z",
                    "undoStatus": "final",
                    "deliveryStatus": null,
                    "dsnBlobIds": [],
                    "mdnBlobIds": []
                }
            ],
            "notFound": []
        });

        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::EmailSubmission> =
            serde_json::from_value(json_val).expect("must deserialize EmailSubmission GetResponse");
        assert_eq!(resp.state, "s5");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].id.as_ref(), "sub1");
    }
}
