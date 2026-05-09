// JMAP Mail — Identity/* method implementations on SessionClient.
//
// Each method follows the standard five-step pattern:
//   1. Validate arguments (defence-in-depth empty-state guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON with `serde_json::json!({…})`.
//   4. Call `build_request(method_name, args, USING_MAIL)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Identity objects by IDs (RFC 8621 §6.1 — Identity/get).
    ///
    /// If `ids` is `None`, the server returns all Identities for the account.
    /// Pass `properties: None` to return all fields.
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
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("Identity/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Identity objects since `since_state` (RFC 8621 §6.2 — Identity/changes).
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
        let req = super::build_request("Identity/changes", args, super::USING_MAIL);
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
        let req = super::build_request("Identity/set", args, super::USING_MAIL);
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

    // identity_get_empty_id_returns_invalid_argument was deleted in JMAP-6by7.2
    // (typed-Id refactor): under `Option<&[Id]>` the empty-Id case becomes
    // impossible to express through the typed API.

    // The InvalidArgument guard for empty since_state lives in identity_changes
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    /// Oracle: Identity/get request shape is correct.
    #[test]
    fn identity_get_request_shape() {
        let args = json!({
            "accountId": "acc1",
            "ids": null,
            "properties": null,
        });
        let req = build_request("Identity/get", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Identity/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
    }

    /// Oracle: Identity/set with create and destroy sends correct keys.
    #[test]
    fn identity_set_request_shape() {
        let create_val = json!({"k1": {"name": "Work", "email": "work@example.com"}});
        let destroy_val = serde_json::Value::Array(
            vec!["id-old"]
                .into_iter()
                .map(|id| serde_json::Value::String(id.to_owned()))
                .collect(),
        );
        let mut args = json!({ "accountId": "acc1" });
        args["create"] = create_val;
        args["destroy"] = destroy_val;

        let req = build_request("Identity/set", args, USING_MAIL);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Identity/set"));
        assert_eq!(
            calls[0][1]["create"]["k1"]["email"],
            json!("work@example.com")
        );
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert!(destroy_arr.contains(&json!("id-old")));
    }

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
