//! Quota/get — urn:ietf:params:jmap:quota
//!
//! Retrieves storage quota information from the server.  Only call when
//! `ChatSessionExt::supports_quotas()` returns true.
//!
//! Spec: RFC 9425

use serde::Deserialize;

use jmap_types::{Id, State};

use super::{ChangesResponse, GetResponse};

/// A single JMAP Quota object (RFC 9425 §4).
///
/// Describes a storage limit that applies to one or more data types within
/// a given scope.  Poll with [`SessionClient::quota_get`] to display storage
/// usage in the UI and warn the user when approaching limits.
///
/// [`SessionClient::quota_get`]: super::SessionClient::quota_get
#[non_exhaustive]
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
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the
    /// typed-field wire names above (the camelCase spelling — e.g.
    /// `"accountId"`, `"ids"`, `"properties"`, `"blobIds"`,
    /// `"fromAccountId"`, etc.). On collision the typed-field value
    /// wins on the wire and the `extra` value is silently dropped at
    /// serialization. Place vendor extensions under vendor-prefixed
    /// keys (e.g. `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl super::SessionClient {
    /// Fetch all Quota objects for the account (RFC 9425 §4.2 Quota/get).
    ///
    /// Returns all quota records for the primary JMAP Chat account.  Each
    /// [`Quota`] includes `used`, `hard_limit`, and optional `warn_limit` fields
    /// that callers can use to display storage bars and warnings.
    ///
    /// The returned [`GetResponse::state`] token is preserved for
    /// [`quota_changes`](Self::quota_changes) delta-sync support.
    ///
    /// Only call when [`crate::session::ChatSessionExt::supports_quotas`]
    /// returns `true`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call):
    ///   [`Http`](jmap_base_client::ClientError::Http),
    ///   [`Parse`](jmap_base_client::ClientError::Parse),
    ///   [`AuthFailed`](jmap_base_client::ClientError::AuthFailed),
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError)
    ///   (wraps RFC 8620 §3.6.2 method-level errors such as
    ///   `accountNotFound`, `invalidArguments`, `serverFail`; servers
    ///   that do not advertise `urn:ietf:params:jmap:quota` return
    ///   `unknownCapability`),
    ///   [`MethodNotFound`](jmap_base_client::ClientError::MethodNotFound),
    ///   [`ResponseTooLarge`](jmap_base_client::ClientError::ResponseTooLarge),
    ///   or
    ///   [`UnexpectedResponse`](jmap_base_client::ClientError::UnexpectedResponse).
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
    /// returns `true`.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_state` is the empty string (defence-in-depth —
    ///   `State` constructed via [`State::from`](jmap_types::State::from)
    ///   accepts empty strings, but an empty `sinceState` is never
    ///   useful).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:chat`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::quota_get`].
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // The test deserialises wire JSON containing a synthetic `acmeCorp*`
    // vendor field and asserts it survives in `extra`. The vendor field
    // name cannot collide with any field defined in RFC 9425 §4, so the
    // test is independent of the code under test (workspace
    // test-integrity rule).

    /// `Quota.extra` captures unknown fields on deserialize.
    #[test]
    fn quota_preserves_vendor_extras() {
        let raw = json!({
            "id": "Q1",
            "name": "Message Storage",
            "scope": "account",
            "resourceType": "octets",
            "types": ["Message"],
            "used": 1024,
            "hardLimit": 1048576,
            "acmeCorpBillingTier": "enterprise"
        });
        let obj: Quota = serde_json::from_value(raw).expect("Quota must deserialize");
        assert_eq!(
            obj.extra
                .get("acmeCorpBillingTier")
                .and_then(|v| v.as_str()),
            Some("enterprise")
        );
    }
}
