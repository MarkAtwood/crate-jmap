# jmap-server

Backend-agnostic JMAP server framework for Rust. Implements the [RFC 8620] wire
protocol: request parsing, ResultReference resolution, and the `Dispatcher`
machinery. No opinion on authentication, method sets, capability URIs, or storage.

## What it is

The foundation server crate for the `jmap-*` family — `Dispatcher`, the
`JmapHandler` trait, `parse_request`, `ResultReference` resolution, the
`JmapBackend` supertrait, generic `/get` / `/changes` / `/query` /
`/queryChanges` handlers, the `CallerCtx` identity seam, and panic isolation
via `tokio::task::spawn`. No HTTP / SSE / WebSocket transport — that is the
consumer's responsibility, per the workspace's library-kit posture.

## What it's for

Every `jmap-*-server` extension in the workspace (mail, chat, contacts,
calendars, tasks, filenode, sharing, metadata) builds on this crate by
extending the `JmapBackend` supertrait with its own backend methods and
registering handlers via `Dispatcher::register`. Downstream consumers like
kith and stoa, and the in-workspace `jmap-testjig`, wire this crate into
their own HTTP transport, auth integration, and persistence story.

## How to use

```rust
use std::sync::Arc;
use jmap_server::{CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler};

// 1. Implement JmapHandler for each method.
struct FooGetHandler;

impl JmapHandler<String> for FooGetHandler {        // String is the CallerCtx type
    // cx carries method, call_id, args and caller (the auth identity).
    fn call(&self, _cx: CallContext<String>) -> HandlerFuture {
        Box::pin(async move { Ok(HandlerOutput::new(serde_json::json!({"list": [], "state": "s0"}))) })
    }
}

// 2. Register handlers and dispatch requests.
let mut dispatcher: Dispatcher<String> = Dispatcher::new();
dispatcher.register("Foo/get", Arc::new(FooGetHandler));

// In your handler:
// let request = jmap_server::parse_request(body_json, 16)?;
// let response = dispatcher.dispatch(request, caller_identity, session_state).await;
```

## CallerCtx

`CallerCtx` is whatever your authentication layer produces — an identity struct,
a session token, `()`, etc. The dispatcher clones it for each method call and
passes it through to the handler unchanged. It must be `Clone + Send + 'static`;
use `Arc<T>` to share non-static data (e.g. a database connection pool).

## Request parsing

```rust
use jmap_server::{parse_request, request_error};

// Parse and validate a JMAP request body.
let req = parse_request(body_json, /* max_calls */ 16)
    .map_err(request_error)?;   // returns RequestError on failure
```

`parse_request` validates:
- The body is a valid JMAP Request object.
- `using` is non-empty.
- The number of method calls does not exceed `max_calls`.

Capability URI checking is NOT performed here — that is the caller's responsibility.

## Error responses

```rust
use jmap_server::{request_error, JmapError};

// Request-level errors (HTTP 400/403/500 with RFC 7807 body):
let resp = request_error(JmapError::not_request());          // → HTTP 400
let resp = request_error(JmapError::limit("maxCallsInRequest"));  // → HTTP 400

// Method-level errors (HTTP 200, inside methodResponses):
// Return Err(JmapError::...) from your JmapHandler::call implementation.
```

## Re-exports

The crate root re-exports three groups of items so consumers do not have to
take a direct dependency on `jmap-types`:

### Wire types (from `jmap_types`)

```rust
pub use jmap_types::{
    Argument, Id, Invocation, JmapError, JmapRequest, JmapResponse,
    PushSubscription, ResultReference, State, UTCDate,
};
```

These are the RFC 8620 wire primitives: `Id`, `UTCDate`, and `State` for
typed identifier/timestamp/state-token fields; `JmapRequest` / `JmapResponse`
for the top-level request/response envelopes; `Invocation` for individual
method calls (a `(method, args, callId)` tuple); `ResultReference` and
`Argument<T>` for the back-reference machinery in RFC 8620 §3.7;
`JmapError` for both request-level and method-level errors;
`PushSubscription` for the RFC 8620 §7.2 push subscription object.

### Backend infrastructure (from the `backend` module)

```rust
pub use backend::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject,
    JmapBackend, JmapObject, QueryChangesResult, QueryObject, QueryResult,
    SetError, SetErrorType, SetObject,
};
```

Generic backend traits (`JmapBackend`, `GetObject`, `QueryObject`, `SetObject`),
result and error envelopes (`ChangesResult`, `QueryResult`, `QueryChangesResult`,
`SetError` / `SetErrorType` re-exported from `jmap-types`, `BackendSetError`,
`BackendChangesError`), and the
`AddedItem` row used in `Foo/queryChanges` responses. Extension server crates
(`jmap-mail-server`, `jmap-chat-server`, etc.) extend `JmapBackend` to add
their own write/extension methods.

### Helpers (from the `helpers` module)

```rust
pub use helpers::{extract_account_id, not_found_json, now_utc_string, serialize_value};
```

Small utilities used by every method handler: `extract_account_id` pulls the
`accountId` field from a method-arguments `Value` (returning `accountNotFound`
on absence), `now_utc_string` produces an RFC 3339 timestamp suitable for
RFC 8620 `UTCDate` fields, `serialize_value` serializes any `Serialize` value into a
`Value` (mapping serialization failure to `serverFail`), and `not_found_json`
builds the `notFound: [...]` array used by `Foo/get` responses for missing
ids.

### Other entry points

