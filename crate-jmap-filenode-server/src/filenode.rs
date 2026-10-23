//! FileNode/* method handlers (draft-ietf-jmap-filenode-16).
//!
//! Provides all six JMAP FileNode method handlers:
//! - [`handle_filenode_get`]
//! - [`handle_filenode_changes`]
//! - [`handle_filenode_set`]
//! - [`handle_filenode_copy`]
//! - [`handle_filenode_query`]
//! - [`handle_filenode_query_changes`]
//!
//! # Wire-shape contract
//!
//! Every `handle_*` function in this module conforms to the canonical JMAP
//! method shape. The `args: serde_json::Value` parameter MUST be a JSON
//! Object whose fields match the corresponding RFC 8620 §5 method shape
//! (`/get` → §5.1, `/changes` → §5.2, `/set` → §5.3, `/copy` → §5.4,
//! `/query` → §5.5, `/queryChanges` → §5.6), with the type-specific
//! arguments defined by draft-ietf-jmap-filenode-16 §3.2. The returned
//! `Value` is the corresponding method-response object per the same
//! section refs.
//!
//! The returned `HandlerOutput::extra_invocations` carries any invocations
//! that this handler injected into the request stream (RFC 8620 §6.3);
//! for the handlers in this module the vector is **always empty**.
//!
//! Each handler returns `Err(JmapError)` for method-level failures (RFC 8620
//! §3.6.2 and §5):
//!
//! - every handler: `accountNotFound`, `accountNotSupportedByMethod`,
//!   `invalidArguments`, `serverFail`;
//! - `/get`, `/set`: `requestTooLarge` past `maxObjectsInGet` /
//!   `maxObjectsInSet`;
//! - `/set`, `/copy`: `stateMismatch`;
//! - `/copy`: `fromAccountNotFound`, `fromAccountNotSupportedByMethod`;
//! - `/changes`, `/queryChanges`: `cannotCalculateChanges`,
//!   `tooManyChanges`;
//! - `/query`, `/queryChanges`: `unsupportedFilter`;
//! - `/query`: `unsupportedSort`, `anchorNotFound`.
//!
//! Per-target failures inside a `/set` or `/copy` call surface in the
//! `notCreated` / `notUpdated` / `notDestroyed` maps within
//! `Ok((Value, ...))`, not as `Err`.

use jmap_filenode_types::{FileNode, NodeRole, NodeType};
use jmap_types::{Id, Invocation, JmapError, PatchObject, SetObject, UTCDate};
use serde_json::{Value, json};

use crate::FileNodeBackend;
use crate::helpers::{
    SetAccumulators, enforce_max_objects_in_set, extract_account_id, finalize_set_response,
    set_error_value,
};
use jmap_server::{
    HandlerOutput, backend_set_error_value, now_utc_string, reject_read_only_patch,
    reject_server_set, server_fail_from_backend, server_fail_value_from_backend,
};

// ---------------------------------------------------------------------------
// Capability gate
// ---------------------------------------------------------------------------

/// Capability gate shared by the `FileNode/*` handlers.
///
/// Returns `accountNotSupportedByMethod` when
/// [`FileNodeBackend::supports_type::<FileNode>(caller, account_id)`](FileNodeBackend::supports_type)
/// is `false` for the request's account. RFC 8620 §3.6.2 reserves that
/// error for "a valid account", so a missing, malformed or unknown
/// `accountId` passes through and the handler reports
/// `invalidArguments` or `accountNotFound` itself.
async fn ensure_filenode_supported<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: &Value,
) -> Result<(), JmapError> {
    let Some(account_id) = args
        .get("accountId")
        .and_then(Value::as_str)
        .and_then(|s| Id::new_validated(s).ok())
    else {
        return Ok(());
    };
    let exists = backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    if exists && !backend.supports_type::<FileNode>(caller, &account_id) {
        return Err(JmapError::account_not_supported_by_method());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// FileNode/get
// ---------------------------------------------------------------------------

/// Handle a `FileNode/get` method call (draft-ietf-jmap-filenode-16 §3.2.1).
///
/// `args` is the RFC 8620 §5.1 `/get` request shape (`accountId`, optional
/// `ids`, optional `properties`), augmented with the
/// draft-ietf-jmap-filenode-16 §3.2.1 `fetchParents` Boolean argument;
/// the returned `Value` is the §5.1 `/get` response shape (`accountId`,
/// `state`, `list`, `notFound`).
///
/// If `fetchParents` is `true` in the request, the ancestor nodes of all
/// returned nodes are fetched via [`FileNodeBackend::get_ancestors`] and
/// appended to the response `list` (deduplicated against already-present nodes).
/// Oracle: §3.2.1.
///
/// Unless `properties` excludes `myRights`, each node in `list` whose
/// [`FileNodeBackend::files_rights`] is `Some` reports that value as its
/// `myRights`, which draft-ietf-jmap-filenode-16 makes "server-set".
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_filenode_get<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_filenode_supported(backend, caller, &args).await?;

    let fetch_parents = args
        .get("fetchParents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // RFC 8620 §5.1: `properties` null or absent means all properties.
    let wants = |property: &str| match args.get("properties") {
        Some(Value::Array(props)) => props.iter().any(|p| p == property),
        _ => true,
    };
    let wants_sha256 = wants("sha256");
    let wants_my_rights = wants("myRights");

    // Delegate to the generic /get handler. We let it own account_id
    // parsing — the response carries accountId back, so the
    // post-processing below extracts it from there rather than
    // duplicating the parse upfront. Matches the canonical
    // jmap-mail-server pass-through pattern.
    let HandlerOutput {
        primary: mut response,
        extra_invocations: tail,
        ..
    } = jmap_server::handle_get::<FileNode, B>(backend, caller, args).await?;

    if fetch_parents {
        // Recover account_id from the response shape the generic
        // handler emitted. Bail out cleanly if missing (the generic
        // handler always emits it on success; this is for refactor
        // safety, not a real runtime case).
        let Some(account_id) = response
            .get("accountId")
            .and_then(|v| v.as_str())
            .map(Id::from)
        else {
            return Ok(HandlerOutput::new(response).with_extra(tail));
        };
        // Read phase: borrow the list immutably to collect ids of nodes
        // already in the response. The immutable borrow ends with this
        // block before the .await on get_ancestors, so the later
        // mutable re-borrow does not need a runtime shape assertion.
        let (existing_ids, node_ids) = match response.get("list").and_then(Value::as_array) {
            Some(list) => {
                let existing_ids: std::collections::HashSet<String> = list
                    .iter()
                    .filter_map(|item| {
                        item.get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_owned())
                    })
                    .collect();
                // Id::from: ids from this handler's own /get response.
                let node_ids: Vec<Id> = existing_ids.iter().map(|s| Id::from(s.as_str())).collect();
                (existing_ids, node_ids)
            }
            None => (std::collections::HashSet::new(), Vec::new()),
        };

        if !node_ids.is_empty() {
            // draft-ietf-jmap-filenode-16 §3.2.1 makes fetchParents
            // part of the request contract: the response shape does
            // not advertise that ancestors are missing, so silently
            // dropping a backend error would let the client believe
            // the requested nodes are root-level when they may have
            // ancestors. Surface a serverFail instead.
            let ancestors = backend
                .get_ancestors(caller, &account_id, &node_ids)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            // Write phase: take a fresh mutable borrow. We do NOT
            // require the previously-observed shape; a let-else
            // bail-out preserves the spec's "ancestor expansion is
            // best-effort if the list shape is unexpected" posture
            // without introducing a refactor-fragile expect/panic.
            let Some(list) = response.get_mut("list").and_then(Value::as_array_mut) else {
                return Ok(HandlerOutput::new(response).with_extra(tail));
            };
            for ancestor in ancestors {
                // Deduplicate: only append if not already in the list.
                let ancestor_id = ancestor.id.as_ref().to_owned();
                if !existing_ids.contains(&ancestor_id) {
                    // FileNode is plain data with derive(Serialize): no
                    // non-string map keys, no custom serializers that can
                    // fail, so to_value cannot return Err.
                    list.push(
                        serde_json::to_value(&ancestor)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
            }
        }
    }

    if wants_my_rights {
        report_files_rights(backend, caller, &mut response).await?;
    }

    // draft-atwood-jmap-cid-00 §FileNode Extension: with the capability,
    // "FileNode objects MUST include" `sha256`, which "MUST be `null` for
    // directory FileNodes". FileNode omits a `None` digest, so fill the
    // null here (directories, and files created before the capability).
    if wants_sha256 && let Some(account_id) = response.get("accountId").and_then(Value::as_str) {
        // Id::from: the accountId this handler already validated and echoed.
        let account_id = Id::from(account_id);
        if backend
            .cid_advertised(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
            && let Some(list) = response.get_mut("list").and_then(Value::as_array_mut)
        {
            for node in list.iter_mut().filter_map(Value::as_object_mut) {
                node.entry("sha256").or_insert(Value::Null);
            }
        }
    }

    Ok(HandlerOutput::new(response).with_extra(tail))
}

/// Replace the `myRights` of each node in a `FileNode/get` response `list`
/// with the [`FileNodeBackend::files_rights`] result, where it is `Some`.
///
/// ponytail: re-reads every listed node with one `get_objects` call,
/// because `properties` may have stripped the fields a rights policy
/// needs (`parentId`, `role`, `spaceId`, `@type`). This costs one extra
/// read per `FileNode/get`, even with the default hook. The upgrade is to
/// deserialize the list directly when `properties` is null.
async fn report_files_rights<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    response: &mut Value,
) -> Result<(), JmapError> {
    let Some(account_id) = response
        .get("accountId")
        .and_then(Value::as_str)
        .map(Id::from)
    else {
        return Ok(());
    };
    let ids: Vec<Id> = response
        .get("list")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|n| n.get("id").and_then(Value::as_str).map(Id::from))
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Ok(());
    }
    let (nodes, _) = backend
        .get_objects::<FileNode>(caller, &account_id, Some(&ids), None)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    let mut rights = std::collections::HashMap::new();
    for node in &nodes {
        if let Some(r) = backend
            .files_rights(caller, &account_id, node)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            rights.insert(node.id.as_ref().to_owned(), r);
        }
    }
    if let Some(list) = response.get_mut("list").and_then(Value::as_array_mut) {
        for node in list.iter_mut().filter_map(Value::as_object_mut) {
            let Some(r) = node
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| rights.remove(id))
            else {
                continue;
            };
            node.insert(
                "myRights".to_owned(),
                serde_json::to_value(r).expect("FilesRights serializes to JSON"),
            );
        }
    }
    Ok(())
}

/// Replace the `myRights` of a `FileNode/set` or `FileNode/copy`
/// `created` entry with the [`FileNodeBackend::files_rights`] result for
/// the created `node`, where it is `Some`.
///
/// RFC 8620 §5.3 puts "all server-set properties" in `created`, and
/// draft-ietf-jmap-filenode-16 makes `myRights` server-set. The node is
/// already stored when the hook is asked, so an `Err` leaves the create
/// standing and the entry reports the stored `myRights`.
async fn report_created_files_rights<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    account_id: &Id,
    node: &FileNode,
    entry: &mut Value,
) {
    if let (Ok(Some(rights)), Value::Object(m)) =
        (backend.files_rights(caller, account_id, node).await, entry)
    {
        m.insert(
            "myRights".to_owned(),
            serde_json::to_value(rights).expect("FilesRights serializes to JSON"),
        );
    }
}

// ---------------------------------------------------------------------------
// FileNode/changes
// ---------------------------------------------------------------------------

/// Handle a `FileNode/changes` method call (draft-ietf-jmap-filenode-16 §3.2.2).
///
/// `args` is the RFC 8620 §5.2 `/changes` request shape (`accountId`,
/// `sinceState`, optional `maxChanges`); the returned `Value` is the
/// §5.2 `/changes` response shape (`accountId`, `oldState`, `newState`,
/// `hasMoreChanges`, `created`, `updated`, `destroyed`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_filenode_changes<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_filenode_supported(backend, caller, &args).await?;
    jmap_server::handle_changes::<FileNode, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// FileNode/set
// ---------------------------------------------------------------------------

/// Collision-handling policy for `FileNode/set` create operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OnExists {
    /// null / absent (default) — return `alreadyExists`.
    Reject,
    /// `"replace"` — destroy the existing node and create the new one.
    Replace,
    /// `"rename"` — find a non-colliding name by appending a counter suffix.
    Rename,
    /// `"newest"` — compare modified timestamps; replace only if incoming is
    /// strictly later, otherwise reject with `alreadyExists`.
    Newest,
}

