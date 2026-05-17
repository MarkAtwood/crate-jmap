//! JMAP Calendars — ParticipantIdentity/* method implementations on SessionClient.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, SetResponse};

impl super::SessionClient {
    /// Fetch `ParticipantIdentity` objects by IDs
    /// (draft-ietf-jmap-calendars-26 §3.1).
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
    pub async fn participant_identity_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_calendars_types::ParticipantIdentity>, jmap_base_client::ClientError>
    {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `calendar_get` for the rationale.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("ParticipantIdentity/get", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to `ParticipantIdentity` objects since `since_state`
    /// (draft-ietf-jmap-calendars-26 §3.2).
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
    ///   the matching error list on [`Self::participant_identity_get`].
    pub async fn participant_identity_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `calendar_event_changes`.
        if since_state.as_ref().is_empty() {
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

    /// Create, update, or destroy `ParticipantIdentity` objects
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
    /// - `if_in_state`: optional optimistic-concurrency guard per RFC 8620
    ///   §5.3. If supplied, the value must equal the current
    ///   `ParticipantIdentity` state on the server or the method rejects
    ///   with `stateMismatch`. Pass the `newState` returned by a prior
    ///   /get or /set response.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if any key in `create` is the empty string (caller-precondition
    ///   guard — RFC 8620 §5.3 requires non-empty creation ids), or if
    ///   `serde_json::to_value` fails on the `create` or `update` map
    ///   (pathological conditions only; see [`Self::calendar_set`] for
    ///   the memory-cost discussion that applies identically here).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::participant_identity_get`].
    pub async fn participant_identity_set(
        &self,
        create: Option<HashMap<String, jmap_calendars_types::ParticipantIdentity>>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<&[Id]>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<jmap_calendars_types::ParticipantIdentity>, jmap_base_client::ClientError>
    {
        if create.is_none() && update.is_none() && destroy.is_none() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "participant_identity_set: at least one of create, update, destroy must be Some \
                 (an all-None /set is a no-op round-trip)"
                    .into(),
            ));
        }
        if let Some(m) = &create {
            if m.keys().any(|k| k.is_empty()) {
                return Err(jmap_base_client::ClientError::InvalidArgument(
                    "participant_identity_set: create map key (creation id) may not be empty"
                        .into(),
                ));
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
            args["destroy"] = serde_json::to_value(d).expect("Id slice Serialize is infallible");
        }
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("ParticipantIdentity/set", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// participant_identity_get_request_shape and
// participant_identity_set_destroy_request_shape were vacuous: they
// hand-built args and fed them to build_request, never exercising the
// production participant_identity_get / _set builders. Deleted in
// JMAP-231o.8. Real production-path coverage now lives in
// tests/participant_identity_smoke_tests.rs (JMAP-uuoi.1):
//   - participant_identity_get_basic_shape
//   - participant_identity_changes_basic_shape
//   - participant_identity_set_create_round_trip
//   - participant_identity_set_destroy_only_passthrough
