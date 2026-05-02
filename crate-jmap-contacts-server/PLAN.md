# jmap-contacts-server — Implementation Plan

Backend-agnostic JMAP Contacts method handlers.

## Spec

- `~/PROJECT/jmap-chat-spec/references/draft-ietf-jmap-contacts-10.txt`

## Crate Family Position

```
jmap-types
    └── jmap-contacts-types
            └── jmap-contacts-server  ← this crate
```

## What This Crate Is

Method handlers for JMAP Contacts, plugged into `jmap-server::Dispatcher` via
`register_contacts_handlers(dispatcher, backend)`. Mirrors the pattern of
`jmap-mail-server` and `jmap-chat-server`.

## Methods (draft-ietf-jmap-contacts-10)

- `AddressBook/get`, `AddressBook/set`, `AddressBook/changes`, `AddressBook/query`
- `Contact/get`, `Contact/set`, `Contact/changes`, `Contact/query`, `Contact/queryChanges`
- `ContactGroup/get`, `ContactGroup/set`, `ContactGroup/changes`

## Backend Trait

```rust
pub trait ContactsBackend: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn get_objects<O>(...) -> Result<...>;
    async fn set_objects<O>(...) -> Result<...>;
    // etc.
}
```

## Module Layout

```
src/
  lib.rs            ContactsBackend trait, register_contacts_handlers
  addressbook.rs    AddressBook/get, set, changes, query
  contact.rs        Contact/get, set, changes, query, queryChanges
  group.rs          ContactGroup/get, set, changes
  backend.rs        ContactsBackend trait, error types
  helpers.rs        shared utilities
```

## Pattern to Follow

`~/PROJECT/JMAP/crate-jmap-mail-server/` — identical handler structure.
`~/PROJECT/JMAP/crate-jmap-chat-server/` — alternative reference.
