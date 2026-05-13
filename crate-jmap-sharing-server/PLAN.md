# jmap-sharing-server — Implementation Plan

RFC 9670 (JMAP Sharing) method handlers. Plugs into `jmap-server`'s
`Dispatcher`. Backend-agnostic: defines a `SharingBackend` trait; consumers
provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server            dispatcher
    └── jmap-sharing-types     data types
            └── jmap-sharing-server  ← this crate
```

## What This Crate Is

Method handler implementations for every RFC 9670 JMAP Sharing method:
`Principal/get`, `Principal/changes`, `Principal/set`,
`Principal/query`, `Principal/queryChanges`, `ShareNotification/get`,
`ShareNotification/changes`, `ShareNotification/set`,
`ShareNotification/query`, `ShareNotification/queryChanges`.

Defines a `SharingBackend` trait that the application implements. The crate
handles all JMAP protocol semantics (ordering, partial success,
ResultReference threading, error type mapping). The backend handles storage
and directory integration.

Known consumers: any server that also runs `jmap-mail-server` and wants
to expose the sharing framework — registered on the same `Dispatcher`.

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage or directory service (LDAP, Active
  Directory, PostgreSQL, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not defining `shareWith` semantics on domain types — `jmap-mail-server`
  and future domain server crates handle that independently
- Not axum-specific — any `http`-based framework works

## Source Material

### Normative

`~/PROJECT/jmap-chat-spec/references/rfc9670.txt` — read the relevant
section before implementing each handler. Wire field names come from the
spec, not from memory or reference code.

### Backend trait pattern — follow this exactly

`~/PROJECT/crate-jmap/crate-jmap-mail-server/src/backend.rs`

`MailBackend` and its supertrait `JmapBackend` are the exact pattern to
follow for `SharingBackend`. The read-side operations (`get_objects`,
`get_state`, `get_changes`, `query_objects`, `query_changes`) come from
`JmapBackend`. Write operations and sharing-specific operations are added
in `SharingBackend`.

`~/PROJECT/crate-jmapchat-server/jmapchat-server/src/backend.rs` —
original `StorageBackend`, `BackendChangesError`, `BackendSetError`,
`ChangesResult`, `QueryResult` error types. Use the same error taxonomy.

### Handler logic reference — read, do not copy

The Stalwart JMAP server at `~/GIT/stalwart-jmap-server/` (AGPL-3.0-only,
do not copy or translate) provides the best available reference for what
each method handler must do. Read it to understand logic; implement
independently from the spec.

Relevant paths (relative to `~/GIT/stalwart-jmap-server/`):

| Handler | Stalwart path | What to study |
|---|---|---|
| Principal/* | `main/crates/jmap/src/principal/` | get, query, set pattern |
| ShareNotification/* | `main/crates/jmap/src/sharenotification/` (if present) | destroy-only set |

## Dependencies

```toml
jmap-types         = { path = "../crate-jmap-types" }
jmap-sharing-types = { path = "../crate-jmap-sharing-types" }
jmap-server        = { path = "../crate-jmap-server" }
serde              = { version = "1", features = ["derive"] }
serde_json         = "1"
thiserror          = "2"
tokio              = { version = "1", features = ["rt"] }
```

No directory service clients. No HTTP client. No database drivers.

## RFC 9670 Method Coverage

| Object | Methods | RFC §§ | Notes |
|---|---|---|---|
| Principal | get | §2.1 | standard /get |
| Principal | changes | §2.2 | may always return `cannotCalculateChanges` |
| Principal | set | §2.3 | admin operation; server may reject with `forbidden` |
| Principal | query | §2.4 | filter by type, email, name, text, timeZone, accountIds |
| Principal | queryChanges | §2.5 | may always return `cannotCalculateChanges` |
| ShareNotification | get | §3.1 | standard /get |
| ShareNotification | changes | §3.2 | standard /changes |
| ShareNotification | set | §3.3 | destroy-only; create/update → `forbidden` |
| ShareNotification | query | §3.4 | filter by after, before, objectType, objectAccountId |
| ShareNotification | queryChanges | §3.5 | standard /queryChanges |

Total: 10 method registrations.

## Key Design Decisions

### 1. SharingBackend follows JmapBackend/MailBackend exactly for generic CRUD

Same AFIT pattern, same `BackendChangesError`/`BackendSetError` error types,
same `ChangesResult`/`QueryResult` structs. `JmapBackend` is the supertrait,
providing all read-side operations. `SharingBackend` adds write operations
and any sharing-specific operations.

`SharingBackend` is NOT object-safe (it has generic methods). The dispatcher
and all handlers are generic over `B: SharingBackend`, monomorphized at
compile time. No `#[async_trait]` macro — AFIT is stable since Rust 1.75.

