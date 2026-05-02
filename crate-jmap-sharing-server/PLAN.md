# jmap-sharing-server — Implementation Plan

Backend-agnostic JMAP Sharing method handlers (RFC 9670).

## Spec

- `~/PROJECT/jmap-chat-spec/references/rfc9670.txt` — normative

## Crate Family Position

```
jmap-types
    └── jmap-sharing-types
            └── jmap-sharing-server  ← this crate
```

## What This Crate Is

Method handlers for JMAP Sharing, plugged into `jmap-server::Dispatcher` via
`register_sharing_handlers(dispatcher, backend)`.

## Methods (RFC 9670)

- `Principal/get`, `Principal/changes`, `Principal/set`, `Principal/query`, `Principal/queryChanges`
- `ShareNotification/get`, `ShareNotification/changes`, `ShareNotification/set`,
  `ShareNotification/query`, `ShareNotification/queryChanges`

## Backend Trait

```rust
pub trait SharingBackend: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn get_objects<O>(...) -> Result<...>;
    async fn set_objects<O>(...) -> Result<...>;
    // etc.
}
```

## Integration with Domain Crates

This crate registers the `Principal/*` and `ShareNotification/*` methods.
The domain crates (mail, calendars, contacts, filenode) handle `shareWith`
on their own types independently — they do not call into this crate.
The `Principal` id is just a `jmap_types::Id` from their perspective.

The server consumer (e.g. the application wiring up the Dispatcher) is
responsible for registering both this crate's handlers and the domain
crate's handlers on the same Dispatcher.

## Module Layout

```
src/
  lib.rs            SharingBackend trait, register_sharing_handlers
  principal.rs      Principal/get, set, changes, query, queryChanges
  notification.rs   ShareNotification/get, set, changes, query, queryChanges
  backend.rs        SharingBackend trait, error types
  helpers.rs        shared utilities
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-server/` — identical handler structure.
