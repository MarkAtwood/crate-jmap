# jmap-metadata-server

## What it is

JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/))
method handlers and the `MetadataBackend` trait. Plugs into [`jmap-server`]'s
`Dispatcher`. Implements all five `Metadata/*` method names. Storage-agnostic
— consumers implement the `MetadataBackend` trait for their own data layer.

## What it's for

Implements draft-ietf-jmap-metadata-01 — `Metadata/get`, `/changes`, `/set`,
`/query`, `/queryChanges` — so consumers can attach typed `Annotation`
records (ImapMetadata, WebDavMetadata, vendor extensions) to other JMAP
objects and query them. This is the **workspace-recommended IETF-track
escape** for vendor data that needs to be queryable beyond the workspace
extras-preservation pattern (which round-trips unknown fields but does not
make them filter-targets). Sibling to `jmap-mail-server` (the canonical
extension-server template) and the other `jmap-*-server` crates; all of
them inter-depend through the workspace `JmapHandler` trait in
`jmap-server`. The consumer supplies a `MetadataBackend` impl (storage), a
`CallerCtx` type (auth identity, required for `isPrivate` visibility
scoping), and wires the dispatcher into HTTP / SSE / WebSocket transport
themselves.

## How to use

```rust
use std::sync::Arc;
use jmap_metadata_server::{MetadataBackend, register_metadata_handlers};
use jmap_server::Dispatcher;

// 1. Implement MetadataBackend for your storage layer (see trait section below).
struct MyBackend { /* db pool, etc. */ }
impl MetadataBackend for MyBackend { /* ... */ }

// 2. Wire all 5 Metadata methods into a Dispatcher in one call.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_metadata_handlers(&mut dispatcher, Arc::new(MyBackend { /* ... */ }));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, (), session_state).await;
```

