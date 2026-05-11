//! JMAP Calendars — Calendar/* method implementations on SessionClient.
//!
//! Each method follows the standard pattern:
//!   1. Validate arguments (empty-string guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON.
//!   4. Call `build_request(method_name, args, USING_CALENDARS)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch Calendar objects by IDs (draft-ietf-jmap-calendars-26 §4.1).
    ///
    /// Pass `ids: None` to fetch all calendars. Pass `properties: None` to
    /// return all fields.
    pub async fn calendar_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_calendars_types::Calendar>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` entirely when None rather than sending
        // an explicit JSON null. RFC 8620 §5.1 accepts both shapes, but the
        // crate's other builders (set/changes/query) consistently use the
        // conditional-add idiom; matching it here keeps the wire request
        // canonical and avoids "present-but-null vs absent" interop quirks
        // in proxies / audit loggers.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("Calendar/get", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Calendar objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §4.2).
    pub async fn calendar_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: `State::new_validated` rejects empty strings, but
        // `State::from` does not. Guard against pathological constructions.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_changes: since_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state.as_ref(),
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
    /// - `update`: map of existing Calendar id → [`PatchObject`]
    ///   (RFC 8620 §5.3). Wire format is unchanged from a plain JSON object
    ///   because [`PatchObject`] is `#[serde(transparent)]`; the typed
    ///   parameter exists to bind the JSON Pointer key + null-leaf-removal
    ///   contract to the type system. Pass `None` to omit `update` entirely.
    /// - `destroy`: list of Calendar ids to destroy.
    /// - `on_destroy_remove_events`: if `true`, destroying a calendar also
    ///   destroys all its events. If `false` (the default), the server MUST
    ///   reject a destroy if the calendar still has events
    ///   (`calendarHasEvent` error).
    pub async fn calendar_set(
        &self,
        create: Option<HashMap<String, jmap_calendars_types::Calendar>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[Id]>,
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
            args["destroy"] = serde_json::to_value(d).expect("Id slice Serialize is infallible");
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
// Tests — see tests/calendar_smoke_tests.rs (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// The previous inline `mod tests` was a collection of vacuous tests that
// hand-built `args` Values and fed them to `build_request`, never
// exercising the production `calendar_*` builders. Deleted in
// JMAP-231o.8.
//
// Production-path coverage for this module lives in
// `tests/calendar_smoke_tests.rs`:
//   - calendar_get_smoke (success path)
//   - calendar_get_empty_id_returns_invalid_argument (guard path)
//
// `calendar_set` onDestroyRemoveEvents flag passthrough is covered
// by `calendar_set_on_destroy_remove_events_*_passthrough` in
// tests/calendar_smoke_tests.rs (added under JMAP-uuoi.1).
//
// `build_request`, `CALL_ID`, and `USING_CALENDARS` themselves have
// their own focused tests in `methods/mod.rs`.
