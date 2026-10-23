# jmap-sharing-server

## What it is

JMAP Sharing ([RFC 9670]) method handlers and the `SharingBackend` trait.
Plugs into [`jmap-server`]'s `Dispatcher`. Implements all 10 RFC 9670 method
names. Storage-agnostic — consumers implement the `SharingBackend` trait for
their own data layer.

## What it's for

Implements RFC 9670 / draft-ietf-jmap-sharing — Principal and
ShareNotification objects, plus the workspace-canonical `myRights` model that
other extension-server crates propagate to their shareable objects — so
consumers can wire JMAP Sharing method dispatch into their HTTP transport.
Sibling to `jmap-mail-server` (the canonical extension-server template) and
the other `jmap-*-server` crates; all of them inter-depend through the
workspace `JmapHandler` trait in `jmap-server`. The consumer supplies a
`SharingBackend` impl (storage), a `CallerCtx` type (auth identity), and
wires the dispatcher into HTTP / SSE / WebSocket transport themselves.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_sharing_server::{SharingBackend, register_sharing_handlers};
use jmap_server::Dispatcher;

// 1. Implement SharingBackend for your storage layer. The impl must also
//    satisfy the JmapBackend supertrait (account_exists, get_objects,
//    get_state, get_changes, query_objects, query_changes) and declare
//    `type Error` and `type CallerCtx`. See the SharingBackend trait
//    section below for the full surface.

// 2. Construct a dispatcher parameterised on your CallerCtx type, then
//    register all 10 RFC 9670 methods in one call.
let mut dispatcher: Dispatcher<MyCallerCtx> = Dispatcher::new();
register_sharing_handlers(&mut dispatcher, Arc::new(my_backend));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, caller_ctx, session_state).await;
```

`Dispatcher<C>` requires `B::CallerCtx = C`; `register_sharing_handlers`
enforces the match at the type level. For a single-user dev server use
`Dispatcher<()>` with a backend whose `CallerCtx = ()`. For
multi-tenant production, define a typed caller context (e.g. a
principal-id newtype) and parameterise both sides on that type.

For a runnable, type-checked variant of this example, see the
[crate-level documentation](https://docs.rs/jmap-sharing-server) — the
doctest in `lib.rs` is verified by `cargo test --doc`.

After `register_sharing_handlers` returns, the dispatcher handles every method
name listed in the [Registered methods](#registered-methods) section below. The
same `Arc<MyBackend>` can be shared with other parts of your application.

## Registered methods

All 10 method names from RFC 9670 are registered:

| Object | Methods |
|---|---|
| `Principal` | `get`, `changes`, `set`, `query`, `queryChanges` |
| `ShareNotification` | `get`, `changes`, `set`, `query`, `queryChanges` |

## SharingBackend trait

Implement this trait to connect the handlers to your storage system. The
read-side methods (`get_objects`, `get_state`, `get_changes`, `query_objects`,
`query_changes`) are defined on the `JmapBackend` supertrait (from
`jmap-server`). `SharingBackend` adds write operations.

```rust,ignore
pub trait SharingBackend: JmapBackend {
    /// Create a new Principal.
    ///
    /// `build` receives the backend-assigned id and returns the object to
    /// store. Returns (assigned_id, created_object) on success.
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send;

