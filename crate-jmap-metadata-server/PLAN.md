# jmap-metadata-server — Implementation Plan

JMAP Object Metadata extension
([draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/))
method handlers. Plugs into `jmap-server`'s `Dispatcher`. Backend-agnostic:
defines a `MetadataBackend` trait; consumers provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server               dispatcher
    └── jmap-metadata-types       data types
            └── jmap-metadata-server  ← this crate
```

## What This Crate Is

Method handler implementations for every JMAP Metadata method defined in
draft-ietf-jmap-metadata-01: `Metadata/get`, `Metadata/changes`,
`Metadata/set`, `Metadata/query`, `Metadata/queryChanges`.

Defines a `MetadataBackend` trait that the application implements. The crate
handles JMAP wire-protocol semantics (request shape parsing, partial set
success, response envelope construction). The backend handles storage AND
the Metadata-specific server-side constraints documented in
draft-ietf-jmap-metadata-01 §3.1 (see "Server-side semantic constraints"
below).

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage (SQLite, PostgreSQL, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not axum-specific — any `http`-based framework works
- Not enforcing the uniqueness / `maySetPrivate` / quota / related-object
  validation constraints itself — see "Server-side semantic constraints"

## Source Material

### Normative

`~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-metadata-01.txt` — read
the relevant section before implementing each handler. Wire field names,
error codes, and behavioral requirements come from the spec, not from memory.

### Canonical template

`~/PROJECT/JMAP/crate-jmap-sharing-server/` — closest single-extension-server
analog. `MetadataBackend` is cookie-cut from `SharingBackend`; the `helpers.rs`,
`register_metadata_handlers`, and dispatcher-registration scaffolding are
copied verbatim with type names adjusted.

The canonical extension-server template is `jmap-mail-server` per the
workspace `AGENTS.md` canonical-template propagation rule; `jmap-sharing-server`
is the closest precedent for a single-capability single-object-family server,
which matches Metadata's shape.

## Capability URI

`urn:ietf:params:jmap:metadata` (§1.2.1)

Re-exported from `jmap-metadata-types` as `JMAP_METADATA_URI` and from this
crate at the top level. Include this string as a key in the session-level
`capabilities` object (value: empty object) and the per-account
`accountCapabilities` object (value: a `MetadataCapability` struct from
`jmap-metadata-types`).

## RFC Method Coverage

| Method | Draft § | Handler notes |
|---|---|---|
| `Metadata/get` | §3.2 | standard get; `ids: null` returns all metadata |
| `Metadata/changes` | §3.3 | standard changes + `filterRelatedType` + `filterMetadataType` post-filtering on returned arrays (state token unaffected) |
| `Metadata/set` | §3.1 | standard set; server-side uniqueness / `maySetPrivate` / quota / related-object validation enforced by backend |
| `Metadata/query` | §3.4 | standard query; filter supports `@type`, `relatedType`, `relatedId`/`relatedIds`, `isPrivate`, `textMatch`; sort on `id`, `@type`, `relatedType`, `relatedId`, `isPrivate` |
| `Metadata/queryChanges` | §3.5 | standard queryChanges |

Total: 5 method registrations.

## Server-side semantic constraints (§3.1)

`Metadata/set` carries four server-side constraints that the **backend**
enforces and reports via [`BackendSetError::SetError`]:

1. **Uniqueness**. The tuple `(relatedType, relatedId, @type, isPrivate)`
   MUST be unique within the user's visible set. Duplicate create →
   `alreadyExists{ existingId: ... }`.
2. **`maySetPrivate` gating** (§1.2.1). If the per-account capability
   reports `maySetPrivate: false` and the client supplies `isPrivate: true`,
   the backend returns `forbidden`.
3. **Quota** (§6). If the operation would exceed the account's metadata
   quota, the backend returns `overQuota`.
4. **Related-object validation** (§3.1). Backends MUST verify the
   `relatedType` is supported and the `relatedId` references an existing
   object of that type. Failures return `invalidProperties` listing
   `relatedType` and/or `relatedId`.

The handler itself is generic — it just translates between the JMAP wire
format and the backend's `create_object` / `update_object` / `destroy_object`
results.

## MetadataBackend Trait

Cookie-cut from `SharingBackend`. Read-side operations come from `JmapBackend`
(supertrait); write operations are defined here.

```rust
pub trait MetadataBackend: JmapBackend {
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    fn supports_type<O: JmapObject>(&self) -> bool;
}
```

Implementor invariants:
1. **State monotonicity**: `get_state` returns a different token after every
   successful mutation. Token does not change on failure.
2. **Initial state**: `"0"` is the valid initial state sentinel.
3. **Uniqueness enforcement**: see §3.1 above — the backend is the final
   authority for the uniqueness tuple.
4. **Partial set success**: per-object failures do not roll back other
   objects in the same `/set` call (RFC 8620 §5.3).

## Module Layout

```
src/
  lib.rs        re-exports; register_metadata_handlers; JMAP_METADATA_URI re-export
  backend.rs    MetadataBackend trait; re-exports from jmap-server
  helpers.rs    SetAccumulators; finalize_set_response; set_error_value
  metadata.rs   all 5 method handlers
  memory.rs     [feature="memory"] reference impl of MetadataBackend
