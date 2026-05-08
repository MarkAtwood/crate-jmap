// JMAP Calendars — ParticipantIdentity/* method implementations on SessionClient.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject};

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch ParticipantIdentity objects by IDs
    /// (draft-ietf-jmap-calendars-26 §3.1).
    pub async fn participant_identity_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_calendars_types::ParticipantIdentity>, jmap_base_client::ClientError>
    {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "participant_identity_get: ids element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": ids,
            "properties": properties,
        });
        let req = super::build_request("ParticipantIdentity/get", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ParticipantIdentity objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §3.2).
    pub async fn participant_identity_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "participant_identity_changes: since_state may not be empty".into(),
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
        let req = super::build_request("ParticipantIdentity/changes", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy ParticipantIdentity objects
    /// (draft-ietf-jmap-calendars-26 §3.3).
    ///
    /// - `create`: map of creation id → typed
    ///   [`ParticipantIdentity`](jmap_calendars_types::ParticipantIdentity).
    /// - `update`: map of existing identity id → [`PatchObject`]
    ///   (RFC 8620 §5.3). Wire format is unchanged from a plain JSON
    ///   object because [`PatchObject`] is `#[serde(transparent)]`; the
    ///   typed parameter binds the JSON Pointer key + null-leaf removal
    ///   contract to the type system.
    /// - `destroy`: list of identity ids to destroy.
    pub async fn participant_identity_set(
        &self,
        create: Option<HashMap<String, jmap_calendars_types::ParticipantIdentity>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[&str]>,
    ) -> Result<SetResponse<jmap_calendars_types::ParticipantIdentity>, jmap_base_client::ClientError>
    {
        if let Some(ref m) = create {
            for k in m.keys() {
                if k.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "participant_identity_set: create map key (creation id) may not be empty"
                            .into(),
                    ));
                }
            }
        }
        if let Some(ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "participant_identity_set: destroy element may not be empty".into(),
                    ));
                }
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(c) = create {
            args["create"] = serde_json::to_value(&c).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "participant_identity_set: serializing create map failed: {e}"
                ))
            })?;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "participant_identity_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::Value::Array(
                d.iter()
                    .map(|id| serde_json::Value::String((*id).to_owned()))
                    .collect(),
            );
        }
        let req = super::build_request("ParticipantIdentity/set", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{build_request, CALL_ID, USING_CALENDARS};
    use serde_json::json;

    /// Oracle: ParticipantIdentity/get request has correct method name.
    #[test]
    fn participant_identity_get_request_shape() {
        let args = json!({ "accountId": "acc1", "ids": null, "properties": null });
        let req = build_request("ParticipantIdentity/get", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("ParticipantIdentity/get"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
        let using = v["using"].as_array().expect("using");
        assert!(using.contains(&json!("urn:ietf:params:jmap:calendars")));
    }

    // The InvalidArgument guard for empty since_state lives in
    // participant_identity_changes production code; testing it requires
    // a wiremock-backed async harness. See JMAP-sc1b.64.

    /// Oracle: ParticipantIdentity/set with destroy sends destroy array.
    #[test]
    fn participant_identity_set_destroy_request_shape() {
        let destroy_val = serde_json::Value::Array(vec![json!("pid1"), json!("pid2")]);
        let mut args = json!({ "accountId": "acc1" });
        args["destroy"] = destroy_val;
        let req = build_request("ParticipantIdentity/set", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("ParticipantIdentity/set"));
        let destroy_arr = calls[0][1]["destroy"].as_array().expect("destroy array");
        assert_eq!(destroy_arr.len(), 2);
    }
}
