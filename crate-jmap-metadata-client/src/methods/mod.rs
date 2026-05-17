//! Typed JMAP Metadata method wrappers — response types, SessionClient,
//! constants, and helpers.
//!
//! Response types mirror RFC 8620 standard shapes (§5.1 /get, §5.5 /query,
//! §5.2 /changes, §5.3 /set, §5.6 /queryChanges). Method implementations
//! live in sub-modules and operate on `SessionClient`.

pub mod metadata;

// ---------------------------------------------------------------------------
// Response types (RFC 8620 §5)
// ---------------------------------------------------------------------------
//
// Re-exported from `jmap-types::methods` so all `jmap-*-client` crates share
// one canonical set of /get, /set, /changes, /query, /queryChanges shapes.
// The wire format is identical to the previous local definitions.

pub use jmap_types::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SetError,
    SetResponse,
};

// ---------------------------------------------------------------------------
// Method-level argument structs (draft-ietf-jmap-metadata-01 §3 + workspace
// extras-preservation policy)
// ---------------------------------------------------------------------------

/// Extra method-level arguments for `Metadata/get`
/// (draft-ietf-jmap-metadata-01 §3.2).
///
/// Draft-01 defines no method-specific args on `Metadata/get` beyond the
/// RFC 8620 §5.1 standard set; this struct carries only the vendor /
/// site / private-extension `extra` flatten field. Future draft revisions
/// or vendor extensions add typed knobs without a breaking signature change
/// because `metadata_get` accepts `Option<MetadataGetParams>`.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataGetParams {
    /// Catch-all for vendor / site / private extension fields. Preserves
    /// unknown fields across deserialize/serialize round-trip per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    ///
    /// Keys MUST NOT collide with the standard RFC 8620 §5.1 arg names
    /// (`accountId`, `ids`, `properties`); `metadata_get` will silently
    /// retain the typed-field value if an extras key collides.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra method-level arguments for `Metadata/set`
/// (draft-ietf-jmap-metadata-01 §3.1).
///
/// Draft-01 defines no method-specific args on `Metadata/set` beyond the
/// RFC 8620 §5.3 standard set; this struct carries only the vendor /
/// site / private-extension `extra` flatten field. The `if_in_state`
/// argument is passed as a positional parameter to `metadata_set`,
/// mirroring the canonical `email_set` shape.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataSetParams {
    /// Catch-all for vendor / site / private extension fields. Preserves
    /// unknown fields across deserialize/serialize round-trip per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    ///
    /// Keys MUST NOT collide with the standard RFC 8620 §5.3 arg names
    /// (`accountId`, `ifInState`, `create`, `update`, `destroy`);
    /// `metadata_set` will silently retain the typed-field value if an
    /// extras key collides.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra method-level arguments for `Metadata/changes`
/// (draft-ietf-jmap-metadata-01 §3.3).
///
/// Both filter fields are optional. Pass `None` (or `Default::default()`)
/// to fetch all Metadata changes regardless of `relatedType` or `@type`.
///
/// Per §3.3, when both filters are specified the server MUST return only
/// changes to Metadata objects that satisfy both criteria (logical AND).
/// The `state` token returned in the response represents the complete
/// account state and is independent of the filters; clients that use these
/// filters MUST re-use the same filter values across subsequent
/// `Metadata/changes` calls to ensure consistent synchronisation.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataChangesParams {
    /// Restrict the `created`/`updated`/`destroyed` arrays in the response
    /// to Metadata objects whose `relatedType` equals this value
    /// (draft-ietf-jmap-metadata-01 §3.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_related_type: Option<String>,

    /// Restrict the `created`/`updated`/`destroyed` arrays in the response
    /// to Metadata objects whose `@type` is in this list
    /// (draft-ietf-jmap-metadata-01 §3.3).
    ///
    /// `None` disables filtering on `@type` (all types are returned).
    /// `Some(vec![])` is a legal empty array meaning "match no `@type`
    /// values" per draft §3.3 — the server returns empty `created` /
    /// `updated` / `destroyed` arrays regardless of actual changes. If
    /// you mean "return everything", use `None`, not `Some(vec![])`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_metadata_type: Option<Vec<String>>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// Keys MUST NOT collide with the standard RFC 8620 §5.2 / draft-01
    /// §3.3 arg names (`accountId`, `sinceState`, `maxChanges`,
    /// `filterRelatedType`, `filterMetadataType`); `metadata_changes` will
    /// silently retain the typed-field value if an extras key collides.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra method-level arguments for `Metadata/query`
