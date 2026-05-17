//! JMAP Mail — Email/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_MAIL)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, EmailCopyParams, EmailGetParams, EmailImportInput, EmailImportResponse,
    EmailParseParams, EmailParseResponse, GetResponse, QueryChangesResponse, QueryResponse,
    SetResponse,
};

impl super::SessionClient {
    /// Fetch Email objects by IDs (RFC 8621 §4.1.8 — Email/get).
    ///
    /// If `ids` is `None`, the server returns all Emails for the account.
    /// Pass `properties: None` to return all fields.
    /// Pass `params: None` to use server defaults for body-fetch options.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`] if the bound session has no
    ///   primary account for `urn:ietf:params:jmap:mail`.
    /// - [`ClientError::InvalidArgument`] if `params` is `Some` and
    ///   serializing it to JSON fails (pathological conditions only —
    ///   allocation failure, or a vendor value in `params.extra` that
    ///   itself fails to serialize).
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
    ///
    /// [`ClientError::InvalidSession`]: jmap_base_client::ClientError::InvalidSession
    /// [`ClientError::InvalidArgument`]: jmap_base_client::ClientError::InvalidArgument
    pub async fn email_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        params: Option<EmailGetParams>,
    ) -> Result<GetResponse<jmap_mail_types::Email>, jmap_base_client::ClientError> {
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
        if let Some(p) = params {
            let pv = serde_json::to_value(p).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "email_get: failed to serialize params: {e}"
                ))
            })?;
            if let serde_json::Value::Object(map) = pv {
                // Use `entry().or_insert()` so a caller who put a typed
                // wire key (e.g. "accountId", "ids", "properties") into
                // `params.extra` cannot silently clobber the value
                // computed from the bound session and the typed args.
                // Typed wins on collision.
                let args_obj = args
                    .as_object_mut()
                    .expect("email_get: args is constructed as Object");
                for (k, v) in map {
                    args_obj.entry(k).or_insert(v);
                }
            }
        }
        let req = super::build_request("Email/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Email objects since `since_state` (RFC 8621 §4.2 — Email/changes).
    ///
    /// If `has_more_changes` is true in the response, call again with `new_state`
    /// as `since_state` until the flag is false.
    ///
    /// # `max_changes` spec magic-values (RFC 8620 §5.2)
    ///
    /// - `None` omits the wire field and lets the server apply its
    ///   default cap.
    /// - `Some(0)` is wire-legal and means "no client limit"; the
    ///   server may still apply its own cap. This is distinct from
    ///   `None`: `None` says "I haven't expressed a preference",
    ///   `Some(0)` says "I want as many entries as the server is
    ///   willing to return in one round-trip".
    /// - `Some(n)` with `n > 0` requests at most `n` entries; the
    ///   server may return fewer.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth — `State`
    ///   constructed via [`State::from`](jmap_types::State::from) accepts
    ///   empty strings, but an empty `sinceState` is never useful and
    ///   would otherwise generate a wasted round-trip).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::email_get`].
    pub async fn email_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Email/changes", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Email objects (RFC 8621 §4.3 — Email/set).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard.
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
    ///   patch map. In practice this happens only under pathological
    ///   conditions (allocation failure on a very large `HashMap`, or
    ///   a `PatchObject` whose JSON tree exceeds `serde_json`'s
    ///   recursion limit). The size of `update` is otherwise bounded
    ///   only by available memory; the wire request is buffered by the
    ///   HTTP client (`reqwest::RequestBuilder::json` calls
    ///   `serde_json::to_vec` upfront), so the transient peak holds
    ///   the source `HashMap`, the intermediate `serde_json::Value`
    ///   tree, and the serialized `Vec<u8>` body simultaneously —
    ///   roughly 3-4× the `HashMap`'s in-memory size. Callers dealing
    ///   with thousands of patches per call may prefer to batch.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::email_get`].
    pub async fn email_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse<jmap_mail_types::Email>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "email_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("Email/set", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Email IDs with optional filter and sort (RFC 8621 §4.4 — Email/query).
    ///
    /// Pass `filter: None` and `sort: None` to return all Emails with
    /// server-default ordering. Use `position` and `limit` for pagination.
    /// Pass `collapse_threads: Some(true)` to return at most one email per thread.
    ///
    /// # Numeric parameter spec magic-values (RFC 8620 §5.5)
    ///
    /// - `position: Some(0)` selects the first item (zero-indexed); the
    ///   spec also accepts negative values, but `u64` does not represent
    ///   them — pass `None` to omit and use the server default of `0`.
    /// - `limit: Some(0)` is wire-legal but means "server's default
    ///   cap", NOT "zero results"; the server is free to return its
    ///   default page size. Pass `None` to omit (server applies its
    ///   default), or `Some(n)` with `n > 0` for an explicit cap.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::email_get`]. RFC 8620 §5.5
    ///   defines additional method-level errors specific to /query
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`); they surface here as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   with the corresponding `error_type` string.
    pub async fn email_query(
        &self,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        position: Option<u64>,
        limit: Option<u64>,
        collapse_threads: Option<bool>,
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
        if let Some(ct) = collapse_threads {
            args["collapseThreads"] = ct.into();
        }
        let req = super::build_request("Email/query", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Email since `since_query_state`
    /// (RFC 8621 §4.5 — Email/queryChanges).
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `Email/query` call that returned `since_query_state` —
    /// RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set. Omitting them when
    /// the original query had a non-trivial filter or sort tells the
    /// server "the original query had no filter and default sort", which
    /// gives the wrong added/removed deltas (or `cannotCalculateChanges`).
    ///
    /// `up_to_id` is the highest-index id the client has cached
    /// (RFC 8620 §5.6); the server may use it to omit changes past that
    /// point when both `filter` and `sort` are on immutable properties.
    ///
    /// `calculate_total` requests the new total result count.
    ///
    /// `max_changes` follows the same magic-value semantics as
    /// [`Self::email_changes`].
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::email_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::email_get`]. RFC 8620 §5.6
    ///   also defines `cannotCalculateChanges` (returned when the server
    ///   cannot honour the request given the supplied filter / sort);
    ///   it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    #[allow(clippy::too_many_arguments)] // RFC 8620 §5.6 + RFC 8621 §4.5 args
    pub async fn email_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        collapse_threads: Option<bool>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_query_changes: since_query_state may not be empty".into(),
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
        if let Some(ct) = collapse_threads {
            args["collapseThreads"] = ct.into();
        }
        let req = super::build_request("Email/queryChanges", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Copy Emails from another account (RFC 8621 §4.7 — Email/copy).
    ///
    /// `params` carries `fromAccountId` and optional destroy-after-copy flags.
    /// `create` is a map of creation keys to partial Email objects (with new
    /// mailboxIds etc.) as described in RFC 8621 §4.7.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::email_get`]. RFC 8620 §5.4
    ///   /copy adds method-level errors `fromAccountNotFound`,
    ///   `fromAccountNotSupportedByMethod`, and `anchorNotFound` (the
    ///   latter under `onSuccessDestroyOriginal`); they surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn email_copy(
        &self,
        params: EmailCopyParams,
        create: serde_json::Value,
    ) -> Result<SetResponse<jmap_mail_types::Email>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "fromAccountId": params.from_account_id,
            "create": create,
        });
        if let Some(v) = params.on_success_destroy_original {
            args["onSuccessDestroyOriginal"] = v.into();
        }
        if let Some(v) = params.destroy_from_if_in_state {
            args["destroyFromIfInState"] = v.as_ref().into();
        }
        // Route caller-supplied vendor extras onto the wire (workspace
        // extras-preservation policy). Use `entry().or_insert()` so a
        // caller who put a typed wire key into `params.extra` cannot
        // silently clobber the typed value — typed wins on collision.
        if !params.extra.is_empty() {
            let args_obj = args
                .as_object_mut()
                .expect("email_copy: args is constructed as Object");
            for (k, v) in params.extra {
                args_obj.entry(k).or_insert(v);
            }
        }
        let req = super::build_request("Email/copy", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Import raw RFC 5322 messages into the account (RFC 8621 §4.8 — Email/import).
    ///
    /// Each entry in `emails` maps a caller-chosen creation id to an
    /// [`EmailImportInput`] referencing a previously uploaded blob and the
    /// target mailbox(es). The blob must have been uploaded via the standard
    /// blob-upload mechanism on `jmap-base-client` before calling this method.
    ///
    /// At least one mailbox id is required per RFC 8621 §4.8; the empty-set
    /// case is rejected client-side as `InvalidArgument`. An empty `emails`
    /// map is also rejected — a no-op import is never useful and would
    /// generate a round-trip with no successful creations.
    ///
    /// Pass `if_in_state: Some(&state)` for an optimistic-concurrency guard
    /// against the Email object state (RFC 8621 §4.8 returns `stateMismatch`
    /// if the supplied state does not match).
    ///
    /// Per-creation failures appear in [`EmailImportResponse::not_created`]
    /// as [`super::SetError`] values; possible error codes include `alreadyExists`
    /// (with an `existingId` extra field), `invalidProperties`, `overQuota`,
    /// and `invalidEmail`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `emails` is empty, if any entry's `mailbox_ids` is empty
    ///   (RFC 8621 §4.8 requires at least one mailbox per import), or
    ///   if serializing the `emails` map fails (pathological conditions
    ///   only — allocation failure, or a vendor value in
    ///   `EmailImportInput.extra` that itself fails to serialize).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::email_get`]. RFC 8621 §4.8
    ///   defines `maxQuotaReached`, `fromAccountNotFound`, and
    ///   `stateMismatch` (the last when `if_in_state` does not match);
    ///   they surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    ///   Per-creation failures (e.g. `alreadyExists`, `invalidEmail`)
    ///   do NOT surface as `Err` — they appear in
    ///   [`EmailImportResponse::not_created`].
    pub async fn email_import(
        &self,
        emails: &HashMap<String, EmailImportInput<'_>>,
        if_in_state: Option<&State>,
    ) -> Result<EmailImportResponse, jmap_base_client::ClientError> {
        if emails.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_import: emails map may not be empty".into(),
            ));
        }
        for (key, input) in emails {
            if input.mailbox_ids.is_empty() {
                return Err(jmap_base_client::ClientError::InvalidArgument(format!(
                    "email_import: mailboxIds for creation id '{key}' may not be empty (RFC 8621 §4.8)"
                )));
            }
        }
        let (api_url, account_id) = self.session_parts()?;
        let emails_value = serde_json::to_value(emails).map_err(|e| {
            jmap_base_client::ClientError::InvalidArgument(format!(
                "email_import: serializing emails map failed: {e}"
            ))
        })?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "emails": emails_value,
        });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        let req = super::build_request("Email/import", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Parse uploaded blobs as RFC 5322 messages without importing them
    /// (RFC 8621 §4.9 — Email/parse).
    ///
    /// Returns Email objects derived from each blob. Per RFC 8621 §4.9 the
    /// returned Emails have `id`, `mailboxIds`, `keywords`, and `receivedAt`
    /// set to `null` (the messages are not in the mail store); `threadId`
    /// MAY be present if the server can predict the assignment.
    ///
    /// Pass `params: None` to use server defaults for properties and body
    /// fetching. The set of properties returned defaults to the spec-listed
    /// header/body fields documented in RFC 8621 §4.9.
    ///
    /// Empty `blob_ids` is rejected as `InvalidArgument` — a no-op parse
    /// round-trip is never useful.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `blob_ids` is empty, or if `params` is `Some` and serializing
    ///   it to JSON fails (pathological conditions only — allocation
    ///   failure, or a vendor value in `params.extra` that itself fails
    ///   to serialize).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:mail`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::email_get`].
    pub async fn email_parse(
        &self,
        blob_ids: &[Id],
        params: Option<EmailParseParams>,
    ) -> Result<EmailParseResponse, jmap_base_client::ClientError> {
        if blob_ids.is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "email_parse: blob_ids may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "blobIds": blob_ids,
        });
        if let Some(p) = params {
            let pv = serde_json::to_value(&p).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "email_parse: failed to serialize params: {e}"
                ))
            })?;
            if let serde_json::Value::Object(map) = pv {
                // Use `entry().or_insert()` so a caller who put a typed
                // wire key (e.g. "accountId", "blobIds", "properties")
                // into `params.extra` cannot silently clobber the value
                // computed from the bound session and the typed args.
                // Typed wins on collision.
                let args_obj = args
                    .as_object_mut()
                    .expect("email_parse: args is constructed as Object");
                for (k, v) in map {
                    args_obj.entry(k).or_insert(v);
                }
            }
        }
        let req = super::build_request("Email/parse", args, super::USING_MAIL);
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

    // email_get_empty_id_returns_invalid_argument was deleted in JMAP-6by7.2
    // (typed-Id refactor): under `Option<&[Id]>` the empty-Id case becomes
    // impossible to express through the typed API.

    // The InvalidArgument guards for empty since_state and since_query_state
    // live in email_changes / email_query_changes production code; testing them
    // requires a wiremock-backed async harness. See JMAP-sc1b.64.

    // Deleted in JMAP-tco1.5 as Pattern E (vacuous inline tests):
    //   - email_get_request_shape
    //   - email_changes_request_includes_since_state
    //   - email_set_destroy_request_shape
    //   - email_copy_request_shape
    //   - email_query_request_shape
    // Each hand-built `args = json!({...})` and fed it to `build_request`,
    // never invoking the `email_get` / `email_changes` / `email_set` /
    // `email_copy` / `email_query` production builders. Real production-path
    // coverage for these methods is tracked as a wiremock-smoke gap under
    // JMAP-uuoi (no `tests/email_*.rs` smoke files exist yet).
    //
    // `build_request`, `CALL_ID`, and `USING_MAIL` themselves have their
    // own focused tests in `methods/mod.rs`.

    /// Oracle: Email deserialization from RFC 8621 §4 example JSON subset.
    /// Only fields present in the fixture are checked; Email has many optional fields.
    #[test]
    fn email_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s10",
            "list": [
                {
                    "id": "e1",
                    "blobId": "b1",
                    "threadId": "t1",
                    "mailboxIds": { "mb1": true },
                    "keywords": { "$seen": true },
                    "size": 1024,
                    "receivedAt": "2024-01-01T00:00:00Z"
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Email> =
            serde_json::from_value(json).expect("must deserialize Email GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].id.as_ref(), "e1");
    }
}
