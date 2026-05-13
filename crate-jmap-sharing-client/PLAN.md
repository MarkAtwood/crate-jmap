# jmap-sharing-client — Implementation Plan

RFC 9670 (JMAP Sharing) method implementations on top of `jmap-base-client`.

## Crate Family Position

```
jmap-types
    ├── jmap-sharing-types
    │       └── (types used here)
    └── jmap-base-client
            └── jmap-sharing-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-base-client` that adds typed methods for every
RFC 9670 operation: `Principal/get`, `Principal/changes`, `Principal/set`,
`Principal/query`, `Principal/queryChanges`, `ShareNotification/get`,
`ShareNotification/changes`, `ShareNotification/set`,
`ShareNotification/query`, `ShareNotification/queryChanges`.

Consumers call `jmap-base-client::JmapClient::call()` directly or use the
typed helpers defined here. No new HTTP machinery — all network operations
go through `jmap-base-client`.

## What This Crate Is Not

- Not a server-side crate
- Not a standalone HTTP client (no auth, no transport — that's
  `jmap-base-client`)
- Not defining `shareWith` methods on domain types — those belong in the
  domain client crates (e.g., `jmap-mail-client`). A `Mailbox/set` call
  that modifies `shareWith` is issued through `JmapMailExt::mailbox_set`,
  not through this crate.

## What This Crate Explicitly Excludes

The `shareWith` property appears on `Mailbox` (via
`draft-ietf-jmap-mail-sharing-00`), and on future Calendar, AddressBook,
and FileNode types. This crate does NOT provide methods to set or modify
`shareWith` on those types. Each domain client crate handles its own
`shareWith` operations via its own extension trait, because:

1. The rights type (`MailboxRights`, `CalendarRights`, etc.) is defined in
   the domain types crate, not here.
2. The method call (`Mailbox/set`, `Calendar/set`, etc.) belongs to the
   domain crate.
3. This crate's sole responsibility is `Principal/*` and
   `ShareNotification/*`.

## Source Material

This is **greenfield** — no existing Rust implementation to extract from.

Design pattern to follow:
- `~/PROJECT/crate-jmap/crate-jmap-mail-client/` — identical extension trait
  pattern, identical `JmapMailExt` structure
- `~/PROJECT/crate-jmapchat-client/src/methods/` — how method
  inputs/outputs are structured and how `JmapRequestBuilder` is used
- `~/PROJECT/jmap-chat-spec/references/rfc9670.txt` — normative spec

## Dependencies

```toml
jmap-types         = { path = "../crate-jmap-types" }
jmap-sharing-types = { path = "../crate-jmap-sharing-types" }
jmap-base-client   = { path = "../crate-jmap-base-client" }
serde_json         = "1"
thiserror          = "2"
# No direct reqwest/tokio dependency — all I/O goes through jmap-base-client
```

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule). To add methods
to `JmapClient` from this crate, an **extension trait** is used — the same
pattern as `JmapMailExt` in `jmap-mail-client`:

```rust
pub trait JmapSharingExt {
    async fn principal_get(...) -> Result<...>;
    // ...
}

impl JmapSharingExt for JmapClient {
    async fn principal_get(...) -> Result<...> { ... }
    // ...
}
```

Callers must bring the trait into scope: `use jmap_sharing_client::JmapSharingExt;`

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used — no `async-trait`
crate needed. This works because we do not need `dyn JmapSharingExt`. If
dyn dispatch is ever required, wrap with `async-trait 0.1` at that time.

## Planned Public API

The shipped API uses the `SessionClient` shape (extension trait returns a
session-bound client; methods are inherent on `SessionClient`). Id-shaped
parameters are `&Id` / `&[Id]` / `Option<&[Id]>` / `Option<Vec<Id>>` and
state-shaped parameters are `&State` since bd:JMAP-6by7.7 (2026-05-09); the
brief drift to `&str` / `&[&str]` from earlier 0.1.x preview revisions was
reverted in that bead. The sketch below uses the trait-method shape from the
original plan; the actual shipped API on `SessionClient` is documented in the
README "Registered methods" table.

