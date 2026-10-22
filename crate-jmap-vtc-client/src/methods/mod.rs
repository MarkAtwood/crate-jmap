//! Typed JMAP VTC method wrappers — response types, SessionClient,
//! constants, and helpers.
//!
//! Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.2
//! /changes, §5.3 /set, §5.5 /query, §5.6 /queryChanges). Every VTC method
//! is a standard method of one of those five shapes
//! (draft-atwood-jmap-vtc-00 §Methods), so the five request builders live
//! here once and the per-type sub-modules bind them to a method name, a
//! filter type and a comparator type.

pub mod vtc_call;
pub mod vtc_livestream;
pub mod vtc_participant;
pub mod vtc_recording;

use std::collections::BTreeMap;

use jmap_types::{query::Filter, Id, PatchObject, State};
use serde::{de::DeserializeOwned, Serialize};

// ---------------------------------------------------------------------------
// Response types (RFC 8620 §5)
// ---------------------------------------------------------------------------
//
// Re-exported from `jmap-types::methods` so all `jmap-*-client` crates share
// one canonical set of /get, /set, /changes, /query, /queryChanges shapes.

pub use jmap_types::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetError,
    SetResponse,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced by
/// [`build_request`]. Pass directly to `jmap_base_client::extract_response`.
pub(crate) const CALL_ID: &str = "r1";

/// Capability URIs for JMAP VTC method calls.
pub(crate) const USING_VTC: &[&str] = &["urn:ietf:params:jmap:core", jmap_vtc_types::JMAP_VTC_URI];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// `using` is the complete `using` array for the request (RFC 8620 §3.3).
/// Use the pre-defined constant [`USING_VTC`] for standard calls.
///
/// The embedded call-id is [`CALL_ID`]; pass it directly to
/// `jmap_base_client::extract_response`.
pub(crate) fn build_request(
    method: &str,
    args: serde_json::Value,
    using: &[&str],
) -> jmap_types::JmapRequest {
    let using_vec: Vec<String> = using.iter().map(|&s| s.to_owned()).collect();
    let invocation = jmap_types::Invocation::new(method, args, CALL_ID);
    jmap_types::JmapRequest::new(using_vec, vec![invocation], None)
}

/// Serialize a caller-supplied argument, mapping failure to
/// `InvalidArgument` naming the method and argument.
fn to_arg<T: Serialize + ?Sized>(
    method: &str,
    name: &str,
    value: &T,
) -> Result<serde_json::Value, jmap_base_client::ClientError> {
    serde_json::to_value(value).map_err(|e| {
        jmap_base_client::ClientError::InvalidArgument(format!(
            "{method}: serializing {name} failed: {e}"
        ))
    })
}

// ---------------------------------------------------------------------------
// SessionClient — session-bound client
// ---------------------------------------------------------------------------

/// A `JmapClient` bound to a JMAP session.
///
/// Obtain via [`JmapVtcExt::with_vtc_session`](crate::JmapVtcExt::with_vtc_session).
/// All JMAP VTC methods are available on this type without needing to pass
/// `&Session` on every call.
///
/// # Session lifecycle
///
/// `SessionClient` captures the `Session` at construction time. After
/// re-fetching the session via `JmapClient::fetch_session`, construct a new
/// `SessionClient` with the updated session. Reusing a stale `SessionClient`
/// after session expiry will result in `unknownAccount` or similar errors
/// from the server.
///
/// `Clone` is derived because `JmapClient` is itself cheap-to-clone (it
/// already implements `Clone` and `with_vtc_session` clones one
/// internally), enabling parallel-task fan-out with one bound session.
///
/// `Debug` is implemented manually to redact the inner `JmapClient` (which
/// holds an HTTP client and is intentionally not `Debug` in
/// `jmap-base-client`); only the `Session` is shown.
///
/// # Thread safety
///
/// `SessionClient` is `Send + Sync`. Both
/// [`jmap_base_client::JmapClient`] and [`jmap_base_client::Session`] are
/// `Send + Sync` per jmap-base-client's contract. A compile-time assertion
/// in `methods/mod.rs` (`_assert_session_client_send_sync`) guards against
/// a regression landing silently.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
    /// Account targeted by every method; `None` means the session's
    /// primary VTC account. Set only by [`SessionClient::for_account`].
    pub(crate) account_id: Option<String>,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            // The inner JmapClient is not Debug — show a placeholder so
            // callers know it is present without leaking HTTP-client
            // internals.
            .field("client", &"<JmapClient>")
            .field("session", &self.session)
            .field("account_id", &self.account_id)
            .finish()
    }
}

