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
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
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
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("Calendar/get", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Calendar objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §4.2).
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
    ///   `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::calendar_get`].
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
    /// - `update`: map of existing Calendar id → [`PatchObject`]
    ///   (RFC 8620 §5.3). Wire format is unchanged from a plain JSON object
    ///   because [`PatchObject`] is `#[serde(transparent)]`; the typed
    ///   parameter exists to bind the JSON Pointer key + null-leaf-removal
    ///   contract to the type system. Pass `None` to omit `update` entirely.
    /// - `destroy`: list of Calendar ids to destroy.
    /// - `if_in_state`: optional optimistic-concurrency guard per RFC 8620
    ///   §5.3. If supplied, the value must equal the current Calendar state
    ///   on the server or the method rejects with `stateMismatch`. Pass the
    ///   `newState` returned by a prior /get or /set response.
    /// - `params`: extra method-level arguments
    ///   ([`CalendarSetParams`](super::CalendarSetParams)). Pass `None`
    ///   (or `Some(Default::default())`) for spec-default behavior. Use
    ///   [`CalendarSetParams::on_destroy_remove_events`](super::CalendarSetParams::on_destroy_remove_events)
    ///   to allow destroying a non-empty calendar (otherwise the server
    ///   returns `calendarHasEvent`), and
    ///   [`CalendarSetParams::extra`](super::CalendarSetParams::extra) for
    ///   vendor / site extension fields (workspace extras-preservation
    ///   policy).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if any key in `create` is the empty string (caller-precondition
    ///   guard — RFC 8620 §5.3 requires non-empty creation ids), or if
    ///   `serde_json::to_value` fails on the `create` or `update` map
    ///   (pathological conditions only — allocation failure, a
    ///   `Calendar` value or a `PatchObject` whose JSON tree exceeds
    ///   `serde_json`'s recursion limit). The transient memory peak for
    ///   very large maps is roughly 3-4× the source map's in-memory
    ///   size (source map + `serde_json::Value` tree + serialized
    ///   `Vec<u8>` body); callers may prefer to batch.
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::calendar_get`].
    pub async fn calendar_set(
        &self,
        create: Option<HashMap<String, jmap_calendars_types::Calendar>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[Id]>,
        if_in_state: Option<&State>,
        params: Option<super::CalendarSetParams>,
    ) -> Result<SetResponse<jmap_calendars_types::Calendar>, jmap_base_client::ClientError> {
        if create.is_none() && update.is_none() && destroy.is_none() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_set: at least one of create, update, destroy must be Some \
                 (an all-None /set is a no-op round-trip)"
                    .into(),
            ));
        }
        if let Some(m) = &create {
            if m.keys().any(|k| k.is_empty()) {
                return Err(jmap_base_client::ClientError::InvalidArgument(
                    "calendar_set: create map key (creation id) may not be empty".into(),
                ));
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        let mut params_extra: Option<serde_json::Map<String, serde_json::Value>> = None;
        if let Some(p) = params {
            if let Some(flag) = p.on_destroy_remove_events {
                args["onDestroyRemoveEvents"] = flag.into();
            }
            if !p.extra.is_empty() {
                params_extra = Some(p.extra);
            }
        }
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
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
        // Route caller-supplied vendor extras onto the wire (workspace
        // extras-preservation policy). Use `entry().or_insert()` so a
        // caller who put a typed wire key into `params.extra` cannot
        // silently clobber the typed value — typed wins on collision.
        if let Some(extra) = params_extra {
            let args_obj = args
                .as_object_mut()
                .expect("calendar_set: args is constructed as Object");
            for (k, v) in extra {
                args_obj.entry(k).or_insert(v);
            }
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