### 2. ShareNotification/set rejects creates and updates in the handler layer

RFC 9670 §3.3: "Only destroy is supported; any attempt to create/update MUST
be rejected with a 'forbidden' SetError."

The handler inspects the `create` and `update` maps of the incoming
`/set` request before touching the backend. Any entries found in either map
are immediately responded to with `SetError { type: "forbidden" }`. Only
the `destroy` list is forwarded to the backend.

The backend has no knowledge of this constraint — it receives only destroy
calls for `ShareNotification`. This mirrors how `VacationResponse` works in
`jmap-mail-server`: handler enforces the constraint, backend is unaware.

### 3. Principal/set: backend decides what is permitted

RFC 9670 §2.3: "A server MUST reject any change it doesn't allow with a
'forbidden' SetError." The spec also notes that servers SHOULD allow a user
to update their own `name`, `description`, and `timeZone`.

The handler forwards all creates, updates, and destroys to the backend
without pre-filtering. The backend returns `BackendSetError::Forbidden` for
operations it does not permit. This is the correct split: the handler
handles JMAP protocol framing; the backend encodes business rules about
who can manage Principals.

Alternative considered: having the handler check the caller's role before
forwarding to the backend. Rejected because "role" is not a concept in this
crate — it is deployment-specific. The backend has all the context needed.

### 4. Principal/changes and Principal/queryChanges: backends may return cannotCalculateChanges

RFC 9670 §2.2 and §2.5 both explicitly note that implementations backed
by an external directory may be unable to calculate changes. The backend
returns `Err(BackendChangesError::CannotCalculateChanges)` and the handler
maps this to `cannotCalculateChanges` per RFC 8620 §5.2.

Backends that can track changes (e.g., in-memory or SQL-backed) return the
standard `ChangesResult`. This requires no special API — the same error enum
is used for both Principal and ShareNotification.

### 5. Integration: this crate and domain crates are wired together by the consumer

This crate registers `Principal/*` and `ShareNotification/*` handlers. The
domain crates (`jmap-mail-server`, future calendar/contacts/filenode server
crates) register their own handlers for their own object types. Both sets
of handlers are registered on the same `jmap-server::Dispatcher`.

The `shareWith` validation in domain handlers (e.g., checking that a
Principal id in `Mailbox.shareWith` is valid) is the domain handler's
responsibility. If it needs to validate Principal ids, it calls into the
same storage layer — but not through this crate's API. The two crates do not
call each other at runtime.

### 6. Capability URI constants are re-exported from jmap-sharing-types

```rust
pub use jmap_sharing_types::JMAP_PRINCIPALS_URI;
pub use jmap_sharing_types::JMAP_PRINCIPALS_OWNER_URI;
```

These are re-exported from `lib.rs` for use by the server consumer when
building the Session object. The consumer is responsible for inserting the
appropriate capability values into the Session and Account capability maps;
this crate provides the URI strings and the types
(`PrincipalsCapability`, `PrincipalsOwnerCapability` from
`jmap-sharing-types`) but does not build the Session object itself.

### 7. register_sharing_handlers is the entry point

One function registers all 10 method handlers with the caller's
`jmap-server::Dispatcher<C>`. The backend is wrapped in `Arc<B>` and cloned
into each handler closure — same pattern as `jmap-mail-server`.

## Planned Public API