impl SessionClient {
    /// Borrow the underlying [`JmapClient`](jmap_base_client::JmapClient).
    ///
    /// Useful for ad-hoc operations outside the typed JMAP method surface,
    /// such as blob download of an available `VTCRecording.blobId` or an
    /// event-source subscription.
    pub fn client(&self) -> &jmap_base_client::JmapClient {
        &self.client
    }

    /// Borrow the captured [`Session`](jmap_base_client::Session).
    ///
    /// Use it to read `accountCapabilities["urn:ietf:params:jmap:vtc"]`
    /// (draft-atwood-jmap-vtc-00 §Account-Level Capability Object) or to
    /// compare the captured session `state` against a fresh fetch.
    pub fn session(&self) -> &jmap_base_client::Session {
        &self.session
    }

    /// Return the account id every typed method on this handle targets:
    /// the account chosen with [`Self::for_account`], or else the primary
    /// account for `urn:ietf:params:jmap:vtc`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if no account was chosen with [`Self::for_account`] and the
    ///   session has no primary account for `urn:ietf:params:jmap:vtc`.
    ///   No network I/O is done.
    pub fn vtc_account_id(&self) -> Result<&str, jmap_base_client::ClientError> {
        if let Some(id) = &self.account_id {
            return Ok(id);
        }
        self.session
            .primary_account_id(jmap_vtc_types::JMAP_VTC_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:vtc".into(),
                )
            })
    }

    /// Return a handle whose typed methods target `account_id` instead of
    /// the primary VTC account.
    ///
    /// Every method on a call's records goes to the call's hosting account
    /// (draft-atwood-jmap-vtc-00 L1654: "A participant who answers, mutes,
    /// or leaves therefore sends `VTCParticipant/set` to the hosting
    /// account, not to their personal account"). Use this handle for a
    /// call hosted in a shared account, and once per account to list the
    /// calls of every account the user can access. The returned handle is
    /// a cheap clone sharing the HTTP connection pool; `self` keeps its
    /// own target. Per-method `InvalidSession` errors documented as "no
    /// primary account" cannot occur on the returned handle.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `account_id` is not in the session's `accounts` map, or its
    ///   `accountCapabilities` lacks `urn:ietf:params:jmap:vtc`
    ///   (RFC 8620 §2: that object is "the set of capability URIs for the
    ///   methods supported in this account"). No network I/O is done.
    pub fn for_account(&self, account_id: &str) -> Result<Self, jmap_base_client::ClientError> {
        let vtc_capable = self.session.accounts.get(account_id).is_some_and(|a| {
            a.account_capabilities
                .contains_key(jmap_vtc_types::JMAP_VTC_URI)
        });
        if !vtc_capable {
            return Err(jmap_base_client::ClientError::InvalidArgument(format!(
                "for_account: {account_id} is not a urn:ietf:params:jmap:vtc account in this session"
            )));
        }
        Ok(Self {
            account_id: Some(account_id.to_owned()),
            ..self.clone()
        })
    }

    /// Extract `(api_url, vtc_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` under the same condition as
    /// [`Self::vtc_account_id`].
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let account_id = self.vtc_account_id()?;
        Ok((self.session.api_url.as_str(), account_id))
    }

    /// Send `args` as a single `method` invocation and extract the typed
    /// result for [`CALL_ID`].
    async fn invoke<T: DeserializeOwned>(
        &self,
        api_url: &str,
        method: &str,
        args: serde_json::Value,
    ) -> Result<T, jmap_base_client::ClientError> {
        let req = build_request(method, args, USING_VTC);
        let resp = self.client.call(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, CALL_ID)
    }

    /// RFC 8620 §5.1 `/get`. `ids` and `properties` are omitted when `None`.
    pub(crate) async fn standard_get<T: DeserializeOwned>(
        &self,
        method: &str,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<T>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(ids) = ids {
            args["ids"] = to_arg(method, "ids", ids)?;
        }
        if let Some(props) = properties {
            args["properties"] = to_arg(method, "properties", props)?;
        }
        self.invoke(api_url, method, args).await
    }

    /// RFC 8620 §5.2 `/changes`. An empty `since_state` is rejected before
    /// any I/O: it is never a valid state token.
    pub(crate) async fn standard_changes(
        &self,
        method: &str,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(format!(
                "{method}: since_state may not be empty"
            )));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceState": since_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        self.invoke(api_url, method, args).await
    }

    /// RFC 8620 §5.3 `/set`. At least one of `create`, `update`, `destroy`
    /// must be `Some`.
    pub(crate) async fn standard_set(
        &self,
        method: &str,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
    ) -> Result<SetResponse, jmap_base_client::ClientError> {
        if create.is_none() && update.is_none() && destroy.is_none() {
            return Err(jmap_base_client::ClientError::InvalidArgument(format!(
                "{method}: at least one of create, update, destroy must be Some \
                 (an all-None /set is a no-op round-trip)"
            )));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(s) = if_in_state {
            args["ifInState"] = s.as_ref().into();
        }
        if let Some(c) = create {
            args["create"] = c;
        }
        if let Some(u) = update {
            args["update"] = to_arg(method, "update", &u)?;
        }
        if let Some(d) = destroy {
            args["destroy"] = to_arg(method, "destroy", &d)?;
        }
        self.invoke(api_url, method, args).await
    }

    /// RFC 8620 §5.5 `/query` with `filter`, `sort` and the window [`QueryParams`](jmap_base_client::QueryParams).
    pub(crate) async fn standard_query<C: Serialize, S: Serialize>(
        &self,
        method: &str,
        filter: Option<&Filter<C>>,
        sort: Option<&[S]>,
        params: &jmap_base_client::QueryParams,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = params.to_arguments(account_id);
        if let Some(f) = filter {
            args["filter"] = to_arg(method, "filter", f)?;
        }
        if let Some(s) = sort {
            args["sort"] = to_arg(method, "sort", s)?;
        }
        self.invoke(api_url, method, args).await
    }

    /// RFC 8620 §5.6 `/queryChanges`. An empty `since_query_state` is
    /// rejected before any I/O.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn standard_query_changes<C: Serialize, S: Serialize>(
        &self,
        method: &str,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&Filter<C>>,
        sort: Option<&[S]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(format!(
                "{method}: since_query_state may not be empty"
            )));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(f) = filter {
            args["filter"] = to_arg(method, "filter", f)?;
        }
        if let Some(s) = sort {
            args["sort"] = to_arg(method, "sort", s)?;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(id) = up_to_id {
            args["upToId"] = to_arg(method, "upToId", id)?;
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        self.invoke(api_url, method, args).await
    }
}

