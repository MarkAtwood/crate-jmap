//! JMAP FileNode — FileNode/* method implementations on SessionClient.
//!
//! Each method follows the standard five-step pattern:
//!   1. Validate arguments (empty-string guards).
//!   2. Call `self.session_parts()?` → `(api_url, account_id)`.
//!   3. Build args JSON with `serde_json::json!({…})`.
//!   4. Call `build_request(method_name, args, USING_FILENODE)`.
//!   5. Call `self.call_internal(api_url, &req).await?`.
//!   6. Call `jmap_base_client::extract_response(&resp, CALL_ID)?`.

use std::collections::BTreeMap;

use super::{
    ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse, SetResponse,
};
use jmap_filenode_types::FileNodeComparator;
use jmap_types::{Filter, Id, PatchObject, State};
use serde::Serialize;

// ---------------------------------------------------------------------------
// FileNode-specific input types
// ---------------------------------------------------------------------------

/// Parameters for FileNode/get requests
/// (draft-ietf-jmap-filenode-16 §3.2.1).
///
/// Carries the extension-specific request arguments beyond the standard
/// RFC 8620 §5.1 `accountId` / `ids` / `properties` set, which are passed
/// as positional arguments to [`SessionClient::filenode_get`].
///
/// The current draft (revision 16) defines a single extension argument,
/// `fetchParents`. Future draft revisions may add more (e.g. a depth or
/// path-resolution control); housing the args in a `#[non_exhaustive]`
/// struct lets the workspace add fields without a breaking change to the
/// `filenode_get` signature.
///
/// [`SessionClient::filenode_get`]: super::SessionClient::filenode_get
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeGetParams {
    /// If `Some(true)`, the server returns all ancestor nodes of the
    /// requested ids in addition to the requested nodes themselves
    /// (draft-ietf-jmap-filenode-16 §3.2.1). Server default when absent
    /// is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_parents: Option<bool>,

    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    ///
    /// **Constraint**: keys in `extra` MUST NOT collide with the standard
    /// RFC 8620 §5.1 arg names (`accountId`, `ids`, `properties`) or with
    /// the typed-field wire names above (`fetchParents`); `filenode_get`
    /// will silently retain the typed-field value if an extras key
    /// collides. Place vendor extensions under vendor-prefixed keys (e.g.
    /// `"acmeCorpFoo"`) to avoid the collision class.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The action to take when a FileNode with the same name already exists
/// (draft-ietf-jmap-filenode-16 §3.2.3 `onExists` argument).
///
/// The `onExists` value set has grown across draft revisions (`"newest"`
/// in -14, `"unchanged"` in -16), so this enum is `#[non_exhaustive]` and
/// a `match` outside this crate needs a wildcard arm:
///
/// ```compile_fail,E0004
/// use jmap_filenode_client::FileNodeOnExists;
/// fn f(o: FileNodeOnExists) -> u8 {
///     match o {
///         FileNodeOnExists::Replace => 0,
///         FileNodeOnExists::Rename => 1,
///         FileNodeOnExists::Newest => 2,
///         FileNodeOnExists::Unchanged => 3,
///     }
/// }
/// ```
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum FileNodeOnExists {
    /// Replace the existing node with the new one.
    Replace,
    /// Rename the new node to avoid the collision.
    Rename,
    /// "If the incoming item has a strictly later modified value, the
    /// existing node is destroyed and the operation proceeds as with
    /// "replace". Otherwise the operation is rejected as with null,
    /// returning an "alreadyExists" error with the existingId of the
    /// surviving node."
    Newest,
    /// Replace the colliding node only if the precondition the client
    /// supplies in the `ifUnchangedBy` argument (JMAP-CONDITIONAL) for that
    /// node's id is satisfied; otherwise the server rejects as with null.
    /// Pass `ifUnchangedBy` through the params' `extra` map. "A server that
    /// does not support [JMAP-CONDITIONAL] MUST reject "unchanged" with an
    /// "invalidArguments" error."
    Unchanged,
}

/// Parameters for FileNode/set requests
/// (draft-ietf-jmap-filenode-16 §3.2.3).
///
/// All fields are optional; omitted fields take server defaults.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeSetParams {
    /// If true, destroying a directory also destroys all its descendants.
    /// Server default: false (destroy fails if children exist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_children: Option<bool>,
    /// How to handle a name collision with an existing sibling node.
    /// `None` means the server's default policy applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_exists: Option<FileNodeOnExists>,
    /// If true, name comparisons are case-insensitive when checking for
    /// sibling name collisions. Server default: false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare_case_insensitively: Option<bool>,

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

/// Parameters for FileNode/copy requests
/// (draft-ietf-jmap-filenode-16 §3.2.4).
///
/// `from_account_id` is the source account; the destination account is the
/// session's primary FileNode account. `on_destroy_remove_children`,
/// `on_exists`, and `compare_case_insensitively` mirror the like-named
/// arguments on `FileNode/set` (§3.2.3) and control cascade-destroy and
/// name-collision behaviour at the destination. §3.2.4 names only
/// `onDestroyRemoveChildren` and `onExists` as shared with `FileNode/set`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNodeCopyParams {
    /// The account that is the source of the copy operation.
    pub from_account_id: Id,

    /// If true, destroying a directory also destroys all its descendants.
    /// Server default: false (destroy fails if children exist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_destroy_remove_children: Option<bool>,

    /// How to handle a name collision with an existing sibling node at the
    /// destination. `None` means the server's default policy applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_exists: Option<FileNodeOnExists>,

    /// If true, name comparisons are case-insensitive when checking for
    /// sibling name collisions at the destination. Server default: false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare_case_insensitively: Option<bool>,

    /// State guard for the source account: the server aborts with
    /// `stateMismatch` unless the source state matches (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_from_in_state: Option<jmap_types::State>,
    /// State guard for the destination account: the server aborts with
    /// `stateMismatch` unless the destination state matches (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_in_state: Option<jmap_types::State>,
    /// If `true`, destroy originals after successful copy (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success_destroy_original: Option<bool>,
    /// If-in-state guard for the source account destroy step (RFC 8620 §5.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destroy_from_if_in_state: Option<jmap_types::State>,

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

// ---------------------------------------------------------------------------
// Method implementations
// ---------------------------------------------------------------------------

impl super::SessionClient {
    /// Fetch FileNode objects by IDs (draft-ietf-jmap-filenode-16 §3.2.1).
    ///
    /// If `ids` is `None`, the server returns all FileNodes for the account,
    /// SUBJECT TO the server's `maxObjectsInGet` cap (RFC 8620 §5.1).
    /// For production use, scope the result set via the corresponding
    /// /query method first and pass explicit ids here to avoid
    /// `requestTooLarge` errors when the account holds more objects
    /// than the cap.
    /// Pass `properties: None` to return all fields.
    /// Use `params` to set FileNode-specific top-level arguments such as
    /// `fetchParents` (draft-ietf-jmap-filenode-16 §3.2.1) and vendor
    /// extensions via the [`FileNodeGetParams::extra`] flatten field. Pass
    /// `params: None` for the spec-default behavior.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
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
    pub async fn filenode_get(
        &self,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
        params: Option<FileNodeGetParams>,
    ) -> Result<GetResponse<jmap_filenode_types::FileNode>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        // Omit `ids` / `properties` entirely when None rather than sending
        // an explicit JSON null. RFC 8620 §5.1 accepts both shapes, but the
        // crate's other builders (set/changes/query) consistently use the
        // conditional-add idiom; matching it here keeps the wire request
        // canonical and avoids "present-but-null vs absent" interop quirks
        // in proxies / audit loggers.
        let mut args = serde_json::json!({ "accountId": account_id });
        if let Some(id_slice) = ids {
            args["ids"] = jmap_base_client::to_args(id_slice)?;
        }
        if let Some(props) = properties {
            args["properties"] = jmap_base_client::to_args(props)?;
        }
        if let Some(mut p) = params {
            // Take `extra` out before serializing `p`: a flattened map
            // overwrites `p`'s own typed fields inside `to_value`. It is
            // merged last below so typed values win on collision.
            let extra = std::mem::take(&mut p.extra);
            let pv = jmap_base_client::to_args(&p)?;
            if let serde_json::Value::Object(map) = pv {
                // Use `entry().or_insert()` so a caller who put a typed
                // wire key (e.g. "accountId", "ids", "properties",
                // "fetchParents") into `params.extra` cannot silently
                // clobber the value computed from the bound session and
                // the typed args. Typed wins on collision.
                let args_obj = args
                    .as_object_mut()
                    .expect("filenode_get: args is constructed as Object");
                for (k, v) in map.into_iter().chain(extra) {
                    args_obj.entry(k).or_insert(v);
                }
            }
        }
        let req = super::build_request("FileNode/get", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch changes to FileNode objects since `since_state`
    /// (draft-ietf-jmap-filenode-16 §3.2.2).
    ///
    /// If `has_more_changes` is true in the response, call again with
    /// `new_state` as `since_state` until the flag is false.
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
    ///   `urn:ietf:params:jmap:filenode`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::filenode_get`].
    pub async fn filenode_changes(
        &self,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: an empty State token has no meaning on the wire
        // even though State is a typed newtype.
        if since_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "filenode_changes: since_state may not be empty".into(),
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
        let req = super::build_request("FileNode/changes", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Create, update, or destroy FileNode objects
    /// (draft-ietf-jmap-filenode-16 §3.2.3).
    ///
    /// Pass `create`, `update`, and/or `destroy` as needed. All three are
    /// optional; pass `None` to omit any operation from the request.
    ///
    /// `update` is `Option<BTreeMap<Id, PatchObject>>` (RFC 8620 §5.3). Wire
    /// format is unchanged from a plain JSON object because [`PatchObject`]
    /// is `#[serde(transparent)]`; the typed parameter binds the JSON Pointer
    /// key + null-leaf removal contract to the type system.
    ///
    /// Use `params` to set FileNode-specific top-level arguments
    /// (`onDestroyRemoveChildren`, `onExists`, `compareCaseInsensitively`).
    ///
    /// Pass `if_in_state: Some(&state)` to use an optimistic-concurrency guard
    /// (RFC 8620 §5.3 `ifInState`): the server rejects the whole call with
    /// `stateMismatch` unless its current state matches.
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::filenode_get`].
    pub async fn filenode_set(
        &self,
        create: Option<serde_json::Value>,
        update: Option<BTreeMap<Id, PatchObject>>,
        destroy: Option<Vec<Id>>,
        if_in_state: Option<&State>,
        params: Option<FileNodeSetParams>,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        if create.is_none() && update.is_none() && destroy.is_none() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "filenode_set: at least one of create, update, destroy must be Some \
                 (an all-None /set is a no-op round-trip)"
                    .into(),
            ));
        }
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
            args["update"] = jmap_base_client::to_args(&u)?;
        }
        if let Some(d) = destroy {
            args["destroy"] = jmap_base_client::to_args(&d)?;
        }
        if let Some(mut p) = params {
            // Take `extra` out before serializing `p`: a flattened map
            // overwrites `p`'s own typed fields inside `to_value`. It is
            // merged last below so typed values win on collision.
            let extra = std::mem::take(&mut p.extra);
            let params_val = jmap_base_client::to_args(&p)?;
            if let serde_json::Value::Object(map) = params_val {
                // Use `entry().or_insert()` so a caller who put a typed
                // wire key (e.g. "accountId", "create", "update",
                // "destroy", "ifInState") into `params.extra` cannot
                // silently clobber the typed args. Typed wins on collision.
                let args_obj = args
                    .as_object_mut()
                    .expect("filenode_set: args is constructed as Object");
                for (k, v) in map.into_iter().chain(extra) {
                    args_obj.entry(k).or_insert(v);
                }
            }
        }
        let req = super::build_request("FileNode/set", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Copy FileNode objects from `params.from_account_id` into this account
    /// (draft-ietf-jmap-filenode-16 §3.2.4 — FileNode/copy).
    ///
    /// `params` carries `fromAccountId` plus the optional destination-side
    /// arguments shared with `FileNode/set`:
    /// - `on_destroy_remove_children`: when `true`, any children of a
    ///   destroyed source node are also removed (§3.2.4, §3.2.3).
    /// - `on_exists`: collision policy at the destination.
    /// - `compare_case_insensitively`: case-folding for name collisions.
    /// - `if_from_in_state` / `if_in_state`: source / destination state
    ///   guards (RFC 8620 §5.4).
    /// - `on_success_destroy_original` / `destroy_from_if_in_state`:
    ///   destroy the originals after a successful copy, optionally
    ///   state-guarded (RFC 8620 §5.4).
    ///
    /// `create` is the RFC 8620 §5.4 `create` map (a JSON object mapping
    /// creation keys to copy-creation objects).
    ///
    /// The response's [`SetResponse::destroyed`] lists destination nodes
    /// that onExists replacement destroyed, cascades included; it is
    /// `None` when the copy destroyed nothing.
    ///
    /// This returns a [`SetResponse`] rather than the RFC 8620 §5.4
    /// [`CopyResponse`](jmap_types::CopyResponse) because
    /// draft-ietf-jmap-filenode-16 §3.2.4 gives FileNode/copy the
    /// FileNode/set `onExists` behaviour, under which "the server MUST
    /// include the id of the replaced item in the destroyed response
    /// list" (§3.2.3); `CopyResponse` has no `destroyed`. The response's
    /// `fromAccountId` lands in [`SetResponse::extra`]. Each `created`
    /// value is a [`PartialObject`] holding the server-set properties.
    ///
    /// Use [`FileNodeCopyParams::extra`] to plumb vendor / site extension
    /// fields through to the wire (workspace extras-preservation policy).
    ///
    /// # Errors
    ///
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::filenode_get`]. RFC 8620
    ///   §5.4 /copy adds method-level errors `fromAccountNotFound`,
    ///   `fromAccountNotSupportedByMethod`, and `anchorNotFound`; they
    ///   surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn filenode_copy(
        &self,
        params: FileNodeCopyParams,
        create: serde_json::Value,
    ) -> Result<SetResponse<PartialObject>, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "fromAccountId": params.from_account_id,
            "accountId": account_id,
            "create": create,
        });
        if let Some(odrc) = params.on_destroy_remove_children {
            args["onDestroyRemoveChildren"] = odrc.into();
        }
        if let Some(oe) = params.on_exists {
            args["onExists"] = jmap_base_client::to_args(&oe)?;
        }
        if let Some(cci) = params.compare_case_insensitively {
            args["compareCaseInsensitively"] = cci.into();
        }
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
                .expect("filenode_copy: args is constructed as Object");
            for (k, v) in params.extra {
                args_obj.entry(k).or_insert(v);
            }
        }
        let req = super::build_request("FileNode/copy", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Query FileNode IDs with optional filter, sort, and pagination
    /// (draft-ietf-jmap-filenode-16 §3.2.5).
    ///
    /// The `depth` parameter controls recursive descent: `None` or `Some(0)`
    /// means no recursion; `Some(n)` recurses `n` levels into subdirectories.
    ///
    /// `filter` is an RFC 8620 §5.5 `Filter`: a bare condition
    /// (`Filter::Condition`) or a `FilterOperator` combining sub-filters
    /// with `AND` / `OR` / `NOT`. The condition type `C` is normally
    /// [`FileNodeFilterCondition`](jmap_filenode_types::FileNodeFilterCondition); use
    /// `jmap_metadata_types::WithMetadata<FileNodeFilterCondition>` to add the
    /// draft-ietf-jmap-metadata-04 §3.6 `metadata*` conditions.
    /// Pass `None` to omit it; name the type when you do (`None::<&FileNodeFilter>`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::filenode_get`]. RFC 8620
    ///   §5.5 defines additional /query method-level errors
    ///   (`anchorNotFound`, `unsupportedFilter`, `unsupportedSort`,
    ///   `tooManyChanges`) that surface as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn filenode_query<C: Serialize>(
        &self,
        filter: Option<&Filter<C>>,
        sort: Option<&[FileNodeComparator]>,
        params: &jmap_base_client::QueryParams,
        depth: Option<u64>,
    ) -> Result<QueryResponse, jmap_base_client::ClientError> {
        let (api_url, account_id) = self.session_parts()?;
        let mut args = params.to_arguments(account_id);
        if let Some(f) = filter {
            args["filter"] = jmap_base_client::to_args(f)?;
        }
        if let Some(s) = sort {
            args["sort"] = jmap_base_client::to_args(s)?;
        }
        if let Some(d) = depth {
            args["depth"] = d.into();
        }
        let req = super::build_request("FileNode/query", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }

    /// Fetch query-result changes for FileNode since `since_query_state`
    /// (draft-ietf-jmap-filenode-16 §3.2.6).
    ///
    /// Returns which FileNode IDs were removed from or added to the query
    /// result set since the given state. `max_changes` may be `None`.
    ///
    /// `filter` and `sort` MUST match the `filter` / `sort` passed to the
    /// original `FileNode/query` call that returned `since_query_state` —
    /// RFC 8620 §5.6 is explicit that the server uses them to compute
    /// which entries entered or left the result set.
    ///
    /// `up_to_id` is the highest-index id the client has cached;
    /// `calculate_total` requests the new total result count.
    ///
    /// `filter` is an RFC 8620 §5.5 `Filter`: a bare condition
    /// (`Filter::Condition`) or a `FilterOperator` combining sub-filters
    /// with `AND` / `OR` / `NOT`. The condition type `C` is normally
    /// [`FileNodeFilterCondition`](jmap_filenode_types::FileNodeFilterCondition); use
    /// `jmap_metadata_types::WithMetadata<FileNodeFilterCondition>` to add the
    /// draft-ietf-jmap-metadata-04 §3.6 `metadata*` conditions.
    /// Pass `None` to omit it; name the type when you do (`None::<&FileNodeFilter>`).
    ///
    /// # Errors
    ///
    /// - [`ClientError::Serialize`](jmap_base_client::ClientError::Serialize)
    ///   if serializing a caller-supplied value fails.
    /// - [`ClientError::InvalidArgument`](jmap_base_client::ClientError::InvalidArgument)
    ///   if `since_query_state` is the empty string (defence-in-depth
    ///   empty-state guard; see [`Self::filenode_changes`]).
    /// - [`ClientError::InvalidSession`](jmap_base_client::ClientError::InvalidSession)
    ///   if the bound session has no primary account for
    ///   `urn:ietf:params:jmap:filenode`.
    /// - Any transport / protocol variant returned by
    ///   [`JmapClient::call`](jmap_base_client::JmapClient::call) — see
    ///   the matching error list on [`Self::filenode_get`]. RFC 8620
    ///   §5.6 also defines `cannotCalculateChanges` (returned when the
    ///   server cannot honour the request given the supplied filter /
    ///   sort); it surfaces as
    ///   [`MethodError`](jmap_base_client::ClientError::MethodError).
    pub async fn filenode_query_changes<C: Serialize>(
        &self,
        since_query_state: &State,
        max_changes: Option<u64>,
        filter: Option<&Filter<C>>,
        sort: Option<&[FileNodeComparator]>,
        up_to_id: Option<&Id>,
        calculate_total: Option<bool>,
    ) -> Result<QueryChangesResponse, jmap_base_client::ClientError> {
        // Defence-in-depth: see filenode_changes.
        if since_query_state.as_ref().is_empty() {
            return Err(jmap_base_client::ClientError::InvalidArgument(
                "filenode_query_changes: since_query_state may not be empty".into(),
            ));
        }
        let (api_url, account_id) = self.session_parts()?;
        let mut args = serde_json::json!({
            "accountId": account_id,
            "sinceQueryState": since_query_state,
        });
        if let Some(f) = filter {
            args["filter"] = jmap_base_client::to_args(f)?;
        }
        if let Some(s) = sort {
            args["sort"] = jmap_base_client::to_args(s)?;
        }
        if let Some(mc) = max_changes {
            args["maxChanges"] = mc.into();
        }
        if let Some(uti) = up_to_id {
            args["upToId"] = jmap_base_client::to_args(uti)?;
        }
        if let Some(ct) = calculate_total {
            args["calculateTotal"] = ct.into();
        }
        let req = super::build_request("FileNode/queryChanges", args, super::USING_FILENODE);
        let resp = self.call_internal(api_url, &req).await?;
        jmap_base_client::extract_response(&resp, super::CALL_ID)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests — see tests/integration_get_query.rs and tests/integration_set_copy.rs
// (wiremock-backed end-to-end)
// ---------------------------------------------------------------------------
//
// `filenode_get_request_shape`, `filenode_query_with_depth_includes_depth_in_args`,
// `filenode_query_without_depth_omits_depth`,
// `filenode_changes_request_includes_since_state`,
// `filenode_set_destroy_request_shape`,
// `filenode_copy_request_includes_from_account_id`,
// `filenode_copy_with_on_destroy_remove_children_includes_key`, and
// `filenode_copy_without_on_destroy_remove_children_omits_key` were vacuous:
// they hand-built `args` Values and fed them to `build_request`, never
// exercising the production `filenode_get`, `filenode_query`,
// `filenode_changes`, `filenode_set`, or `filenode_copy` builders.
// Deleted in JMAP-tco1.25.
//
// Real production-path coverage:
//   - tests/integration_get_query.rs (wiremock-backed FileNode/get and
//     FileNode/query)
//   - tests/integration_set_copy.rs (wiremock-backed FileNode/set and
//     FileNode/copy)
//
// Specific-flag passthrough coverage that may be lost is tracked
// under JMAP-uuoi for follow-up wiremock smoke tests.
//
// `build_request`, `CALL_ID`, and `USING_FILENODE` themselves have their
// own focused tests in `methods/mod.rs`.

#[cfg(test)]
mod tests {
    use super::super::{QueryResponse, SetResponse};
    use super::{FileNodeCopyParams, FileNodeOnExists, FileNodeSetParams};
    use jmap_types::Id;
    use serde_json::json;

    // ── Empty-string guards ─────────────────────────────────────────────────

    // The `filenode_get_empty_id_guard` inline smoke test was removed by the
    // JMAP-6by7.6 typed-Id refactor. It was vacuous because it only iterated
    // a local `&[""]` slice and asserted `is_empty()` found the empty value,
    // without invoking any production method. Under typed `&[Id]` parameters,
    // an empty-Id input is impossible to express through the API
    // (`Id::new_validated("")` returns `Err` at the call site) so the bug it
    // pretended to test is unrepresentable.

    // The InvalidArgument guards for empty since_state, since_query_state,
    // destroy IDs, and from_account_id live in the FileNode production code;
    // testing them requires a wiremock-backed async harness.
    // See JMAP-sc1b.64.

    // ── Serialization oracles ───────────────────────────────────────────────

    /// Oracle: FileNodeOnExists::Replace serializes to "replace".
    /// Expected value taken directly from draft-ietf-jmap-filenode-16 §3.2.3.
    #[test]
    fn file_node_on_exists_replace_serializes() {
        let val = serde_json::to_value(FileNodeOnExists::Replace).expect("serialize");
        assert_eq!(
            val,
            json!("replace"),
            "Replace must serialize to \"replace\""
        );
    }

    /// Oracle: FileNodeOnExists::Rename serializes to "rename".
    /// Expected value taken directly from draft-ietf-jmap-filenode-16 §3.2.3.
    #[test]
    fn file_node_on_exists_rename_serializes() {
        let val = serde_json::to_value(FileNodeOnExists::Rename).expect("serialize");
        assert_eq!(val, json!("rename"), "Rename must serialize to \"rename\"");
    }

    /// Oracle: FileNodeOnExists::Newest serializes to "newest".
    /// Expected value taken directly from draft-ietf-jmap-filenode-16 §3.2.3.
    #[test]
    fn file_node_on_exists_newest_serializes() {
        let val = serde_json::to_value(FileNodeOnExists::Newest).expect("serialize");
        assert_eq!(val, json!("newest"), "Newest must serialize to \"newest\"");
    }

    /// Oracle: FileNodeOnExists::Unchanged serializes to "unchanged".
    /// Expected value taken directly from draft-ietf-jmap-filenode-16 §3.2.3.
    #[test]
    fn file_node_on_exists_unchanged_serializes() {
        let val = serde_json::to_value(FileNodeOnExists::Unchanged).expect("serialize");
        assert_eq!(
            val,
            json!("unchanged"),
            "Unchanged must serialize to \"unchanged\""
        );
    }

    /// Oracle: FileNodeSetParams with onDestroyRemoveChildren=true serializes correctly.
    /// Expected field name "onDestroyRemoveChildren" from draft-ietf-jmap-filenode-16 §3.2.3.
    #[test]
    fn filenode_set_params_on_destroy_serializes() {
        let params = FileNodeSetParams {
            on_destroy_remove_children: Some(true),
            on_exists: None,
            compare_case_insensitively: None,
            extra: serde_json::Map::new(),
        };
        let val = serde_json::to_value(&params).expect("serialize");
        assert_eq!(
            val["onDestroyRemoveChildren"],
            json!(true),
            "onDestroyRemoveChildren must be true"
        );
        assert!(
            val.get("onExists").is_none(),
            "onExists must be absent when None"
        );
    }

    /// Oracle: FileNodeSetParams with all fields set serializes all three.
    #[test]
    fn filenode_set_params_all_fields_serializes() {
        let params = FileNodeSetParams {
            on_destroy_remove_children: Some(false),
            on_exists: Some(FileNodeOnExists::Replace),
            compare_case_insensitively: Some(true),
            extra: serde_json::Map::new(),
        };
        let val = serde_json::to_value(&params).expect("serialize");
        assert_eq!(val["onDestroyRemoveChildren"], json!(false));
        assert_eq!(val["onExists"], json!("replace"));
        assert_eq!(val["compareCaseInsensitively"], json!(true));
    }

    // ── Response deserialization oracles ────────────────────────────────────

    /// Oracle: QueryResponse deserializes from RFC 8620 §5.5 shape.
    /// JSON shape from RFC 8620 §5.5, not from the code.
    #[test]
    fn query_response_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "queryState": "qs1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": ["id1", "id2"],
            "total": 2,
            "limit": 256
        });
        let resp: QueryResponse =
            serde_json::from_value(json).expect("QueryResponse must deserialize");
        assert_eq!(resp.account_id, "acc1");
        assert_eq!(resp.query_state, "qs1");
        assert!(resp.can_calculate_changes);
        assert_eq!(resp.position, 0);
        assert_eq!(resp.ids.len(), 2);
        assert_eq!(resp.total, Some(2));
        assert_eq!(resp.limit, Some(256));
    }

    /// Oracle: SetResponse<FileNode> deserializes from RFC 8620 §5.3 shape
    /// with a typed FileNode in the created map.
    /// FileNode JSON follows the draft-ietf-jmap-filenode-16 §3.1.5 directory
    /// example: `@type` is `"Directory"` and file/symlink properties are omitted.
    #[test]
    fn set_response_with_typed_file_node_deserializes() {
        let json = json!({
            "accountId": "acc1",
            "oldState": "s1",
            "newState": "s2",
            "created": {
                "new1": {
                    "@type": "Directory",
                    "id": "node-abc",
                    "parentId": null,
                    "name": "Documents",
                    "shareWith": null
                }
            },
            "updated": null,
            "destroyed": null,
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        });
        let resp: SetResponse<jmap_filenode_types::FileNode> =
            serde_json::from_value(json).expect("SetResponse<FileNode> must deserialize");
        let created = resp.created;
        let node = created.get("new1").expect("new1 must be present");
        assert_eq!(node.id, "node-abc");
        assert_eq!(node.name, "Documents");
        assert_eq!(
            node.node_type,
            Some(jmap_filenode_types::NodeType::Directory)
        );
        assert!(
            node.extra.is_empty(),
            "extra must be empty: {:?}",
            node.extra
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.9) ─────────────────
    //
    // For Serialize-only method-argument structs, the test constructs a
    // struct with a vendor field in `extra` and asserts that the field
    // flattens into the serialized JSON. Uses synthetic `acmeCorp*` keys
    // that are guaranteed not to appear in any draft-ietf-jmap-filenode
    // typed field — so the tests are independent of the crate under test.

    /// `FileNodeSetParams.extra` flattens into serialized JSON.
    #[test]
    fn filenode_set_params_propagates_vendor_extras() {
        let mut params = FileNodeSetParams::default();
        params
            .extra
            .insert("acmeCorpCascade".into(), json!("strict"));
        let v = serde_json::to_value(&params).expect("serialize FileNodeSetParams");
        assert_eq!(v["acmeCorpCascade"], json!("strict"));
    }

    /// `FileNodeCopyParams.extra` flattens into serialized JSON.
    #[test]
    fn filenode_copy_params_propagates_vendor_extras() {
        let mut extra = serde_json::Map::new();
        extra.insert("acmeCorpAudit".into(), json!(true));
        let params = FileNodeCopyParams {
            from_account_id: Id::from("acct-src"),
            on_destroy_remove_children: None,
            on_exists: None,
            compare_case_insensitively: None,
            if_from_in_state: None,
            if_in_state: None,
            on_success_destroy_original: None,
            destroy_from_if_in_state: None,
            extra,
        };
        let v = serde_json::to_value(&params).expect("serialize FileNodeCopyParams");
        assert_eq!(v["acmeCorpAudit"], json!(true));
    }

    /// Oracle: FileNodeCopyParams with on_destroy_remove_children and
    /// compare_case_insensitively serializes all three named fields
    /// (fromAccountId, onDestroyRemoveChildren, compareCaseInsensitively).
    /// Expected field names from RFC 8620 §5.4 (fromAccountId) and
    /// draft-ietf-jmap-filenode-16 §3.2.4 / §3.2.3 (the /set arguments).
    #[test]
    fn filenode_copy_params_all_fields_serializes() {
        let params = FileNodeCopyParams {
            from_account_id: Id::from("acct-src"),
            on_destroy_remove_children: Some(true),
            on_exists: Some(FileNodeOnExists::Rename),
            compare_case_insensitively: Some(false),
            if_from_in_state: None,
            if_in_state: None,
            on_success_destroy_original: None,
            destroy_from_if_in_state: None,
            extra: serde_json::Map::new(),
        };
        let v = serde_json::to_value(&params).expect("serialize FileNodeCopyParams");
        assert_eq!(v["fromAccountId"], json!("acct-src"));
        assert_eq!(v["onDestroyRemoveChildren"], json!(true));
        assert_eq!(v["onExists"], json!("rename"));
        assert_eq!(v["compareCaseInsensitively"], json!(false));
    }
}