/// (draft-ietf-jmap-metadata-01 §3.4).
///
/// Draft-01 defines no method-specific args on `Metadata/query` beyond the
/// RFC 8620 §5.5 standard set; this struct carries only the vendor /
/// site / private-extension `extra` flatten field.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataQueryParams {
    /// Catch-all for vendor / site / private extension fields. Preserves
    /// unknown fields across deserialize/serialize round-trip per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    ///
    /// Keys MUST NOT collide with the standard RFC 8620 §5.5 arg names
    /// (`accountId`, `filter`, `sort`, `position`, `limit`, etc.);
    /// `metadata_query` will silently retain the typed-field value if an
    /// extras key collides.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Extra method-level arguments for `Metadata/queryChanges`
/// (draft-ietf-jmap-metadata-01 §3.5).
///
/// Draft-01 defines no method-specific args on `Metadata/queryChanges`
/// beyond the RFC 8620 §5.6 standard set; this struct carries only the
/// vendor / site / private-extension `extra` flatten field.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataQueryChangesParams {
    /// Catch-all for vendor / site / private extension fields. Preserves
    /// unknown fields across deserialize/serialize round-trip per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    ///
    /// Keys MUST NOT collide with the standard RFC 8620 §5.6 arg names
    /// (`accountId`, `sinceQueryState`, `maxChanges`, etc.);
    /// `metadata_query_changes` will silently retain the typed-field value
    /// if an extras key collides.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The call-id embedded in every single-method JMAP request produced by
/// [`build_request`]. Pass directly to `jmap_base_client::extract_response`.
pub(crate) const CALL_ID: &str = "r1";

/// Capability URIs for JMAP Metadata method calls
/// (draft-ietf-jmap-metadata-01 §1.2).
pub(crate) const USING_METADATA: &[&str] = &[
    "urn:ietf:params:jmap:core",
    jmap_metadata_types::JMAP_METADATA_URI,
];

// ---------------------------------------------------------------------------
// build_request helper
// ---------------------------------------------------------------------------

/// Build a single-method JMAP request.
///
/// `using` is the complete `using` array for the request (RFC 8620 §3.3).
/// Use the pre-defined constant [`USING_METADATA`] for standard calls.
///
/// The embedded call-id is [`CALL_ID`]; pass it directly to
/// `jmap_base_client::extract_response`.
pub(crate) fn build_request(
    method: &str,
    args: serde_json::Value,
    using: &[&str],
) -> jmap_types::JmapRequest {
    let using_vec: Vec<String> = using.iter().map(|&s| s.to_owned()).collect();
    let invocation: jmap_types::Invocation = (method.to_owned(), args, CALL_ID.to_owned());
    jmap_types::JmapRequest::new(using_vec, vec![invocation], None)
}

// ---------------------------------------------------------------------------
// SessionClient — session-bound client
// ---------------------------------------------------------------------------

/// A `JmapClient` bound to a JMAP session.
///
/// Obtain via [`JmapMetadataExt::with_metadata_session`](crate::JmapMetadataExt::with_metadata_session).
/// All JMAP Metadata methods are available on this type without needing to
/// pass `&Session` on every call.
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
/// already implements `Clone` and `with_metadata_session` clones one
/// internally), enabling parallel-task fan-out with one bound session.
///
/// `Debug` is implemented manually to redact the inner `JmapClient` (which
/// holds an HTTP client and is intentionally not `Debug` in
/// `jmap-base-client`); only the `Session` is shown. This lets callers
/// embed a `SessionClient` in a `#[derive(Debug)]` struct without manual
/// impls of their own.
#[non_exhaustive]
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: jmap_base_client::JmapClient,
    pub(crate) session: jmap_base_client::Session,
}

impl std::fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionClient")
            // The inner JmapClient is not Debug — show a placeholder so
            // callers know it is present without leaking HTTP-client
            // internals.
            .field("client", &"<JmapClient>")
            .field("session", &self.session)
            .finish()
    }
}