/// Handle a `FileNode/set` method call (draft-ietf-jmap-filenode-16 §3.2.3).
///
/// `args` is the RFC 8620 §5.3 `/set` request shape (`accountId`, optional
/// `ifInState`, optional `create` / `update` / `destroy` maps), augmented
/// with the FileNode-specific arguments described below; the returned
/// `Value` is the §5.3 `/set` response shape (`accountId`, `oldState`,
/// `newState`, plus the per-operation `created` / `notCreated` /
/// `updated` / `notUpdated` / `destroyed` / `notDestroyed` maps).
///
/// ## FileNode-specific arguments
///
/// - `onDestroyRemoveChildren` (bool, default `false`): when `true`, the
///   backend is responsible for cascading the destroy to children. When
///   `false` (the default), a destroy of a node that has children returns
///   `notDestroyed` with error type `nodeHasChildren`.
/// - `onExists` (object, optional): collision-handling policy when a create
///   would produce a name that already exists in the same parent directory.
///   See the spec §3.2.3 for full semantics.
///
/// ## Circular reference prevention
///
/// When an `update` sets `parentId` to a new value, the handler calls
/// [`FileNodeBackend::get_descendant_ids`] on the node being moved.  If the
/// proposed new parent is in the descendant set, a cycle would result and the
/// update is placed in `notUpdated` with `invalidProperties`.
///
/// ## Rights in `created`
///
/// Each `created` entry whose node has a `Some`
/// [`FileNodeBackend::files_rights`] reports that value as its `myRights`.
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_filenode_set<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_filenode_supported(backend, caller, &args).await?;
    let (account_id, mut args) = extract_account_id(args)?;
    // RFC 8620 §3.6.2 accountNotFound, as in the canonical
    // jmap-mail-server::handle_mailbox_set (bd:JMAP-2wuk).
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }

    // RFC 8620 §5.3 maxObjectsInSet (bd:JMAP-ayoz.41.7). Reject
    // unbounded /set batches before touching the storage layer.
    enforce_max_objects_in_set(&args, backend.max_objects_in_set(caller, &account_id))?;

    let old_state = backend
        .get_state::<FileNode>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    // Parse FileNode-specific set arguments.
    let on_destroy_remove_children: bool = args
        .get("onDestroyRemoveChildren")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Parse onExists collision-handling policy (§3.2.3).
    let on_exists: OnExists = {
        let v = args.remove("onExists");
        match v.as_ref() {
            None | Some(Value::Null) => OnExists::Reject,
            Some(Value::String(s)) if s == "replace" => OnExists::Replace,
            Some(Value::String(s)) if s == "rename" => OnExists::Rename,
            Some(Value::String(s)) if s == "newest" => OnExists::Newest,
            Some(other) => {
                return Err(JmapError::invalid_arguments(format!(
                    "invalid onExists value: expected \"replace\", \"rename\", \"newest\" or null; got {}",
                    jmap_server::json_type_name(other)
                )));
            }
        }
    };

    let case_folding = crate::CaseFolding::from_wire_bool(
        args.get("compareCaseInsensitively")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    );

    let mut created = serde_json::Map::new();
    let mut not_created = serde_json::Map::new();
    let mut updated = serde_json::Map::new();
    let mut not_updated = serde_json::Map::new();
    let mut destroyed_list: Vec<Value> = Vec::new();
    let mut not_destroyed = serde_json::Map::new();
    let mut mutated = false;

    // -----------------------------------------------------------------------
    // create
    // -----------------------------------------------------------------------
    if let Some(Value::Object(create_map)) = args.remove("create") {
        // draft-atwood-jmap-cid-00 §FileNode Extension: with the capability,
        // FileNode objects "MUST include" `sha256`, which "MUST be `null`
        // for directory FileNodes"; RFC 8620 §5.3 puts server-set properties
        // in `created`. Asked once, before any create mutates state, so an
        // Err is a clean top-level serverFail.
        let cid_advertised = backend
            .cid_advertised(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        for (create_id, obj_val) in create_map {
            if let Some(err) = obj_val.as_object().and_then(reject_server_set::<FileNode>) {
                not_created.insert(create_id, set_error_value(&err));
                continue;
            }
            // `FileNode.id` is required, so the parse fills it with "",
            // which is not an Id (RFC 8620 §1.2: "at least 1 and a maximum
            // of 255 octets"); `build` overwrites it with the minted id.
            let obj_with_id = match obj_val {
                Value::Object(mut m) => {
                    m.insert("id".to_owned(), Value::String(String::new()));
                    // `name` is required and non-nullable; if missing, serde will
                    // produce an `invalidProperties` error below.
                    Value::Object(m)
                }
                other => other,
            };

            // draft-ietf-jmap-filenode-16 §3.1.1: `@type` MUST be provided on
            // creation; the server does not infer it.
            if obj_with_id.get("@type").is_none_or(Value::is_null) {
                not_created.insert(
                    create_id,
                    json!({
                        "type": "invalidProperties",
                        "properties": ["@type"],
                        "description": "@type is required on create",
                    }),
                );
                continue;
            }

            // Validate fully before acting on onExists, so an invalid create
            // cannot destroy the colliding node (RFC 8620 §5.3: each creation
            // is an atomic unit).
            let mut node: FileNode = match serde_json::from_value(obj_with_id) {
                Ok(n) => n,
                Err(e) => {
                    not_created.insert(
                        create_id,
                        json!({ "type": "invalidProperties", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            if let Some(err) = create_type_error(&node) {
                not_created.insert(create_id, err);
                continue;
            }

            // draft-atwood-jmap-cid-00 §FileNode Extension: `sha256` is
            // server-set ("MUST be `null` for directory FileNodes"; for a
            // file, the digest of the stored blob). The client could not send
            // it, so only the File branch below fills it.

            // For file nodes, verify the blob exists. A transient backend
            // error propagates as a top-level serverFail so the client
            // retries instead of silently mis-classifying as blob-not-found.
            if matches!(node.node_type.as_ref(), Some(NodeType::File))
                && let Some(ref blob_id) = node.blob_id
            {
                match backend.blob_exists(caller, &account_id, blob_id).await {
                    Ok(true) => {}
                    Ok(false) => {
                        not_created.insert(
                                create_id,
                                json!({ "type": "invalidProperties", "properties": ["blobId"], "description": "blob not found" }),
                            );
                        continue;
                    }
                    Err(e) => return Err(server_fail_from_backend(&e)),
                }
                // draft-atwood-jmap-cid-00 §FileNode Extension: `sha256`
                // is server-set from the stored blob, and exists only
                // when urn:ietf:params:jmap:cid is advertised.
                match backend.blob_sha256(caller, &account_id, blob_id).await {
                    Ok(digest) => node.sha256 = digest,
                    Err(e) => {
                        not_created.insert(create_id, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // Tracks the rename applied by OnExists::Rename so the
            // post-create response can enforce the spec's MUST that the
            // renamed name appears in the response (§3.2.3 lines 687-690)
            // even if the backend echoes a different name.
            let mut renamed_to: Option<String> = None;

            // -------------------------------------------------------------------
            // Collision detection (onExists — §3.2.3).
            // -------------------------------------------------------------------
            let node_name = node.name.clone();
            let parent_id_for_collision: Option<Id> = node.parent_id.clone();

            if !node_name.is_empty() {
                match backend
                    .find_sibling_by_name(
                        caller,
                        &account_id,
                        parent_id_for_collision.as_ref(),
                        &node_name,
                        case_folding,
                    )
                    .await
                {
                    Ok(Some(existing_id)) => {
                        match on_exists {
                            OnExists::Reject => {
                                not_created.insert(
                                    create_id,
                                    json!({ "type": "alreadyExists", "existingId": existing_id.as_ref() }),
                                );
                                continue;
                            }
                            OnExists::Replace => {
                                // draft-ietf-jmap-filenode-16 §3.2.3 lines
                                // 677-682: "If 'replace', the existing item
                                // will be destroyed. [...] if the replaced
                                // item is a directory which has children,
                                // then the server MUST respond with a
                                // nodeHasChildren error to this action
                                // UNLESS onDestroyRemoveChildren is true."
                                let desc_ids = match backend
                                    .get_descendant_ids(caller, &account_id, &existing_id)
                                    .await
                                {
                                    Ok(ids) => ids,
                                    Err(e) => {
                                        not_created
                                            .insert(create_id, server_fail_value_from_backend(&e));
                                        continue;
                                    }
                                };
                                if !desc_ids.is_empty() && !on_destroy_remove_children {
                                    not_created.insert(
                                        create_id,
                                        json!({ "type": "nodeHasChildren", "existingId": existing_id.as_ref() }),
                                    );
                                    continue;
                                }
                                // When descendants exist AND
                                // onDestroyRemoveChildren=true, cascade the
                                // destroy of descendants first. Same
                                // ordering as the regular destroy path
                                // (filenode.rs around §destroy: descendants
                                // first, then the targeted node).
                                let mut cascade_failed = false;
                                for desc_id in &desc_ids {
                                    match backend
                                        .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                        .await
                                    {
                                        Ok(()) => {
                                            mutated = true;
                                            destroyed_list
                                                .push(Value::String(desc_id.as_ref().to_owned()));
                                        }
                                        Err(e) => {
                                            not_created.insert(
                                                create_id.clone(),
                                                backend_set_error_value(e),
                                            );
                                            cascade_failed = true;
                                            break;
                                        }
                                    }
                                }
                                if cascade_failed {
                                    continue;
                                }
                                // Now destroy the existing colliding node.
                                match backend
                                    .destroy_object::<FileNode>(caller, &account_id, &existing_id)
                                    .await
                                {
                                    Ok(()) => {
                                        mutated = true;
                                        destroyed_list
                                            .push(Value::String(existing_id.as_ref().to_owned()));
                                    }
                                    Err(e) => {
                                        not_created.insert(create_id, backend_set_error_value(e));
                                        continue;
                                    }
                                }
                                // Fall through to create the new node.
                            }
                            OnExists::Rename => {
                                // Find a non-colliding name by appending a counter suffix.
                                let mut renamed = false;
                                let mut rename_error = false;
                                for suffix in 1_u32..=100 {
                                    let candidate = format!("{node_name}-{suffix}");
                                    match backend
                                        .find_sibling_by_name(
                                            caller,
                                            &account_id,
                                            parent_id_for_collision.as_ref(),
                                            &candidate,
                                            case_folding,
                                        )
                                        .await
                                    {
                                        Ok(None) => {
                                            node.name = candidate.clone();
                                            renamed_to = Some(candidate);
                                            renamed = true;
                                            break;
                                        }
                                        Ok(Some(_)) => continue,
                                        Err(e) => {
                                            not_created.insert(
                                                create_id.clone(),
                                                server_fail_value_from_backend(&e),
                                            );
                                            rename_error = true;
                                            break;
                                        }
                                    }
                                }
                                if rename_error {
                                    continue;
                                }
                                if !renamed {
                                    not_created.insert(
                                        create_id,
                                        json!({ "type": "serverFail", "description": "could not find unique name after 100 attempts" }),
                                    );
                                    continue;
                                }
                                // Fall through to create the renamed node.
                            }
                            OnExists::Newest => {
                                // draft-ietf-jmap-filenode-16 §3.2.3: compare
                                // modified timestamps. If the incoming item has
                                // a strictly later modified value, proceed as
                                // with "replace". Otherwise reject as with null.
                                //
                                // Validate the incoming modified through
                                // UTCDate::new_validated to prevent malformed
                                // strings (e.g. "9999") from lexicographically
                                // beating any real timestamp.
                                let incoming_modified = node
                                    .modified
                                    .as_ref()
                                    .and_then(|d| UTCDate::new_validated(d.as_ref()).ok())
                                    .map(|d| d.as_ref().to_owned())
                                    .unwrap_or_default();
                                let (existing_nodes, _): (Vec<FileNode>, _) = backend
                                    .get_objects::<FileNode>(
                                        caller,
                                        &account_id,
                                        Some(std::slice::from_ref(&existing_id)),
                                        None,
                                    )
                                    .await
                                    .map_err(|e| server_fail_from_backend(&e))?;
                                let existing_modified = existing_nodes
                                    .first()
                                    .and_then(|n| n.modified.as_ref())
                                    .map(|d| d.as_ref())
                                    .unwrap_or("");
                                if incoming_modified.as_str() > existing_modified {
                                    // Incoming is strictly later — fall through
                                    // to the Replace logic below.
                                } else {
                                    not_created.insert(
                                        create_id,
                                        json!({ "type": "alreadyExists", "existingId": existing_id.as_ref() }),
                                    );
                                    continue;
                                }
                                // Proceed with replace: destroy existing node
                                // (same cascade logic as OnExists::Replace).
                                let desc_ids = match backend
                                    .get_descendant_ids(caller, &account_id, &existing_id)
                                    .await
                                {
                                    Ok(ids) => ids,
                                    Err(e) => {
                                        not_created
                                            .insert(create_id, server_fail_value_from_backend(&e));
                                        continue;
                                    }
                                };
                                if !desc_ids.is_empty() && !on_destroy_remove_children {
                                    not_created.insert(
                                        create_id,
                                        json!({ "type": "nodeHasChildren", "existingId": existing_id.as_ref() }),
                                    );
                                    continue;
                                }
                                let mut cascade_failed = false;
                                for desc_id in &desc_ids {
                                    match backend
                                        .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                        .await
                                    {
                                        Ok(()) => {
                                            mutated = true;
                                            destroyed_list
                                                .push(Value::String(desc_id.as_ref().to_owned()));
                                        }
                                        Err(e) => {
                                            not_created.insert(
                                                create_id.clone(),
                                                backend_set_error_value(e),
                                            );
                                            cascade_failed = true;
                                            break;
                                        }
                                    }
                                }
                                if cascade_failed {
                                    continue;
                                }
                                match backend
                                    .destroy_object::<FileNode>(caller, &account_id, &existing_id)
                                    .await
                                {
                                    Ok(()) => {
                                        mutated = true;
                                        destroyed_list
                                            .push(Value::String(existing_id.as_ref().to_owned()));
                                    }
                                    Err(e) => {
                                        not_created.insert(create_id, backend_set_error_value(e));
                                        continue;
                                    }
                                }
                                // Fall through to create the new node.
                            }
                        }
                    }
                    Ok(None) => {} // No collision, proceed normally.
                    Err(e) => {
                        not_created.insert(create_id, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // §3.1.1: created, modified and accessed default to the current
            // server time; a null modified or accessed is set to it.
            let now = now_utc_string();
            for date in [&mut node.created, &mut node.modified, &mut node.accessed] {
                date.get_or_insert_with(|| now.clone());
            }

            let build = move |id: &Id| {
                node.id = id.clone();
                node
            };
            match backend
                .create_object::<FileNode, _>(caller, &account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    let mut value = serde_json::to_value(&created_obj)
                        .expect("derive(Serialize) on plain data is infallible");
                    // draft-ietf-jmap-filenode-16 §3.2.3 lines 687-690:
                    // "If the server changes the name, it MUST include
                    // the new 'name' value in the created or updated
                    // response field for this id." Enforce regardless
                    // of whether the backend echoed the supplied name.
                    if let Value::Object(ref mut m) = value {
                        if let Some(name) = renamed_to.as_deref() {
                            m.insert("name".to_owned(), Value::String(name.to_owned()));
                        }
                        if cid_advertised {
                            m.entry("sha256").or_insert(Value::Null);
                        }
                    }
                    report_created_files_rights(
                        backend,
                        caller,
                        &account_id,
                        &created_obj,
                        &mut value,
                    )
                    .await;
                    created.insert(create_id, value);
                }
                Err(e) => {
                    not_created.insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // update
    // -----------------------------------------------------------------------
    if let Some(Value::Object(update_map)) = args.remove("update") {
        for (id_str, patch_val) in update_map {
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_updated.insert(id_str, set_error_value(&err));
                    continue;
                }
            };

            // Convert wire-format Value into a typed PatchObject FIRST so
            // the subsequent cycle check sees the same key view the backend
            // will eventually receive. RFC 8620 §5.3 mandates a PatchObject
            // is a JSON Object; non-object values produce an `invalidPatch`
            // SetError. Doing the typed parse up front avoids a class of
            // bug where the handler's introspection sees a different shape
            // than the backend (bd:JMAP-510h.13).
            let mut patch = match serde_json::from_value::<PatchObject>(patch_val) {
                Ok(p) => p,
                Err(e) => {
                    not_updated.insert(
                        id_str,
                        json!({ "type": "invalidPatch", "description": e.to_string() }),
                    );
                    continue;
                }
            };

            // draft-ietf-jmap-filenode-16 §3.1: setting a property not
            // defined for the node's @type to a non-null value is
            // invalidProperties; a null value is ignored. The stored node
            // is fetched only when the patch sets a type-specific property
            // or @type. A JSON-pointer key ("mediaType/x") patches the
            // property named by its first segment (RFC 8620 §5.3). A null
            // pointer patch is kept: §5.3 requires its leading parts to
            // exist, so one into another type's property is invalidPatch.
            let typed: Vec<(&str, NodeType, bool)> = patch
                .as_map()
                .iter()
                .filter_map(|(k, v)| {
                    let (prop, pointer) = k
                        .split_once('/')
                        .map_or((k.as_str(), false), |(head, _)| (head, true));
                    if v.is_null() && !pointer {
                        return None;
                    }
                    property_owner(prop).map(|owner| (prop, owner, v.is_null()))
                })
                .collect();
            // §3.1.1, §3.1.2, §3.1.4: name, blobId, mediaType and target
            // are not nullable and have no default; a null one defined for
            // the node's @type is invalidProperties (other types' nulls are
            // ignored, above). Defaulted properties reset on null (RFC 8620
            // §5.3): a removed executable or isSubscribed reads back as its
            // default; created is filled in below.
            let mut nulled: Vec<&str> = ["name", "blobId", "mediaType", "target"]
                .into_iter()
                .filter(|p| patch.as_map().get(*p).is_some_and(Value::is_null))
                .collect();
            // A server-set or immutable property (`@type`, `sha256`,
            // `spaceId`, ...) may appear only with its stored value (RFC 8620
            // §5.3), so the check below needs the stored node.
            let read_only_patched = patch.as_map().keys().any(|k| {
                let head = k.split('/').next();
                FileNode::SERVER_SET
                    .iter()
                    .chain(FileNode::IMMUTABLE)
                    .any(|p| Some(*p) == head)
            });
            // draft-atwood-jmap-chat-filenode-00 §The `spaceId` Extension
            // Property: `spaceId` is present only on a Space file tree root,
            // so a root's role may not change either.
            let role_patch = patch.as_map().get("role");
            if !typed.is_empty()
                || read_only_patched
                || role_patch.is_some()
                || nulled.iter().any(|p| property_owner(p).is_some())
            {
                let stored = match backend
                    .get_objects::<FileNode>(
                        caller,
                        &account_id,
                        Some(std::slice::from_ref(&id)),
                        None,
                    )
                    .await
                {
                    Ok((nodes, _)) => nodes.into_iter().next(),
                    Err(e) => {
                        not_updated.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                };
                nulled.retain(|p| {
                    property_owner(p).is_none_or(|owner| {
                        stored
                            .as_ref()
                            .is_some_and(|n| n.node_type.as_ref() == Some(&owner))
                    })
                });
                // A missing node falls through so update_object reports
                // notFound.
                if let Some(node) = stored {
                    // §3.1.1 `@type` is immutable; draft-atwood-jmap-cid-00
                    // `sha256` and the other server-set properties may
                    // repeat only their stored value (RFC 8620 §5.3).
                    let current = match json!(node) {
                        Value::Object(m) => Some(m),
                        _ => None,
                    };
                    if let Some(err) =
                        reject_read_only_patch::<FileNode>(patch.as_map(), current.as_ref())
                    {
                        not_updated.insert(id_str, set_error_value(&err));
                        continue;
                    }
                    // draft-atwood-jmap-chat-filenode-00 §Creating the File
                    // Tree Root: "A server MUST reject a `FileNode/set`
                    // update that changes `role` to or from `\"chatspace\"`".
                    let chat_space = Some(NodeRole::ChatSpace.to_wire_str());
                    if role_patch.is_some_and(|r| {
                        (r.as_str() == chat_space) != (node.role == Some(NodeRole::ChatSpace))
                    }) {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "invalidProperties",
                                "properties": ["role"],
                                "description": "role chatspace can be set only at create",
                            }),
                        );
                        continue;
                    }
                    let bad: Vec<(&str, bool)> = typed
                        .iter()
                        .filter(|(_, owner, _)| node.node_type.as_ref() != Some(owner))
                        .map(|(k, _, null_pointer)| (*k, *null_pointer))
                        .collect();
                    if bad.iter().any(|(_, null_pointer)| *null_pointer) {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "invalidPatch",
                                "description": "patch path into a property not defined for this node's @type",
                            }),
                        );
                        continue;
                    }
                    if !bad.is_empty() {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "invalidProperties",
                                "properties": bad.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
                                "description": "property not defined for this node's @type",
                            }),
                        );
                        continue;
                    }
                }
            }

            if !nulled.is_empty() {
                not_updated.insert(
                    id_str,
                    json!({
                        "type": "invalidProperties",
                        "properties": nulled,
                        "description": "property cannot be null",
                    }),
                );
                continue;
            }

            // §3.1.1: a null modified or accessed is set to the current
            // server time, as is a null created (its default, RFC 8620 §5.3).
            for key in ["created", "modified", "accessed"] {
                if let Some(v) = patch.as_map_mut().get_mut(key).filter(|v| v.is_null()) {
                    *v = json!(now_utc_string());
                }
            }

            // Circular reference check: if the patch touches `parentId`,
            // verify the move would not create a cycle. Introspection is
            // on PatchObject's underlying typed map so what the cycle
            // guard sees is byte-identical to what the backend will see
            // via `update_object`.
            //
            // RFC 8620 §5.3: a property value of the wrong type is an
            // invalidProperties SetError. parentId is `Id|null`
            // (draft-ietf-jmap-filenode-16 §3.1.1), so reject anything else
            // here instead of skipping the cycle guard and relying on each
            // backend to catch the bad type.
            if let Some(new_parent_val) = patch.as_map().get("parentId") {
                if !new_parent_val.is_null() && !new_parent_val.is_string() {
                    not_updated.insert(
                        id_str,
                        json!({
                            "type": "invalidProperties",
                            "properties": ["parentId"],
                            "description": "parentId must be a string or null"
                        }),
                    );
                    continue;
                }
                if let Some(new_parent_str) = new_parent_val.as_str() {
                    let new_parent_id = match jmap_server::parse_wire_id(new_parent_str, "parentId")
                    {
                        Ok(id) => id,
                        Err(err) => {
                            not_updated.insert(id_str, set_error_value(&err));
                            continue;
                        }
                    };
                    // A node is trivially an ancestor of itself
                    // (draft-ietf-jmap-filenode-16 §3.2.3), and
                    // `get_descendant_ids` documents that the
                    // returned set excludes the node itself — so
                    // the descendant check below cannot catch
                    // `parentId == id`. Reject it explicitly.
                    if new_parent_id == id {
                        not_updated.insert(
                            id_str,
                            json!({
                                "type": "invalidProperties",
                                "properties": ["parentId"],
                                "description": "a node cannot be its own parent"
                            }),
                        );
                        continue;
                    }
                    match backend.get_descendant_ids(caller, &account_id, &id).await {
                        Ok(descendant_ids) => {
                            if descendant_ids.iter().any(|did| did == &new_parent_id) {
                                not_updated.insert(
                                    id_str,
                                    json!({
                                        "type": "invalidProperties",
                                        "properties": ["parentId"],
                                        "description": "moving this node to the proposed parent would create a cycle"
                                    }),
                                );
                                continue;
                            }
                        }
                        Err(e) => {
                            not_updated.insert(id_str, server_fail_value_from_backend(&e));
                            continue;
                        }
                    }
                }
            }

            // draft-atwood-jmap-cid-00 §FileNode Extension: "When a
            // `FileNode/set` ({{JMAP-FILENODE}}) create or update changes
            // the associated blob, the server MUST update `sha256` to
            // reflect the new blob content before returning the response."
            // A null blobId was rejected above, so only a string is left.
            // Without the capability the patch nulls sha256, so a digest of
            // the old blob cannot outlive it.
            let new_sha256 = match patch.as_map().get("blobId").and_then(Value::as_str) {
                Some(blob_id) => {
                    let blob_id = match jmap_server::parse_wire_id(blob_id, "blobId") {
                        Ok(id) => id,
                        Err(err) => {
                            not_updated.insert(id_str, set_error_value(&err));
                            continue;
                        }
                    };
                    match backend.blob_sha256(caller, &account_id, &blob_id).await {
                        Ok(digest) => {
                            patch
                                .as_map_mut()
                                .insert("sha256".to_owned(), json!(digest));
                            digest
                        }
                        Err(e) => {
                            not_updated.insert(id_str, server_fail_value_from_backend(&e));
                            continue;
                        }
                    }
                }
                None => None,
            };

            match backend
                .update_object::<FileNode>(caller, &account_id, &id, patch)
                .await
            {
                Ok(Some(obj)) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        serde_json::to_value(&obj)
                            .expect("derive(Serialize) on plain data is infallible"),
                    );
                }
                // RFC 8620 §5.3: the updated entry carries any property
                // "that changed that were not explicitly requested by the
                // client"; a server-set sha256 is one.
                Ok(None) => {
                    mutated = true;
                    updated.insert(
                        id_str,
                        new_sha256.map_or(Value::Null, |d| json!({ "sha256": d })),
                    );
                }
                Err(e) => {
                    not_updated.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // destroy
    // -----------------------------------------------------------------------

    // Collect the full set of IDs being destroyed in this request *before*
    // consuming the array, so the nodeHasChildren check can inspect it.
    let destroy_id_set: std::collections::HashSet<Id> =
        if let Some(Value::Array(arr)) = args.get("destroy") {
            arr.iter()
                .filter_map(|v| v.as_str().map(Id::from))
                .collect()
        } else {
            std::collections::HashSet::new()
        };

    if let Some(Value::Array(destroy_arr)) = args.remove("destroy") {
        // RFC 8620 §5.3: every element of the destroy array MUST be a string Id.
        // Reject the whole request if any element is non-string rather than
        // silently skipping it, which would produce a misleading response.
        if let Some(bad) = destroy_arr.iter().find(|v| !v.is_string()) {
            return Err(JmapError::invalid_arguments(format!(
                "destroy: every element must be a string Id; got {}",
                jmap_server::json_type_name(bad)
            )));
        }
        for id_val in destroy_arr {
            let id_str = match id_val.as_str() {
                Some(s) => s.to_owned(),
                None => continue, // unreachable: validated above
            };
            let id = match jmap_server::parse_wire_id(&id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_destroyed.insert(id_str, set_error_value(&err));
                    continue;
                }
            };

            // If onDestroyRemoveChildren is false, check whether all
            // descendants are also in this destroy request.
            // RFC §3.2.3: MUST NOT return nodeHasChildren if all descendants
            // are also being destroyed in the same request.
            //
            // Partial-tree subtlety (bd:JMAP-510h.12): get_descendant_ids
            // returns the TRANSITIVE descendant set. When a destroy set
            // includes {A, B} where A is an ancestor of B and B has its
            // own descendants C that are NOT in the set, both A and B
            // report nodeHasChildren — A because its subtree contains
            // the uncovered C, B because B's subtree contains the
            // uncovered C. The error attribution is at every uncovered
            // ancestor, not at the deepest uncovered descendant, but
            // the overall verdict (no destroys happen) is correct and
            // never falsely succeeds. Backends that want to attribute
            // the error closer to the actual uncovered descendant can
            // override this loop with their own ordering logic; the
            // reference handler is intentionally simple.
            if !on_destroy_remove_children {
                match backend.get_descendant_ids(caller, &account_id, &id).await {
                    Ok(desc_ids) => {
                        let all_covered = desc_ids.iter().all(|did| destroy_id_set.contains(did));
                        if !desc_ids.is_empty() && !all_covered {
                            not_destroyed.insert(id_str, json!({ "type": "nodeHasChildren" }));
                            continue;
                        }
                    }
                    Err(e) => {
                        not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // When onDestroyRemoveChildren=true, cascade to all descendants first.
            if on_destroy_remove_children {
                match backend.get_descendant_ids(caller, &account_id, &id).await {
                    Ok(desc_ids) => {
                        let mut cascade_failed = false;
                        for desc_id in &desc_ids {
                            match backend
                                .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                .await
                            {
                                Ok(()) => {
                                    mutated = true;
                                    destroyed_list.push(Value::String(desc_id.as_ref().to_owned()));
                                }
                                Err(e) => {
                                    not_destroyed
                                        .insert(id_str.clone(), backend_set_error_value(e));
                                    cascade_failed = true;
                                    break;
                                }
                            }
                        }
                        if cascade_failed {
                            continue;
                        }
                    }
                    Err(e) => {
                        not_destroyed.insert(id_str, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            match backend
                .destroy_object::<FileNode>(caller, &account_id, &id)
                .await
            {
                Ok(()) => {
                    mutated = true;
                    destroyed_list.push(Value::String(id_str));
                }
                Err(e) => {
                    not_destroyed.insert(id_str, backend_set_error_value(e));
                }
            }
        }
    }

    finalize_set_response::<B, FileNode>(
        backend,
        caller,
        &account_id,
        old_state,
        mutated,
        SetAccumulators {
            created,
            updated,
            destroyed: destroyed_list,
            not_created,
            not_updated,
            not_destroyed,
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// FileNode/copy
// ---------------------------------------------------------------------------

/// Handle a `FileNode/copy` method call (draft-ietf-jmap-filenode-16 §3.2.4).
///
/// `args` is the RFC 8620 §5.4 `/copy` request shape (`fromAccountId`,
/// optional `ifFromInState`, `accountId`, optional `ifInState`, `create`
/// map of creationId → object, optional `onSuccessDestroyOriginal`,
/// optional `destroyFromIfInState`); the returned `Value` is the §5.4
/// `/copy` response shape (`fromAccountId`, `accountId`, `oldState`,
/// `newState`, `created` / `notCreated` maps), plus `destroyed`: the ids
/// of destination nodes that onExists replacement destroyed, cascades
/// included, omitted when none were.
///
/// `FileNode/copy` uses its own wire shape (source account + destination account),
/// separate from `*/set`. This handler parses the wire arguments and delegates
/// each copy entry to [`FileNodeBackend::create_object`] in the destination
/// account, using the source node's properties.
///
/// The source nodes are fetched with `get_objects` before copying.
///
/// Each `created` entry whose node has a `Some`
/// [`FileNodeBackend::files_rights`] in the destination account reports
/// that value as its `myRights`.
///
/// Returns `(response_args, extra_invocations)`. When
/// `onSuccessDestroyOriginal` is true the extra list holds the implicit
/// `FileNode/set` response (RFC 8620 §5.4, §6.3), or an `error` of type
/// `stateMismatch` if `destroyFromIfInState` does not match the source
/// account's FileNode state, in which case the originals are left untouched.
pub async fn handle_filenode_copy<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    call_id: &str,
) -> Result<HandlerOutput, JmapError> {
    let from_account_id: Id = match args.get("fromAccountId").and_then(|v| v.as_str()) {
        Some(s) => Id::new_validated(s)
            .map_err(|e| JmapError::invalid_arguments(format!("fromAccountId: {e}")))?,
        None => return Err(JmapError::invalid_arguments("fromAccountId is required")),
    };
    let (account_id, mut args) = extract_account_id(args)?;

    // Verify both accounts exist. RFC 8620 §5.4 distinguishes
    // fromAccountNotFound (the source) from accountNotFound (the
    // destination) so a client can tell which side of the copy is
    // misconfigured. Mirrors the canonical jmap-mail-server::handle_email_copy
    // pattern.
    if !backend
        .account_exists(caller, &from_account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::from_account_not_found());
    }
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    // RFC 8620 §3.6.2 / §5.4: both accounts exist, so a type opt-out on
    // either side is reported against that side.
    if !backend.supports_type::<FileNode>(caller, &account_id) {
        return Err(JmapError::account_not_supported_by_method());
    }
    if !backend.supports_type::<FileNode>(caller, &from_account_id) {
        return Err(JmapError::from_account_not_supported_by_method());
    }

    // ifFromInState: check source account state (RFC 8620 §5.4).
    if let Some(if_from_in_state) = args.get("ifFromInState").and_then(|v| v.as_str()) {
        let from_state = backend
            .get_state::<FileNode>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        if if_from_in_state != from_state.as_ref() {
            return Err(JmapError::state_mismatch());
        }
    }

    let old_state = backend
        .get_state::<FileNode>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;

    // ifInState: check destination account state (RFC 8620 §5.4).
    if let Some(if_in_state) = args.get("ifInState").and_then(|v| v.as_str())
        && if_in_state != old_state.as_ref()
    {
        return Err(JmapError::state_mismatch());
    }

    // draft-ietf-jmap-filenode-16 §3.2.4: "This is a standard Foo/copy
    // function with the same additional top-level arguments as
    // FileNode/set, onDestroyRemoveChildren and onExists, with the
    // same behaviour." Parse and apply them to the destination account
    // (find_sibling_by_name uses account_id, NOT from_account_id).
    let on_destroy_remove_children: bool = args
        .get("onDestroyRemoveChildren")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let on_exists: OnExists = {
        let v = args.remove("onExists");
        match v.as_ref() {
            None | Some(Value::Null) => OnExists::Reject,
            Some(Value::String(s)) if s == "replace" => OnExists::Replace,
            Some(Value::String(s)) if s == "rename" => OnExists::Rename,
            Some(Value::String(s)) if s == "newest" => OnExists::Newest,
            Some(other) => {
                return Err(JmapError::invalid_arguments(format!(
                    "invalid onExists value: expected \"replace\", \"rename\", \"newest\" or null; got {}",
                    jmap_server::json_type_name(other)
                )));
            }
        }
    };

    let case_folding = crate::CaseFolding::from_wire_bool(
        args.get("compareCaseInsensitively")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    );

    // RFC 8620 §5.4: if onSuccessDestroyOriginal is true, after emitting the
    // Foo/copy response the server MUST make a single call to Foo/set to
    // destroy the original of each successfully copied record.
    let on_success_destroy_original: bool = args
        .get("onSuccessDestroyOriginal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut copied = serde_json::Map::new();
    let mut not_copied = serde_json::Map::new();
    // Tracks (copy_id, source_id) pairs for successfully copied entries so
    // the implicit `onSuccessDestroyOriginal` Foo/set can destroy them.
    let mut copied_source_ids: Vec<(String, Id)> = Vec::new();
    // Ids destroyed by onExists "replace"/"newest", including
    // onDestroyRemoveChildren cascades. RFC 8620 §5.4 defines no
    // `destroyed` on Foo/copy, but §3.2.4 gives copy FileNode/set's
    // onExists "with the same behaviour", and §3.2.3 says "the server
    // MUST include the id of the replaced item in the destroyed response
    // list" (draft-ietf-jmap-filenode-16 L677-679, L655-657, L773-775).
    // Recorded as each destroy succeeds: a later failure for the same
    // entry does not bring the node back.
    let mut destroyed: Vec<Id> = Vec::new();
    let mut mutated = false;

    if let Some(Value::Object(create_map)) = args.remove("create") {
        // draft-atwood-jmap-cid-00 §FileNode Extension: as for FileNode/set
        // create, the destination's capability decides whether each copy
        // carries `sha256`. Asked once, before any copy mutates state, so an
        // Err is a clean top-level serverFail.
        let cid_advertised = backend
            .cid_advertised(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        for (create_id, obj_val) in create_map {
            // obj_val is the per-copy descriptor from the client: it must
            // contain `id` (the source node id in fromAccountId) plus any
            // property overrides for the copy.
            let Some(source_id_str) = obj_val.get("id").and_then(|v| v.as_str()) else {
                not_copied.insert(
                    create_id,
                    json!({ "type": "invalidProperties", "description": "id (source) is required" }),
                );
                continue;
            };
            let source_id = match jmap_server::parse_wire_id(source_id_str, "id") {
                Ok(id) => id,
                Err(err) => {
                    not_copied.insert(create_id, set_error_value(&err));
                    continue;
                }
            };

            // Fetch the source node.
            let (mut nodes, not_found): (Vec<FileNode>, _) = backend
                .get_objects::<FileNode>(
                    caller,
                    &from_account_id,
                    Some(std::slice::from_ref(&source_id)),
                    None,
                )
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            if !not_found.is_empty() || nodes.is_empty() {
                not_copied.insert(create_id, json!({ "type": "notFound" }));
                continue;
            }

            let source_node = nodes.remove(0);

            // Validate parentId override shape before the generic merge:
            // parentId must be a string or null. Silently coercing a
            // non-string non-null value to None would surface as a
            // 'moved to root' side effect with no error signal.
            if let Some(new_parent) = obj_val.get("parentId")
                && !new_parent.is_null()
                && new_parent.as_str().is_none()
            {
                not_copied.insert(
                    create_id,
                    json!({
                        "type": "invalidProperties",
                        "properties": ["parentId"],
                        "description": "parentId must be a string Id or null",
                    }),
                );
                continue;
            }

            // RFC 8620 §5.4: "a copy of the source object with the given
            // properties overridden". draft-ietf-jmap-filenode-16 §3.2.4
            // references this verbatim. Apply ALL override fields from
            // obj_val on top of the source, not just (parentId, name).
            // The 'id' key is the source reference, not a write target —
            // skip it. Excluded fields that the server must own:
            //   - id: the destination assigns the new id
            //   - created/modified/accessed/changed: server timestamps
            //   - size: derived from blobId on the destination
            //   - myRights: derived from caller's permissions on the dst
            //
            // Round-trip: serialize source_node, apply override map, then
            // deserialize back to FileNode. A bad override surfaces as an
            // invalidProperties SetError instead of corrupting state.
            let mut source_node = match merge_filenode_overrides(source_node, &obj_val) {
                Ok(n) => n,
                Err(err_obj) => {
                    not_copied.insert(create_id, err_obj);
                    continue;
                }
            };
            // draft-ietf-jmap-filenode-16 §3.2.4: "the same additional
            // errors as FileNode/set apply". Checked before any onExists
            // destroy so an invalid copy has no side effects.
            if let Some(err) = create_type_error(&source_node) {
                not_copied.insert(create_id, err);
                continue;
            }
            // draft-atwood-jmap-cid-00 §FileNode Extension: `sha256` is
            // server-set ("MUST be `null` for directory FileNodes"; for a
            // file, the digest of the stored blob), so neither the source's
            // nor an override's value is kept. Computed before any onExists
            // destroy so a failure has no side effects.
            source_node.sha256 = None;
            if matches!(source_node.node_type.as_ref(), Some(NodeType::File))
                && let Some(ref blob_id) = source_node.blob_id
            {
                match backend.blob_sha256(caller, &account_id, blob_id).await {
                    Ok(digest) => source_node.sha256 = digest,
                    Err(e) => {
                        not_copied.insert(create_id, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }
            // Same-account cycle check (bd:JMAP-510h.59): when
            // fromAccountId == accountId and the copy descriptor moves
            // the new node under one of the source node's own
            // descendants, the user's intent ("a copy that mirrors the
            // source as a subtree") is silently violated. RFC 8620 §5.4
            // does not prohibit same-account copies, but filenode draft
            // §3.2.3 nodeHasChildren / cycle semantics apply to the
            // resulting tree. Mirror the FileNode/set update cycle
            // guard. Skipped for cross-account copies because the
            // destination tree is unrelated to the source.
            if from_account_id == account_id
                && let Some(ref new_parent) = source_node.parent_id
            {
                if new_parent == &source_id {
                    not_copied.insert(
                        create_id,
                        json!({
                            "type": "invalidProperties",
                            "properties": ["parentId"],
                            "description": "a node cannot be its own parent"
                        }),
                    );
                    continue;
                }
                match backend
                    .get_descendant_ids(caller, &from_account_id, &source_id)
                    .await
                {
                    Ok(descendants) => {
                        if descendants.iter().any(|d| d == new_parent) {
                            not_copied.insert(
                                    create_id,
                                    json!({
                                        "type": "invalidProperties",
                                        "properties": ["parentId"],
                                        "description": "copying this node under its own descendant would create a cycle"
                                    }),
                                );
                            continue;
                        }
                    }
                    Err(e) => {
                        not_copied.insert(create_id, server_fail_value_from_backend(&e));
                        continue;
                    }
                }
            }

            // Tracks the rename applied by OnExists::Rename so the
            // post-create response can enforce the spec's MUST that the
            // renamed name appears in the response (§3.2.3 lines 687-690)
            // even if the backend echoes a different name.
            let mut renamed_to: Option<String> = None;

            // -------------------------------------------------------------------
            // Collision detection in the DESTINATION account (§3.2.4 →
            // §3.2.3 onExists semantics). Mirrors handle_filenode_set's
            // collision block; the key difference is that account_id is
            // the destination, NOT from_account_id.
            // -------------------------------------------------------------------
            let node_name = source_node.name.clone();
            let parent_id_for_collision: Option<Id> = source_node.parent_id.clone();

            if !node_name.is_empty() {
                let collision = match backend
                    .find_sibling_by_name(
                        caller,
                        &account_id,
                        parent_id_for_collision.as_ref(),
                        &node_name,
                        case_folding,
                    )
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        not_copied.insert(create_id, server_fail_value_from_backend(&e));
                        continue;
                    }
                };

                if let Some(existing_id) = collision {
                    match on_exists {
                        OnExists::Reject => {
                            not_copied.insert(
                                create_id,
                                json!({ "type": "alreadyExists", "existingId": existing_id.as_ref() }),
                            );
                            continue;
                        }
                        OnExists::Replace => {
                            // §3.2.3 lines 565-570: nodeHasChildren UNLESS
                            // onDestroyRemoveChildren=true; in which case
                            // cascade-destroy descendants first.
                            let desc_ids = match backend
                                .get_descendant_ids(caller, &account_id, &existing_id)
                                .await
                            {
                                Ok(ids) => ids,
                                Err(e) => {
                                    not_copied
                                        .insert(create_id, server_fail_value_from_backend(&e));
                                    continue;
                                }
                            };
                            if !desc_ids.is_empty() && !on_destroy_remove_children {
                                not_copied.insert(
                                    create_id,
                                    json!({ "type": "nodeHasChildren", "existingId": existing_id.as_ref() }),
                                );
                                continue;
                            }
                            let mut cascade_failed = false;
                            for desc_id in &desc_ids {
                                match backend
                                    .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                    .await
                                {
                                    Ok(()) => {
                                        mutated = true;
                                        destroyed.push(desc_id.clone());
                                    }
                                    Err(e) => {
                                        not_copied
                                            .insert(create_id.clone(), backend_set_error_value(e));
                                        cascade_failed = true;
                                        break;
                                    }
                                }
                            }
                            if cascade_failed {
                                continue;
                            }
                            // Now destroy the colliding node itself.
                            match backend
                                .destroy_object::<FileNode>(caller, &account_id, &existing_id)
                                .await
                            {
                                Ok(()) => {
                                    mutated = true;
                                    destroyed.push(existing_id.clone());
                                }
                                Err(e) => {
                                    not_copied.insert(create_id, backend_set_error_value(e));
                                    continue;
                                }
                            }
                            // Fall through to create the new node.
                        }
                        OnExists::Rename => {
                            // Find a non-colliding name by appending a
                            // counter suffix; matches handle_filenode_set's
                            // 100-attempt bound.
                            let mut renamed = false;
                            let mut rename_error = false;
                            for suffix in 1_u32..=100 {
                                let candidate = format!("{node_name}-{suffix}");
                                match backend
                                    .find_sibling_by_name(
                                        caller,
                                        &account_id,
                                        parent_id_for_collision.as_ref(),
                                        &candidate,
                                        case_folding,
                                    )
                                    .await
                                {
                                    Ok(None) => {
                                        source_node.name = candidate.clone();
                                        renamed_to = Some(candidate);
                                        renamed = true;
                                        break;
                                    }
                                    Ok(Some(_)) => continue,
                                    Err(e) => {
                                        not_copied.insert(
                                            create_id.clone(),
                                            server_fail_value_from_backend(&e),
                                        );
                                        rename_error = true;
                                        break;
                                    }
                                }
                            }
                            if rename_error {
                                continue;
                            }
                            if !renamed {
                                not_copied.insert(
                                    create_id,
                                    json!({
                                        "type": "alreadyExists",
                                        "description": "no available name within 100 rename attempts",
                                    }),
                                );
                                continue;
                            }
                        }
                        OnExists::Newest => {
                            // draft-ietf-jmap-filenode-16 §3.2.3: compare
                            // modified timestamps. Incoming strictly later →
                            // replace; otherwise → alreadyExists.
                            //
                            // Note: source_node.modified is already a typed
                            // Option<UTCDate> (deserialized from the source
                            // account). The `modified` field is server-owned
                            // (listed in SERVER_OWNED at
                            // merge_filenode_overrides), so any client override
                            // in the copy descriptor is silently dropped — the
                            // comparison always uses the source node's original
                            // timestamp.
                            let incoming_modified = source_node
                                .modified
                                .as_ref()
                                .map(|d| d.as_ref())
                                .unwrap_or("");
                            let (existing_nodes, _): (Vec<FileNode>, _) = backend
                                .get_objects::<FileNode>(
                                    caller,
                                    &account_id,
                                    Some(std::slice::from_ref(&existing_id)),
                                    None,
                                )
                                .await
                                .map_err(|e| server_fail_from_backend(&e))?;
                            let existing_modified = existing_nodes
                                .first()
                                .and_then(|n| n.modified.as_ref())
                                .map(|d| d.as_ref())
                                .unwrap_or("");
                            if incoming_modified > existing_modified {
                                // Incoming is strictly later — proceed with
                                // replace (same cascade as OnExists::Replace).
                            } else {
                                not_copied.insert(
                                    create_id,
                                    json!({ "type": "alreadyExists", "existingId": existing_id.as_ref() }),
                                );
                                continue;
                            }
                            let desc_ids = match backend
                                .get_descendant_ids(caller, &account_id, &existing_id)
                                .await
                            {
                                Ok(ids) => ids,
                                Err(e) => {
                                    not_copied
                                        .insert(create_id, server_fail_value_from_backend(&e));
                                    continue;
                                }
                            };
                            if !desc_ids.is_empty() && !on_destroy_remove_children {
                                not_copied.insert(
                                    create_id,
                                    json!({ "type": "nodeHasChildren", "existingId": existing_id.as_ref() }),
                                );
                                continue;
                            }
                            let mut cascade_failed = false;
                            for desc_id in &desc_ids {
                                match backend
                                    .destroy_object::<FileNode>(caller, &account_id, desc_id)
                                    .await
                                {
                                    Ok(()) => {
                                        mutated = true;
                                        destroyed.push(desc_id.clone());
                                    }
                                    Err(e) => {
                                        not_copied
                                            .insert(create_id.clone(), backend_set_error_value(e));
                                        cascade_failed = true;
                                        break;
                                    }
                                }
                            }
                            if cascade_failed {
                                continue;
                            }
                            match backend
                                .destroy_object::<FileNode>(caller, &account_id, &existing_id)
                                .await
                            {
                                Ok(()) => {
                                    mutated = true;
                                    destroyed.push(existing_id.clone());
                                }
                                Err(e) => {
                                    not_copied.insert(create_id, backend_set_error_value(e));
                                    continue;
                                }
                            }
                            // Fall through to create the new node.
                        }
                    }
                }
            }

            // Create in the destination account, under the id it assigns.
            let build = move |id: &Id| {
                source_node.id = id.clone();
                source_node
            };
            match backend
                .create_object::<FileNode, _>(caller, &account_id, &create_id, build)
                .await
            {
                Ok((_new_id, created_obj)) => {
                    mutated = true;
                    let mut value = serde_json::to_value(&created_obj)
                        .expect("derive(Serialize) on plain data is infallible");
                    // §3.2.3 lines 687-690 MUST: enforce the renamed
                    // name in the response regardless of whether the
                    // backend echoed it.
                    if let Value::Object(ref mut m) = value {
                        if let Some(name) = renamed_to.as_deref() {
                            m.insert("name".to_owned(), Value::String(name.to_owned()));
                        }
                        if cid_advertised {
                            m.entry("sha256").or_insert(Value::Null);
                        }
                    }
                    report_created_files_rights(
                        backend,
                        caller,
                        &account_id,
                        &created_obj,
                        &mut value,
                    )
                    .await;
                    copied_source_ids.push((create_id.clone(), source_id));
                    copied.insert(create_id, value);
                }
                Err(e) => {
                    not_copied.insert(create_id, backend_set_error_value(e));
                }
            }
        }
    }

    let new_state = if mutated {
        backend
            .get_state::<FileNode>(caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
    } else {
        old_state.clone()
    };

    let mut resp = json!({
        "fromAccountId": from_account_id.as_ref(),
        "accountId": account_id.as_ref(),
        "oldState": old_state.as_ref(),
        "newState": new_state.as_ref(),
        "created":   if copied.is_empty()     { Value::Null } else { Value::Object(copied) },
        "notCreated": if not_copied.is_empty() { Value::Null } else { Value::Object(not_copied) },
    });
    if !destroyed.is_empty() {
        resp["destroyed"] = json!(destroyed);
    }

    // RFC 8620 §5.4 onSuccessDestroyOriginal: after the Foo/copy response,
    // emit a single implicit Foo/set call to destroy each successfully copied
    // source record. The dispatcher appends the returned `Invocation` to
    // methodResponses verbatim (RFC 8620 §6.3), so we build the full Foo/set
    // RESPONSE object here, not request args.
    //
    // Honors the top-level `onDestroyRemoveChildren` flag for the implicit
    // destroys, matching draft-ietf-jmap-filenode-16 §3.2.4's "with the same
    // behaviour" reference to FileNode/set (§3.2.3 nodeHasChildren semantics).
    let mut extra: Vec<Invocation> = Vec::new();
    if on_success_destroy_original && !copied_source_ids.is_empty() {
        let from_old_state = backend
            .get_state::<FileNode>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        // RFC 8620 §5.4: `destroyFromIfInState` "is passed on as the
        // "ifInState" argument to the implicit "Foo/set" call"; per §5.3 a
        // mismatch aborts that whole call with a method-level stateMismatch
        // error in place of the FileNode/set response. Nothing is destroyed.
        if let Some(if_in_state) = args.get("destroyFromIfInState").and_then(Value::as_str)
            && if_in_state != from_old_state.as_ref()
        {
            let err = json!({ "type": "stateMismatch" });
            return Ok(HandlerOutput::new(resp).with_extra(vec![Invocation::new(
                "error",
                err,
                call_id.to_owned(),
            )]));
        }

        let mut destroyed_ids: Vec<Value> = Vec::new();
        let mut not_destroyed = serde_json::Map::new();

        for (_, source_id) in &copied_source_ids {
            // Honor onDestroyRemoveChildren for the implicit destroy: if the
            // source has descendants and the flag is false, the destroy fails
            // with nodeHasChildren. If true, cascade-destroy descendants first.
            let desc_ids: Vec<Id> = backend
                .get_descendant_ids(caller, &from_account_id, source_id)
                .await
                .map_err(|e| server_fail_from_backend(&e))?;

            // Unlike FileNode/set destroy which allows children to pass when
            // all children are also in the same destroy batch, this implicit
            // destroy has no client-controlled batch — onSuccessDestroyOriginal
            // destroys only the copied source nodes, so the all-covered check
            // is inapplicable.
            if !desc_ids.is_empty() && !on_destroy_remove_children {
                not_destroyed.insert(
                    source_id.as_ref().to_owned(),
                    json!({ "type": "nodeHasChildren" }),
                );
                continue;
            }

            // Cascade-destroy descendants from deepest first to maintain
            // tree invariants — matches the FileNode/set §3.2.3 behaviour.
            // Cascade-destroyed descendant IDs are intentionally NOT added
            // to destroyed_ids: the implicit /set response reports only
            // the top-level source nodes the client named, not internal
            // cascade details. This differs from FileNode/set destroy
            // which tracks all cascade IDs because the client controls
            // the batch and needs the full accounting.
            let mut cascade_failed = false;
            if on_destroy_remove_children {
                for desc_id in desc_ids.iter().rev() {
                    if let Err(e) = backend
                        .destroy_object::<FileNode>(caller, &from_account_id, desc_id)
                        .await
                    {
                        not_destroyed
                            .insert(source_id.as_ref().to_owned(), backend_set_error_value(e));
                        cascade_failed = true;
                        break;
                    }
                }
            }
            if cascade_failed {
                continue;
            }

            match backend
                .destroy_object::<FileNode>(caller, &from_account_id, source_id)
                .await
            {
                Ok(()) => {
                    destroyed_ids.push(Value::String(source_id.as_ref().to_owned()));
                }
                Err(e) => {
                    not_destroyed.insert(source_id.as_ref().to_owned(), backend_set_error_value(e));
                }
            }
        }

        let from_new_state = backend
            .get_state::<FileNode>(caller, &from_account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;

        let set_resp = json!({
            "accountId": from_account_id.as_ref(),
            "oldState": from_old_state.as_ref(),
            "newState": from_new_state.as_ref(),
            "created": Value::Null,
            "updated": Value::Null,
            "destroyed": if destroyed_ids.is_empty() { Value::Null } else { Value::Array(destroyed_ids) },
            "notCreated": Value::Null,
            "notUpdated": Value::Null,
            "notDestroyed": if not_destroyed.is_empty() { Value::Null } else { Value::Object(not_destroyed) },
        });
        extra.push(Invocation::new(
            "FileNode/set",
            set_resp,
            call_id.to_owned(),
        ));
    }

    Ok(HandlerOutput::new(resp).with_extra(extra))
}

/// Merge override fields from a FileNode/copy create entry over a source node.
///
/// RFC 8620 §5.4 ("a copy of the source object with the given properties
/// overridden") and draft-ietf-jmap-filenode-16 §3.2.4 ("the same behaviour"
/// as FileNode/set). The caller-supplied `override_val` is applied on top
/// of the serialized source. Fields the server owns are excluded:
///
/// - `id`: the destination assigns a new id; the entry's `id` is the
///   source reference, not a write target.
/// - `created` / `modified` / `accessed` / `changed`: server timestamps;
///   the copy gets fresh values from the destination backend.
/// - `size`: derived from `blobId` on the destination, not client-set.
/// - `myRights`: derived from caller's permissions on the destination.
///
/// Returns the merged FileNode on success. On a malformed override
/// (e.g. wrong-shape `role`, malformed `nodeType`), returns an
/// `invalidProperties`-shaped error Value suitable for `notCreated`.
fn merge_filenode_overrides(source: FileNode, override_val: &Value) -> Result<FileNode, Value> {
    // Serialize the source into a mutable Value map.
    let mut merged =
        serde_json::to_value(&source).expect("derive(Serialize) on FileNode is infallible");

    let map = merged
        .as_object_mut()
        .expect("FileNode serializes to a JSON object");

    if let Some(over_obj) = override_val.as_object() {
        // Fields the server owns; never overridable from the client.
        // Note: "id" is handled by the explicit check above (different
        // semantic — source reference, not a server-owned property).
        const SERVER_OWNED: &[&str] = &[
            "created", "modified", "accessed", "changed", "size", "myRights",
        ];
        for (k, v) in over_obj {
            if k == "id" {
                // Skip: caller's "id" is the source reference per the wire shape.
                continue;
            }
            if SERVER_OWNED.contains(&k.as_str()) {
                // Silently ignore — RFC 8620 §5.4 does not let clients
                // override server-owned fields via /copy.
                continue;
            }
            map.insert(k.clone(), v.clone());
        }
    }

    serde_json::from_value::<FileNode>(merged).map_err(|e| {
        json!({
            "type": "invalidProperties",
            "description": format!("override produced invalid FileNode: {e}"),
        })
    })
}

/// Create-time `@type` consistency checks (§3.1 type constraints), shared by
/// `FileNode/set` create and `FileNode/copy` (draft-ietf-jmap-filenode-16
/// §3.2.4: "the same additional errors as FileNode/set apply").
///
/// Returns an `invalidProperties` SetError value naming the offending
/// property, or `None` if the node is acceptable for creation.
fn create_type_error(node: &FileNode) -> Option<Value> {
    let (field, description) = match node.node_type.as_ref() {
        // draft-ietf-jmap-filenode-16 §3.1.1: `@type` MUST be provided on
        // creation; the server does not infer it.
        None => ("@type", "@type is required on create".to_owned()),
        // draft-ietf-jmap-filenode-16 §3.1.1: an @type outside
        // supportedNodeTypes MUST be rejected.
        Some(NodeType::Other(s)) => (
            "@type",
            format!("unsupported @type {s:?}; expected File, Directory, or SymLink"),
        ),
        Some(node_type) => {
            // draft-ietf-jmap-filenode-16 §3.1: setting a property not
            // defined for the node's @type to a non-null value is
            // invalidProperties; a null value (None here) is ignored.
            let other: Vec<&str> = [
                ("blobId", node.blob_id.is_some()),
                ("size", node.size.is_some()),
                ("mediaType", node.media_type.is_some()),
                ("executable", node.executable.is_some()),
                ("role", node.role.is_some()),
                ("target", node.target.is_some()),
            ]
            .into_iter()
            .filter(|(prop, set)| *set && property_owner(prop).as_ref() != Some(node_type))
            .map(|(prop, _)| prop)
            .collect();
            if !other.is_empty() {
                return Some(json!({
                    "type": "invalidProperties",
                    "properties": other,
                    "description": "property not defined for this node's @type",
                }));
            }
            // draft-atwood-jmap-chat-filenode-00 §The `spaceId` Extension
            // Property: `spaceId` is "Absent on all FileNode objects that
            // are not Space file tree roots" (role
            // `chatspace`).
            if node.space_id.is_some() && node.role != Some(NodeRole::ChatSpace) {
                return Some(json!({
                    "type": "invalidProperties",
                    "properties": ["spaceId"],
                    "description": "spaceId is allowed only on a Space file tree root",
                }));
            }
            match node_type {
                NodeType::File if node.blob_id.is_none() => {
                    ("blobId", "file node requires blobId".to_owned())
                }
                NodeType::Symlink if node.target.as_ref().is_none_or(Vec::is_empty) => {
                    ("target", "symlink node requires target".to_owned())
                }
                _ => return None,
            }
        }
    };
    Some(json!({ "type": "invalidProperties", "properties": [field], "description": description }))
}

/// The `@type` that defines a type-specific property
/// (draft-ietf-jmap-filenode-16 §3.1.2-§3.1.4), or `None` for a common or
/// unknown property.
fn property_owner(property: &str) -> Option<NodeType> {
    match property {
        "blobId" | "size" | "mediaType" | "executable" => Some(NodeType::File),
        "role" => Some(NodeType::Directory),
        "target" => Some(NodeType::Symlink),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// FileNode/query
// ---------------------------------------------------------------------------

/// Handle a `FileNode/query` method call (draft-ietf-jmap-filenode-16 §3.2.5).
///
/// `args` is the RFC 8620 §5.5 `/query` request shape (`accountId`, optional
/// `filter`, optional `sort`, optional `position` / `anchor` /
/// `anchorOffset`, optional `limit`, optional `calculateTotal`),
/// augmented with the draft §3.2.5 `depth` non-negative integer argument;
/// the returned `Value` is the §5.5 `/query` response shape (`accountId`,
/// `queryState`, `canCalculateChanges`, `position`, `ids`, optional
/// `total`, optional `limit`).
///
/// Supports the `depth` argument (§3.2.5): when `depth > 0`, the query is
/// recursively expanded by re-querying with `parentId = <matched_id>` for up to
/// `depth` additional levels. IDs are deduplicated across all levels.
///
/// When `depth` is absent, `null`, or `0`, the query is a flat one-liner
/// delegated to [`jmap_server::handle_query`].
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_filenode_query<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_filenode_supported(backend, caller, &args).await?;
    // Extract depth before delegating — the generic handler strips unrecognised args.
    let depth: u64 = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(0);

    // Delegate to the generic query handler for the first (level-0) result.
    // The generic handler owns account_id parsing; we recover it from
    // the response shape if depth > 0 needs it for the recursion call.
    // Matches the canonical jmap-mail-server pass-through pattern.
    let HandlerOutput {
        primary: mut response,
        extra_invocations: tail,
        ..
    } = jmap_server::handle_query::<FileNode, B>(backend, caller, args).await?;

    if depth == 0 {
        return Ok(HandlerOutput::new(response).with_extra(tail));
    }

    let Some(account_id) = response
        .get("accountId")
        .and_then(|v| v.as_str())
        .map(Id::from)
    else {
        return Ok(HandlerOutput::new(response).with_extra(tail));
    };

    // depth > 0: use backend.query_subtree for recursive expansion.
    // query_subtree returns all descendant IDs up to `depth` levels deep.
    // root_ids are the IDs from the initial query result.
    let root_ids: Vec<Id> = response["ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(Id::from)).collect())
        .unwrap_or_default();

    if root_ids.is_empty() {
        return Ok(HandlerOutput::new(response).with_extra(tail));
    }

    // draft-ietf-jmap-filenode-16 §3.2.5: when `depth > 0` the client is
    // requesting an expanded subtree. A backend error here is not optional
    // — silently downgrading to a flat result would surface as a
    // successful depth=N query containing only the level-0 ids, with no
    // way for the client to detect the partiality.
    let descendant_ids = backend
        .query_subtree(caller, &account_id, &root_ids, depth)
        .await
        .map_err(JmapError::from)?;

    // Merge root_ids + descendant_ids, preserving root order then descendant order.
    let mut combined: Vec<Value> = root_ids
        .iter()
        .map(|id| Value::String(id.as_ref().to_owned()))
        .collect();
    for id in &descendant_ids {
        combined.push(Value::String(id.as_ref().to_owned()));
    }
    let total = combined.len() as u64;
    response["ids"] = Value::Array(combined);
    if let Some(t) = response.get_mut("total") {
        *t = Value::Number(total.into());
    }

    Ok(HandlerOutput::new(response).with_extra(tail))
}

// ---------------------------------------------------------------------------
// FileNode/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `FileNode/queryChanges` method call (draft-ietf-jmap-filenode-16 §3.2.6).
///
/// `args` is the RFC 8620 §5.6 `/queryChanges` request shape (`accountId`,
/// optional `filter`, optional `sort`, `sinceQueryState`, optional
/// `maxChanges`, optional `upToId`, optional `calculateTotal`); the
/// returned `Value` is the §5.6 `/queryChanges` response shape
/// (`accountId`, `oldQueryState`, `newQueryState`, optional `total`,
/// `removed`, `added`).
///
/// Returns `(response_args, extra_invocations)`. The extra list is always empty.
pub async fn handle_filenode_query_changes<B: FileNodeBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<HandlerOutput, JmapError> {
    ensure_filenode_supported(backend, caller, &args).await?;
    jmap_server::handle_query_changes::<FileNode, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::memory::{MemoryBackend, MemoryError};
    use crate::{
        BackendChangesError, BackendQueryError, CaseFolding, ChangesResult, GetObject, JmapBackend,
        JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult,
        SetObject,
    };
    use jmap_server::BackendSetError;
    use jmap_types::State;

    // -----------------------------------------------------------------------
    // FaultyBackend — fault-injection wrapper around MemoryBackend
    // -----------------------------------------------------------------------

    /// Thin wrapper around [`MemoryBackend`] that can inject errors or
    /// override behavior for specific backend operations.
    ///
    /// Mirrors the canonical [`jmap-mail-server` `FaultyBackend` pattern
    /// in `tests/common/mod.rs`][canonical] (see workspace AGENTS.md
    /// "Canonical Templates"). Unlike the canonical, this wrapper lives
    /// inside `mod tests` because the src-side unit tests cannot
    /// import from `tests/`.
    ///
    /// # Fault knobs
    ///
    /// - [`inject_get_ancestors_err`][Self::inject_get_ancestors_err]: next
    ///   call to `get_ancestors` returns `Err`.
    /// - [`inject_blob_exists_err`][Self::inject_blob_exists_err]: next call
    ///   to `blob_exists` returns `Err`.
    /// - [`inject_cid_advertised_err`][Self::inject_cid_advertised_err]: next
    ///   call to `cid_advertised` returns `Err`.
    /// - [`inject_files_rights_err`][Self::inject_files_rights_err]: next
    ///   call to `files_rights` returns `Err`.
    /// - [`set_files_rights`][Self::set_files_rights]: persistent rights
    ///   that `files_rights` returns for one node id (`None` otherwise).
    /// - [`inject_query_objects_err`][Self::inject_query_objects_err]: next
    ///   call to `query_objects` returns `Err`.
    /// - [`inject_query_objects_err_after`][Self::inject_query_objects_err_after]:
    ///   first `n` calls to `query_objects` succeed, then the next returns
    ///   `Err`. Used to exercise depth-expansion paths in
    ///   `query_subtree` where level-0 must succeed and the first
    ///   per-level recursion must fail.
    /// - [`set_create_object_override_name`][Self::set_create_object_override_name]:
    ///   persistent override that swaps the `name` field of any created
    ///   object. Simulates a backend that normalises names rather than
    ///   echoing the supplied value. Used to exercise the handler's
    ///   enforcement of the rename MUST
    ///   (draft-ietf-jmap-filenode-16 §3.2.3 lines 687-690).
    ///
    /// Test-only — kept inside `mod tests` rather than the public
    /// reference implementation to avoid promoting "fault-injection"
    /// as part of the stable public API.
    ///
    /// [canonical]: https://github.com/MarkAtwood/crate-jmap/blob/main/crate-jmap-mail-server/tests/common/mod.rs
    struct FaultyBackend {
        inner: MemoryBackend,
        faults: std::sync::Arc<std::sync::Mutex<FaultState>>,
    }

    #[derive(Default)]
    struct FaultState {
        /// When `Some`, the next call to `get_ancestors` returns
        /// `Err(MemoryError::new(msg))`. Cleared on consumption.
        get_ancestors_err: Option<String>,
        /// When `Some`, the next call to `blob_exists` returns
        /// `Err(MemoryError::new(msg))`. Cleared on consumption.
        blob_exists_err: Option<String>,
        /// When `Some`, the next call to `cid_advertised` returns
        /// `Err(MemoryError::new(msg))`. Cleared on consumption.
        cid_advertised_err: Option<String>,
        /// When `Some`, the next call to `files_rights` returns
        /// `Err(MemoryError::new(msg))`. Cleared on consumption.
        files_rights_err: Option<String>,
        /// Rights `files_rights` returns, keyed by node id. Persistent.
        files_rights: std::collections::HashMap<String, jmap_filenode_types::FilesRights>,
        /// When `Some`, `query_objects` returns `Err(MemoryError::new(msg))`
        /// once `query_objects_calls` reaches `after_calls`. The first
        /// matching call fires the fault and clears the entry; subsequent
        /// calls go through to the inner backend unaffected.
        query_objects_err: Option<QueryObjectsErrConfig>,
        /// Running count of `query_objects` invocations on this backend.
        query_objects_calls: u64,
        /// When `Some`, every `create_object` call overrides the `name`
        /// field of the returned object with this string. Persistent
        /// across calls (not consumed) — simulates a backend that
        /// normalises names systematically rather than per-call.
        create_object_override_name: Option<String>,
        /// Running count of `destroy_object` invocations on this backend.
        /// Lets tests positively assert handler-side guard ordering
        /// (e.g. `nodeHasChildren` short-circuit must NOT call destroy
        /// on the parent).
        destroy_object_calls: u64,
        /// Running count of `update_object` invocations on this backend.
        /// Lets tests assert a handler-side guard rejected an update
        /// before it reached the backend.
        update_object_calls: u64,
    }

    struct QueryObjectsErrConfig {
        msg: String,
        /// When `Some`, the fault is this RFC 8620 §5.5 method error
        /// (`BackendQueryError::Method`) instead of `MemoryError::new(msg)`.
        method: Option<JmapError>,
        /// Number of calls to allow through before failing. `0` means the
        /// very next call fails.
        after_calls: u64,
    }

    impl FaultyBackend {
        fn new_with_account(account_id: &str) -> Self {
            Self {
                inner: MemoryBackend::new().with_account(account_id),
                faults: std::sync::Arc::new(std::sync::Mutex::new(FaultState::default())),
            }
        }

        /// Schedule a `MemoryError::new(msg)` for the next `get_ancestors`
        /// call. Single-shot.
        fn inject_get_ancestors_err(&self, msg: &str) {
            self.faults.lock().unwrap().get_ancestors_err = Some(msg.to_owned());
        }

        /// Schedule a `MemoryError::new(msg)` for the next `cid_advertised`
        /// call. Single-shot.
        fn inject_cid_advertised_err(&self, msg: &str) {
            self.faults.lock().unwrap().cid_advertised_err = Some(msg.to_owned());
        }

        /// Schedule a `MemoryError::new(msg)` for the next `files_rights`
        /// call. Single-shot.
        fn inject_files_rights_err(&self, msg: &str) {
            self.faults.lock().unwrap().files_rights_err = Some(msg.to_owned());
        }

        /// Make `files_rights` return `rights` (hand-written wire JSON)
        /// for node `id`. Persistent.
        fn set_files_rights(&self, id: &str, rights: Value) {
            let rights = serde_json::from_value(rights).expect("FilesRights fixture");
            self.faults
                .lock()
                .unwrap()
                .files_rights
                .insert(id.to_owned(), rights);
        }

        /// Schedule a `MemoryError::new(msg)` for the next `blob_exists`
        /// call. Single-shot.
        fn inject_blob_exists_err(&self, msg: &str) {
            self.faults.lock().unwrap().blob_exists_err = Some(msg.to_owned());
        }

        /// Schedule a `MemoryError::new(msg)` for the next `query_objects`
        /// call. Single-shot.
        fn inject_query_objects_err(&self, msg: &str) {
            self.faults.lock().unwrap().query_objects_err = Some(QueryObjectsErrConfig {
                msg: msg.to_owned(),
                method: None,
                after_calls: 0,
            });
        }

        /// Schedule a `MemoryError::new(msg)` for the `n`-th-and-later
        /// `query_objects` call. The first `n` calls succeed; the
        /// (`n`+1)-th fails. Single-shot: only the first matching call
        /// fires the fault.
        fn inject_query_objects_err_after(&self, msg: &str, after_calls: u64) {
            self.faults.lock().unwrap().query_objects_err = Some(QueryObjectsErrConfig {
                msg: msg.to_owned(),
                method: None,
                after_calls,
            });
        }

        /// Like [`Self::inject_query_objects_err_after`], but the fault is
        /// `BackendQueryError::Method(err)`.
        fn inject_query_objects_method_err_after(&self, err: JmapError, after_calls: u64) {
            self.faults.lock().unwrap().query_objects_err = Some(QueryObjectsErrConfig {
                msg: String::new(),
                method: Some(err),
                after_calls,
            });
        }

        /// Persistently override the `name` field of every object
        /// returned from `create_object` with `name`. Not consumed on
        /// call — simulates a backend that normalises names
        /// systematically.
        fn set_create_object_override_name(&self, name: &str) {
            self.faults.lock().unwrap().create_object_override_name = Some(name.to_owned());
        }

        /// Forward to the inner [`MemoryBackend::seed_node`] for fixture
        /// setup. Lets fault-injecting tests pre-populate the tree the
        /// same way the no-fault tests do.
        #[allow(dead_code)]
        fn seed_node(&self, account_id: &str, node: FileNode) {
            self.inner.seed_node(account_id, node);
        }

        /// Running total of `destroy_object` invocations seen by this
        /// backend wrapper. Lets tests positively assert handler-side
        /// guard ordering (e.g. that the `nodeHasChildren` short-circuit
        /// in `FileNode/set` destroy does NOT delegate to the backend
        /// for the parent).
        #[allow(dead_code)]
        fn destroy_object_call_count(&self) -> u64 {
            self.faults.lock().unwrap().destroy_object_calls
        }

        /// Running total of `update_object` invocations seen by this
        /// backend wrapper.
        fn update_object_call_count(&self) -> u64 {
            self.faults.lock().unwrap().update_object_calls
        }
    }

    impl JmapBackend for FaultyBackend {
        type Error = MemoryError;
        type CallerCtx = ();

        async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
            self.inner.account_exists(&(), account_id).await
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            ids: Option<&[Id]>,
            properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            self.inner
                .get_objects::<O>(&(), account_id, ids, properties)
                .await
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
        ) -> Result<State, Self::Error> {
            self.inner.get_state::<O>(&(), account_id).await
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            since_state: &State,
            max_changes: Option<u64>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            self.inner
                .get_changes::<O>(&(), account_id, since_state, max_changes)
                .await
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            params: QueryParams<'_, O>,
        ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
            let QueryParams {
                filter,
                sort,
                limit,
                position,
                anchor,
                anchor_offset,
                ..
            } = params;
            // Single-shot fault: consume the entry only when the
            // configured call count is reached.
            {
                let mut guard = self.faults.lock().unwrap();
                let call_index = guard.query_objects_calls;
                guard.query_objects_calls = guard.query_objects_calls.saturating_add(1);
                let fire = guard
                    .query_objects_err
                    .as_ref()
                    .is_some_and(|cfg| call_index >= cfg.after_calls);
                if fire {
                    return Err(match guard.query_objects_err.take() {
                        Some(QueryObjectsErrConfig {
                            method: Some(err), ..
                        }) => BackendQueryError::Method(err),
                        cfg => BackendQueryError::Other(MemoryError::new(
                            cfg.map(|c| c.msg).unwrap_or_default(),
                        )),
                    });
                }
            }
            self.inner
                .query_objects::<O>(
                    &(),
                    account_id,
                    QueryParams::new()
                        .filter(filter)
                        .sort(sort)
                        .limit(limit)
                        .position(position)
                        .anchor(anchor)
                        .anchor_offset(anchor_offset),
                )
                .await
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            params: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            self.inner.query_changes::<O>(&(), account_id, params).await
        }
    }

    impl FileNodeBackend for FaultyBackend {
        async fn create_object<O, F>(
            &self,
            _caller: &(),
            account_id: &Id,
            create_id: &str,
            build: F,
        ) -> Result<(Id, O), BackendSetError<Self::Error>>
        where
            O: SetObject + Send + Sync,
            F: FnOnce(&Id) -> O + Send,
        {
            // Persistent override: every create returns the same
            // overridden name field. The override is a behavior swap,
            // not a fault, so it does not consume.
            let override_name = self
                .faults
                .lock()
                .unwrap()
                .create_object_override_name
                .clone();
            if let Some(name) = override_name {
                let reshape = move |id: &Id| -> O {
                    let mut v = serde_json::to_value(build(id)).expect("fixture serializes");
                    if let serde_json::Value::Object(ref mut m) = v {
                        m.insert("name".to_owned(), serde_json::Value::String(name));
                    }
                    serde_json::from_value(v).expect("renamed fixture deserializes")
                };
                return self
                    .inner
                    .create_object(&(), account_id, create_id, reshape)
                    .await;
            }
            self.inner
                .create_object(&(), account_id, create_id, build)
                .await
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
            patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            {
                let mut guard = self.faults.lock().unwrap();
                guard.update_object_calls = guard.update_object_calls.saturating_add(1);
            }
            self.inner
                .update_object::<O>(&(), account_id, id, patch)
                .await
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            {
                let mut guard = self.faults.lock().unwrap();
                guard.destroy_object_calls = guard.destroy_object_calls.saturating_add(1);
            }
            self.inner.destroy_object::<O>(&(), account_id, id).await
        }

        fn supports_type<O: JmapObject>(
            &self,
            caller: &Self::CallerCtx,
            account_id: &jmap_types::Id,
        ) -> bool {
            self.inner.supports_type::<O>(caller, account_id)
        }

        async fn get_ancestors(
            &self,
            _caller: &(),
            account_id: &Id,
            ids: &[Id],
        ) -> Result<Vec<FileNode>, Self::Error> {
            // Single-shot: consume the entry on this call.
            if let Some(msg) = self.faults.lock().unwrap().get_ancestors_err.take() {
                return Err(MemoryError::new(msg));
            }
            self.inner.get_ancestors(&(), account_id, ids).await
        }

        async fn get_descendant_ids(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
        ) -> Result<Vec<Id>, Self::Error> {
            self.inner.get_descendant_ids(&(), account_id, id).await
        }

        async fn blob_exists(
            &self,
            _caller: &(),
            account_id: &Id,
            blob_id: &Id,
        ) -> Result<bool, Self::Error> {
            // Single-shot: consume the entry on this call.
            if let Some(msg) = self.faults.lock().unwrap().blob_exists_err.take() {
                return Err(MemoryError::new(msg));
            }
            self.inner.blob_exists(&(), account_id, blob_id).await
        }

        async fn cid_advertised(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
            // Single-shot: consume the entry on this call.
            if let Some(msg) = self.faults.lock().unwrap().cid_advertised_err.take() {
                return Err(MemoryError::new(msg));
            }
            self.inner.cid_advertised(&(), account_id).await
        }

        async fn files_rights(
            &self,
            _caller: &(),
            _account_id: &Id,
            node: &FileNode,
        ) -> Result<Option<jmap_filenode_types::FilesRights>, Self::Error> {
            let mut faults = self.faults.lock().unwrap();
            if let Some(msg) = faults.files_rights_err.take() {
                return Err(MemoryError::new(msg));
            }
            Ok(faults.files_rights.get(node.id.as_ref()).cloned())
        }

        async fn find_sibling_by_name(
            &self,
            _caller: &(),
            account_id: &Id,
            parent_id: Option<&Id>,
            name: &str,
            folding: CaseFolding,
        ) -> Result<Option<Id>, Self::Error> {
            self.inner
                .find_sibling_by_name(&(), account_id, parent_id, name, folding)
                .await
        }
    }

    // -----------------------------------------------------------------------
    // FaultyBackend — wrapper self-tests
    // -----------------------------------------------------------------------
    //
    // These tests verify the FaultyBackend wrapper does what it says. The
    // oracle is the behaviour of the inner MemoryBackend: a fault-injecting
    // call must return Err with the configured message, the next call must
    // succeed by delegating to the inner. The .14.3 migrations exercise the
    // wrapper through the full handler stack; these self-tests pin the
    // single-shot semantics independent of any handler-level assertion.

    /// Oracle: inject_get_ancestors_err fires once, then the next call
    /// delegates to the inner MemoryBackend.
    #[tokio::test]
    async fn faulty_backend_get_ancestors_err_is_single_shot() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.inject_get_ancestors_err("simulated boom");
        let err = backend
            .get_ancestors(&(), &Id::from("acc1"), &[Id::from("any")])
            .await
            .expect_err("first call must surface the injected error");
        assert_eq!(err.description(), "simulated boom");
        // Second call: no fault armed, delegates to inner MemoryBackend
        // which returns Ok(empty) for an unknown id.
        let ok = backend
            .get_ancestors(&(), &Id::from("acc1"), &[Id::from("any")])
            .await
            .expect("second call must delegate to inner");
        assert!(ok.is_empty());
    }

    /// Oracle: inject_blob_exists_err fires once, then the next call
    /// delegates to the inner (which always returns Ok(true) for the
    /// reference impl).
    #[tokio::test]
    async fn faulty_backend_blob_exists_err_is_single_shot() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.inject_blob_exists_err("simulated boom");
        let err = backend
            .blob_exists(&(), &Id::from("acc1"), &Id::from("blob1"))
            .await
            .expect_err("first call must surface the injected error");
        assert_eq!(err.description(), "simulated boom");
        let ok = backend
            .blob_exists(&(), &Id::from("acc1"), &Id::from("blob1"))
            .await
            .expect("second call must delegate to inner");
        assert!(ok);
    }

    /// Oracle: inject_query_objects_err fires on the very next call
    /// (after_calls = 0).
    #[tokio::test]
    async fn faulty_backend_query_objects_err_fires_immediately() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.inject_query_objects_err("simulated boom");
        let err = backend
            .query_objects::<FileNode>(&(), &Id::from("acc1"), QueryParams::new())
            .await
            .expect_err("first call must surface the injected error");
        let BackendQueryError::Other(err) = err else {
            panic!("injected fault must be BackendQueryError::Other");
        };
        assert_eq!(err.description(), "simulated boom");
    }

    /// Oracle: inject_query_objects_err_after(_, 1) lets the first call
    /// through and fires on the second.
    #[tokio::test]
    async fn faulty_backend_query_objects_err_after_one_succeeds_then_fails() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.inject_query_objects_err_after("simulated boom", 1);
        // First call: delegates to inner.
        let _first = backend
            .query_objects::<FileNode>(&(), &Id::from("acc1"), QueryParams::new())
            .await
            .expect("first call must succeed (after_calls = 1)");
        // Second call: fires the fault.
        let err = backend
            .query_objects::<FileNode>(&(), &Id::from("acc1"), QueryParams::new())
            .await
            .expect_err("second call must surface the injected error");
        let BackendQueryError::Other(err) = err else {
            panic!("injected fault must be BackendQueryError::Other");
        };
        assert_eq!(err.description(), "simulated boom");
    }

    /// Oracle: set_create_object_override_name is persistent. Every
    /// create_object call swaps the name field with the override.
    #[tokio::test]
    async fn faulty_backend_create_object_name_override_is_persistent() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.set_create_object_override_name("backend-normalised");
        let fixture = serde_json::from_value::<FileNode>(json!({
            "id": "ignored-on-create",
            "name": "client-supplied",
            "parentId": null,
            "role": null,
            "isExecutable": false,
            "isTopLevel": true,
            "@type": "Directory",
            "target": null,
            "blobId": null,
            "size": 0,
            "shareWith": null,
            "myRights": null,
            "annotations": null,
            "created": "1970-01-01T00:00:00Z",
            "modified": "1970-01-01T00:00:00Z",
            "accessed": null
        }))
        .expect("fixture must deserialize");
        let (_id, created) = backend
            .create_object(&(), &Id::from("acc1"), "c0", |id: &Id| {
                let mut node = fixture.clone();
                node.id = id.clone();
                node
            })
            .await
            .expect("create must succeed");
        assert_eq!(
            created.name, "backend-normalised",
            "the override MUST replace the client-supplied name"
        );
        // Persistent: a second create also gets the override.
        let (_id2, second) = backend
            .create_object(&(), &Id::from("acc1"), "c1", |id: &Id| {
                let mut node = fixture;
                node.id = id.clone();
                node
            })
            .await
            .expect("second create must succeed");
        assert_eq!(
            second.name, "backend-normalised",
            "the override MUST persist across calls"
        );
    }

    // -----------------------------------------------------------------------
    // Fixture helpers
    // -----------------------------------------------------------------------

    /// Build a minimal FileNode by JSON round-trip — FileNode is
    /// `#[non_exhaustive]` so its struct literal is unavailable outside
    /// the defining crate. `parent_id_str` is `None` for top-level nodes.
    fn make_filenode(id: &str, parent_id_str: Option<&str>, name: &str) -> FileNode {
        let parent = match parent_id_str {
            Some(s) => serde_json::Value::String(s.to_owned()),
            None => serde_json::Value::Null,
        };
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "parentId": parent,
            "role": null,
            "@type": "Directory",
            "target": null,
            "blobId": null,
            "size": null,
            "shareWith": null,
            "myRights": null,
            "created": "1970-01-01T00:00:00Z",
            "modified": "1970-01-01T00:00:00Z",
            "accessed": null
        }))
        .expect("make_filenode fixture must deserialize")
    }

    // -----------------------------------------------------------------------
    // Capability gate — supports_type::<FileNode>(caller, account_id) => false
    // -----------------------------------------------------------------------

    /// Thin wrapper around MemoryBackend whose
    /// `supports_type::<FileNode>(caller, account_id)` returns `false`, simulating a
    /// deployment that has opted out of the FileNode capability.
    /// Every other method delegates to the inner backend (and must
    /// never be reached when the gate works).
    ///
    /// Mirrors the canonical jmap-mail-server NoSnippetBackend
    /// pattern (tests/integration.rs:490).
    struct NoFileNodeBackend(MemoryBackend);

    impl JmapBackend for NoFileNodeBackend {
        type Error = MemoryError;
        type CallerCtx = ();

        async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
            self.0.account_exists(&(), account_id).await
        }

        async fn get_objects<O: GetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            ids: Option<&[Id]>,
            properties: Option<&[String]>,
        ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
            self.0
                .get_objects::<O>(&(), account_id, ids, properties)
                .await
        }

        async fn get_state<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
        ) -> Result<State, Self::Error> {
            self.0.get_state::<O>(&(), account_id).await
        }

        async fn get_changes<O: JmapObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            since_state: &State,
            max_changes: Option<u64>,
        ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
            self.0
                .get_changes::<O>(&(), account_id, since_state, max_changes)
                .await
        }

        async fn query_objects<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            params: QueryParams<'_, O>,
        ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
            self.0.query_objects::<O>(&(), account_id, params).await
        }

        async fn query_changes<O: QueryObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            params: QueryChangesParams<'_, O>,
        ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
            self.0.query_changes::<O>(&(), account_id, params).await
        }
    }

    impl FileNodeBackend for NoFileNodeBackend {
        async fn create_object<O, F>(
            &self,
            _caller: &(),
            account_id: &Id,
            create_id: &str,
            build: F,
        ) -> Result<(Id, O), BackendSetError<Self::Error>>
        where
            O: SetObject + Send + Sync,
            F: FnOnce(&Id) -> O + Send,
        {
            self.0
                .create_object(&(), account_id, create_id, build)
                .await
        }

        async fn update_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
            patch: O::Patch,
        ) -> Result<Option<O>, BackendSetError<Self::Error>> {
            self.0.update_object::<O>(&(), account_id, id, patch).await
        }

        async fn destroy_object<O: SetObject + Send + Sync>(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
        ) -> Result<(), BackendSetError<Self::Error>> {
            self.0.destroy_object::<O>(&(), account_id, id).await
        }

        fn supports_type<O: JmapObject>(
            &self,
            caller: &Self::CallerCtx,
            account_id: &jmap_types::Id,
        ) -> bool {
            // Opt out of FileNode on every account except "acc2";
            // everything else delegates.
            if std::any::TypeId::of::<O>() == std::any::TypeId::of::<FileNode>()
                && account_id.as_ref() != "acc2"
            {
                false
            } else {
                self.0.supports_type::<O>(caller, account_id)
            }
        }

        async fn get_ancestors(
            &self,
            _caller: &(),
            account_id: &Id,
            ids: &[Id],
        ) -> Result<Vec<FileNode>, Self::Error> {
            self.0.get_ancestors(&(), account_id, ids).await
        }

        async fn get_descendant_ids(
            &self,
            _caller: &(),
            account_id: &Id,
            id: &Id,
        ) -> Result<Vec<Id>, Self::Error> {
            self.0.get_descendant_ids(&(), account_id, id).await
        }

        async fn blob_exists(
            &self,
            _caller: &(),
            account_id: &Id,
            blob_id: &Id,
        ) -> Result<bool, Self::Error> {
            self.0.blob_exists(&(), account_id, blob_id).await
        }

        async fn find_sibling_by_name(
            &self,
            _caller: &(),
            account_id: &Id,
            parent_id: Option<&Id>,
            name: &str,
            folding: CaseFolding,
        ) -> Result<Option<Id>, Self::Error> {
            self.0
                .find_sibling_by_name(&(), account_id, parent_id, name, folding)
                .await
        }
    }

    /// Oracle: every FileNode/* handler MUST short-circuit with
    /// `accountNotSupportedByMethod` when the backend's
    /// `supports_type::<FileNode>(caller, account_id)` returns `false`. Without this
    /// gate, register_filenode_handlers would route the request into
    /// the handler and the handler would call backend methods on a
    /// type the backend has opted out of (bd:JMAP-510h.43 +
    /// bd:JMAP-510h.34). The gate mirrors the canonical
    /// jmap-mail-server snippet.rs:35 pattern.
    #[tokio::test]
    async fn all_handlers_gate_on_supports_type_false() {
        let backend = NoFileNodeBackend(MemoryBackend::new().with_account("acc1"));

        let cases: &[(&str, &dyn Fn(serde_json::Value) -> serde_json::Value)] = &[
            (
                "FileNode/get",
                &|_| json!({"accountId": "acc1", "ids": null}),
            ),
            (
                "FileNode/changes",
                &|_| json!({"accountId": "acc1", "sinceState": "0"}),
            ),
            (
                "FileNode/set",
                &|_| json!({"accountId": "acc1", "destroy": []}),
            ),
            (
                "FileNode/copy",
                &|_| json!({"fromAccountId": "acc1", "accountId": "acc1", "create": {}}),
            ),
            (
                "FileNode/query",
                &|_| json!({"accountId": "acc1", "filter": null, "sort": null}),
            ),
            (
                "FileNode/queryChanges",
                &|_| json!({"accountId": "acc1", "sinceQueryState": "0"}),
            ),
        ];

        for (method, build_args) in cases {
            let args = build_args(json!(null));
            let err = match *method {
                "FileNode/get" => handle_filenode_get(&backend, &(), args).await.err(),
                "FileNode/changes" => handle_filenode_changes(&backend, &(), args).await.err(),
                "FileNode/set" => handle_filenode_set(&backend, &(), args).await.err(),
                "FileNode/copy" => handle_filenode_copy(&backend, &(), args, "c0").await.err(),
                "FileNode/query" => handle_filenode_query(&backend, &(), args).await.err(),
                "FileNode/queryChanges" => handle_filenode_query_changes(&backend, &(), args)
                    .await
                    .err(),
                _ => panic!("unknown method: {method}"),
            };
            let err =
                err.unwrap_or_else(|| panic!("{method} must error when FileNode is unsupported"));
            assert_eq!(
                err.error_type.to_string(),
                "accountNotSupportedByMethod",
                "{method}: must return accountNotSupportedByMethod, got: {:?}",
                err.error_type
            );
        }
    }

    /// Oracle: RFC 8620 §3.6.2 — "accountNotSupportedByMethod: The
    /// accountId given corresponds to a valid account, but the account
    /// does not support this method or data type." An unknown account is
    /// therefore `accountNotFound`, even on a backend that opts out of
    /// FileNode for it.
    #[tokio::test]
    async fn gate_reports_unknown_account_as_account_not_found() {
        let backend = NoFileNodeBackend(MemoryBackend::new().with_account("acc2"));
        let cases = [
            "FileNode/get",
            "FileNode/changes",
            "FileNode/set",
            "FileNode/copy",
            "FileNode/query",
            "FileNode/queryChanges",
        ];
        for method in cases {
            let args = json!({
                "accountId": "nosuch",
                "fromAccountId": "acc2",
                "ids": null,
                "sinceState": "0",
                "sinceQueryState": "0",
            });
            let err = match method {
                "FileNode/get" => handle_filenode_get(&backend, &(), args).await.err(),
                "FileNode/changes" => handle_filenode_changes(&backend, &(), args).await.err(),
                "FileNode/set" => handle_filenode_set(&backend, &(), args).await.err(),
                "FileNode/copy" => handle_filenode_copy(
                    &backend,
                    &(),
                    json!({"fromAccountId": "acc2", "accountId": "nosuch", "create": {}}),
                    "c0",
                )
                .await
                .err(),
                "FileNode/query" => handle_filenode_query(&backend, &(), args).await.err(),
                "FileNode/queryChanges" => handle_filenode_query_changes(&backend, &(), args)
                    .await
                    .err(),
                _ => panic!("unknown method: {method}"),
            };
            let err = err.unwrap_or_else(|| panic!("{method} must error for unknown account"));
            assert_eq!(
                err.error_type.to_string(),
                "accountNotFound",
                "{method}: got {:?}",
                err.error_type
            );
        }
    }

    /// Oracle: RFC 8620 §1.6.2 / §2 — capabilities are per account, so
    /// opting "acc1" out of FileNode must not affect "acc2".
    #[tokio::test]
    async fn gate_is_per_account() {
        let backend = NoFileNodeBackend(
            MemoryBackend::new()
                .with_account("acc1")
                .with_account("acc2"),
        );
        let HandlerOutput { primary: resp, .. } =
            handle_filenode_get(&backend, &(), json!({"accountId": "acc2", "ids": null}))
                .await
                .expect("acc2 supports FileNode");
        assert_eq!(resp["accountId"], "acc2");
        let err = handle_filenode_get(&backend, &(), json!({"accountId": "acc1", "ids": null}))
            .await
            .expect_err("acc1 opts out of FileNode");
        assert_eq!(err.error_type.to_string(), "accountNotSupportedByMethod");
    }

    /// Oracle: RFC 8620 §5.4 — "fromAccountNotSupportedByMethod: The
    /// fromAccountId given corresponds to a valid account, but the account
    /// does not support this data type." A supported destination with an
    /// opted-out source reports the source side.
    #[tokio::test]
    async fn copy_from_unsupported_account_reports_from_side() {
        let backend = NoFileNodeBackend(
            MemoryBackend::new()
                .with_account("acc1")
                .with_account("acc2"),
        );
        let err = handle_filenode_copy(
            &backend,
            &(),
            json!({"fromAccountId": "acc1", "accountId": "acc2", "create": {}}),
            "c0",
        )
        .await
        .expect_err("source account opts out of FileNode");
        assert_eq!(
            err.error_type.to_string(),
            "fromAccountNotSupportedByMethod"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/get
    // -----------------------------------------------------------------------

    /// Oracle: FileNode/get with unknown accountId returns accountNotFound.
    /// Source: RFC 8620 §3.6.2.
    #[tokio::test]
    async fn get_unknown_account_returns_account_not_found() {
        let backend = MemoryBackend::new();
        let args = json!({ "accountId": "unknown", "ids": null });
        let err = handle_filenode_get(&backend, &(), args)
            .await
            .expect_err("must return error for unknown account");
        assert_eq!(
            err.error_type.to_string(),
            "accountNotFound",
            "got: {:?}",
            err.error_type
        );
    }

    /// Oracle: FileNode/get with known account returns empty list (no nodes seeded).
    #[tokio::test]
    async fn get_known_account_returns_empty_list() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "ids": null });
        let HandlerOutput { primary: resp, .. } = handle_filenode_get(&backend, &(), args)
            .await
            .expect("must succeed for known account");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["list"].as_array().unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // FileNode/get — fetchParents
    // -----------------------------------------------------------------------

    /// Oracle: fetchParents=false → response unchanged (no ancestor expansion).
    /// Source: draft-ietf-jmap-filenode-16 §3.2.1.
    #[tokio::test]
    async fn get_fetch_parents_false_unchanged() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "ids": null, "fetchParents": false });
        let HandlerOutput { primary: resp, .. } = handle_filenode_get(&backend, &(), args)
            .await
            .expect("must succeed");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["list"].as_array().unwrap().is_empty());
    }

    /// Oracle: fetchParents=true with no nodes → list unchanged (no ancestors to fetch).
    /// Source: draft-ietf-jmap-filenode-16 §3.2.1.
    #[tokio::test]
    async fn get_fetch_parents_true_no_nodes() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "ids": null, "fetchParents": true });
        let HandlerOutput { primary: resp, .. } = handle_filenode_get(&backend, &(), args)
            .await
            .expect("must succeed");
        assert!(resp["list"].as_array().unwrap().is_empty());
    }

    /// Oracle: fetchParents=true with a non-empty list, and the backend
    /// returns `Err` from `get_ancestors`: the handler MUST surface the
    /// error as a JMAP serverFail rather than silently returning the
    /// un-expanded list (which would look identical on the wire to a
    /// successful fetchParents=true response where every node is at root).
    ///
    /// Regression for bd JMAP-510h.6 — the prior code used
    /// `if let Ok(ancestors) = ... { ... } // Non-fatal` which discarded
    /// the error.
    #[tokio::test]
    async fn get_fetch_parents_propagates_get_ancestors_err() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Seed one FileNode so the response `list` is non-empty and
        // the fetchParents branch is taken.
        backend.seed_node("acc1", make_filenode("child", None, "child"));
        backend.inject_get_ancestors_err("simulated DB timeout");

        let args = json!({
            "accountId": "acc1",
            "ids": ["child"],
            "fetchParents": true
        });
        let result = handle_filenode_get(&backend, &(), args).await;
        assert!(
            result.is_err(),
            "fetchParents + get_ancestors Err must propagate as JMAP \
             error, not return a silently-truncated list. got Ok: {:?}",
            result.ok()
        );
    }

    /// Oracle: FileNodeBackend::blob_exists contract — `Err(_)` MUST
    /// propagate as a top-level serverFail. Silently treating a backend
    /// error as Ok(false) and returning `invalidProperties: blob not
    /// found` mis-classifies a transient connectivity failure as a
    /// deterministic error, so the client will not retry.
    ///
    /// Regression for bd JMAP-510h.31 — the prior signature returned
    /// bool, leaving the implementor no way to signal Err.
    #[tokio::test]
    async fn set_create_blob_exists_err_propagates_as_server_fail() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.inject_blob_exists_err("simulated S3 timeout");

        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "name": "doc.bin",
                    "parentId": null,
                    "role": null,
                    "@type": "File",
                    "blobId": "B-some-blob"
                }
            }
        });
        let result = handle_filenode_set(&backend, &(), args).await;
        let err = result
            .expect_err("blob_exists Err must surface as a JmapError, not Ok with notCreated");
        assert_eq!(
            err.error_type.to_string(),
            "serverFail",
            "transient backend error MUST surface as serverFail so the \
             client retries; got: {err:?}"
        );
    }

    /// Oracle: FileNodeBackend::cid_advertised contract, "`Err(e)`: the
    /// `FileNode/get` call ... fails with `serverFail`". Dropping the
    /// error would omit the `sha256` that draft-atwood-jmap-cid-00
    /// (jmap-chat-spec 33702a5 L200-202) says "FileNode objects MUST
    /// include" when the capability is advertised.
    #[tokio::test]
    async fn get_cid_advertised_err_propagates_as_server_fail() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("n1", None, "n1"));
        backend.inject_cid_advertised_err("simulated config store timeout");

        let args = json!({ "accountId": "acc1", "ids": ["n1"] });
        let err = handle_filenode_get(&backend, &(), args)
            .await
            .expect_err("cid_advertised Err must surface as a JmapError");
        assert_eq!(err.error_type.to_string(), "serverFail", "{err:?}");
    }

    /// All six draft-ietf-jmap-filenode-16 FilesRights flags, `mayRead`
    /// only: a hand-written policy result.
    fn read_only_rights() -> Value {
        json!({
            "mayRead": true,
            "mayAddChildren": false,
            "mayRename": false,
            "mayDelete": false,
            "mayModifyContent": false,
            "mayShare": false
        })
    }

    /// Oracle: draft-ietf-jmap-filenode-16 L431-434, "myRights:
    /// "FilesRights" (server-set) The set of rights (ACLs) the user has in
    /// relation to this node", and the FileNodeBackend::files_rights
    /// contract: `Some(rights)` is reported as `myRights`, `None` leaves
    /// the stored value.
    #[tokio::test]
    async fn get_reports_files_rights_as_my_rights() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("n1", None, "n1"));
        backend.seed_node("acc1", make_filenode("n2", None, "n2"));
        backend.set_files_rights("n1", read_only_rights());

        let args = json!({ "accountId": "acc1", "ids": ["n1", "n2"] });
        let resp = handle_filenode_get(&backend, &(), args)
            .await
            .expect("get")
            .primary;
        let by_id = |id: &str| {
            resp["list"]
                .as_array()
                .expect("list")
                .iter()
                .find(|n| n["id"] == id)
                .cloned()
                .expect("listed")
        };
        assert_eq!(by_id("n1")["myRights"], read_only_rights(), "{resp}");
        assert_eq!(by_id("n2")["myRights"], Value::Null, "{resp}");
    }

    /// Oracle: draft-ietf-jmap-filenode-16 L431-434 (as above) for every
    /// node in `list`, including the ancestors `fetchParents` appends.
    #[tokio::test]
    async fn get_fetch_parents_reports_ancestor_files_rights() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("p", None, "p"));
        backend.seed_node("acc1", make_filenode("c", Some("p"), "c"));
        backend.set_files_rights("p", read_only_rights());

        let args = json!({ "accountId": "acc1", "ids": ["c"], "fetchParents": true });
        let resp = handle_filenode_get(&backend, &(), args)
            .await
            .expect("get")
            .primary;
        let parent = resp["list"]
            .as_array()
            .expect("list")
            .iter()
            .find(|n| n["id"] == "p")
            .cloned()
            .expect("fetchParents appends p");
        assert_eq!(parent["myRights"], read_only_rights(), "{resp}");
    }

    /// Oracle: RFC 8620 §5.1, `properties` "If supplied, only the
    /// properties listed in the array are returned for each object". With
    /// `myRights` not listed the hook is not asked, so its scheduled Err
    /// never fires and no `myRights` appears.
    #[tokio::test]
    async fn get_without_my_rights_property_skips_files_rights() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("n1", None, "n1"));
        backend.set_files_rights("n1", read_only_rights());
        backend.inject_files_rights_err("simulated policy store timeout");

        let args = json!({ "accountId": "acc1", "ids": ["n1"], "properties": ["name"] });
        let resp = handle_filenode_get(&backend, &(), args)
            .await
            .expect("get")
            .primary;
        assert!(resp["list"][0].get("myRights").is_none(), "{resp}");
    }

    /// Oracle: FileNodeBackend::files_rights contract, "`Err(e)`: the
    /// `FileNode/get` call fails with `serverFail`"; reporting the stored
    /// value instead would misstate the server-set rights.
    #[tokio::test]
    async fn get_files_rights_err_propagates_as_server_fail() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("n1", None, "n1"));
        backend.inject_files_rights_err("simulated policy store timeout");

        let args = json!({ "accountId": "acc1", "ids": ["n1"] });
        let err = handle_filenode_get(&backend, &(), args)
            .await
            .expect_err("files_rights Err must surface as a JmapError");
        assert_eq!(err.error_type.to_string(), "serverFail", "{err:?}");
    }

    /// Oracle: RFC 8620 §5.3, `created` "includes all server-set
    /// properties", and draft-ietf-jmap-filenode-16 L431-434 makes
    /// `myRights` "server-set"; the FileNodeBackend::files_rights contract
    /// reports `Some(rights)` in the `created` entry.
    #[tokio::test]
    async fn set_create_reports_files_rights_as_my_rights() {
        let backend = FaultyBackend::new_with_account("acc1");
        // MemoryBackend names the first node it creates `node-1`.
        backend.set_files_rights("node-1", read_only_rights());

        let args = json!({
            "accountId": "acc1",
            "create": { "d1": { "@type": "Directory", "name": "docs", "parentId": null } }
        });
        let resp = handle_filenode_set(&backend, &(), args)
            .await
            .expect("set")
            .primary;
        assert_eq!(resp["created"]["d1"]["id"], "node-1", "{resp}");
        assert_eq!(
            resp["created"]["d1"]["myRights"],
            read_only_rights(),
            "{resp}"
        );
    }

    /// Oracle: FileNodeBackend::files_rights contract, "After a
    /// `FileNode/set` or `FileNode/copy` create the node is already stored,
    /// so the create stands and `created` reports the `myRights` that
    /// [`Self::create_object`] returned"; the stored value is read back
    /// with `FileNode/get`.
    #[tokio::test]
    async fn set_create_files_rights_err_keeps_create() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.inject_files_rights_err("simulated policy store timeout");

        let args = json!({
            "accountId": "acc1",
            "create": { "d1": { "@type": "Directory", "name": "docs", "parentId": null } }
        });
        let resp = handle_filenode_set(&backend, &(), args)
            .await
            .expect("files_rights Err after the insert must not fail the call")
            .primary;
        let created = &resp["created"]["d1"];
        assert!(created.is_object(), "the create must stand: {resp}");

        let get = handle_filenode_get(
            &backend,
            &(),
            json!({ "accountId": "acc1", "ids": [created["id"]] }),
        )
        .await
        .expect("get")
        .primary;
        assert_eq!(
            created["myRights"], get["list"][0]["myRights"],
            "{resp} {get}"
        );
    }

    /// Oracle: RFC 8620 §5.4, `/copy` `created` is "as for the
    /// `created` argument" of `/set`, which "includes all server-set
    /// properties" (§5.3); draft-ietf-jmap-filenode-16 L431-434 makes
    /// `myRights` "server-set".
    #[tokio::test]
    async fn copy_reports_files_rights_as_my_rights() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("p", None, "p"));
        backend.seed_node("acc1", make_filenode("n1", None, "n1"));
        // Seeded ids are not `node-N`, so the copy is the first `node-1`.
        backend.set_files_rights("node-1", read_only_rights());

        let args = json!({
            "fromAccountId": "acc1",
            "accountId": "acc1",
            "create": { "c1": { "id": "n1", "parentId": "p" } }
        });
        let resp = handle_filenode_copy(&backend, &(), args, "c0")
            .await
            .expect("copy")
            .primary;
        assert_eq!(resp["created"]["c1"]["id"], "node-1", "{resp}");
        assert_eq!(
            resp["created"]["c1"]["myRights"],
            read_only_rights(),
            "{resp}"
        );
    }

    /// Oracle: FileNodeBackend::files_rights contract (as for
    /// `set_create_files_rights_err_keeps_create`), for `FileNode/copy`.
    #[tokio::test]
    async fn copy_files_rights_err_keeps_create() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("p", None, "p"));
        backend.seed_node("acc1", make_filenode("n1", None, "n1"));
        backend.inject_files_rights_err("simulated policy store timeout");

        let args = json!({
            "fromAccountId": "acc1",
            "accountId": "acc1",
            "create": { "c1": { "id": "n1", "parentId": "p" } }
        });
        let resp = handle_filenode_copy(&backend, &(), args, "c0")
            .await
            .expect("files_rights Err after the insert must not fail the call")
            .primary;
        let created = &resp["created"]["c1"];
        assert!(created.is_object(), "the copy must stand: {resp}");

        let get = handle_filenode_get(
            &backend,
            &(),
            json!({ "accountId": "acc1", "ids": [created["id"]] }),
        )
        .await
        .expect("get")
        .primary;
        assert_eq!(
            created["myRights"], get["list"][0]["myRights"],
            "{resp} {get}"
        );
    }

    /// Oracle: FileNodeBackend::cid_advertised contract, "`Err(e)`: ... a
    /// `FileNode/set` call with `create`, fails with `serverFail` (before
    /// any create is applied)"; RFC 8620 §3.6.2 serverFail is a
    /// method-level error, so nothing may be created.
    #[tokio::test]
    async fn set_create_cid_advertised_err_propagates_as_server_fail() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.inject_cid_advertised_err("simulated config store timeout");

        let args = json!({
            "accountId": "acc1",
            "create": { "d1": { "@type": "Directory", "name": "docs", "parentId": null } }
        });
        let err = handle_filenode_set(&backend, &(), args)
            .await
            .expect_err("cid_advertised Err must surface as a JmapError");
        assert_eq!(err.error_type.to_string(), "serverFail", "{err:?}");

        let get = handle_filenode_get(&backend, &(), json!({ "accountId": "acc1", "ids": null }))
            .await
            .expect("get after failed set");
        assert_eq!(get.primary["list"], json!([]), "{}", get.primary);
    }

    // -----------------------------------------------------------------------
    // FileNode/changes
    // -----------------------------------------------------------------------

    /// Oracle: FileNode/changes returns the standard changes response shape.
    #[tokio::test]
    async fn changes_returns_standard_shape() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "sinceState": "0" });
        let HandlerOutput { primary: resp, .. } = handle_filenode_changes(&backend, &(), args)
            .await
            .expect("must succeed");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["created"].is_array());
        assert!(resp["updated"].is_array());
        assert!(resp["destroyed"].is_array());
    }

    // -----------------------------------------------------------------------
    // FileNode/query
    // -----------------------------------------------------------------------

    /// Oracle: FileNode/query returns standard query response shape.
    #[tokio::test]
    async fn query_returns_standard_shape() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "filter": null, "sort": null });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        assert_eq!(resp["accountId"], "acc1");
        assert!(resp["ids"].is_array());
    }

    // ── depth parameter (draft-ietf-jmap-filenode-16 §3.2.5) ────────────────

    /// Oracle: depth absent → flat query, same result as before.
    #[tokio::test]
    async fn query_depth_absent_flat_query() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "filter": null, "sort": null });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed for absent depth");
        assert!(resp["ids"].as_array().unwrap().is_empty());
    }

    /// Oracle: depth=0 → same as absent, no recursion.
    #[tokio::test]
    async fn query_depth_zero_flat_query() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "depth": 0, "filter": null, "sort": null });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed for depth=0");
        assert!(resp["ids"].as_array().unwrap().is_empty());
    }

    /// Oracle: depth=0 with a non-empty account returns ONLY the level-0
    /// match set, NOT the recursive subtree. Regression guard for
    /// bd:JMAP-510h.45 — the trait doc previously claimed
    /// `0 = direct children only` which contradicted both the spec
    /// (draft-ietf-jmap-filenode-16 §3.2.5 "If absent, null, or zero,
    /// do not recurse") and the handler's actual short-circuit. This
    /// test pins down the spec semantics: depth=0 must not include
    /// children even when children exist.
    #[tokio::test]
    async fn query_depth_zero_excludes_children_even_when_present() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("dir1", None, "dir1"));
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));
        backend.seed_node("acc1", make_filenode("child2", Some("dir1"), "child2"));

        let args = json!({
            "accountId": "acc1",
            "depth": 0,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed for depth=0 with seeded children");
        let ids = resp["ids"].as_array().expect("ids must be array");
        assert_eq!(
            ids.len(),
            1,
            "depth=0 must return only the level-0 match (dir1), not its children: {resp}"
        );
        let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(id_strs, vec!["dir1"], "level-0 match must be dir1");
    }

    /// Oracle: depth=1 → initial result plus direct children of matched nodes.
    /// Backend is seeded so that "dir1" has children ["child1", "child2"].
    /// The initial query returns ["dir1"] (via parentId=None/root filter);
    /// depth=1 expansion fetches parentId="dir1" and adds children.
    #[tokio::test]
    async fn query_depth_one_returns_children() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Root-level node returned by the initial query.
        backend.seed_node("acc1", make_filenode("dir1", None, "dir1"));
        // dir1's children.
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));
        backend.seed_node("acc1", make_filenode("child2", Some("dir1"), "child2"));

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed for depth=1");
        let ids = resp["ids"].as_array().expect("ids must be array");
        // dir1 + child1 + child2 = 3 ids total.
        assert_eq!(
            ids.len(),
            3,
            "depth=1 must include dir and its children: {ids:?}"
        );
        let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
        assert!(id_strs.contains(&"dir1"), "dir1 must be in result");
        assert!(id_strs.contains(&"child1"), "child1 must be in result");
        assert!(id_strs.contains(&"child2"), "child2 must be in result");
    }

    /// Oracle: depth=1 with a deep tree stops at 1 level.
    /// child1 has grandchildren, but depth=1 must NOT return them.
    #[tokio::test]
    async fn query_depth_one_stops_at_first_level() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("dir1", None, "dir1"));
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));
        backend.seed_node(
            "acc1",
            make_filenode("grandchild1", Some("child1"), "grandchild1"),
        );

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        let ids = resp["ids"].as_array().expect("ids must be array");
        // dir1 + child1 = 2; grandchild1 must NOT appear.
        assert_eq!(ids.len(), 2, "depth=1 must stop at direct children");
        let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            !id_strs.contains(&"grandchild1"),
            "grandchild must not appear at depth=1"
        );
    }

    /// Oracle: depth=2 → two levels of expansion.
    #[tokio::test]
    async fn query_depth_two_returns_grandchildren() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("dir1", None, "dir1"));
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));
        backend.seed_node(
            "acc1",
            make_filenode("grandchild1", Some("child1"), "grandchild1"),
        );

        let args = json!({
            "accountId": "acc1",
            "depth": 2,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        let ids = resp["ids"].as_array().expect("ids must be array");
        // dir1 + child1 + grandchild1 = 3.
        assert_eq!(ids.len(), 3, "depth=2 must include grandchild");
        let id_strs: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            id_strs.contains(&"grandchild1"),
            "grandchild1 must appear at depth=2"
        );
    }

    /// Oracle: query_subtree with depth=1 returns same results as the manual loop.
    /// Regression test: the refactored implementation must produce identical output.
    #[tokio::test]
    async fn query_depth_one_via_subtree_matches_manual_loop() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("dir1", None, "dir1"));
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));
        backend.seed_node("acc1", make_filenode("child2", Some("dir1"), "child2"));

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_query(&backend, &(), args)
            .await
            .expect("must succeed");
        let ids = resp["ids"].as_array().expect("ids array");
        assert_eq!(ids.len(), 3, "dir1 + child1 + child2 = 3");
    }

    /// Oracle: handle_filenode_query with depth>0 must propagate a backend
    /// failure FROM THE DEPTH-EXPANSION PATH as a JMAP serverFail, not
    /// silently downgrade to a flat (level-0-only) result.
    ///
    /// Setup: seed level-0 (root) so the initial top-level query succeeds,
    /// then arm `query_objects` to fail on its second call (the first
    /// per-level call inside `query_subtree`). The handler must surface
    /// that Err rather than swallowing it and returning the level-0
    /// result.
    ///
    /// Regression for bd JMAP-510h.5 — the prior code used
    /// `match backend.query_subtree(...) { Err(e) => { let _ = e;
    /// return Ok((response, tail)); } }` which made a depth>0 failure
    /// indistinguishable from a depth>0 success with zero descendants.
    #[tokio::test]
    async fn query_depth_gt_zero_propagates_query_subtree_err() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("root", None, "root"));
        // Succeed on the initial top-level query_objects (call 0),
        // fail on the per-level recursion inside query_subtree (call 1).
        backend.inject_query_objects_err_after("simulated DB timeout", 1);

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
            "sort": null
        });
        let result = handle_filenode_query(&backend, &(), args).await;
        assert!(
            result.is_err(),
            "depth>0 + backend failure during expansion must produce \
             JMAP error, not a silently-downgraded flat result. got Ok: {:?}",
            result.ok()
        );
    }

    /// Oracle: RFC 8620 §5.5 (https://www.rfc-editor.org/rfc/rfc8620.txt,
    /// Section 5.5 "/query"):
    ///
    /// > The following additional errors may be returned instead of the
    /// > "Foo/query" response:
    /// >
    /// > "unsupportedFilter": The "filter" is syntactically valid, but the
    /// > server cannot process it.
    ///
    /// A backend `BackendQueryError::Method` raised while `query_subtree`
    /// expands `depth > 0` (draft-ietf-jmap-filenode-16 §3.2.5) MUST reach
    /// the client unchanged, not as `serverFail` (bd JMAP-yt1w.24).
    #[tokio::test]
    async fn query_depth_gt_zero_passes_backend_method_error_through() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("root", None, "root"));
        let mut injected = JmapError::unsupported_filter();
        injected.description = Some("backend cannot expand this subtree".into());
        // Call 0 is the top-level query; call 1 is the first per-level
        // query inside query_subtree.
        backend.inject_query_objects_method_err_after(injected, 1);

        let args = json!({
            "accountId": "acc1",
            "depth": 1,
            "filter": {"isTopLevel": true},
        });
        let err = handle_filenode_query(&backend, &(), args)
            .await
            .expect_err("a backend method error during expansion must fail the call");

        assert_eq!(
            serde_json::to_value(&err).expect("JmapError must serialize"),
            json!({
                "type": "unsupportedFilter",
                "description": "backend cannot expand this subtree",
            }),
        );
    }

    /// Oracle: when the backend's `query_objects` returns `Err` on a per-level
    /// recursion call from the default `query_subtree` impl, the error MUST
    /// propagate to the caller. Workspace policy treats silent-drop in a
    /// query result as a server-side correctness bug (workspace AGENTS.md,
    /// Filter algebra exclusion §1).
    ///
    /// Regression for bd JMAP-510h.4 — the prior impl had
    /// `if let Ok(result) = ...` which swallowed transient backend errors
    /// and returned a truncated subtree.
    #[tokio::test]
    async fn query_subtree_default_impl_propagates_query_objects_err() {
        use crate::FileNodeBackend;

        let backend = FaultyBackend::new_with_account("acc1");
        // Seed level 0 so the loop has work to do, then arm the failure
        // injection for the per-level query_objects call.
        backend.seed_node("acc1", make_filenode("root", None, "root"));
        backend.inject_query_objects_err("simulated DB timeout");

        let root_ids = [Id::from("root")];
        let result = backend
            .query_subtree(&(), &Id::from("acc1"), &root_ids, 2)
            .await;
        assert!(
            result.is_err(),
            "query_subtree must propagate query_objects Err, got Ok({:?})",
            result.ok()
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("simulated DB timeout"),
            "propagated error must carry the backend error message, got: {err_msg}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — nodeHasChildren guard
    // -----------------------------------------------------------------------

    /// Oracle: destroy of a node that has children when onDestroyRemoveChildren
    /// is false (the default) returns notDestroyed with type "nodeHasChildren".
    ///
    /// Source: draft-ietf-jmap-filenode-16 §3.2.3.
    ///
    /// Positively asserts handler-side guard ordering: the
    /// `nodeHasChildren` short-circuit MUST fire before any
    /// `destroy_object` call reaches the backend. Without the
    /// `destroy_object_call_count()` check, a future regression where
    /// the guard fires AND the backend is also called (e.g. a
    /// belt-and-suspenders impl that ignores the early return) would
    /// still pass the `notDestroyed.type == "nodeHasChildren"`
    /// assertion. Regression guard for bd:JMAP-510h.24.
    #[tokio::test]
    async fn set_destroy_node_with_children_returns_node_has_children() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Seed one child whose parent_id is dir1, so MemoryBackend's
        // get_descendant_ids(dir1) returns [child1]. dir1 itself is
        // intentionally NOT seeded — the nodeHasChildren guard fires
        // before backend.destroy is called, so dir1's existence does
        // not matter to this assertion.
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));

        let args = json!({
            "accountId": "acc1",
            "destroy": ["dir1"]
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp["destroyed"].is_null(),
            "nothing should be destroyed: {resp}"
        );
        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["dir1"]["type"], "nodeHasChildren",
            "must return nodeHasChildren error: {resp}"
        );
        // Positive guard-ordering assertion: the handler's
        // `nodeHasChildren` short-circuit must fire before any
        // `destroy_object` call reaches the backend.
        assert_eq!(
            backend.destroy_object_call_count(),
            0,
            "nodeHasChildren guard must short-circuit before destroy_object: \
             expected 0 destroy_object calls, got {}",
            backend.destroy_object_call_count()
        );
    }

    /// Oracle: destroy of a node with children when onDestroyRemoveChildren
    /// is true proceeds to the backend (no nodeHasChildren guard).
    #[tokio::test]
    async fn set_destroy_node_with_children_and_flag_true_calls_backend() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Seed one descendant so MemoryBackend's get_descendant_ids
        // returns [child1] — the nodeHasChildren guard would fire if
        // onDestroyRemoveChildren were false. dir1 itself is NOT
        // seeded, so when the handler eventually calls destroy_object,
        // the inner MemoryBackend returns notFound.
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));

        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveChildren": true,
            "destroy": ["dir1"]
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // Backend returned notFound — but it was called (no nodeHasChildren
        // interception). notDestroyed must exist with notFound, not nodeHasChildren.
        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );
        assert_eq!(
            not_destroyed["dir1"]["type"], "notFound",
            "backend error should be notFound (was called), not nodeHasChildren: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — cascade destroy (onDestroyRemoveChildren=true)
    // -----------------------------------------------------------------------

    /// Oracle: onDestroyRemoveChildren=true; descendants go to backend destroy
    /// (notFound from mock), NOT with nodeHasChildren error.
    /// Source: draft-ietf-jmap-filenode-16 §3.2.3.
    #[tokio::test]
    async fn set_destroy_with_remove_children_true_includes_descendants_in_destroyed() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Seed two descendants but NOT dir1. With MemoryBackend, the
        // cascade succeeds on child1+child2 (they exist), then the
        // top-level destroy of dir1 fails with notFound (dir1 never
        // existed). The wire-level result is the same as the original
        // mock-only test: notDestroyed["dir1"]["type"] == "notFound"
        // and nodeHasChildren never appears.
        backend.seed_node("acc1", make_filenode("child1", Some("dir1"), "child1"));
        backend.seed_node("acc1", make_filenode("child2", Some("dir1"), "child2"));

        let args = json!({
            "accountId": "acc1",
            "onDestroyRemoveChildren": true,
            "destroy": ["dir1"]
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let not_destroyed = &resp["notDestroyed"];
        assert!(
            not_destroyed.is_object(),
            "notDestroyed must be present: {resp}"
        );

        assert_ne!(
            not_destroyed["dir1"]["type"].as_str(),
            Some("nodeHasChildren"),
            "nodeHasChildren MUST NOT appear when onDestroyRemoveChildren=true: {resp}"
        );
        // The error must be notFound — either propagated from a
        // cascade failure (mock semantics) or from the top-level
        // destroy of the unseeded dir1 (memory semantics).
        assert_eq!(
            not_destroyed["dir1"]["type"], "notFound",
            "destroy of unseeded dir1 should report notFound: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — circular reference guard
    // -----------------------------------------------------------------------

    /// Oracle: update that sets parentId to a value that would create a cycle
    /// returns notUpdated with invalidProperties on parentId.
    ///
    /// Source: draft-ietf-jmap-filenode-16 §3.2.3 (no cycles constraint).
    #[tokio::test]
    async fn set_update_circular_parent_returns_invalid_properties() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Seed node2 with parent_id=node1, so MemoryBackend's
        // get_descendant_ids(node1) returns [node2]. Moving node1
        // under node2 would create a cycle.
        backend.seed_node("acc1", make_filenode("node2", Some("node1"), "node2"));

        let args = json!({
            "accountId": "acc1",
            "update": {
                "node1": { "parentId": "node2" }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present: {resp}"
        );
        assert_eq!(
            not_updated["node1"]["type"], "invalidProperties",
            "cycle must produce invalidProperties: {resp}"
        );
        let props = &not_updated["node1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("parentId")))
                .unwrap_or(false),
            "parentId must be listed in properties: {resp}"
        );
    }

    /// Oracle: RFC 8620 §5.3 — "invalidProperties": "The record given is
    /// invalid in some way. For example: It contains properties that are
    /// invalid according to the type specification of this record type."
    /// and "The SetError object SHOULD also have a property called
    /// "properties" of type "String[]" that lists *all* the properties
    /// that were invalid." draft-ietf-jmap-filenode-16 §3.1.1 types
    /// parentId as `Id|null`, so a number is a wrong-typed value.
    ///
    /// The handler itself must reject it: the update must not reach
    /// `update_object`, which would otherwise be trusted to catch the
    /// bad type after the cycle guard was skipped.
    #[tokio::test]
    async fn set_update_non_string_parent_id_returns_invalid_properties() {
        let backend = FaultyBackend::new_with_account("acc1");
        backend.seed_node("acc1", make_filenode("node1", None, "node1"));

        let args = json!({
            "accountId": "acc1",
            "update": {
                "node1": { "parentId": 42 }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert_eq!(
            resp["notUpdated"]["node1"]["type"], "invalidProperties",
            "non-string parentId must produce invalidProperties: {resp}"
        );
        assert_eq!(
            resp["notUpdated"]["node1"]["properties"],
            json!(["parentId"]),
            "parentId must be the only listed property: {resp}"
        );
        assert_eq!(
            backend.update_object_call_count(),
            0,
            "handler must reject the wrong-typed parentId before update_object"
        );
    }

    /// Oracle: update that sets parentId equal to the node's own id is the
    /// trivial cycle (a node cannot be its own parent). It must produce
    /// notUpdated with invalidProperties on parentId.
    ///
    /// Source: draft-ietf-jmap-filenode-16 §3.2.3 — "an attempt to move a
    /// node to a parent for which this node is also an ancestor is an
    /// error". A node is trivially an ancestor of itself. The standard
    /// descendant-set check cannot catch this because `get_descendant_ids`
    /// excludes the node itself by contract (backend.rs §76).
    #[tokio::test]
    async fn set_update_self_parent_returns_invalid_properties() {
        let backend = MemoryBackend::new().with_account("acc1");
        // No descendants seeded: the only thing the check has to fall
        // back on is the explicit self-parent guard.
        let args = json!({
            "accountId": "acc1",
            "update": {
                "node1": { "parentId": "node1" }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        let not_updated = &resp["notUpdated"];
        assert!(
            not_updated.is_object(),
            "notUpdated must be present: {resp}"
        );
        assert_eq!(
            not_updated["node1"]["type"], "invalidProperties",
            "self-parent must produce invalidProperties: {resp}"
        );
        let props = &not_updated["node1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("parentId")))
                .unwrap_or(false),
            "parentId must be listed in properties: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — destroy parent + all children in same request
    // -----------------------------------------------------------------------

    /// Oracle: draft-ietf-jmap-filenode-16 §3.2.3 — MUST NOT return
    /// nodeHasChildren when all children of the destroyed node are also in
    /// the same destroy request.
    #[tokio::test]
    async fn set_destroy_parent_and_all_children_succeeds() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Seed node_child with parent_id=node_parent, so
        // get_descendant_ids(node_parent) returns [node_child]. Both
        // are in the destroy request — the all-covered check passes
        // and the handler proceeds to backend.destroy. node_parent
        // itself is unseeded (destroy → notFound) but the test only
        // asserts that nodeHasChildren never appears.
        backend.seed_node(
            "acc1",
            make_filenode("node_child", Some("node_parent"), "node_child"),
        );

        let args = json!({
            "accountId": "acc1",
            "destroy": ["node_parent", "node_child"]
            // onDestroyRemoveChildren is absent (defaults to false)
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        // Both should be in notDestroyed with type=notFound (from mock backend),
        // NOT with type=nodeHasChildren. The all-covered check must pass.
        let not_destroyed = resp["notDestroyed"].as_object();
        if let Some(nd) = not_destroyed {
            for (_, err) in nd {
                assert_ne!(
                    err["type"].as_str(),
                    Some("nodeHasChildren"),
                    "nodeHasChildren MUST NOT appear when all children are in the destroy set"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // FileNode/set — create with missing required field
    // -----------------------------------------------------------------------

    /// Oracle: create with missing `name` returns invalidProperties.
    #[tokio::test]
    async fn set_create_missing_name_returns_invalid_properties() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "@type": "Directory",
                    "parentId": null,
                    "blobId": null,
                    "target": null,
                    "size": null,
                    "shareWith": null
                }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "missing name must produce invalidProperties: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — @type requirement and blobId validation (§3.1)
    // -----------------------------------------------------------------------

    /// Oracle: draft-ietf-jmap-filenode-16 §3.1.1 — `@type` "MUST be provided
    /// on creation", so a create without it (or with it null) is rejected with
    /// invalidProperties naming `@type`, and nothing is created.
    #[tokio::test]
    async fn set_create_without_type_returns_invalid_properties() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "mydir", "role": null },
                "c2": { "@type": null, "name": "other" }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert!(resp["created"].is_null(), "nothing may be created: {resp}");
        for cid in ["c1", "c2"] {
            let err = &resp["notCreated"][cid];
            assert_eq!(err["type"], "invalidProperties", "{cid}: {resp}");
            assert_eq!(err["properties"], json!(["@type"]), "{cid}: {resp}");
        }
    }

    /// Oracle: draft-ietf-jmap-filenode-16 §3.1.1 — an `@type` outside
    /// supportedNodeTypes is invalidProperties.  The -14 lowercase spelling
    /// `"directory"` is not a -16 type name.
    #[tokio::test]
    async fn set_create_unsupported_type_returns_invalid_properties() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": { "c1": { "@type": "directory", "name": "mydir" } }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let err = &resp["notCreated"]["c1"];
        assert_eq!(err["type"], "invalidProperties", "{resp}");
        assert_eq!(err["properties"], json!(["@type"]), "{resp}");
    }

    /// Oracle: draft-ietf-jmap-filenode-16 §3.1.1 / §3.1.3 — a Directory
    /// create with `@type` succeeds and is returned with `@type` and `role`,
    /// and without any File or SymLink property (§3.1).
    #[tokio::test]
    async fn set_create_directory_returns_directory_shape() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "@type": "Directory", "name": "mydir", "blobId": null, "target": null }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        assert!(resp["notCreated"].is_null(), "{resp}");
        let created_obj = resp["created"]["c1"].as_object().expect("created.c1");
        assert_eq!(created_obj["@type"], "Directory", "{resp}");
        assert!(created_obj["role"].is_null(), "{resp}");
        for key in ["blobId", "size", "mediaType", "executable", "target"] {
            assert!(
                !created_obj.contains_key(key),
                "{key} must be absent: {resp}"
            );
        }
    }

    /// Oracle: create with @type="File" and blobId=null → invalidProperties
    /// on "blobId" (file node requires blobId).
    /// Source: draft-ietf-jmap-filenode-16 §3.1.2.
    #[tokio::test]
    async fn set_create_file_without_blobid_returns_invalid_properties() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "myfile", "@type": "File", "blobId": null, "role": null }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "file without blobId must produce invalidProperties: {resp}"
        );
        let props = &not_created["c1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("blobId")))
                .unwrap_or(false),
            "blobId must be listed in properties: {resp}"
        );
    }

    /// Oracle: create with @type="SymLink" and target=null → invalidProperties
    /// Oracle: file node with both blobId and target → invalidProperties on
    /// "target". `target` is meaningful only for symlinks per the spec; a
    /// file carrying `target` is internally inconsistent and would otherwise
    /// be stored verbatim.
    /// Source: draft-ietf-jmap-filenode-16 §3.1 (target field is SymLink-only).
    /// Regression for bd JMAP-510h.7.
    #[tokio::test]
    async fn set_create_file_with_blob_and_target_returns_invalid_properties() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": {
                    "name": "weird",
                    "@type": "File",
                    "blobId": "B-blob-1",
                    "target": ["etc", "passwd"],
                    "role": null
                }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "file with target must produce invalidProperties: {resp}"
        );
        let props = &not_created["c1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("target")))
                .unwrap_or(false),
            "target must be listed in properties: {resp}"
        );
    }

    /// on "target" (symlink node requires target).
    /// Source: draft-ietf-jmap-filenode-16 §3.1.4.
    #[tokio::test]
    async fn set_create_symlink_without_target_returns_invalid_properties() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "name": "mylink", "@type": "SymLink", "target": null, "role": null }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "invalidProperties",
            "symlink without target must produce invalidProperties: {resp}"
        );
        let props = &not_created["c1"]["properties"];
        assert!(
            props
                .as_array()
                .map(|a| a.contains(&json!("target")))
                .unwrap_or(false),
            "target must be listed in properties: {resp}"
        );
    }

    // -----------------------------------------------------------------------
    // FileNode/set — onExists collision detection (§3.2.3)
    // -----------------------------------------------------------------------

    /// Oracle: create when a sibling with the same name exists and onExists is
    /// absent (default Reject) → notCreated with type "alreadyExists".
    /// Source: draft-ietf-jmap-filenode-16 §3.2.3.
    #[tokio::test]
    async fn set_create_collision_reject_returns_already_exists() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Register a sibling named "foo" under root (no parent).
        backend.seed_node("acc1", make_filenode("existing-id-1", None, "foo"));

        let args = json!({
            "accountId": "acc1",
            "create": {
                "c1": { "@type": "Directory", "name": "foo", "parentId": null, "role": null }
            }
            // onExists absent → default Reject
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        let not_created = &resp["notCreated"];
        assert!(
            not_created.is_object(),
            "notCreated must be present: {resp}"
        );
        assert_eq!(
            not_created["c1"]["type"], "alreadyExists",
            "collision must produce alreadyExists: {resp}"
        );
        assert_eq!(
            not_created["c1"]["existingId"], "existing-id-1",
            "existingId must match: {resp}"
        );
    }

    /// Oracle: create with onExists="rename" → name is suffixed to avoid collision.
    /// Source: draft-ietf-jmap-filenode-16 §3.2.3 lines 687-690 — "If the
    /// server changes the name, it MUST include the new 'name' value in the
    /// created or updated response field for this id."
    #[tokio::test]
    async fn set_create_collision_rename_uses_suffixed_name() {
        let backend = FaultyBackend::new_with_account("acc1");
        // "foo" collides; "foo-1" does not.
        backend.seed_node("acc1", make_filenode("existing-id-1", None, "foo"));

        let args = json!({
            "accountId": "acc1",
            "onExists": "rename",
            "create": {
                "c1": { "@type": "Directory", "name": "foo", "parentId": null, "role": null }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");
        // With rename, creation should succeed (mock create_object returns Ok).
        assert!(
            resp["notCreated"].is_null(),
            "rename must not produce notCreated: {resp}"
        );
        assert!(
            resp["created"].is_object() && resp["created"]["c1"].is_object(),
            "created must be present: {resp}"
        );
        // §3.2.3 MUST: the renamed name appears in the response. With the
        // only seeded sibling at "foo", the rename loop's first non-colliding
        // candidate ("foo-1") wins. Asserting on the concrete value catches
        // regressions where the rename branch returns without mutating
        // obj_with_id (the JMAP-510h.26 failure mode).
        let name = resp["created"]["c1"]["name"]
            .as_str()
            .expect("created.c1.name must be present");
        assert_eq!(
            name, "foo-1",
            "rename MUST surface the renamed name in created.c1 (§3.2.3): {resp}"
        );
    }

    /// Oracle: draft-ietf-jmap-filenode-16 §3.2.3 lines 687-690 — "If
    /// the server changes the name, it MUST include the new 'name'
    /// value in the created or updated response field for this id."
    ///
    /// The handler must enforce this MUST regardless of what the
    /// backend echoes back: a backend that normalises names (strips
    /// trailing whitespace, applies NFC, etc.) and returns a value
    /// different from the supplied one would otherwise hide the rename
    /// from the client. Regression for bd JMAP-510h.8.
    #[tokio::test]
    async fn set_create_rename_response_name_overrides_backend_echo() {
        let backend = FaultyBackend::new_with_account("acc1");
        // Pre-seed collision on "foo".
        backend.seed_node("acc1", make_filenode("existing-id-1", None, "foo"));
        // Simulate a backend that returns a different name from what
        // the handler asked it to store. Without the post-create
        // enforcement, the response would carry "backend-normalised"
        // instead of the renamed "foo-1".
        backend.set_create_object_override_name("backend-normalised");

        let args = json!({
            "accountId": "acc1",
            "onExists": "rename",
            "create": {
                "c1": { "@type": "Directory", "name": "foo", "parentId": null, "role": null }
            }
        });
        let HandlerOutput { primary: resp, .. } = handle_filenode_set(&backend, &(), args)
            .await
            .expect("must not return top-level error");

        assert!(
            resp["created"].is_object() && resp["created"]["c1"].is_object(),
            "created must be present: {resp}"
        );
        let name = resp["created"]["c1"]["name"]
            .as_str()
            .expect("created.c1.name must be present");
        assert_eq!(
            name, "foo-1",
            "the renamed name MUST appear in the response (§3.2.3 MUST), \
             not the backend's echoed name: {resp}"
        );
    }

    /// RFC 8620 §5.3 types `destroy` as "Id[]|null", so an object element
    /// is "invalidArguments: One of the arguments is of the wrong type"
    /// (§3.6.2). The description names only the JSON type: §3.6.1 reserves
    /// it for "unstructured English text suitable for displaying to a
    /// developer", so the client-supplied value is never echoed
    /// (bd JMAP-zegc).
    #[tokio::test]
    async fn set_non_string_destroy_element_does_not_echo_value() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "destroy": [{ "note": "CANARY-zegc" }] });
        let err = handle_filenode_set(&backend, &(), args)
            .await
            .expect_err("non-string destroy element must be a method error");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        let desc = err.description.as_deref().unwrap_or_default();
        assert!(desc.contains("got object"), "{desc}");
        assert!(!desc.contains("CANARY"), "{desc}");
    }

    /// draft-ietf-jmap-filenode-16 §3.2.3 types `onExists` as one of
    /// "replace", "rename", "newest" or null, so an object is the wrong type
    /// (RFC 8620 §3.6.2 invalidArguments). The §3.6.1 description names
    /// only the JSON type and never echoes the client value (bd JMAP-zegc).
    #[tokio::test]
    async fn set_invalid_on_exists_does_not_echo_value() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({ "accountId": "acc1", "onExists": { "note": "CANARY-zegc" } });
        let err = handle_filenode_set(&backend, &(), args)
            .await
            .expect_err("object onExists must be a method error");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        let desc = err.description.as_deref().unwrap_or_default();
        assert!(desc.contains("got object"), "{desc}");
        assert!(!desc.contains("CANARY"), "{desc}");
    }

    /// §3.2.4 gives FileNode/copy the same `onExists` argument "with the
    /// same behaviour" as FileNode/set, so the same no-echo rule applies.
    #[tokio::test]
    async fn copy_invalid_on_exists_does_not_echo_value() {
        let backend = MemoryBackend::new().with_account("acc1");
        let args = json!({
            "fromAccountId": "acc1",
            "accountId": "acc1",
            "onExists": { "note": "CANARY-zegc" },
            "create": {}
        });
        let err = handle_filenode_copy(&backend, &(), args, "c0")
            .await
            .expect_err("object onExists must be a method error");
        assert_eq!(err.error_type.to_string(), "invalidArguments");
        let desc = err.description.as_deref().unwrap_or_default();
        assert!(desc.contains("got object"), "{desc}");
        assert!(!desc.contains("CANARY"), "{desc}");
    }
}