/// Compile-time assertion that [`SessionClient`] is `Send + Sync`.
///
/// If a future jmap-base-client release adds a `!Sync` field to
/// `JmapClient` or `Session`, this fails at compile time rather than at a
/// downstream caller's "cannot send between threads safely" error.
#[allow(dead_code)]
fn _assert_session_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SessionClient>();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: RFC 8620 §3.2 — an Invocation is `[name, arguments, callId]`.
    #[test]
    fn build_request_method_name_and_call_id() {
        let req = build_request("VTCCall/get", json!({"accountId": "acc1"}), USING_VTC);
        let v = serde_json::to_value(&req).expect("serialize JmapRequest");
        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 1, "must have exactly 1 method call");
        assert_eq!(calls[0][0], json!("VTCCall/get"));
        assert_eq!(calls[0][2], json!("r1"));
    }

    /// Oracle: draft-atwood-jmap-vtc-00 L188 "Servers supporting this
    /// specification MUST advertise the `urn:ietf:params:jmap:vtc`
    /// capability"; RFC 8620 §3.3 `using` lists the capabilities used.
    #[test]
    fn using_vtc_contains_core_and_vtc() {
        let req = build_request("VTCCall/get", json!({}), USING_VTC);
        let v = serde_json::to_value(&req).expect("serialize");
        assert_eq!(
            v["using"],
            json!(["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:vtc"])
        );
    }
}
