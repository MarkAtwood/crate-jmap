# jmap-contacts-server — Implementation Plan

JMAP Contacts (RFC 9610) method handlers.  Plugs into
`jmap-server`'s `Dispatcher`.  Backend-agnostic: defines a `ContactsBackend`
trait; consumers provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server              dispatcher
    └── jmap-contacts-types      data types
            └── jmap-contacts-server  ← this crate
```

## What This Crate Is

Method handler implementations for every JMAP Contacts method: AddressBook and
ContactCard.  (Groups are ContactCards with `kind: "group"` — no separate type.)

Defines a `ContactsBackend` trait that the application implements.  The crate
handles all JMAP protocol semantics (ordering, partial success,
`onDestroyRemoveContents`, ResultReference threading, error type mapping).  The
backend handles storage.

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage (SQLite, PostgreSQL, CardDAV, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not handling vCard import/export — backend's responsibility if needed
- Not axum-specific — any `http`-based framework works

## Source Material

### Normative

`~/PROJECT/jmap-chat-spec/references/RFC 9610.txt` — read
the relevant section before implementing each handler.  Wire field names and
method semantics come from the spec, not from memory.

`~/PROJECT/jmap-chat-spec/references/rfc8620.txt` — base protocol
(get/changes/query/queryChanges/set/copy semantics, error types).

`~/PROJECT/jmap-chat-spec/references/rfc9553.txt` — JSContact Card format
(for understanding ContactCard fields used in filters and handlers).

### Backend trait pattern — follow this exactly

`~/PROJECT/JMAP/crate-jmap-mail-server/src/backend.rs`

`JmapBackend` supertrait (in `jmap-server`) provides the generic read-side
operations.  `MailBackend` extends it with write operations and mail-specific
methods.  `ContactsBackend` follows the same pattern: extends `JmapBackend`
with write operations and contacts-specific methods.

The actual trait signatures to copy: `create_object`, `update_object`,
`destroy_object` — identical signatures to `MailBackend`.  The contacts-specific
method (`copy_contact_card`) mirrors `copy_email`.

### Handler logic reference — read, do not copy

`~/PROJECT/JMAP/crate-jmap-mail-server/src/` — the mail server handler modules
are the primary structural reference.  Mailbox maps to AddressBook;
Email maps to ContactCard.

`~/PROJECT/crate-jmapchat-server/jmapchat-server/src/` — secondary reference
for the StorageBackend/JmapBackend pattern.

## Dependencies

```toml
jmap-types          = { path = "../crate-jmap-types" }
jmap-contacts-types = { path = "../crate-jmap-contacts-types" }
jmap-server         = { path = "../crate-jmap-server" }
serde               = { version = "1", features = ["derive"] }
serde_json          = "1"
thiserror           = "2"
tokio               = { version = "1", features = ["rt"] }
```

No vCard libraries.  No HTTP client.  No database drivers.

## JMAP Method Coverage (RFC 9610)

| Object | Methods | Draft §§ | Notes |
|---|---|---|---|
| AddressBook | get | §2.1 | Standard get; ids=null fetches all |
| AddressBook | changes | §2.2 | Standard changes |
| AddressBook | set | §2.3 | Extra args: onDestroyRemoveContents, onSuccessSetIsDefault |
| ContactCard | get | §3.1 | Standard get |
| ContactCard | changes | §3.2 | Standard changes |
| ContactCard | query | §3.3 | filter + sort; see §3.3.1, §3.3.2 |
| ContactCard | queryChanges | §3.4 | Standard queryChanges |
| ContactCard | set | §3.5 | Standard set; photo upload via blobId |
| ContactCard | copy | §3.6 | Standard copy (RFC 8620 §5.4) |

Total: 9 method registrations.

Note: there is no `AddressBook/query` or `AddressBook/queryChanges` in the draft.
AddressBooks are fetched via `AddressBook/get` (ids=null to fetch all).

## Key Design Decisions

### 1. ContactsBackend mirrors MailBackend exactly for generic CRUD

Same AFIT pattern as `MailBackend` in `jmap-mail-server`, which itself mirrors
`StorageBackend` in `jmapchat-server`.  Implementors who have built a
`MailBackend` will find the write-side contract identical.

`ContactsBackend` is NOT object-safe (generic methods).  The dispatcher and
all handlers are generic over `B: ContactsBackend`, monomorphized at compile
time.  No `#[async_trait]` macro — AFIT is stable since Rust 1.75.

### 2. AddressBook/set: onDestroyRemoveContents is handler logic

The contacts draft (§2.3) defines `onDestroyRemoveContents: Boolean` (default:
false) as an extra argument to `AddressBook/set`.  The handler implements this:

- When false (default): if the AddressBook contains any ContactCards, the
  destroy MUST fail with `addressBookHasContents` SetError.  The handler calls
  `query_objects<ContactCard>` (filter by `inAddressBook`) to check, then
  returns the error without touching the backend.
