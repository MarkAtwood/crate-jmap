# jmap-sharing-server

JMAP Sharing ([RFC 9670]) method handlers for Rust. Plugs into
[`jmap-server`]'s `Dispatcher`. Implements all 10 RFC 9670 method names.
Storage-agnostic — consumers implement the `SharingBackend` trait for their
own data layer.

## Usage

```rust
use std::sync::Arc;
use jmap_sharing_server::{SharingBackend, register_sharing_handlers};
use jmap_server::Dispatcher;

// 1. Implement SharingBackend for your storage layer (see trait section below).
struct MyBackend { /* db pool, directory client, etc. */ }
impl SharingBackend for MyBackend { /* ... */ }

// 2. Wire all 10 RFC 9670 methods into a Dispatcher in one call.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_sharing_handlers(&mut dispatcher, Arc::new(MyBackend { /* ... */ }));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, (), session_state).await;
```

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

```rust
pub trait SharingBackend: JmapBackend {
    /// Create a new Principal.
    ///
    /// Returns (assigned_id, created_object) on success.
    ///
    /// The returned O MUST have its `id` field set to the server-assigned Id.
    /// The handler relies on this to populate the `created` response map per
    /// RFC 8620 §5.3.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing Principal.
    ///
    /// Returns Some(updated_object) if the backend modified server-set fields
    /// beyond the patch (RFC 8620 §5.3 echo); None if the patch was applied
    /// verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy a Principal or ShareNotification by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    ///
    /// Not called internally — used by the session capability builder.
    /// Backends that support all types unconditionally can return true always.
    fn supports_type<O: JmapObject>(&self) -> bool;
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a
  `serverFail` response

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

### Permission enforcement

`Principal/set` supports `create`, `update`, and `destroy` at the wire level,
but RFC 9670 §2.3 specifies that the server may reject any of these with
`forbidden` if the caller does not have sufficient permission. Permission
enforcement is entirely the backend's responsibility: backends return
`BackendSetError::SetError(SetError::new(SetErrorType::Forbidden))` from
`create_object`, `update_object`, or `destroy_object` as appropriate. The
handler routes these `forbidden` errors back to the caller in `notCreated`,
`notUpdated`, or `notDestroyed`.

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

## Known Limitations

- Permission enforcement is entirely the backend's responsibility. The handler
  only routes `forbidden` SetErrors back to the caller. A backend that does not
  enforce access control will silently permit any operation.
- No storage backend ships with this crate. The in-memory `MockBackend` in the
  `test_support` module is a test harness only and is not suitable for
  production use.

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

## License

MIT OR Apache-2.0
