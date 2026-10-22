//! JMAP Calendars — ParticipantIdentity/* method implementations on SessionClient.

use std::collections::BTreeMap;

use jmap_types::{Id, PatchObject, State};

use super::{ChangesResponse, GetResponse, PartialObject, SetResponse};

impl super::SessionClient {
    /// Fetch `ParticipantIdentity` objects by IDs
    /// (draft-ietf-jmap-calendars-33 §3.1).
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
    /// (draft-ietf-jmap-calendars-33 §3.2).
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
    /// (draft-ietf-jmap-calendars-33 §3.3).
    ///
    /// - `create`: JSON object mapping each creation id to the
    ///   `ParticipantIdentity` properties to set (RFC 8620 §5.3).
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
    /// - `params`: extra method-level arguments
    ///   ([`ParticipantIdentitySetParams`](super::ParticipantIdentitySetParams)).
    ///   Pass `None` for spec-default behavior. Set
    ///   [`ParticipantIdentitySetParams::on_success_set_is_default`](super::ParticipantIdentitySetParams::on_success_set_is_default)
    ///   to change the default identity; with it set, `create`, `update`
    ///   and `destroy` may all be `None`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if any key in `create` is the empty string (caller-precondition
    ///   guard — RFC 8620 §5.3 requires non-empty creation ids), or if
    ///   `serde_json::to_value` fails on the `update` map
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
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
        params: Option<super::ParticipantIdentitySetParams>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        let sets_default = params
            .as_ref()
            .is_some_and(|p| p.on_success_set_is_default.is_some());
        if create.is_none() && update.is_none() && destroy.is_none() && !sets_default {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "participant_identity_set: at least one of create, update, destroy or \
                 params.on_success_set_is_default must be Some \
                 (an all-None /set is a no-op round-trip)"
                    .into(),
            ));
        }
        if let Some(m) = create.as_ref().and_then(serde_json::Value::as_object) {
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
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "participant_identity_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(d).expect("Id Vec Serialize is infallible");
        }
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let mut params_extra: Option<serde_json::Map<String, serde_json::Value>> = None;
        if let Some(p) = params {
            if let Some(v) = p.on_success_set_is_default {
                args["onSuccessSetIsDefault"] = v.into();
            }
            if !p.extra.is_empty() {
                params_extra = Some(p.extra);
            }
        }
        // Route caller-supplied vendor extras onto the wire (workspace
        // extras-preservation policy). Use `entry().or_insert()` so a
        // caller who put a typed wire key into `params.extra` cannot
        // silently clobber the typed value — typed wins on collision.
        if let Some(extra) = params_extra {
            let args_obj = args
                .as_object_mut()
                .expect("participant_identity_set: args is constructed as Object");
            for (k, v) in extra {
                args_obj.entry(k).or_insert(v);
            }
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