- When true: the handler calls `query_objects<ContactCard>` to list all cards
  in the address book, then for each card:
  - If the card belongs to only this address book: `destroy_object<ContactCard>`
  - If the card belongs to other address books too: `update_object<ContactCard>`
    to remove this address book from `addressBookIds`
  - Then `destroy_object<AddressBook>`

All of this is handler logic.  The backend has no `onDestroyRemoveContents`
concept.

### 3. AddressBook/set: onSuccessSetIsDefault is handler logic

The contacts draft (§2.3) defines `onSuccessSetIsDefault: Id|null`.  If set and
all creates/updates/destroys succeed, the handler calls
`update_object<AddressBook>` on the target ID to set `isDefault: true`, and
`update_object<AddressBook>` on the previous default (if any) to set
`isDefault: false`.  The previous default must be found via
`query_objects<AddressBook>` filtered to `isDefault: true`.

All changed objects from this post-set operation MUST appear in the `updated`
map of the set response.  The handler assembles this; the backend just applies
the updates.

### 4. addressBookHasContents — custom SetError type

The contacts draft (§7.4.1) registers `addressBookHasContents` as a new JMAP
SetError type.  The handler returns this error (as a `SetError` with type
`"addressBookHasContents"`) when destroying an AddressBook that has contents
and `onDestroyRemoveContents` is false.

Represent as a variant in `BackendSetError` or as a handler-level check that
constructs a `SetError { type: "addressBookHasContents", ... }` before the
backend is consulted.  Prefer the handler-level check: it requires no backend
involvement.

### 5. ContactCard/set: photo blobId validation

The draft (§3.5) requires the server to reject attempts to set a Media object
with a non-image file type as the photo.  The handler calls
`blob_exists(account_id, blob_id)` and checks the media type, rejecting with
`invalidProperties` if the blob is not a recognized image type.  Backend
provides `blob_exists`; the handler applies the policy.

### 6. ContactCard/copy follows RFC 8620 §5.4

The draft (§3.6) lists `/copy` as a standard method.  The handler follows RFC
8620 §5.4 semantics exactly: copy from one account to another, assigning new IDs
in the destination.  A dedicated `copy_contact_card` backend method handles the
cross-account duplication.

### 7. ContactCard groups — no special handler

A ContactCard with `kind: "group"` is handled by the same generic
`ContactCard/get`, `/set`, `/query` handlers as any other card.  The backend
stores it identically.  Queries with `hasMember` in the filter condition are
dispatched to the backend's `query_objects<ContactCard>` with the filter
passed through.

### 8. queryChanges backend path

`ContactCard/queryChanges` requires a backend method beyond generic get/changes.
`query_changes<ContactCard>` returns `QueryChangesResult` (with `removed: Vec<Id>`
and `added: Vec<AddedItem>`).  Backends that cannot compute incremental query
deltas return `Err(BackendChangesError::TooManyChanges { limit: 0 })`, which the
handler maps to `cannotCalculateChanges` per RFC 8620 §5.6.

Note: there is no `AddressBook/queryChanges` in the contacts draft.

### 9. register_contacts_handlers is the entry point

One function registers all 9 method handlers with the caller's
`jmap-server::Dispatcher<C>`.  The backend is wrapped in `Arc<B>` and cloned
into each handler closure — same pattern as `register_mail_handlers` and
`register_chat_handlers`.

### 10. Capability URI

`urn:ietf:params:jmap:contacts` — registered per RFC 9610 §1.4.1.

Account capability object contains:
- `maxAddressBooksPerCard: UnsignedInt|null` — max AddressBooks per card
- `mayCreateAddressBook: Boolean` — whether the account may create AddressBooks

The handler crate does not produce the capability object — that is the
application's responsibility when building the session response.  But the
capability URI string is a `pub const` in this crate for use by applications.

## Planned Public API

