# jmap-server — Implementation Plan

A backend-agnostic JMAP server framework crate for Rust. Implements the
RFC 8620 wire protocol, dispatch machinery, and ResultReference resolution.
No opinion on authentication, method sets, or capability URIs.

## Crate Family

This crate is one piece of a planned family. Naming convention: `jmap-{extension}-{role}`.

```
jmap-types                   serde/serde_json only — shared wire types
    ├── jmap-server          + tokio, http — dispatcher, parse, ResultReference
    ├── jmap-mail-types      + RFC 8621 data types
    │       └── jmap-mail-server
    ├── jmap-chat-types      + Chat extension data types
    │       ├── jmap-chat-server
    │       └── jmap-chat-client
    └── (future extensions)
```

Directory naming: `crate-{crate-name}` (e.g. `crate-jmap-server`, `crate-jmap-types`).

**Status:** `jmap-types` (`../crate-jmap-types`) exists. Wire types (`JmapError`,
`JmapRequest`, `JmapResponse`, `Invocation`, `ResultReference`, `Argument<T>`) live there
and are re-exported by this crate. This crate owns only the server-side layer: parsing,
ResultReference resolution, the dispatcher, and HTTP response helpers
(framework-agnostic via the `http` crate).

## What This Crate Is

A library that any JMAP server can use, regardless of what methods it exposes
or how it authenticates callers. Known consumers: `kith` (JMAP Chat) and
`stoa` (email/Usenet, possibly Chat too).

## What This Crate Is Not

- Not a full JMAP server
- Not opinionated about method names, capability URIs, or account models
- Not coupled to any auth system (Tailscale, bearer tokens, OIDC, etc.)
- Not a client library

## Source Material

All types and logic already exist and are tested. This crate is an extraction,
not a from-scratch implementation.

