// Quota/get — urn:ietf:params:jmap:quota
//
// Retrieves storage quota information from the server.  Only call when
// `ChatSessionExt::supports_quotas()` returns true.
//
// Spec: RFC 8621 §2

use serde::Deserialize;

use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse};

/// A single JMAP Quota object (RFC 8621 §2).
///
/// Describes a storage limit that applies to one or more data types within
/// a given scope.  Poll with [`SessionClient::quota_get`] to display storage
/// usage in the UI and warn the user when approaching limits.
///
/// [`SessionClient::quota_get`]: super::SessionClient::quota_get
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    /// Server-assigned identifier.
    pub id: Id,
    /// Human-readable name for this quota (e.g. `"Message Storage"`).
    pub name: String,
    /// Scope of the quota: `"account"`, `"domain"`, or `"global"`.
    pub scope: crate::types::QuotaScope,
    /// Resource type: `"octets"` (byte-based) or `"count"` (object-count-based).
    pub resource_type: String,
    /// Data type names covered by this quota (e.g. `["Message", "Chat"]`).
    pub types: Vec<String>,
    /// Bytes currently consumed.
    pub used: u64,
    /// Hard limit in bytes; requests that would exceed this MUST fail.
    pub hard_limit: u64,
    /// Warning threshold in bytes; clients SHOULD warn the user above this.
    #[serde(default)]
    pub warn_limit: Option<u64>,
    /// Soft limit in bytes (server may begin rejecting requests above this).
    #[serde(default)]
    pub soft_limit: Option<u64>,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: Option<String>,
}

impl super::SessionClient {
    /// Fetch all Quota objects for the account (RFC 8621 §2.1 Quota/get).
    ///
    /// Returns all quota records for the primary JMAP Chat account.  Each
    /// [`Quota`] includes `used`, `hard_limit`, and optional `warn_limit` fields
    /// that callers can use to display storage bars and warnings.
    ///
    /// The returned [`GetResponse::state`] token is preserved for
    /// [`quota_changes`](Self::quota_changes) delta-sync support.
    ///
    /// Only call when [`crate::session::ChatSessionExt::supports_quotas`]
    /// returns `true`.  Returns `ClientError::InvalidSession` if the session
    /// has no primary JMAP Chat account.
    pub async fn quota_get(&self) -> Result<GetResponse<Quota>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let args = serde_json::json!({
            "accountId": account_id,
            "ids": serde_json::Value::Null,
        });
        let req = super::build_request("Quota/get", args, super::USING_QUOTA);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Quota objects since `since_state` (RFC 8620 §5.2 / Quota/changes).
    ///
    /// Returns ids of Quota objects created, updated, or destroyed since the
    /// caller-supplied `since_state` token (typically the
    /// [`GetResponse::state`] returned by an earlier
    /// [`quota_get`](Self::quota_get) call).
    ///
    /// If [`ChangesResponse::has_more_changes`] is `true`, call again with
    /// [`ChangesResponse::new_state`] as `since_state` until the flag is
    /// `false`.
    ///
    /// `max_changes` caps the number of ids the server returns in a single
    /// response; `None` lets the server choose. Servers are not required to
    /// honour a `max_changes` hint exactly.
    ///
    /// Only call when [`crate::session::ChatSessionExt::supports_quotas`]
    /// returns `true`. Returns [`jmap_base_client::ClientError::InvalidArgument`]
    /// if `since_state` is empty, or [`jmap_base_client::ClientError::InvalidSession`]
    /// if the session has no primary JMAP Chat account.
    pub async fn quota_changes(
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
                "quota_changes: since_state may not be empty".into(),
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
        let req = super::build_request("Quota/changes", args, super::USING_QUOTA);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}