```rust
pub use parse::{check_known_capabilities, parse_request, resolve_args};
pub use response::{error_invocation, error_status, request_error, RequestError};
pub use handlers::{handle_changes, handle_get, handle_query, handle_query_changes};
pub use push::{
    is_deliverable, register_push_subscription_handlers, state_change_for, PushSubscriptionBackend,
    StoredPushSubscription, MAX_PUSH_EXPIRY_SECS,
};
// Plus the closure-based JmapHandler adapter defined in this crate's
// crate root: ClosureHandler.
```

Request parsing and capability validation; HTTP response shaping for
request-level errors; generic implementations of the four read-side JMAP
methods (`Foo/get`, `Foo/changes`, `Foo/query`, `Foo/queryChanges`) that
extension server crates plug their backend into; and the closure-based
`JmapHandler` adapter (`ClosureHandler`) used by `register_*_handlers`
macros across the extension server crates. `ClosureHandler` forwards the
dispatcher's `CallerCtx` value through to the wrapped closure as a fourth
argument, so backends that need per-request auth context can read it
without dropping down to a manual `JmapHandler<C>` impl.

`register_push_subscription_handlers` registers `PushSubscription/get`
(RFC 8620 §7.2.1) and `PushSubscription/set` (RFC 8620 §7.2.2) over a
`PushSubscriptionBackend`. Push subscriptions belong to the caller's
credentials, not to an account, so the backend's methods MUST see and change
only the caller's subscriptions. `/get` never returns `url` or `keys` and
answers `forbidden` when either is requested. `/set` takes no `accountId` or
`ifInState` and returns no `oldState` or `newState`. It requires an
`https://` url, rejects changes to `id`, `deviceClientId`, `url` and `keys`,
compares a new `verificationCode` in constant time, and limits `expires` to
`MAX_PUSH_EXPIRY_SECS` (7 days) from now. The backend generates the
verification code, enforces the per-user cap, and may reject a create by
policy with its own `SetError`, which reaches the response verbatim.

`is_deliverable` and `state_change_for` tell the consumer's delivery code
whether it may push to a subscription, and what `StateChange` to send.
Sending the pushes, and the SSRF, rate-limit, credential-revocation and
secure-erase MUSTs, are the consumer's job; the trait docs list them.

## Examples

A runnable end-to-end demo lives in [`examples/dispatcher_minimal.rs`](examples/dispatcher_minimal.rs):

```sh
cargo run --example dispatcher_minimal -p jmap-server
```

It registers a single stub `JmapHandler` for `"Core/echo"`, dispatches a
synthetic [`JmapRequest`], and prints both the wire-format [`JmapResponse`]
and the typed access paths a real server would consume.

## How it works

- `parse_request` decodes the body, validates `using` is non-empty, and
  caps the method-call count at the caller-supplied `max_calls` (see the
  [Request parsing](#request-parsing) section). Capability-URI checking is
  the caller's job, not this crate's.
- `Dispatcher<C>` is generic over a `CallerCtx` type the HTTP/auth layer
  produces; it walks the request's `method_calls`, resolves
  [`ResultReference`](#re-exports) back-references via JSON Pointer
  (RFC 6901), and invokes each handler under `tokio::task::spawn` so a
  panicking handler degrades to `serverFail` instead of crashing the
  process. See [CallerCtx](#callerctx) for the identity seam.
- Errors split cleanly between request-level (HTTP 400 / 403 / 500, RFC
  7807 body — see [Error responses](#error-responses)) and method-level
  (HTTP 200 inside `methodResponses`, returned by the handler).
- The crate re-exports the wire types from `jmap-types`, the backend
  trait surface, and helper utilities so consumers do not have to take a
  direct `jmap-types` dependency — see [Re-exports](#re-exports).

## Gotchas

- **`CallerCtx` is cloned per method call.** `Dispatcher::dispatch` performs one `Clone` of the `CallerCtx` value per method in the batch, plus two `String` clones for the method name and call_id (bd:JMAP-jfia.8). Heavy `CallerCtx` values — auth profiles, permission vectors, session tokens, anything embedding more than a pointer or a small POD — should be wrapped in `Arc<T>` so the per-call clone is a pointer bump rather than a deep copy. A 16-call batch over a non-`Arc`-wrapped `CallerCtx` pays 16 deep clones.
- **`sortAsTree` and `filterAsTree` are RFC 8621-specific.** Those two args are defined by RFC 8621 §5 for `Email/query`, not by the base protocol. The generic `handle_query` handler in this crate has no knowledge of them and per RFC 8620 §3.6 silently ignores unknown args. Tree-mode traversal, when needed, must be implemented in a domain-specific handler (see `handle_email_query` in jmap-mail-server).
- **Filter / sort are delegated to the backend.** `handle_query` parses `filter` and `sort` from method args into the backend trait's `O::Filter` and `O::Comparator` types and passes them through to [`JmapBackend::query_objects`]. The handler does no in-process re-filtering or re-sorting. The backend MUST push filter and sort into indexed storage — that contract is documented on `JmapBackend::query_objects` and a backend that ignores it degenerates to O(n) per page.
- **Single-process only.** The dispatcher holds no shared state between requests and is safe to use concurrently; however, there is no built-in clustering support. State consistency across multiple processes is the backend's responsibility.

## References

- **[RFC 8620]** — JMAP base protocol (request format, ResultReference, error types)
- **[RFC 6901]** — JSON Pointer (used by ResultReference path resolution)

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 6901]: https://www.rfc-editor.org/rfc/rfc6901
