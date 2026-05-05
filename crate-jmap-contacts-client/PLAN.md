# jmap-contacts-client — Implementation Plan

JMAP Contacts (RFC 9610) method implementations on top of
`jmap-base-client`.

## Crate Family Position

```
jmap-types
    ├── jmap-contacts-types
    │       └── (types used here)
    └── jmap-base-client
            └── jmap-contacts-client  ← this crate
```

## What This Crate Is

An extension layer over `jmap-base-client` that adds typed methods for every
JMAP Contacts operation: `AddressBook/get`, `AddressBook/changes`,
`AddressBook/set`, `ContactCard/get`, `ContactCard/changes`,
`ContactCard/query`, `ContactCard/queryChanges`, `ContactCard/set`,
`ContactCard/copy`.

Consumers call `jmap-base-client::JmapClient::call()` directly or use the
typed helpers defined here.  No new HTTP machinery — all network operations go
through `jmap-base-client`.

## What This Crate Is Not

- Not a server-side crate
- Not a standalone HTTP client (no auth, no transport — that's `jmap-base-client`)
- Not handling vCard import/export or CardDAV synchronization
- Not a command-line address book application

## Source Material

This is **greenfield** — no existing Rust implementation to extract from.

Design pattern to follow:
- `~/PROJECT/JMAP/crate-jmap-mail-client/` — identical extension trait pattern,
  module layout, and test strategy
- `~/PROJECT/crate-jmapchat-client/src/methods/` — how method inputs/outputs
  are structured and how `JmapRequestBuilder` is used to issue calls
- `~/PROJECT/jmap-chat-spec/references/RFC 9610.txt` —
  normative spec (§2–§4 for method arguments and responses)
- `~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol (get/set/
  query/changes/queryChanges/copy request and response structures)

## Dependencies

```toml
jmap-types          = { path = "../crate-jmap-types" }
jmap-contacts-types = { path = "../crate-jmap-contacts-types" }
jmap-base-client    = { path = "../crate-jmap-base-client" }
serde_json          = "1"
thiserror           = "2"
```

No direct reqwest/tokio dependency — all I/O goes through `jmap-base-client`.

## Extension Trait Pattern

Cross-crate inherent impls are not valid Rust (orphan rule).  To add methods to
`JmapClient` from this crate, we use an **extension trait**:

```rust
pub trait JmapContactsExt {
    async fn address_book_get(...) -> Result<...>;
}

impl JmapContactsExt for JmapClient {
    async fn address_book_get(...) -> Result<...> { ... }
}
```

Callers must bring the trait into scope: `use jmap_contacts_client::JmapContactsExt;`

Rust 1.75 AFIT (async fn in trait, via RPITIT) is used — no `async-trait` crate
needed.  This works because we do not need `dyn JmapContactsExt`.  If dyn
dispatch is ever required, wrap with `async-trait 0.1` at that time.

## Planned Public API

```rust
use jmap_base_client::{ClientError, JmapClient};
use jmap_base_client::response::{
    ChangesResponse, GetResponse, QueryResponse, QueryChangesResponse, SetResponse,
};
use jmap_contacts_types::{
    AddressBook,
    ContactCard,
    ContactCardFilter, ContactCardComparator,
};
use jmap_types::{Id, State};

/// Extension trait adding JMAP Contacts methods to [`JmapClient`].
///
/// Import this trait to use: `use jmap_contacts_client::JmapContactsExt;`
///
/// All methods serialize to a single JMAP method call and deserialize the
/// response. For multi-method request batching, use
/// `JmapClient::call()` directly with a `JmapRequestBuilder`.
pub trait JmapContactsExt {
    // ── AddressBook ──────────────────────────────────────────────────────

    /// Fetch address books by id. Pass `ids: None` to fetch all.
    async fn address_book_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<AddressBook>, ClientError>;

    /// Fetch changes to address books since `since_state`.
    async fn address_book_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// Create, update, or destroy address books.
    ///
    /// `on_destroy_remove_contents`: if true, cards in destroyed address books
    /// are also destroyed (or detached if they belong to other books).
    /// `on_success_set_is_default`: if Some, set this address book as the
    /// default after all creates/updates/destroys succeed.
    async fn address_book_set(
        &self,
        account_id: &Id,
        req: AddressBookSetRequest,
    ) -> Result<SetResponse<AddressBook>, ClientError>;

    // ── ContactCard ──────────────────────────────────────────────────────

    /// Fetch contact cards by id. Pass `ids: None` to fetch all.
    async fn contact_card_get(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[&str]>,
    ) -> Result<GetResponse<ContactCard>, ClientError>;

    /// Fetch changes to contact cards since `since_state`.
    async fn contact_card_changes(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResponse, ClientError>;

    /// Query contact cards with optional filter and sort.
    async fn contact_card_query(
        &self,
        account_id: &Id,
        req: ContactCardQueryRequest,
    ) -> Result<QueryResponse, ClientError>;

    /// Fetch incremental changes to a query result since `since_query_state`.
    async fn contact_card_query_changes(
        &self,
        account_id: &Id,
        req: ContactCardQueryChangesRequest,
    ) -> Result<QueryChangesResponse, ClientError>;

    /// Create, update, or destroy contact cards.
    async fn contact_card_set(
        &self,
        account_id: &Id,
        req: SetRequest<ContactCard>,
    ) -> Result<SetResponse<ContactCard>, ClientError>;

    /// Copy a contact card from one account to another.
    async fn contact_card_copy(
        &self,
        from_account_id: &Id,
        to_account_id: &Id,
        req: ContactCardCopyRequest,
    ) -> Result<ContactCardCopyResponse, ClientError>;
}

impl JmapContactsExt for JmapClient {
    // implementations in addressbook.rs, card.rs
}
```

## Request and Response Types

These types are defined in `src/` (not in `jmap-contacts-types`, which holds
only wire data objects, not method arguments):

```rust
/// Arguments for AddressBook/set beyond the standard SetRequest fields.
pub struct AddressBookSetRequest {
    pub if_in_state: Option<State>,
    pub create: Option<HashMap<String, AddressBook>>,
    pub update: Option<HashMap<Id, serde_json::Value>>,  // PatchObject
    pub destroy: Option<Vec<Id>>,
    pub on_destroy_remove_contents: bool,                // default: false
    pub on_success_set_is_default: Option<Id>,           // RFC 9610 §2.3
}

/// Arguments for ContactCard/query.
pub struct ContactCardQueryRequest {
    pub filter: Option<ContactCardFilter>,
    pub sort: Option<Vec<ContactCardComparator>>,
    pub position: Option<i64>,
    pub anchor: Option<Id>,
    pub anchor_offset: Option<i64>,
    pub limit: Option<u64>,
    pub calculate_total: Option<bool>,
}

/// Arguments for ContactCard/queryChanges.
pub struct ContactCardQueryChangesRequest {
    pub since_query_state: State,
    pub filter: Option<ContactCardFilter>,
    pub sort: Option<Vec<ContactCardComparator>>,
    pub max_changes: Option<u64>,
    pub up_to_id: Option<Id>,
    pub calculate_total: Option<bool>,
}

/// Arguments for ContactCard/copy (RFC 8620 §5.4).
pub struct ContactCardCopyRequest {
    pub if_from_in_state: Option<State>,
    pub if_in_state: Option<State>,
    pub create: HashMap<String, ContactCardCopyEntry>,
    pub on_success_destroy_original: bool,  // default: false
}

pub struct ContactCardCopyEntry {
    pub id: Id,                                        // source card id
    pub address_book_ids: HashMap<Id, bool>,           // dest address books
}

/// Response for ContactCard/copy.
pub struct ContactCardCopyResponse {
    pub from_account_id: Id,
    pub account_id: Id,
    pub old_state: Option<State>,
    pub new_state: State,
    pub created: Option<HashMap<String, ContactCard>>,
    pub not_created: Option<HashMap<String, SetError>>,
}
```

## Module Layout

```
src/
  lib.rs          pub trait JmapContactsExt; impl JmapContactsExt for JmapClient;
                  re-exports of request/response types
  addressbook.rs  AddressBook/get, /changes, /set — request building and
                  response parsing; AddressBookSetRequest type
  card.rs         ContactCard/get, /changes, /query, /queryChanges, /set, /copy —
                  request building and response parsing;
                  ContactCardQueryRequest, ContactCardQueryChangesRequest,
                  ContactCardCopyRequest, ContactCardCopyResponse types
```

## Test Strategy

- All tests use `wiremock` (or equivalent mock HTTP server) via
  `jmap-base-client`'s HTTP layer — no live network
- Request serialization tests: construct a typed request, serialize to JSON,
  assert it matches the wire format expected by the spec
- Response deserialization tests: feed JSON from the spec examples (§4.1, §4.2),
  assert the typed structs are populated correctly
- Primary oracle: the §4.1 example exchange in RFC 9610

### Key test cases

- `address_book_get(None)` serializes to `{"accountId": ..., "ids": null}` and
  deserializes the §4.1 AddressBook response including `shareWith` and `myRights`
- `address_book_set` with `onSuccessSetIsDefault` serializes the extra field and
  deserializes the §4.2 response (two entries in `updated`)
- `contact_card_get(None)` deserializes the §4.1 ContactCard response including
  `addressBookIds`, `name.components`, and `emails` map
- `contact_card_query` with `ContactCardFilter::Condition(ContactCardFilterCondition
  { in_address_book: Some(id), ..Default::default() })` serializes `inAddressBook`
  correctly
- `contact_card_query` with sort by `"name/surname"` serializes correctly
- `contact_card_set` creates a new card; response includes server-assigned `id`
- `contact_card_copy` serializes `fromAccountId`, `accountId`, and the
  `addressBookIds` destination; deserializes the copy response

## Congruence with jmap-mail-client

| jmap-mail-client method | jmap-contacts-client method | Notes |
|---|---|---|
| `mailbox_get` | `address_book_get` | Same pattern; ids=None fetches all |
| `mailbox_set` | `address_book_set` | Extra args for both (onDestroy...) |
| `email_get` | `contact_card_get` | Both support partial properties |
| `email_set` | `contact_card_set` | Standard SetRequest<T> |
| `email_query` | `contact_card_query` | Typed filter + comparator |
| `email_changes` | `contact_card_changes` | Identical signature |
| (none) | `contact_card_query_changes` | Mail client lacks this method |
| (none) | `contact_card_copy` | Mail client has email_copy equivalent |