```rust
use jmap_base_client::{ClientError, JmapClient};
use jmap_sharing_types::{Principal, ShareNotification};
use jmap_types::{Id, State};

/// Extension trait adding RFC 9670 (JMAP Sharing) methods to [`JmapClient`].
///
/// Import this trait to use: `use jmap_sharing_client::JmapSharingExt;`
pub trait JmapSharingExt {
    // ── Principal ───────────────────────────────────────────────────────────

    /// Principal/get (RFC 9670 §2.1).
    ///
    /// Pass `ids: None` to fetch all principals in the account.
    async fn principal_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<Principal>, ClientError>;

    /// Principal/changes (RFC 9670 §2.2).
    ///
    /// May return `cannotCalculateChanges` if the server is backed by an
    /// external directory with no change tracking.
    async fn principal_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// Principal/set (RFC 9670 §2.3).
    ///
    /// Servers may reject creates/updates with `forbidden`. Only
    /// `name`, `description`, and `timeZone` on the caller's own Principal
    /// are guaranteed to be settable (if the server supports it at all).
    async fn principal_set(
        &self,
        account_id: &Id,
        req: SetRequest<Principal>,
    ) -> Result<SetResponse<Principal>, ClientError>;

    /// Principal/query (RFC 9670 §2.4).
    async fn principal_query(
        &self,
        account_id: &Id,
        req: PrincipalQueryRequest,
    ) -> Result<QueryResponse, ClientError>;

    /// Principal/queryChanges (RFC 9670 §2.5).
    async fn principal_query_changes(
        &self,
        account_id: &Id,
        req: QueryChangesRequest,
    ) -> Result<QueryChangesResponse, ClientError>;

    // ── ShareNotification ───────────────────────────────────────────────────

    /// ShareNotification/get (RFC 9670 §3.1).
    async fn share_notification_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<ShareNotification>, ClientError>;

    /// ShareNotification/changes (RFC 9670 §3.2).
    async fn share_notification_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// ShareNotification/set (RFC 9670 §3.3).
    ///
    /// Only `destroy` is supported by the server. Passing `create` or
    /// `update` entries will result in `forbidden` SetErrors for those
    /// entries. Use `destroy` to dismiss notifications.
    async fn share_notification_set(
        &self,
        account_id: &Id,
        destroy: &[Id],
    ) -> Result<SetResponse<ShareNotification>, ClientError>;

    /// ShareNotification/query (RFC 9670 §3.4).
    async fn share_notification_query(
        &self,
        account_id: &Id,
        req: ShareNotificationQueryRequest,
    ) -> Result<QueryResponse, ClientError>;

    /// ShareNotification/queryChanges (RFC 9670 §3.5).
    async fn share_notification_query_changes(
        &self,
        account_id: &Id,
        req: QueryChangesRequest,
    ) -> Result<QueryChangesResponse, ClientError>;
}

impl JmapSharingExt for JmapClient {
    // implementations in principal.rs, notification.rs
}
```

### Request types defined in this crate

`PrincipalQueryRequest` — wraps `PrincipalFilterCondition` and sort/limit
parameters for `Principal/query`.

`ShareNotificationQueryRequest` — wraps `ShareNotificationFilterCondition`
and sort/limit parameters for `ShareNotification/query`. The `created`
comparator property MUST be supported.

`QueryChangesRequest` — generic query-changes request (since_query_state,
filter, sort, max_changes, up_to_id). May be shared between Principal and
ShareNotification.

These are thin wrappers over the `jmap-types` generic request/response
primitives. They exist to give callers a typed API rather than raw
`serde_json::Value` arguments.

### `share_notification_set` API note

Because `ShareNotification/set` only supports `destroy`, the method
signature accepts `&[Id]` directly rather than a full `SetRequest<T>`. This
makes the common case (dismissing notifications) ergonomic and prevents
callers from accidentally constructing create/update payloads that the
server will reject.

Internally, the implementation builds the `SetRequest` JSON with only the
`destroy` field populated.

## Module Layout

```
src/
  lib.rs             pub trait JmapSharingExt; impl JmapSharingExt for JmapClient;
                     re-exports of request/response types
  principal.rs       Principal/get, /changes, /set, /query, /queryChanges —
                     request structs + JmapSharingExt method bodies
  notification.rs    ShareNotification/get, /changes, /set, /query, /queryChanges —
                     request structs + JmapSharingExt method bodies
```

## Extras-preservation policy (JMAP-lbdy)

This crate has **no in-scope structs** of its own under the workspace
extras-preservation policy (see workspace `AGENTS.md`). The crate
defines only `SessionClient` (internal Rust state, not wire-format).

Wire-format types reach callers through re-exports rather than locally
defined types:

- Standard response wrappers (`GetResponse<T>`, `SetResponse<T>`,
  `ChangesResponse`, `QueryResponse`, `QueryChangesResponse`) are
  re-exported from `jmap-types` and carry their own `extra` field per
  JMAP-lbdy.1.
- The data object types this crate operates on (`Principal`,
  `ShareNotification`, sharing-related types) are defined in
  `jmap-sharing-types` and carry their `extra` field per JMAP-lbdy.8.

### New-type rule

If a future method requires a locally-defined method-argument or
method-response struct, that new struct MUST include the `extra` field
from day one with the documented serde attributes:

```rust
#[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
pub extra: serde_json::Map<String, serde_json::Value>,
```

and at least one round-trip preservation test. Per the canonical-template
propagation rule (workspace AGENTS.md), the new struct should mirror the
shape used in the canonical `jmap-mail-client` extension-client template.

## Test Strategy

- All tests use `wiremock` via `jmap-base-client`'s HTTP layer — no live network
- Request serialization tests: construct a typed request, verify the emitted
  JSON matches the RFC 9670 example in §4.1
- Response deserialization tests: feed RFC 9670 example JSON, verify typed
  structs (`Principal`, `ShareNotification`) deserialize correctly
- `share_notification_set` test: verify that calling with `destroy: &[id]`
  emits JSON with only the `destroy` key (no `create`, no `update`)

Primary oracle: RFC 9670 §4.1 `Principal/get` request/response pair —
copy-paste from the spec as `serde_json::json!({...})` literals and assert
equality. Never derive expected values from the implementation under test.
