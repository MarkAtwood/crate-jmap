//! Metadata/* method handlers (draft-ietf-jmap-metadata-01 §3).
//!
//! Provides all five JMAP Metadata method handlers:
//! - [`handle_metadata_get`]
//! - [`handle_metadata_changes`]
//! - [`handle_metadata_set`]
//! - [`handle_metadata_query`]
//! - [`handle_metadata_query_changes`]

use jmap_metadata_types::Metadata;
use jmap_types::{Invocation, JmapError};
use serde_json::Value;

use crate::backend::MetadataBackend;

// ---------------------------------------------------------------------------
// Metadata/get
// ---------------------------------------------------------------------------

/// Handle a `Metadata/get` method call (draft-ietf-jmap-metadata-01 §3.2).
///
/// Standard JMAP `/get` per RFC 8620 §5.1. The `ids` argument MAY be `null`
/// to fetch all Metadata objects in the account at once (draft §3.2).
pub async fn handle_metadata_get<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_get::<Metadata, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Metadata/changes
// ---------------------------------------------------------------------------

/// Handle a `Metadata/changes` method call (draft-ietf-jmap-metadata-01 §3.3).
///
/// Standard JMAP `/changes` per RFC 8620 §5.2, plus two Metadata-specific
/// optional arguments:
///
/// - `filterRelatedType: String|null` — restrict the response's created /
///   updated / destroyed arrays to Metadata objects with the given
///   `relatedType`. Does not affect the returned state.
/// - `filterMetadataType: String[]|null` — restrict to Metadata objects
///   whose `@type` value is in the array. Combined with `filterRelatedType`
///   via logical AND.
///
/// **TODO (JMAP-06zp.3.3):** filter post-processing is not yet implemented;
/// the handler currently delegates to the standard `/changes` and ignores
/// the filter arguments.
pub async fn handle_metadata_changes<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // TODO(JMAP-06zp.3.3): consume filterRelatedType / filterMetadataType
    // arguments before delegating, then post-filter created / updated /
    // destroyed in the response per §3.3.
    jmap_server::handlers::handle_changes::<Metadata, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Metadata/set
// ---------------------------------------------------------------------------

/// Handle a `Metadata/set` method call (draft-ietf-jmap-metadata-01 §3.1).
///
/// Standard JMAP `/set` per RFC 8620 §5.3 with the following Metadata-specific
/// server-side constraints (enforced by the backend, surfaced via
/// `BackendSetError::SetError`):
///
/// - **Uniqueness** (§3.1): (relatedType, relatedId, `@type`, isPrivate)
///   tuple MUST be unique within the user's visible set →
///   `alreadyExists{ existingId: ... }` on conflict.
/// - **`maySetPrivate` gating** (§1.2.1): if the account does not permit
///   private metadata and the client supplies `isPrivate: true` →
///   `forbidden`.
/// - **Quota** (§6): `overQuota` if the operation would exceed account quota.
/// - **Related-object validation** (§3.1): `invalidProperties` listing
///   `relatedType` / `relatedId` if the referenced object does not exist.
///
/// All four error categories are reported per-entry in `notCreated` /
/// `notUpdated`; the handler itself is generic across these.
///
/// **TODO (JMAP-06zp.3.3):** real create / update / destroy implementation
/// pending. The handler currently returns `serverFail` for every operation.
pub async fn handle_metadata_set<B: MetadataBackend>(
    _backend: &B,
    _caller: &B::CallerCtx,
    _args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    // TODO(JMAP-06zp.3.3): implement full create / update / destroy semantics.
    Err(JmapError::server_fail(
        "Metadata/set: handler implementation pending (JMAP-06zp.3.3)".to_owned(),
    ))
}

// ---------------------------------------------------------------------------
// Metadata/query
// ---------------------------------------------------------------------------

/// Handle a `Metadata/query` method call (draft-ietf-jmap-metadata-01 §3.4).
///
/// Standard JMAP `/query` per RFC 8620 §5.5. The
/// [`MetadataFilterCondition`](jmap_metadata_types::MetadataFilterCondition)
/// supports `@type`, `relatedType`, `relatedId`/`relatedIds`, `isPrivate`,
/// and `textMatch` operators (§3.4.1). Per §3.4.2 the result is sortable on
/// `id`, `@type`, `relatedType`, `relatedId`, and `isPrivate`.
pub async fn handle_metadata_query<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query::<Metadata, B>(backend, caller, args).await
}

// ---------------------------------------------------------------------------
// Metadata/queryChanges
// ---------------------------------------------------------------------------

/// Handle a `Metadata/queryChanges` method call
/// (draft-ietf-jmap-metadata-01 §3.5).
///
/// Standard JMAP `/queryChanges` per RFC 8620 §5.6.
pub async fn handle_metadata_query_changes<B: MetadataBackend>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
) -> Result<(Value, Vec<Invocation>), JmapError> {
    jmap_server::handlers::handle_query_changes::<Metadata, B>(backend, caller, args).await
}