    /// Apply a partial update (patch) to an existing Principal.
    ///
    /// Returns Some(updated_object) if the backend modified server-set fields
    /// beyond the patch (RFC 8620 §5.3 echo); None if the patch was applied
    /// verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a Principal or ShareNotification by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    ///
    /// Not called internally — used by the session capability builder.
    /// Backends that support all types unconditionally can return true always.
    fn supports_type<O: JmapObject>(&self, caller: &Self::CallerCtx, account_id: &Id) -> bool;
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a
  `serverFail` response

## Permission enforcement responsibilities

The handlers in this crate do **no** permission checking. Per the
workspace AGENTS.md "Caller identity (foundation seam)" rule, backends
are canonical for authorization: the handler computes the candidate
mutation and the backend's `/set` impl is the canonical point of
enforcement, returning
`BackendSetError::SetError(SetError::new(SetErrorType::Forbidden))`
from `create_object`, `update_object` or `destroy_object`; the handler
routes it to `notCreated`, `notUpdated` or `notDestroyed`. Caller
identity is read via `JmapBackend::principal_id` on the
`caller: &Self::CallerCtx` parameter every backend method receives.

The reference `memory::MemoryBackend` does **not** implement any of
the checks below — it returns `None` from `principal_id` and stores
whatever the handler hands it. Production backends MUST implement each
of the following or they will silently accept JMAP requests that
[RFC 9670] mandates the server to reject.

### Principal/set ([RFC 9670] §2.3, §6.4)

1. **Reject every change the server does not allow.** "A server MUST
   reject any change it doesn't allow with a "forbidden" SetError."
   Where changes are supported, the server "SHOULD allow an update to
   the "name", "description", and "timeZone" properties of the
   Principal with the same id as the "currentUserPrincipalId"".

2. **Principal creation is controlled.** "The set of Principals within
   a shared environment MUST be strictly controlled" (§6.4); a backend
   that accepts `create` restricts it to authorized callers.

### ShareNotification/set ([RFC 9670] §3, §3.3)

3. **Destroy only the caller's own notifications.** The handler
   rejects create and update with `forbidden` itself ("Only destroy is
   supported"), so the backend sees only destroys and MUST refuse one
   that targets another user's notification.

### Reads ([RFC 9670] §1.4, §3)

4. **Per-user scope.** A ShareNotification "records when the user's
   permissions to access a shared object changes", so `get_objects`
   and `query_objects` return only the caller's own. Principal
   records carry an Account object only "for all Accounts where the
   user has permission to access data for that Principal" (§1.4).

### Shared data in other extensions ([RFC 9670] §4)

`myRights` and `shareWith` on shareable data (Mailbox, Calendar,
AddressBook, TaskList, FileNode) are enforced by the backend of the
crate that serves that data type, as each one's "Permission
enforcement responsibilities" section lists. This crate enforces
nothing on their behalf. When those backends change a user's
permissions, "the server SHOULD create a ShareNotification" (§3) in
the account holding the Principals.

### Caller-identity-less backends

The reference `MemoryBackend` returns `None` from `principal_id` and
is intentionally permissive (fail-open, bd:JMAP-bghk) — it is suitable
for single-user dev servers, fixtures, and the workspace's own
integration tests, but NOT for any deployment where more than one
principal can reach the JMAP dispatcher. A deployment that needs ACL
enforcement MUST override `JmapBackend::principal_id` and implement
each of the checks above; the workspace ships no production-grade
reference impl.

## How it works

### Registration

`register_sharing_handlers` uses `ClosureHandler` (provided by
`jmap-server`) to wrap each handler function and `Arc<B>` into a
`JmapHandler<B::CallerCtx>` and registers it with the dispatcher. The
dispatcher's `CallerCtx` value is forwarded into each closure as `ctx`, then
passed by reference (`&ctx`) into the standard `handle_*` handler bodies,
which themselves forward `caller: &B::CallerCtx` into every `SharingBackend`
method call. One `Arc::clone` per method name; no heap allocation per request.

### Handler structure

Each handler function (e.g. `handle_principal_get`, `handle_share_notification_set`)
receives `args: Value` containing the raw JMAP method arguments after
ResultReference resolution, calls `SharingBackend` methods, and returns the
RFC 8620 response object.

### ShareNotification/set — destroy-only semantics

RFC 9670 §3.3 specifies that ShareNotification objects are server-generated and
immutable. Clients may only destroy them (to dismiss notifications). The
`ShareNotification/set` handler enforces this directly:

- `create` entries in the request body are always rejected with
  `SetErrorType::Forbidden`. No backend call is made.
- `update` entries are always rejected with `SetErrorType::Forbidden`. No
  backend call is made.
- `destroy` entries are forwarded to `destroy_object` normally.

This means a request containing only `destroy` operations is valid and completes
normally. A request containing `create` or `update` entries returns `forbidden`
SetErrors for each such entry in `notCreated` / `notUpdated`, without a
top-level error.

### Principal/set — non-string destroy elements

The `destroy` array in a `/set` request must contain only string IDs per RFC
8620 §5.3. If a non-string element is encountered during pre-flight validation,
the entire request returns a top-level `invalidArguments` error before any
backend calls are made.

### onSuccessSetIsDefault

This argument is not applicable to JMAP Sharing. Neither `Principal` nor
`ShareNotification` defines an `onSuccessSetIsDefault` argument. There is no
single-default invariant to enforce.

## Capability URIs

Include these in your Session object's `capabilities` map:

```rust
// Re-exported from jmap-sharing-types:
pub const JMAP_PRINCIPALS_URI: &str       = "urn:ietf:params:jmap:principals";
pub const JMAP_PRINCIPALS_OWNER_URI: &str = "urn:ietf:params:jmap:principals:owner";
```

`JMAP_PRINCIPALS_URI` indicates the server supports Principal and
ShareNotification objects. `JMAP_PRINCIPALS_OWNER_URI` indicates the server
also supports creating and managing Principals (not just reading them).

## CallerCtx

`register_sharing_handlers` registers each method as a `ClosureHandler`
that forwards the dispatcher's `CallerCtx` value into the closure as `ctx`,
which the closure body then passes by reference (`&ctx`) into the standard
`handle_*` handler. The handler in turn forwards `caller: &B::CallerCtx` into
every `SharingBackend` (and inherited `JmapBackend`) method call.

To use this:

1. Pick a `CallerCtx` type for your backend — e.g. `()` if no auth context is
   needed, or a struct like `AuthCtx { user_id: Id, scopes: Vec<Scope> }`.
2. Implement `JmapBackend` with `type CallerCtx = AuthCtx;` (the bound is
   `Clone + Send + Sync + 'static`).
3. Use the matching `Dispatcher<AuthCtx>` and pass the constructed auth
   context as the second argument to `Dispatcher::dispatch(request, auth, …)`.
4. Inside `SharingBackend` method bodies, read `caller: &AuthCtx` to apply
   per-user visibility rules, rate limits, etc.

Backends that don't need an auth identity use `type CallerCtx = ();` and a
`Dispatcher<()>`. Both shapes register the same way.

## Gotchas

- Permission enforcement is entirely the backend's responsibility. The handler
  only routes `forbidden` SetErrors back to the caller. A backend that does not
  enforce access control will silently permit any operation.
- **`memory::MemoryBackend` is a reference implementation, not production.**
  Gated behind `feature = "memory"` (off by default). State is held in
  `HashMap`s behind a `std::sync::Mutex`; persistence is not implemented; a
  number of RFC 9670 edge cases are simplified. Intended for integration
  tests, smoke tests, examples, and as source-readable study material for
  downstream contributors writing a real (database-backed) backend. See the
  module docs in `src/memory.rs` for the explicit non-production caveats.
  Server-assigned ids are uuid v4, as in `jmap-mail-server`.

## Crate family

```
jmap-types
    ├── jmap-server              Dispatcher this plugs into
    └── jmap-sharing-types       domain types (Principal, ShareNotification)
            └── jmap-sharing-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[RFC 9670]** — JMAP Sharing (normative for all method semantics, Principal
  rights model, ShareNotification destroy-only semantics)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference,
  `/set` response shape, `canCalculateChanges`)

[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-server`]: ../crate-jmap-server