```rust
pub use jmap_contacts_types::{
    AddressBook, AddressBookRights,
    ContactCard,
    ContactCardFilter, ContactCardFilterCondition, ContactCardComparator,
};
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError,
    ChangesResult, GetObject, JmapBackend, JmapObject,
    QueryChangesResult, QueryObject, QueryResult,
    SetError, SetErrorType, SetObject,
};

/// Capability URI for JMAP Contacts.
pub const CONTACTS_CAPABILITY: &str = "urn:ietf:params:jmap:contacts";

/// Storage backend for JMAP Contacts method handlers.
///
/// Read-side operations (get_objects, get_state, get_changes, query_objects,
/// query_changes) are inherited from JmapBackend.
///
/// Uses AFIT (async fn in trait, stable since Rust 1.75). Not object-safe;
/// always monomorphized at compile time.
///
/// Implementor invariants:
/// 1. State monotonicity: get_state returns a different token after every
///    successful mutation.
/// 2. Initial state: "0" is always the valid initial state sentinel.
/// 3. Partial set success: per-object failures do not roll back other objects
///    in the same /set call (RFC 8620 §5.3).
/// 4. isDefault invariant: at most one AddressBook per account has isDefault=true.
///    The backend MUST enforce this when update_object<AddressBook> sets it.
#[allow(async_fn_in_trait)]
pub trait ContactsBackend: JmapBackend {
    // ── Write operations (same contract as MailBackend) ──────────────────

    /// Create a new object. Returns (assigned_id, created_object).
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing object.
    /// Returns Some(updated_object) if the backend modified server-set fields,
    /// None if the patch was applied verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // ── Contacts-specific methods ────────────────────────────────────────

    /// Copy a ContactCard from one account to another (RFC 8620 §5.4).
    /// Returns the new id and the created ContactCard in to_account_id.
    fn copy_contact_card(
        &self,
        from_account_id: &jmap_types::Id,
        card_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        address_book_ids: &HashMap<jmap_types::Id, bool>,
    ) -> impl Future<
        Output = Result<(jmap_types::Id, ContactCard), BackendSetError<Self::Error>>,
    > + Send;

    /// Return true if blob_id exists in account_id's blob store.
    /// Used by ContactCard/set to validate photo blobIds.
    fn blob_exists(
        &self,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl Future<Output = bool> + Send;

    /// Returns true if this account supports the given JMAP object type.
    fn supports_type<O: JmapObject>(&self) -> bool;
}

/// Register all JMAP Contacts handlers with a jmap-server Dispatcher.
///
/// After calling this, the dispatcher handles all 9 JMAP Contacts method names.
/// Wrap backend in Arc before passing — it is cloned into each handler.
pub fn register_contacts_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: ContactsBackend + 'static,
    C: Clone + Send + 'static;
```

## Module Layout

```
src/
  lib.rs            pub re-exports; CONTACTS_CAPABILITY const;
                    register_contacts_handlers
  backend.rs        ContactsBackend trait; re-exports from jmap-server
  addressbook.rs    AddressBook/get, /changes, /set
                    (includes onDestroyRemoveContents and onSuccessSetIsDefault logic)
  card.rs           ContactCard/get, /changes, /query, /queryChanges, /set, /copy
  helpers.rs        shared utilities (query building, error mapping)
```

## Test Strategy

A `MemoryBackend` in `tests/common/mod.rs` provides an in-memory `HashMap`
implementation of `ContactsBackend`.  This serves as both the test harness and
the canonical example for implementors.

```
tests/
  common/
    mod.rs              MemoryBackend implementation
  addressbook_tests.rs
  card_tests.rs
```

Test oracles come from the RFC 9610 example exchanges
(§4.1 and §4.2) and from hand-constructed JSON matching the spec field
descriptions.  Extract them verbatim from the spec and hardcode as
`serde_json::json!({...})` literals.  Never derive expected values from the
implementation under test.

Each integration test constructs a `JmapRequest` matching a spec example,
calls `Dispatcher::dispatch`, and asserts the response matches the spec example
response.

### Non-trivial test cases to include

- `AddressBook/get`: ids=null returns all address books; matches §4.1 response
- `AddressBook/set`: create, update name, destroy; verify state changes
- `AddressBook/set`: destroy with cards, `onDestroyRemoveContents: false` →
  `addressBookHasContents` error
- `AddressBook/set`: destroy with cards, `onDestroyRemoveContents: true` →
  cards destroyed or detached; address book destroyed
- `AddressBook/set`: `onSuccessSetIsDefault` changes default; both old and new
  default appear in `updated` map (§4.2 example)
- `AddressBook/changes`: returns created/updated/destroyed ids since given state
- `ContactCard/get`: ids=null returns all cards; `addressBookIds` field present
- `ContactCard/get`: partial properties response (only `name` and `emails`)
- `ContactCard/set`: create a card in an address book; verify `addressBookIds`
- `ContactCard/set`: update `name/full` patch; verify server echoes changed fields
- `ContactCard/set`: create a group card (`kind: "group"`, `members` map)
- `ContactCard/set`: destroy — card removed from address book
- `ContactCard/query`: filter by `inAddressBook`; verify only cards in that book
- `ContactCard/query`: filter by `email`; verify matching on EmailAddress.address
- `ContactCard/query`: filter by `name/given`; verify matching on NameComponent
- `ContactCard/query`: sort by `name/surname`; verify ascending order
- `ContactCard/queryChanges`: `cannotCalculateChanges` when backend returns
  TooManyChanges
- `ContactCard/copy`: card appears in destination account with new id;
  source card unchanged
- `ContactCard/set`: photo with non-image blobId → `invalidProperties`