Types extracted by `jmap-types` (not this crate's job — depend on it):

| Item | Kith source |
|---|---|
| `JmapError` | `kith/crates/kith-core/src/error.rs` |
| `Invocation`, `JmapRequest`, `JmapResponse` | `kith/crates/kith-core/src/jmap.rs` |
| `ResultReference`, `Argument<T>` | `kith/crates/kith-core/src/resultref.rs` |

Logic extracted / adapted by this crate:

| Item | Best source | Notes |
|---|---|---|
| `parse_request` | `kith/crates/kith-jmap/src/lib.rs` | Strip kith capability check; keep max_calls param |
| `resolve_args` / `ResultReferenceStore` | `crate-jmapchat-server/jmapchat-server/src/ref_store.rs` | Two-phase (atomic on failure); also has `json_pointer` helper and RFC 6901 tilde-escaping tests |
| `Dispatcher`, `JmapHandler` trait | `kith/crates/kith-jmap/src/lib.rs` | Replace `Role`+`Identity` with generic `CallerCtx`; merge `PeerJmapHandler` into single generic trait |
| HTTP handler pattern | `crate-jmapchat-server/jmapchat-server-axum/src/handlers/jmap.rs` | Shows `dispatch()` + `RequestError::into_response()` → `http::Response<String>` (axum accepts this directly) |

`stoa/crates/mail/src/jmap/dispatch.rs` is a synchronous `HandlerFn` type alias only — not useful here.

## Dependencies

```toml
jmap-types = { path = "../crate-jmap-types" }
tokio = { version = "1", features = ["rt"] }
http = "1"
```

`jmap-types` already pulls in serde, serde_json, and thiserror. No other dependencies.
No cloud SDKs, no auth crates, no application logic.

The `http` crate provides framework-agnostic `Response<T>`, `StatusCode`, and header types.
`RequestError::into_response()` returns `http::Response<String>`, which axum, hyper, and any
`http`-based framework accept directly.

## Public API

### Types (re-exported from `jmap-types`)

The following are defined in `jmap-types` and re-exported here for convenience.
Callers that need only types (no server machinery) should depend on `jmap-types` directly.

```rust
// RFC 8620 §7.1 — from jmap-types
pub use jmap_types::{JmapError, Invocation, JmapRequest, JmapResponse,
                     ResultReference, Argument, Id, UTCDate, State};
```

### Parse and validate

```rust
/// Validate a raw JMAP request body. Returns Err(JmapError) on:
/// empty `using`, too many method calls, or deserialization failure.
pub fn parse_request(body: serde_json::Value, max_calls: usize)
    -> Result<JmapRequest, JmapError>
```

Note: `max_calls` is a parameter (not hardcoded) so each server sets its own limit.

### ResultReference resolution

```rust
/// Resolve all #-prefixed keys in `args` against `prior_responses` (RFC 8620 §9).
/// Modifies args in place. Returns Err(JmapError) on any resolution failure.
pub fn resolve_args(
    args: &mut serde_json::Value,
    prior_responses: &[(String, String, serde_json::Value)],
) -> Result<(), JmapError>
```

### HTTP helpers

```rust
pub fn error_invocation(call_id: &str, err: JmapError) -> Invocation
pub fn error_status(err: &JmapError) -> http::StatusCode
pub struct RequestError { /* private fields */ }
impl RequestError { pub fn into_response(self) -> http::Response<String> }
pub fn request_error(err: JmapError) -> RequestError
```

`RequestError::into_response()` returns an RFC 7807 Problem Details body.
No axum dependency — any HTTP framework that speaks `http::Response` can use this.

### Dispatcher

```rust
pub type HandlerFuture =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, JmapError>> + Send>>;

/// Implement this for each JMAP method handler.
/// CallerCtx is whatever your auth layer produces (Identity, session token, ()).
pub trait JmapHandler<CallerCtx>: Send + Sync {
    fn call(&self, method: String, call_id: String,
            args: serde_json::Value, caller: CallerCtx) -> HandlerFuture;
}

pub struct Dispatcher<CallerCtx> { ... }

impl<CallerCtx: Clone + Send + 'static> Dispatcher<CallerCtx> {
    pub fn new() -> Self
    pub fn register(&mut self, method: impl Into<String>,
                    handler: Box<dyn JmapHandler<CallerCtx>>)
    pub async fn dispatch(&self, request: JmapRequest,
                          caller: CallerCtx, session_state: String) -> JmapResponse
}
```

**Key design decisions:**

- Role checks are NOT in the dispatcher. Authorization is the caller's responsibility
  (do it in your HTTP middleware before calling `dispatch`).
- Unknown method name (no registered handler) → `unknownMethod` error invocation.
- Each handler runs in `tokio::task::spawn` for panic isolation. A panicking handler
  returns `serverFail`, not a crashed connection task.
- `createdIds` accumulation across `/set` calls in a batch is handled automatically
  per RFC 8620 §3.4.

## Module Layout

```
src/
  lib.rs        re-exports from jmap-types; Dispatcher<CallerCtx>
  parse.rs      parse_request, resolve_args
  response.rs   error_invocation, error_status, RequestError, request_error
```

`types.rs` and `resultref.rs` do not exist here — those live in `jmap-types`.

## What Stays Out of This Crate

| Item | Lives In |
|---|---|
| `Role` (Owner/Peer) | kith — Tailscale auth concept |
| `Identity` (Tailscale WhoIs result) | kith |
| Method-role ACL table | kith-jmap |
| Session/capability structs | kith-jmap (kith chat caps), stoa (NNTP mail caps) |
| `MAX_*` sizing constants | each consumer sets their own |
| `AuthError`, `KithError` | kith-core |

## Migration Path (after publishing)

### kith
1. Add `jmap-types` to kith-core `Cargo.toml`; add `jmap-server` to kith-jmap `Cargo.toml`
2. Remove `JmapError`, `JmapRequest`, `JmapResponse`, `Invocation`, `ResultReference`,
   `Argument` from kith-core — re-export from `jmap-types`
3. In kith-jmap:
   - All handlers become `JmapHandler<Identity>` (owner handlers ignore the caller arg)
   - `PeerJmapHandler` trait is deleted (merged into single generic trait)
   - Role check moves into the HTTP handler before `dispatcher.dispatch(...)` is called
   - `METHOD_ROLES` table stays — kith still enforces it, just not inside `Dispatcher`
   - `MAX_CALLS_IN_REQUEST = 16` stays — passed as arg to `parse_request(body, 16)`

### stoa
1. Add `jmap-server` to `crates/mail/Cargo.toml`
2. Delete `crates/mail/src/jmap/types.rs` (replaced by crate types)
3. Rewrite `crates/mail/src/jmap/dispatch.rs` using `Dispatcher<StoaCallerCtx>`
4. Keep stoa's own `build_session` equivalent (advertises different capabilities)

## Test Strategy

All tests are extracted from existing sources — no new test logic needed.

| Test area | Primary source | Notes |
|---|---|---|
| `parse_request` | `kith/crates/kith-jmap/src/lib.rs` lines 624–716 | 8 cases: valid, empty using, unknown cap, too many calls, malformed |
| `resolve_args` / ResultReference | `kith/crates/kith-jmap/src/lib.rs` lines 1180–1498 | 12+ cases: no refs, valid ref, unknown resultOf, bad path, multiple refs, name mismatch, key conflict |
| ResultReference atomic failure | `crate-jmapchat-server/jmapchat-server/src/ref_store.rs` | `resolve_is_atomic_on_error` — args unchanged on partial failure |
| `json_pointer` (RFC 6901) | `crate-jmapchat-server/jmapchat-server/src/ref_store.rs` | tilde-escaping (`~0`, `~1`), empty path, array index OOB |
| Dispatcher: error paths | `kith/crates/kith-jmap/src/lib.rs` lines 996–1154 | unknown method, no handler registered, handler error, session_state echo |
| Dispatcher: happy path | `kith/crates/kith-jmap/src/lib.rs` lines 1054–1077 | success invocation, batch mixing |
| Dispatcher: createdIds | `kith/crates/kith-jmap/src/lib.rs` lines 1662–1759 | accumulation across /set calls; absent when no /set |
| Dispatcher: panic isolation | `kith/crates/kith-jmap/src/lib.rs` lines 1762–1842 | panicking handler → serverFail, not crashed task |
| Dispatcher: ResultReference e2e | `kith/crates/kith-jmap/src/lib.rs` lines 1337–1407 | full dispatch path with #ids reference |
| Additional dispatch cases | `crate-jmapchat-server/jmapchat-server/tests/` | `dispatch_error_paths.rs`, `dispatch_happy_path.rs`, `result_reference_tests.rs` |

The CallerCtx substitution (replacing `Role`/`Identity` with `CallerCtx`) does not
require new tests — the same oracle values apply.