impl SessionClient {
    /// Borrow the underlying [`JmapClient`](jmap_base_client::JmapClient).
    ///
    /// Useful for ad-hoc operations outside the typed JMAP method surface —
    /// for example, calling `JmapClient::upload` / `JmapClient::download_blob`,
    /// or constructing a `JmapClient::event_source` subscription using the
    /// bound session's `event_source_url`.
    pub fn client(&self) -> &jmap_base_client::JmapClient {
        &self.client
    }

    /// Borrow the captured [`Session`](jmap_base_client::Session).
    ///
    /// `SessionClient` captures the `Session` at construction time. After
    /// re-fetching the session via `JmapClient::fetch_session`, callers
    /// should construct a new `SessionClient`. This accessor lets a caller
    /// compare the captured session's `state` field against a freshly
    /// fetched session to detect staleness, or inspect
    /// `accountCapabilities` / `primary_accounts` for capability-specific
    /// metadata not exposed via the typed JMAP method surface.
    pub fn session(&self) -> &jmap_base_client::Session {
        &self.session
    }

    /// Return the primary account id for `urn:ietf:params:jmap:metadata`,
    /// or `Err(InvalidSession)` if the session has no primary account for
    /// that capability.
    pub fn metadata_account_id(&self) -> Result<&str, jmap_base_client::ClientError> {
        self.session
            .primary_account_id("urn:ietf:params:jmap:metadata")
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:metadata".into(),
                )
            })
    }

    /// Extract `(api_url, metadata_account_id)` from the bound session.
    ///
    /// Returns `Err(InvalidSession)` if there is no primary account for
    /// `urn:ietf:params:jmap:metadata`.
    pub(crate) fn session_parts(&self) -> Result<(&str, &str), jmap_base_client::ClientError> {
        let api_url = self.session.api_url.as_str();
        let account_id = self
            .session
            .primary_account_id(jmap_metadata_types::JMAP_METADATA_URI)
            .ok_or_else(|| {
                jmap_base_client::ClientError::InvalidSession(
                    "no primary account for urn:ietf:params:jmap:metadata".into(),
                )
            })?;
        Ok((api_url, account_id))
    }

    /// Forward a JMAP request to the underlying HTTP client.
    pub(crate) async fn call_internal(
        &self,
        api_url: &str,
        req: &jmap_types::JmapRequest,
    ) -> Result<jmap_types::JmapResponse, jmap_base_client::ClientError> {
        self.client.call(api_url, req).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Oracle: USING_METADATA contains exactly the two capability URIs from
    /// draft-ietf-jmap-metadata-01 §1.2.
    /// Expected values are taken directly from the draft.
    #[test]
    fn using_metadata_contains_correct_uris() {
        let req = build_request("Metadata/get", json!({}), USING_METADATA);
        let v = serde_json::to_value(&req).expect("serialize");
        let using = v["using"].as_array().expect("using must be array");
        assert_eq!(using.len(), 2, "must have exactly 2 capability URIs");
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:core")),
            "must include jmap:core"
        );
        assert!(
            using.contains(&json!("urn:ietf:params:jmap:metadata")),
            "must include jmap:metadata"
        );
    }

    /// Oracle: build_request produces correct method name and CALL_ID.
    /// Expected: invocation[0] == method, invocation[2] == CALL_ID constant.
    #[test]
    fn build_request_method_name_and_call_id() {
        let req = build_request(
            "Metadata/get",
            json!({"accountId": "acc1", "ids": null}),
            USING_METADATA,
        );
        let v = serde_json::to_value(&req).expect("serialize JmapRequest");

        let calls = v["methodCalls"]
            .as_array()
            .expect("methodCalls must be array");
        assert_eq!(calls.len(), 1, "must have exactly 1 method call");
        assert_eq!(calls[0][0], json!("Metadata/get"), "method name must match");
        assert_eq!(calls[0][2], json!("r1"), "call_id must be CALL_ID constant");
    }

    /// Oracle: session_parts returns None when metadata capability absent.
    #[test]
    fn session_parts_err_no_primary_account() {
        let session_json = json!({
            "capabilities": {},
            "accounts": {},
            "primaryAccounts": {},
            "username": "user@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/dl/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://jmap.example.com/ul/{accountId}/",
            "eventSourceUrl": "https://jmap.example.com/sse/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "s1"
        });
        let session: jmap_base_client::Session =
            serde_json::from_value(session_json).expect("session must deserialize");
        let result = session.primary_account_id("urn:ietf:params:jmap:metadata");
        assert!(
            result.is_none(),
            "must return None when metadata capability is not in primaryAccounts"
        );
    }

    /// Oracle: GetResponse<T> deserializes from RFC 8620 §5.1 shape.
    #[test]
    fn get_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "state": "s42",
            "list": [],
            "notFound": ["missing1"]
        });
        let resp: GetResponse<serde_json::Value> =
            serde_json::from_value(json).expect("GetResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.state, "s42");
        assert!(resp.list.is_empty());
        assert_eq!(
            resp.not_found.as_deref(),
            Some(["missing1".into()].as_slice())
        );
    }

    /// Oracle: ChangesResponse deserializes from RFC 8620 §5.2 shape.
    #[test]
    fn changes_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s10",
            "newState": "s11",
            "hasMoreChanges": false,
            "created": ["id1"],
            "updated": ["id2"],
            "destroyed": []
        });
        let resp: ChangesResponse =
            serde_json::from_value(json).expect("ChangesResponse must deserialize");
        assert_eq!(resp.old_state, "s10");
        assert_eq!(resp.new_state, "s11");
        assert!(!resp.has_more_changes);
        assert_eq!(resp.created.len(), 1);
        assert_eq!(resp.updated.len(), 1);
        assert!(resp.destroyed.is_empty());
    }

    /// Oracle: SetResponse deserializes from RFC 8620 §5.3 shape.
    #[test]
    fn set_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s10",
            "newState": "s11",
            "created": null,
            "updated": null,
            "destroyed": ["id1"],
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        });
        let resp: SetResponse = serde_json::from_value(json).expect("SetResponse must deserialize");
        assert_eq!(resp.new_state, "s11");
        assert_eq!(resp.destroyed.as_deref(), Some(["id1".into()].as_slice()));
    }

    /// Oracle: QueryChangesResponse deserializes from RFC 8620 §5.6 shape.
    #[test]
    fn query_changes_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldQueryState": "qs1",
            "newQueryState": "qs2",
            "total": 5,
            "removed": ["id3"],
            "added": [{"id": "id4", "index": 0}]
        });
        let resp: QueryChangesResponse =
            serde_json::from_value(json).expect("QueryChangesResponse must deserialize");
        assert_eq!(resp.old_query_state, "qs1");
        assert_eq!(resp.new_query_state, "qs2");
        assert_eq!(resp.total, Some(5));
        assert_eq!(resp.removed.len(), 1);
        assert_eq!(resp.added.len(), 1);
        assert_eq!(resp.added[0].index, 0);
    }

    /// Oracle: MetadataChangesParams default (all None) serializes to `{}`.
    /// Expected: skip_serializing_if omits both None fields.
    #[test]
    fn metadata_changes_params_default_is_empty_object() {
        let params = MetadataChangesParams::default();
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            v,
            json!({}),
            "default params must serialize to empty object"
        );
    }

    /// Oracle: MetadataChangesParams serializes filterRelatedType in camelCase.
    /// Expected field name from draft-ietf-jmap-metadata-01 §3.3.
    #[test]
    fn metadata_changes_params_serializes_filter_related_type() {
        let params = MetadataChangesParams {
            filter_related_type: Some("Email".into()),
            filter_metadata_type: None,
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            v["filterRelatedType"],
            json!("Email"),
            "filterRelatedType must serialize in camelCase"
        );
        assert!(
            v.get("filterMetadataType").is_none(),
            "filterMetadataType must be absent when None"
        );
    }

    /// Oracle: MetadataChangesParams serializes filterMetadataType array.
    /// Expected field name from draft-ietf-jmap-metadata-01 §3.3.
    #[test]
    fn metadata_changes_params_serializes_filter_metadata_type() {
        let params = MetadataChangesParams {
            filter_related_type: None,
            filter_metadata_type: Some(vec!["Annotation".into(), "ImapMetadata".into()]),
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&params).expect("serialize");
        assert!(
            v.get("filterRelatedType").is_none(),
            "filterRelatedType must be absent when None"
        );
        assert_eq!(
            v["filterMetadataType"],
            json!(["Annotation", "ImapMetadata"]),
            "filterMetadataType must serialize as array"
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // For Serialize-only method-argument structs, the test constructs a
    // struct with a vendor field in `extra` and asserts that the field
    // flattens into the serialized JSON. Uses synthetic `acmeCorp*` keys
    // that are guaranteed not to appear in any draft-ietf-jmap-metadata
    // typed field — so the tests are independent of the crate under test.

    /// `MetadataChangesParams.extra` flattens into serialized JSON.
    #[test]
    fn metadata_changes_params_propagates_vendor_extras() {
        let mut params = MetadataChangesParams::default();
        params
            .extra
            .insert("acmeCorpCursor".into(), json!("opaque-token"));
        let v = serde_json::to_value(&params).expect("serialize MetadataChangesParams");
        assert_eq!(v["acmeCorpCursor"], json!("opaque-token"));
    }

    /// `MetadataGetParams::default()` serializes to an empty object.
    /// Oracle: `skip_serializing_if = serde_json::Map::is_empty` on the
    /// `extra` flatten field.
    #[test]
    fn metadata_get_params_default_is_empty_object() {
        let v = serde_json::to_value(MetadataGetParams::default())
            .expect("serialize MetadataGetParams");
        assert_eq!(v, json!({}));
    }

    /// `MetadataGetParams.extra` flattens into serialized JSON.
    #[test]
    fn metadata_get_params_propagates_vendor_extras() {
        let mut params = MetadataGetParams::default();
        params.extra.insert("acmeCorpAuditFlag".into(), json!(true));
        let v = serde_json::to_value(&params).expect("serialize MetadataGetParams");
        assert_eq!(v["acmeCorpAuditFlag"], json!(true));
    }

    /// `MetadataSetParams::default()` serializes to an empty object.
    #[test]
    fn metadata_set_params_default_is_empty_object() {
        let v = serde_json::to_value(MetadataSetParams::default())
            .expect("serialize MetadataSetParams");
        assert_eq!(v, json!({}));
    }

    /// `MetadataSetParams.extra` flattens into serialized JSON.
    #[test]
    fn metadata_set_params_propagates_vendor_extras() {
        let mut params = MetadataSetParams::default();
        params.extra.insert("acmeCorpAuditFlag".into(), json!(true));
        let v = serde_json::to_value(&params).expect("serialize MetadataSetParams");
        assert_eq!(v["acmeCorpAuditFlag"], json!(true));
    }

    /// `MetadataQueryParams::default()` serializes to an empty object.
    #[test]
    fn metadata_query_params_default_is_empty_object() {
        let v = serde_json::to_value(MetadataQueryParams::default())
            .expect("serialize MetadataQueryParams");
        assert_eq!(v, json!({}));
    }

    /// `MetadataQueryParams.extra` flattens into serialized JSON.
    #[test]
    fn metadata_query_params_propagates_vendor_extras() {
        let mut params = MetadataQueryParams::default();
        params
            .extra
            .insert("acmeCorpAnchor".into(), json!("MD-cursor-1"));
        let v = serde_json::to_value(&params).expect("serialize MetadataQueryParams");
        assert_eq!(v["acmeCorpAnchor"], json!("MD-cursor-1"));
    }

    /// `MetadataQueryChangesParams::default()` serializes to an empty object.
    #[test]
    fn metadata_query_changes_params_default_is_empty_object() {
        let v = serde_json::to_value(MetadataQueryChangesParams::default())
            .expect("serialize MetadataQueryChangesParams");
        assert_eq!(v, json!({}));
    }

    /// `MetadataQueryChangesParams.extra` flattens into serialized JSON.
    #[test]
    fn metadata_query_changes_params_propagates_vendor_extras() {
        let mut params = MetadataQueryChangesParams::default();
        params.extra.insert("acmeCorpUpTo".into(), json!("MD-99"));
        let v = serde_json::to_value(&params).expect("serialize MetadataQueryChangesParams");
        assert_eq!(v["acmeCorpUpTo"], json!("MD-99"));
    }
}