```

All five handler functions live in `metadata.rs` because the crate has a single
JMAP object type. Same shape as `jmap-filenode-server`.

## Test Strategy

`src/memory.rs` (feature-gated) provides a `MemoryBackend` reference
implementation of `MetadataBackend`. Integration tests in `tests/*.rs` use
this backend and dispatch real JMAP requests through `Dispatcher`. The
memory backend MUST enforce the uniqueness constraint (§3.1) — that is the
one Metadata-specific behavior worth testing in the reference impl.

`maySetPrivate` / quota / related-object validation are configurable
behaviors that vary by deployment; the memory backend stubs them out
(no-op accept) and integration tests for those behaviors are out of scope
for this crate.

### Test cases to include

- `Metadata/get`: fetch by id; `ids: null` fetches all; non-existent id returns
  notFound entry.
- `Metadata/changes`: standard cycle (state advances after set); `filterRelatedType`
  restricts the returned created / updated / destroyed arrays without changing
  the state token; `filterMetadataType` combines via AND with `filterRelatedType`.
- `Metadata/set` create: Annotation with vendor properties; ImapMetadata;
  WebDavMetadata.
- `Metadata/set` create: uniqueness collision → `alreadyExists` with `existingId`.
- `Metadata/set` update: patch a single vendor property.
- `Metadata/set` destroy: existing id; non-existent id → `notFound` in
  `notDestroyed`.
- `Metadata/set`: non-string `destroy` element → top-level `invalidArguments`.
- `Metadata/query`: `relatedType` + `relatedIds` filter; `textMatch` filter;
  combined filters.
- `Metadata/queryChanges`: standard cycle.

## Dependencies

```toml
jmap-types          = { workspace = true }
jmap-metadata-types = { workspace = true }
jmap-server         = { workspace = true }
serde_json          = { workspace = true }
```

No MIME parsing. No HTTP client. No database drivers.

## Decomposition tracker

This crate is being built in four phases tracked under bd JMAP-06zp.3:

- **.3.1** — Crate scaffolding: Cargo.toml, lib.rs shell, trait skeleton, handler
  stubs, helpers.rs, register_metadata_handlers, workspace member registration,
  PLAN.md, README.md. (cargo check + clippy clean; `Metadata/set` returns
  `serverFail`; thin-wrapper handlers fully working.)
- **.3.2** — MetadataBackend trait doc polish + helpers.rs verification.
- **.3.3** — `Metadata/set` full create/update/destroy implementation;
  `Metadata/changes` filter post-processing; inline unit tests with a
  minimal MockBackend.
- **.3.4** — `memory::MemoryBackend` feature-gated reference impl;
  `tests/*.rs` dispatcher-level integration tests; uniqueness enforcement
  in the reference impl.
