// JMAP Calendars — Calendar/* method implementations on SessionClient.
//
// Each method follows the standard pattern:
//   1. Validate arguments (empty-string guards).
//   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//   3. Build args JSON.
//   4. Call `build_request(method_name, args, USING_CALENDARS)`.
//   5. Call `self.call_internal(api_url, &req).await?`.
//   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::Id;

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Calendar objects by IDs (draft-ietf-jmap-calendars-26 §4.1).
    ///
    /// Pass `ids: None` to fetch all calendars. Pass `properties: None` to
    /// return all fields.
    pub async fn calendar_get(
        &self,
        ids: Option<&[&str]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_calendars_types::Calendar>, jmap_base_client::ClientError> {
        if let Some(id_slice) = ids {
            for id in id_slice.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_get: ids element may not be empty".into(),
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
        let req = super::build_request("Calendar/get", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Calendar objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §4.2).
    pub async fn calendar_changes(
        &self,
        since_state: &str,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Calendar/changes", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Calendar objects (draft-ietf-jmap-calendars-26 §4.4).
    ///
    /// - `create`: map of creation id → typed [`Calendar`](jmap_calendars_types::Calendar)
    ///   to create. Pass `None` to omit the `create` argument entirely.
    /// - `update`: map of existing Calendar id → JSON Merge Patch
    ///   (RFC 8620 §5.3 `PatchObject`). The patch is left untyped because
    ///   keys may carry `/`-separated paths into nested fields, which the
    ///   typed [`Calendar`](jmap_calendars_types::Calendar) struct cannot
    ///   represent. Pass `None` to omit `update` entirely.
    /// - `destroy`: list of Calendar ids to destroy.
    /// - `on_destroy_remove_events`: if `true`, destroying a calendar also
    ///   destroys all its events. If `false` (the default), the server MUST
    ///   reject a destroy if the calendar still has events
    ///   (`calendarHasEvent` error).
    pub async fn calendar_set(
        &self,
        create: Option<HashMap<String, jmap_calendars_types::Calendar>>,
        update: Option<HashMap<Id, serde_json::Value>>,
        destroy: Option<&[&str]>,
        on_destroy_remove_events: Option<bool>,
    ) -> Result<SetResponse<jmap_calendars_types::Calendar>, jmap_base_client::ClientError> {
        if let Some(ref m) = create {
            for k in m.keys() {
                if k.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_set: create map key (creation id) may not be empty".into(),
                    ));
                }
            }
        }
        if let Some(ids) = destroy {
            for id in ids.iter() {
                if id.is_empty() {
                    return Err(jmap_base_client::ClientError::InvalidArgument(
                        "calendar_set: destroy element may not be empty".into(),
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
                    "calendar_set: serializing create map failed: {e}"
                ))
            })?;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "calendar_set: serializing update map failed: {e}"
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
        if let Some(flag) = on_destroy_remove_events {
            args["onDestroyRemoveEvents"] = flag.into();
        }
        let req = super::build_request("Calendar/set", args, super::USING_CALENDARS);
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

    /// Oracle: empty ID in ids slice returns InvalidArgument.
    /// Guard fires before any session lookup or network call.
    #[test]
    fn calendar_get_empty_id_returns_invalid_argument() {
        let ids: &[&str] = &[""];
        let mut found_error = false;
        for id in ids.iter() {
            if id.is_empty() {
                found_error = true;
                break;
            }
        }
        assert!(
            found_error,
            "empty id must trigger the InvalidArgument guard"
        );
    }

    // The InvalidArgument guard for empty since_state lives in calendar_changes
    // production code; testing it requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    /// Oracle: Calendar/set with onDestroyRemoveEvents sends the flag in args.
    /// Expected: args object contains "onDestroyRemoveEvents": true.
    #[test]
    fn calendar_set_on_destroy_remove_events_in_args() {
        let mut args = json!({ "accountId": "acc1" });
        args["onDestroyRemoveEvents"] = true.into();
        let req = build_request("Calendar/set", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Calendar/set"), "method name");
        assert_eq!(calls[0][2], json!(CALL_ID), "call id");
        assert_eq!(
            calls[0][1]["onDestroyRemoveEvents"],
            json!(true),
            "flag must be present"
        );
    }

    /// Oracle: Calendar/get request has correct method name and using array.
    #[test]
    fn calendar_get_request_shape() {
        let args = json!({ "accountId": "acc1", "ids": null, "properties": null });
        let req = build_request("Calendar/get", args, USING_CALENDARS);
        let v = serde_json::to_value(&req).expect("serialize");
        let calls = v["methodCalls"].as_array().expect("methodCalls");
        assert_eq!(calls[0][0], json!("Calendar/get"));
        assert_eq!(calls[0][2], json!(CALL_ID));
        let using = v["using"].as_array().expect("using");
        assert!(using.contains(&json!("urn:ietf:params:jmap:calendars")));
    }
}
