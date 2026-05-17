//! JMAP Sharing — ShareNotification/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_SHARING)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.
//!
//! Note: ShareNotification/set is destroy-only per RFC 9670 §3.3.
//! The server MUST reject create/update operations with `forbidden` errors.
//! This method accepts only `destroy` to prevent constructing invalid requests.

use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetResponse};

impl super::SessionClient {
    /// Fetch ShareNotification objects by IDs (RFC 9670 §3.1).
    ///
    /// If `ids` is `None`, the server returns all ShareNotifications for the account,
    /// SUBJECT TO the server's `maxObjectsInGet` cap (RFC 8620 §5.1).
    /// For production use, scope the result set via the corresponding
    /// /query method first and pass explicit ids here to avoid
    /// `requestTooLarge` errors when the account holds more objects
    /// than the cap.
    /// Pass `properties: None` to return all fields.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:principals`.
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
    pub async fn share_notification_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_sharing_types::ShareNotification>, jmap_base_client::ClientError>
    {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `principal_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("ShareNotification/get", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to ShareNotification objects since `since_state` (RFC 9670 §3.2).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
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
    ///   `urn:ietf:params:jmap:principals`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::share_notification_get`].
    pub async fn share_notification_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: even with the typed-`State` parameter (a transparent
        // newtype around `String`), an empty state token is still a logically
        // invalid value that should be caught client-side rather than producing
        // a confusing server-side `cannotCalculateChanges` error.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "share_notification_changes: since_state may not be empty".into(),
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
        let req = super::build_request("ShareNotification/changes", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Destroy ShareNotification objects (RFC 9670 §3.3).
    ///
    /// ShareNotification/set is destroy-only: the server rejects create/update
    /// operations with `forbidden` SetErrors. This method accepts only
    /// `destroy` to prevent constructing invalid requests.
    ///
    /// Pass an empty vec to send an empty destroy list (no-op round-trip).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:principals`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::share_notification_get`].
    pub async fn share_notification_set(
        &self,
        destroy: Vec<Id>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "destroy": destroy,
        });
        let req = super::build_request("ShareNotification/set", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query ShareNotification IDs with optional filter and sort (RFC 9670 §3.4).
    ///
    /// The server MUST support the `created` comparator property (RFC 9670 §3.4.1).
    /// Pass `filter: None` and `sort: None` to return all notifications with
    /// server-default ordering. Use `position` and `limit` for pagination.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:principals`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::share_notification_get`].
    ///   RFC 8620 §5.5 defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn share_notification_query(
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
        let req = super::build_request("ShareNotification/query", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for ShareNotification since `since_query_state`
    /// (RFC 9670 §3.5).
    ///
    /// Returns which ShareNotification IDs were removed from or added to the
    /// query result set since the given state. `max_changes` may be `None`.
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `ShareNotification/query` call that returned
    /// `since_query_state` — RFC 8620 §5.6 is explicit that the server uses
    /// them to compute which entries entered or left the result set.
    ///
    /// `up_to_id` is the highest-index id the client has cached;
    /// `calculate_total` requests the new total result count.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::share_notification_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:principals`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::share_notification_get`].
    ///   RFC 8620 §5.6 also defines `cannotCalculateChanges` (returned
    ///   when the server cannot honour the request given the supplied
    ///   filter / sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn share_notification_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `share_notification_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "share_notification_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(f) = filter {
            args["filter"] = f;
        }
        if let Some(s) = sort {
            args["sort"] = s;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(uti) = up_to_id {
            args["upToId"] = serde_json::to_value(uti).expect("Id Serialize is infallible");
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        let req =
            super::build_request("ShareNotification/queryChanges", args, super::USING_SHARING);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests — see tests/notification_*_tests.rs (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// `share_notification_set_no_destroy_sends_empty_array`,
// `share_notification_set_with_destroy_sends_ids`,
// `share_notification_get_request_shape`, and
// `share_notification_query_request_includes_filter` were vacuous: they
// hand-built `args` Values and fed them to `build_request`, never
// exercising the production `share_notification_get` /
// `share_notification_set` / `share_notification_query` /
// `share_notification_changes` / `share_notification_query_changes`
// builders. Deleted in JMAP-tco1.30.
//
// Real production-path coverage:
//   - `share_notification_get_round_trip` and
//     `share_notification_changes_sends_since_state` in
//     tests/notification_get_changes_tests.rs
//   - `share_notification_set_destroy_only_wire` and
//     `share_notification_set_empty_destroy` in
//     tests/notification_set_tests.rs
//   - `share_notification_query_with_filter` and
//     `share_notification_query_changes_round_trip` in
//     tests/notification_query_tests.rs
//
// Specific-flag passthrough coverage that may be lost is tracked
// under JMAP-uuoi for follow-up wiremock smoke tests.
//
// `build_request`, `CALL_ID`, and `USING_SHARING` themselves have their
// own focused tests in `methods/mod.rs`.
//
// Inline guard smoke tests (e.g. `share_notification_get_empty_id_returns_invalid_argument`,
// `share_notification_changes_empty_state_returns_invalid_argument`,
// `share_notification_query_changes_empty_state_returns_invalid_argument`)
// were removed earlier by the JMAP-6by7.7 typed-Id refactor. They were
// vacuous because they only iterated a local `&[""]` slice (or
// duplicated the guard's `is_empty()` check) and asserted `is_empty()`
// found the empty value, without invoking any production method. Under
// typed `&[Id]` / `Vec<Id>` parameters, an empty-Id input is impossible
// to express through the API (`Id::new_validated("")` returns `Err` at
// the call site) so the bug they pretended to test is unrepresentable.
// Defence-in-depth empty-state guards still live in the production code
// (`share_notification_changes`, `share_notification_query_changes`)
// using `as_ref().is_empty()`.