After `register_metadata_handlers` returns, the dispatcher handles every method
name listed in the [Registered methods](#registered-methods) section below. The
same `Arc<MyBackend>` can be shared with other parts of your application.

## Registered methods

All five method names from draft-ietf-jmap-metadata-01 are registered:

| Method | Draft § |
|---|---|
| `Metadata/get` | §3.2 |
| `Metadata/changes` | §3.3 |
| `Metadata/set` | §3.1 |
| `Metadata/query` | §3.4 |
| `Metadata/queryChanges` | §3.5 |

## MetadataBackend trait

Implement this trait to connect the handlers to your storage system. The
read-side methods (`get_objects`, `get_state`, `get_changes`, `query_objects`,
`query_changes`) are defined on the `JmapBackend` supertrait (from
`jmap-server`). `MetadataBackend` adds write operations.

```rust
pub trait MetadataBackend: JmapBackend {
    /// Create a new Metadata object.
    ///
    /// Returns (assigned_id, created_object) on success.
    /// The returned O MUST have its `id` field set to the server-assigned Id.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing Metadata object.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a Metadata object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    fn supports_type<O: JmapObject>(&self) -> bool;
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `alreadyExists`, `overQuota`,
  etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a
  `serverFail` response

## Server-side semantic constraints

Per draft-ietf-jmap-metadata-01 §3.1, the **backend** (not this crate)
enforces:

- **Uniqueness**: the tuple `(relatedType, relatedId, @type, isPrivate)` MUST
  be unique within the user's visible set. Duplicate create →
  `alreadyExists{ existingId: ... }`.
- **`maySetPrivate` gating** (§1.2.1): if the account does not permit private
  metadata and the client supplies `isPrivate: true` → `forbidden`.
- **Quota** (§6): `overQuota` if the operation would exceed account quota.
- **Related-object validation** (§3.1): `invalidProperties` listing
  `relatedType` and/or `relatedId` if the referenced object does not exist.

The handler is generic across these — it routes whatever `SetError` the
backend returns into the per-entry `notCreated` / `notUpdated` /
`notDestroyed` maps in the response.

## How it works

### Registration

`register_metadata_handlers` uses `ClosureHandler` (provided by `jmap-server`)
to wrap each handler function and `Arc<B>` into a `JmapHandler<B::CallerCtx>`
and registers it with the dispatcher. The dispatcher's `CallerCtx` value is
forwarded into each closure as `ctx`, then passed by reference (`&ctx`) into
the standard `handle_*` handler bodies, which themselves forward
`caller: &B::CallerCtx` into every `MetadataBackend` method call. One
`Arc::clone` per method name; no heap allocation per request.

### Handler structure

Each handler function (e.g. `handle_metadata_get`, `handle_metadata_set`)
receives `args: Value` containing the raw JMAP method arguments after
ResultReference resolution, calls `MetadataBackend` methods, and returns the
RFC 8620 response object.

### Metadata/changes — `filterRelatedType` and `filterMetadataType`

Per §3.3 `Metadata/changes` accepts two optional Metadata-specific arguments:

- `filterRelatedType: String|null` — restrict the response's `created`,
  `updated`, and `destroyed` arrays to Metadata objects with the given
  `relatedType`.
- `filterMetadataType: String[]|null` — restrict to Metadata objects whose
  `@type` is in the given list.

When both are present they combine via logical AND. **The state token is not
affected** — it always reflects the complete state of all Metadata objects.
Clients using these filters MUST track the returned state and reuse it with
the same filter arguments on the next call to maintain consistent
synchronisation.

### Permission enforcement

`Metadata/set` supports `create`, `update`, and `destroy` at the wire level.
RFC 8620 §5.3 permits the server to reject any of these with `forbidden`
(e.g. read-only accounts, missing `maySetPrivate` capability). Permission
enforcement is the backend's responsibility: backends return
`BackendSetError::SetError(SetError::new(SetErrorType::Forbidden))` from
`create_object`, `update_object`, or `destroy_object` as appropriate. The
handler routes these `forbidden` errors back to the caller in `notCreated`,
`notUpdated`, or `notDestroyed`.

## Capability URI

Include this in your Session object's `capabilities` map:

```rust
// Re-exported from jmap-metadata-types:
pub const JMAP_METADATA_URI: &str = "urn:ietf:params:jmap:metadata";
```

The per-account `accountCapabilities` value for this URI is a
[`MetadataCapability`](https://docs.rs/jmap-metadata-types) struct declaring
`dataTypes`, `metadataTypes`, `maxDepth`, and `maySetPrivate` per §1.2.1.

## CallerCtx

`register_metadata_handlers` registers each method as a `ClosureHandler` that
forwards the dispatcher's `CallerCtx` value into the closure as `ctx`, which
the closure body then passes by reference (`&ctx`) into the standard
`handle_*` handler. The handler in turn forwards `caller: &B::CallerCtx` into
every `MetadataBackend` (and inherited `JmapBackend`) method call.

To use this:

1. Pick a `CallerCtx` type for your backend — e.g. `()` if no auth context is
   needed, or a struct like `AuthCtx { user_id: Id, scopes: Vec<Scope> }`.
2. Implement `JmapBackend` with `type CallerCtx = AuthCtx;` (the bound is
   `Clone + Send + Sync + 'static`).
3. Use the matching `Dispatcher<AuthCtx>` and pass the constructed auth
   context as the second argument to `Dispatcher::dispatch(request, auth, …)`.
4. Inside `MetadataBackend` method bodies, read `caller: &AuthCtx` to apply
   per-user visibility rules (`isPrivate: true` filtering), quota counting,
   etc.

Backends that don't need an auth identity use `type CallerCtx = ();` and a
`Dispatcher<()>`. Both shapes register the same way.

## `memory` feature (reference implementation)

Enable the `memory` feature in your `Cargo.toml`:

```toml
jmap-metadata-server = { version = "0.1", features = ["memory"] }
```

…to expose `jmap_metadata_server::memory::MemoryBackend`, a complete
in-memory implementation of `MetadataBackend` used by this crate's own
integration tests. Useful for smoke tests, examples, and as a documented
reference when writing a real backend. **Not production.** API stability
is opt-in via this feature and may break across minor versions while the
crate is pre-1.0.

## Gotchas

- The handler does NOT enforce uniqueness, `maySetPrivate`, quota, or
  related-object validation. Those are backend responsibilities per
  draft §3.1. A backend that does not enforce them will silently permit
  duplicate metadata.
- No storage backend ships with this crate by default. The `memory` feature
  provides an in-memory reference implementation suitable for tests and
  examples; it is not suitable for production use.

## Crate family

```
jmap-types
    ├── jmap-server              Dispatcher this plugs into
    └── jmap-metadata-types      domain types (Metadata, Annotation,
                                  ImapMetadata, WebDavMetadata,
                                  MetadataFilterCondition,
                                  MetadataCapability)
            └── jmap-metadata-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[draft-ietf-jmap-metadata-01]** — JMAP Object Metadata (normative for
  capability URI, Metadata object shapes, filter operators, sort keys,
  uniqueness constraints, `maySetPrivate` semantics)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference,
  `/set` response shape, `canCalculateChanges`)

[draft-ietf-jmap-metadata-01]: https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-server`]: ../crate-jmap-server
