//! JMAP Mail — Identity/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_SUBMISSION)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Identity objects by IDs (RFC 8621 §6.1 — Identity/get).
    ///
    /// If `ids` is `None`, the server returns all Identities for the account.
    /// Pass `properties: None` to return all fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`. (Identity/* uses the
    ///   `urn:ietf:params:jmap:submission` capability for its `using`
    ///   array but is keyed on the mail primary account.)
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
    pub async fn identity_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_mail_types::Identity>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `email_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("Identity/get", args, super::USING_SUBMISSION);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Identity objects since `since_state` (RFC 8621 §6.2 — Identity/changes).
    ///
    /// `max_changes` follows the same RFC 8620 §5.2 magic-value semantics
    /// as [`SessionClient::email_changes`](crate::methods::SessionClient::email_changes):
    /// `None` lets the server apply its default cap, `Some(0)` means
    /// "no client limit", `Some(n>0)` requests at most `n` entries.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth —
    ///   `State` constructed via [`State::from`](jmap_types::State::from)
    ///   accepts empty strings, but an empty `sinceState` is never
    ///   useful and would otherwise generate a wasted round-trip).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::identity_get`].
    pub async fn identity_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "identity_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Identity/changes", args, super::USING_SUBMISSION);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Identity objects (RFC 8621 §6.3 — Identity/set).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. Pass `None` to omit.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `update` is `Some` and `serde_json::to_value` fails on the
    ///   patch map (pathological conditions only; see
    ///   [`Self::email_set`] for the memory-cost discussion that applies
    ///   identically here).
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::identity_get`].
    pub async fn identity_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse<jmap_mail_types::Identity>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "identity_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("Identity/set", args, super::USING_SUBMISSION);
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

    // identity_get_empty_id_returns_invalid_argument was deleted in JMAP-6by7.2
    // (typed-Id refactor): under `Option<&[Id]>` the empty-Id case becomes
    // impossible to express through the typed API.

    // The InvalidArgument guard for empty since_state lives in identity_changes
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    // Deleted in JMAP-tco1.5 as Pattern E (vacuous inline tests):
    //   - identity_get_request_shape
    //   - identity_set_request_shape
    // Each hand-built `args = json!({...})` and fed it to `build_request`,
    // never invoking the `identity_get` / `identity_set` production builders.
    // Real production-path coverage for these methods is tracked as a
    // wiremock-smoke gap under JMAP-uuoi (no `tests/identity_*.rs` smoke
    // file exists yet).
    //
    // `build_request`, `CALL_ID`, and `USING_SUBMISSION` themselves have their
    // own focused tests in `methods/mod.rs`.

    /// Oracle: Identity deserialization from RFC 8621 §6 example.
    #[test]
    fn identity_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s1",
            "list": [
                {
                    "id": "ident1",
                    "name": "Jane Doe",
                    "email": "jane@example.com",
                    "textSignature": "-- \nJane",
                    "htmlSignature": "<p>Jane</p>",
                    "mayDelete": true
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Identity> =
            serde_json::from_value(json).expect("must deserialize Identity GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].name, "Jane Doe");
        assert_eq!(resp.list[0].email, "jane@example.com");
        assert!(resp.list[0].may_delete);
    }
}
