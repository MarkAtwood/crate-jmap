//! JMAP Calendars — CalendarEvent/copy method implementation on SessionClient.
//!
//! CalendarEvent/copy copies events between accounts (draft-ietf-jmap-calendars-33 §5.10).

use super::{CalendarEventCopyParams, CopyResponse, PartialObject};

impl super::SessionClient {
    /// Copy `CalendarEvent` objects from one account to another
    /// (draft-ietf-jmap-calendars-33 §5.10).
    ///
    /// - `params`: `fromAccountId`, the optional `ifFromInState` /
    ///   `ifInState` state guards, the optional destroy-after-copy flags,
    ///   and vendor extras (RFC 8620 §5.4).
    /// - `create`: JSON object mapping each creation id to the source
    ///   event's `id` plus any properties to change. Each entry MUST carry
    ///   the source `id` (RFC 8620 §5.4 — `id` is the source record).
    ///
    /// The target account is the primary Calendars account from the session.
    ///
    /// Returns a [`CopyResponse`]. Each `created` value is a
    /// [`PartialObject`] holding the server-set properties of the copy,
    /// such as its new `id` (RFC 8620 §5.4).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if any key in `create` is the empty string (caller-precondition
    ///   guard — RFC 8620 §5.3 requires non-empty creation ids).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:calendars`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    ///   RFC 8620 §5.4 /copy adds method-level errors
    ///   `fromAccountNotFound`, `fromAccountNotSupportedByMethod`, and
    ///   `anchorNotFound`.
    pub async fn calendar_event_copy(
        &self,
        params: CalendarEventCopyParams,
        create: serde_json::Value,
    ) -> Result<CopyResponse<PartialObject>, jmap_base_client::ClientError> {
        if create
            .as_object()
            .is_some_and(|m| m.keys().any(|k| k.is_empty()))
        {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "calendar_event_copy: create map key (creation id) may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "fromAccountId": params.from_account_id,
            "accountId": account_id,
            "create": create,
        });
        if let Some(v) = params.if_from_in_state {
            args["ifFromInState"] = v.into_inner().into();
        }
        if let Some(v) = params.if_in_state {
            args["ifInState"] = v.into_inner().into();
        }
        if let Some(v) = params.on_success_destroy_original {
            args["onSuccessDestroyOriginal"] = v.into();
        }
        if let Some(v) = params.destroy_from_if_in_state {
            args["destroyFromIfInState"] = v.into_inner().into();
        }
        // Route caller-supplied vendor extras onto the wire (workspace
        // extras-preservation policy). Use `entry().or_insert()` so a
        // caller who put a typed wire key into `params.extra` cannot
        // silently clobber the typed value — typed wins on collision.
        if !params.extra.is_empty() {
            let args_obj = args
                .as_object_mut()
                .expect("calendar_event_copy: args is constructed as Object");
            for (k, v) in params.extra {
                args_obj.entry(k).or_insert(v);
            }
        }
        let req = super::build_request("CalendarEvent/copy", args, super::USING_CALENDARS);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// calendar_event_copy_request_shape was vacuous: it hand-built args and
// fed them to build_request, never exercising the production
// calendar_event_copy builder. Deleted in JMAP-231o.8.
//
// Real production-path coverage:
//   - calendar_event_copy_empty_creation_id_returns_invalid_argument
//     (guard, tests/calendar_smoke_tests.rs)
//   - calendar_event_copy_success_passthrough
//     (cross-account success path, tests/event_smoke_tests.rs)
// Added under JMAP-uuoi.1.
