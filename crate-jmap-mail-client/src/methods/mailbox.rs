//! JMAP Mail — Mailbox/* method implementations on SessionClient.
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
    ChangesResponse, GetResponse, MailboxSetParams, QueryChangesResponse, QueryResponse,
    SetResponse,
};

impl super::SessionClient {
    /// Fetch Mailbox objects by IDs (RFC 8621 §2.1 — Mailbox/get).
    ///
    /// If `ids` is `None`, the server returns all Mailboxes for the account.
    /// Pass `properties: None` to return all fields.
    pub async fn mailbox_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_mail_types::Mailbox>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` when None — see the matching comment on
        // `email_get` for the rationale (consistent with set/changes/query).
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = serde_json::to_value(id_slice).expect("Id slice Serialize is infallible");
        }
        if let Some(props) = properties {
            args["properties"] =
                serde_json::to_value(props).expect("&[&str] Serialize is infallible");
        }
        let req = super::build_request("Mailbox/get", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Mailbox objects since `since_state` (RFC 8621 §2.2 — Mailbox/changes).
    pub async fn mailbox_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "mailbox_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Mailbox/changes", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Mailbox objects (RFC 8621 §2.5 — Mailbox/set).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. Pass `None` to omit.
    /// Pass `params: Some(MailboxSetParams { on_destroy_remove_emails: Some(true) })`
    /// to allow destroying a non-empty mailbox.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    pub async fn mailbox_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        params: Option<MailboxSetParams>,
    ) -> Result<SetResponse<jmap_mail_types::Mailbox>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
        });
        let mut params_extra: Option<serde_json::Map<String, serde_json::Value>> = None;
        if let Some(p) = params {
            if let Some(v) = p.on_destroy_remove_emails {
                args["onDestroyRemoveEmails"] = v.into();
            }
            if !p.extra.is_empty() {
                params_extra = Some(p.extra);
            }
        }
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = serde_json::to_value(&u).map_err(|e| {
                jmap_base_client::ClientError::InvalidArgument(format!(
                    "mailbox_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        // Route caller-supplied vendor extras onto the wire (workspace
        // extras-preservation policy). Use `entry().or_insert()` so a
        // caller who put a typed wire key into `params.extra` cannot
        // silently clobber the typed value — typed wins on collision.
        if let Some(extra) = params_extra {
            let args_obj = args
                .as_object_mut()
                .expect("mailbox_set: args is constructed as Object");
            for (k, v) in extra {
                args_obj.entry(k).or_insert(v);
            }
        }
        let req = super::build_request("Mailbox/set", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Mailbox IDs with optional filter and sort (RFC 8621 §2.3 — Mailbox/query).
    ///
    /// Pass `filter: None` and `sort: None` to return all Mailboxes with
    /// server-default ordering. Use `position` and `limit` for pagination.
    pub async fn mailbox_query(
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
        let req = super::build_request("Mailbox/query", args, super::USING_MAIL);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Mailbox since `since_query_state`
    /// (RFC 8621 §2.4 — Mailbox/queryChanges).
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `Mailbox/query` call that returned `since_query_state` —
    /// RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set. Omitting them when
    /// the original query had a non-trivial filter or sort gives the
    /// wrong added/removed deltas (or `cannotCalculateChanges`).
    ///
    /// `up_to_id` is the highest-index id the client has cached
    /// (RFC 8620 §5.6); the server may use it to omit changes past that
    /// point when both `filter` and `sort` are on immutable properties.
    ///
    /// `calculate_total` requests the new total result count.
    pub async fn mailbox_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<serde_json::Value>,
        sort: Option<serde_json::Value>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `thread_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "mailbox_query_changes: since_query_state may not be empty".into(),
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
        let req = super::build_request("Mailbox/queryChanges", args, super::USING_MAIL);
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

    // mailbox_get_empty_id_returns_invalid_argument was deleted in JMAP-6by7.2
    // (typed-Id refactor): under `Option<&[Id]>` the empty-Id case becomes
    // impossible to express through the typed API.

    // The InvalidArgument guards for empty since_state and since_query_state
    // live in mailbox_changes / mailbox_query_changes production code; testing
    // them requires a wiremock-backed async harness. See JMAP-sc1b.64.

    // Deleted in JMAP-tco1.5 as Pattern E (vacuous inline tests):
    //   - mailbox_get_request_shape
    //   - mailbox_set_on_destroy_remove_emails_request_shape
    //   - mailbox_query_request_includes_filter
    // Each hand-built `args = json!({...})` and fed it to `build_request`,
    // never invoking the `mailbox_get` / `mailbox_set` / `mailbox_query`
    // production builders. Real production-path coverage for these methods
    // is tracked as a wiremock-smoke gap under JMAP-uuoi (no
    // `tests/mailbox_*.rs` smoke file exists yet). Specific-flag passthrough
    // coverage that may be lost (e.g. `onDestroyRemoveEmails`) is also
    // tracked under JMAP-uuoi for follow-up wiremock smoke tests.
    //
    // `build_request`, `CALL_ID`, and `USING_MAIL` themselves have their
    // own focused tests in `methods/mod.rs`.

    /// Oracle: Mailbox deserialization from RFC 8621 §2 example.
    #[test]
    fn mailbox_get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s10",
            "list": [
                {
                    "id": "mb1",
                    "name": "Inbox",
                    "role": "inbox",
                    "sortOrder": 10,
                    "totalEmails": 42,
                    "unreadEmails": 3,
                    "totalThreads": 20,
                    "unreadThreads": 2,
                    "myRights": {
                        "mayReadItems": true,
                        "mayAddItems": true,
                        "mayRemoveItems": true,
                        "maySetSeen": true,
                        "maySetKeywords": true,
                        "mayCreateChild": true,
                        "mayRename": true,
                        "mayDelete": false,
                        "maySubmit": false
                    },
                    "isSubscribed": true
                }
            ],
            "notFound": []
        });
        use super::super::GetResponse;
        let resp: GetResponse<jmap_mail_types::Mailbox> =
            serde_json::from_value(json).expect("must deserialize Mailbox GetResponse");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].name, "Inbox");
    }
}