```rust
/// Implement for your sharing/directory storage system.
///
/// Uses AFIT (async fn in trait, stable since Rust 1.75). Not object-safe;
/// always monomorphized at compile time.
///
/// Implementor invariants (same as JmapBackend/MailBackend):
/// 1. State monotonicity: get_state returns a different token after every
///    successful mutation.
/// 2. Initial state: "0" is always the valid initial state sentinel.
/// 3. Partial set success: per-object failures do not roll back other
///    objects in the same /set call (RFC 8620 §5.3).
/// 4. Principal/changes may return BackendChangesError::CannotCalculateChanges
///    if backed by an external directory with no change tracking.
#[allow(async_fn_in_trait)]
pub trait SharingBackend: JmapBackend {
    // ── Write operations (mirrors MailBackend) ──────────────────────────────

    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // ── Sharing-specific ────────────────────────────────────────────────────

    /// Whether this backend supports operations on the given object type.
    /// Return false for Principal if backed by a read-only directory.
    fn supports_type<O: JmapObject>(&self) -> bool;
}

/// Capability URI for urn:ietf:params:jmap:principals
/// (re-exported from jmap-sharing-types).
pub use jmap_sharing_types::JMAP_PRINCIPALS_URI;

/// Capability URI for urn:ietf:params:jmap:principals:owner
/// (re-exported from jmap-sharing-types).
pub use jmap_sharing_types::JMAP_PRINCIPALS_OWNER_URI;

/// Register all RFC 9670 JMAP Sharing handlers with a jmap-server Dispatcher.
///
/// After calling this, the dispatcher handles all 10 RFC 9670 method names.
/// Wrap `backend` in `Arc` before passing — it is cloned into each handler.
pub fn register_sharing_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: SharingBackend + 'static,
    C: Clone + Send + 'static;

pub use backend::{
    BackendChangesError, BackendSetError,
    ChangesResult, QueryResult, QueryChangesResult, AddedItem,
};
```

## Module Layout

```
src/
  lib.rs            re-exports; register_sharing_handlers;
                    JMAP_PRINCIPALS_URI, JMAP_PRINCIPALS_OWNER_URI re-exports
  backend.rs        SharingBackend trait; error type re-exports
  principal.rs      Principal/get, /changes, /set, /query, /queryChanges
  notification.rs   ShareNotification/get, /changes, /set (destroy-only),
                    /query, /queryChanges
  helpers.rs        shared utilities (e.g., filter evaluation for MemoryBackend)
```

## Test Strategy

A `MemoryBackend` in `tests/common/mod.rs` provides an in-memory `HashMap`
implementation of `SharingBackend`. This serves as both the test harness and
the canonical example for implementors.

Test files per object group:

```
tests/
  common/
    mod.rs            MemoryBackend implementation
  principal_tests.rs
  notification_tests.rs
```

Test oracles come from RFC 9670 example JSON (§4.1 contains a full
`Principal/get` request/response pair — use it verbatim). Never derive
expected values from the implementation under test.

Each test calls `register_sharing_handlers` with the `MemoryBackend`,
constructs a `JmapRequest` matching the RFC example, calls
`Dispatcher::dispatch`, and asserts the response matches the RFC example.

### Non-trivial test cases to include

- `Principal/get` with `ids: null` returns all principals (RFC 9670 §4.1
  example — use verbatim)
- `Principal/get` with a mix of found and not-found ids → partial response
- `Principal/query` filtering by `type: "individual"` — returns only
  individual principals
- `Principal/query` filtering by `text: "Joe"` — substring match across
  name, email, description
- `Principal/set` create/update forwarded to backend and accepted
- `Principal/set` update that backend rejects with `forbidden` → response
  has `notUpdated` with `forbidden` SetError
- `Principal/changes` backend returns `CannotCalculateChanges` → response
  has `cannotCalculateChanges` error
- `ShareNotification/set` with only `destroy` list — succeeds
- `ShareNotification/set` with `create` entries → each entry has `forbidden`
  SetError; no backend call made
- `ShareNotification/set` with `update` entries → each entry has `forbidden`
  SetError; no backend call made
- `ShareNotification/set` with mixed create + destroy — creates all get
  `forbidden`; destroys proceed normally
- `ShareNotification/query` filter by `after` date
- `ShareNotification/query` filter by `objectType: "Mailbox"`
- `ShareNotification/query` sorted by `created` ascending and descending
- `queryChanges` backend returns `CannotCalculateChanges` → `cannotCalculateChanges`
