//! JMAP Metadata — Metadata/* method implementations on SessionClient
//! (draft-ietf-jmap-metadata-01 §3).
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (defence-in-depth empty-state guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_METADATA)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::HashMap;

use jmap_types::{Id, PatchObject, State};

use super::{
    ChangesResponse, GetResponse, MetadataChangesParams, QueryChangesResponse, QueryResponse,
    SetResponse,
};

impl super::SessionClient {
    /// Fetch Metadata objects by IDs (draft-ietf-jmap-metadata-01 §3.2).
    ///
    /// If `ids` is `None`, the server returns all Metadata objects for the
    /// account in one response. Pass `properties: None` to return all
    /// fields.
    pub async fn metadata_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<jmap_metadata_types::Metadata>, jmap_base_client::ClientError> {
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
            args["properties"] = serde_json::Value::Array(
                props.iter().copied().map(serde_json::Value::from).collect(),
            );
        }
        let req = super::build_request("Metadata/get", args, super::USING_METADATA);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to Metadata objects since `since_state`
    /// (draft-ietf-jmap-metadata-01 §3.3).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until the flag is false.
    ///
    /// `params` carries the Metadata-specific optional filters
    /// `filterRelatedType` and `filterMetadataType`. Pass `None` (or
    /// `Some(Default::default())`) to fetch unfiltered changes. Per §3.3,
    /// clients that use these filters MUST re-use the same filter values
    /// across subsequent calls to ensure consistent synchronisation,
    /// because the returned state token represents the complete account
    /// state and is independent of the filter selection.
    pub async fn metadata_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
        params: Option<MetadataChangesParams>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: even with the typed-`State` parameter (a transparent
        // newtype around `String`), an empty state token is still a logically
        // invalid value that should be caught client-side rather than producing
        // a confusing server-side `cannotCalculateChanges` error.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "metadata_changes: since_state may not be empty".into(),
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
        if let Some(p) = params {
            if let Some(v) = p.filter_related_type {
                args["filterRelatedType"] = v.into();
            }
            if let Some(v) = p.filter_metadata_type {
                args["filterMetadataType"] =
                    serde_json::to_value(&v).expect("Vec<String> Serialize is infallible");
            }
            // Vendor extras: flatten any caller-supplied keys directly into
            // the args object. Note that an extras key colliding with a
            // typed-field wire name (`filterRelatedType`, `filterMetadataType`,
            // `accountId`, `sinceState`, `maxChanges`) is the caller's
            // responsibility to avoid; we do NOT overwrite typed fields here.
            for (k, v) in p.extra {
                args.as_object_mut()
                    .expect("args is constructed as an Object")
                    .entry(k)
                    .or_insert(v);
            }
        }
        let req = super::build_request("Metadata/changes", args, super::USING_METADATA);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy Metadata objects
    /// (draft-ietf-jmap-metadata-01 §3.1).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    ///
    /// `update` is `Option<HashMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    ///
    /// The server enforces uniqueness on the tuple
    /// `(relatedType, relatedId, @type, isPrivate)` and may return
    /// `alreadyExists`, `forbidden` (if `maySetPrivate: false` and
    /// `isPrivate: true` is requested), `overQuota`, or
    /// `invalidProperties` SetErrors per §3.1.
    pub async fn metadata_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<HashMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
    ) -> Result<SetResponse<jmap_metadata_types::Metadata>, jmap_base_client::ClientError> {
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
                    "metadata_set: serializing update map failed: {e}"
                ))
            })?;
        }
        if let Some(d) = destroy {
            args["destroy"] = serde_json::to_value(&d).expect("Id Vec Serialize is infallible");
        }
        let req = super::build_request("Metadata/set", args, super::USING_METADATA);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query Metadata IDs with optional filter and sort
    /// (draft-ietf-jmap-metadata-01 §3.4).
    ///
    /// Pass `filter: None` and `sort: None` to return all Metadata IDs with
    /// server-default ordering. Use `position` and `limit` for pagination.
    ///
    /// The `filter` value is typed by the caller; the canonical type is
    /// [`jmap_metadata_types::MetadataFilterCondition`] (§3.4.1) wrapped in
    /// `jmap_types::query::Filter`. This builder accepts a `serde_json::Value`
    /// so callers can mix filter conditions with `FilterOperator` algebra
    /// without forcing a single type through the API.
    ///
    /// Sort comparators MUST set `property` to one of `id`, `@type`,
    /// `relatedType`, `relatedId`, or `isPrivate` per §3.4.2; `id` MUST be
    /// supported by every server, the others SHOULD be supported.
    pub async fn metadata_query(
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
        let req = super::build_request("Metadata/query", args, super::USING_METADATA);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for Metadata since `since_query_state`
    /// (draft-ietf-jmap-metadata-01 §3.5).
    ///
    /// Returns which Metadata IDs were removed from or added to the query
    /// result set since the given state. `max_changes` may be `None`.
    pub async fn metadata_query_changes(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see `metadata_changes`.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "metadata_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        let req = super::build_request("Metadata/queryChanges", args, super::USING_METADATA);
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

    // Production-path coverage lives in tests/metadata_tests.rs
    // (wiremock-backed end-to-end). The tests here exercise the typed
    // wire-shape oracles for response types using JSON literals taken
    // from draft-ietf-jmap-metadata-01 and RFC 8620.

    /// Oracle: Annotation deserialization from draft-ietf-jmap-metadata-01
    /// §1.6.1 (example 1). Expected JSON taken verbatim from the draft.
    #[test]
    fn annotation_deserializes_from_spec_example() {
        let json = json!({
            "@type": "Annotation",
            "id": "MD789",
            "relatedType": "Email",
            "relatedId": "EM456",
            "isPrivate": true,
            "acme.example.com:workflowState": "pending-review"
        });
        let meta: jmap_metadata_types::Metadata =
            serde_json::from_value(json).expect("Annotation must deserialize");
        match meta {
            jmap_metadata_types::Metadata::Annotation(a) => {
                assert_eq!(a.related_type, "Email");
                assert_eq!(a.related_id.as_ref(), "EM456");
                assert_eq!(a.is_private, Some(true));
                assert_eq!(
                    a.extra
                        .get("acme.example.com:workflowState")
                        .and_then(|v| v.as_str()),
                    Some("pending-review")
                );
            }
            _ => panic!("expected Annotation variant"),
        }
    }

    /// Oracle: GetResponse<Metadata> deserializes from RFC 8620 §5.1 shape.
    #[test]
    fn get_response_metadata_deserializes() {
        use super::super::GetResponse;

        let json = json!({
            "accountId": "acc1",
            "state": "s42",
            "list": [
                {
                    "@type": "Annotation",
                    "id": "MD1",
                    "relatedType": "Mailbox",
                    "relatedId": "MB1",
                    "isPrivate": false,
                    "acme.example.com:color": "blue"
                }
            ],
            "notFound": []
        });
        let resp: GetResponse<jmap_metadata_types::Metadata> =
            serde_json::from_value(json).expect("GetResponse<Metadata> must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.state, "s42");
        assert_eq!(resp.list.len(), 1);
        assert_eq!(resp.list[0].related_type(), "Mailbox");
    }

    /// Oracle: SetResponse<Metadata> deserializes with a created entry.
    #[test]
    fn set_response_metadata_with_created_deserializes() {
        use super::super::SetResponse;

        let json = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "created": {
                "newMeta": {
                    "@type": "Annotation",
                    "id": "server-md-id",
                    "relatedType": "Email",
                    "relatedId": "EM1",
                    "isPrivate": false,
                    "acme.example.com:tag": "important"
                }
            },
            "updated": null,
            "destroyed": null,
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        });
        let resp: SetResponse<jmap_metadata_types::Metadata> =
            serde_json::from_value(json).expect("SetResponse<Metadata> must deserialize");
        assert_eq!(resp.new_state, "s2");
        let created = resp.created.expect("created must be present");
        assert!(
            created.contains_key("newMeta"),
            "created must contain 'newMeta' key"
        );
        let meta = &created["newMeta"];
        assert_eq!(meta.id().map(|id| id.as_ref()), Some("server-md-id"));
        assert_eq!(meta.related_type(), "Email");
    }
}
