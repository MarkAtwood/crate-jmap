# jmap-contacts-server

## What it is

JMAP Contacts ([RFC 9610]) method handlers and the `ContactsBackend` trait.
Plugs into [`jmap-server`]'s `Dispatcher`. Implements all 9 contacts method
names. Storage-agnostic — consumers implement the `ContactsBackend` trait for
their own data layer.

## What it's for

Implements draft-ietf-jmap-contacts (RFC 9610) — AddressBook and ContactCard
objects with `ContactCard/copy` and `AddressBook/set` cascade semantics — so
consumers can wire JMAP Contacts method dispatch into their HTTP transport.
Sibling to `jmap-mail-server` (the canonical extension-server template) and
the other `jmap-*-server` crates; all of them inter-depend through the
workspace `JmapHandler` trait in `jmap-server`. The consumer supplies a
`ContactsBackend` impl (storage), a `CallerCtx` type (auth identity), and
wires the dispatcher into HTTP / SSE / WebSocket transport themselves.

## How to use

```rust
use std::sync::Arc;
use jmap_contacts_server::{ContactsBackend, register_contacts_handlers};
use jmap_server::Dispatcher;

// 1. Implement ContactsBackend for your storage layer (see trait section below).
struct MyBackend { /* db pool, etc. */ }
impl ContactsBackend for MyBackend { /* ... */ }

// 2. Wire all 9 contacts methods into a Dispatcher in one call.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_contacts_handlers(&mut dispatcher, Arc::new(MyBackend { /* ... */ }));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, (), session_state).await;
```

After `register_contacts_handlers` returns, the dispatcher handles every method
name listed in the [Registered methods](#registered-methods) section below. The
same `Arc<MyBackend>` can be shared with other parts of your application.

## Registered methods

| Object | Methods |
|---|---|
| `AddressBook` | `get`, `changes`, `set` |
| `ContactCard` | `get`, `changes`, `set`, `copy`, `query`, `queryChanges` |

Note: `AddressBook/query` and `AddressBook/queryChanges` are **not** registered.
RFC 9610 does not define these methods.

## ContactsBackend trait

Implement this trait to connect the handlers to your storage system. The
read-side methods (`get_objects`, `get_state`, `get_changes`, `query_objects`,
`query_changes`) are defined on the `JmapBackend` supertrait (from
`jmap-server`). `ContactsBackend` adds write operations and contacts-specific
operations.

```rust
pub trait ContactsBackend: JmapBackend {
    /// Create a new AddressBook or ContactCard.
    /// Returns (assigned_id, created_object) on success.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial update (patch) to an existing AddressBook or ContactCard.
    /// Returns Some(updated_object) if the backend modified server-set fields
    /// beyond the patch (RFC 8620 §5.3 echo); None if the patch was applied
    /// verbatim.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an AddressBook or ContactCard by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    /// Returns true if this account supports the given JMAP object type.
    /// Not called internally — used by the session capability builder.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Copy a ContactCard from one account to another.
    /// Called by ContactCard/copy. The card has already been fetched from
    /// from_account_id; the backend must store it in to_account_id and
    /// return the newly assigned (id, card).
    fn copy_contact_card(
        &self,
        from_account_id: &Id,
        to_account_id: &Id,
        card: ContactCard,
    ) -> impl Future<Output = Result<(Id, ContactCard), BackendSetError<Self::Error>>> + Send;

    /// Check whether an AddressBook has any ContactCards in it.
    /// Called by AddressBook/set destroy processing when onDestroyRemoveContents
    /// is false (the default). If this returns true, the destroy is rejected
    /// with addressBookHasContents.
    fn address_book_has_contents(
        &self,
        account_id: &Id,
        address_book_id: &Id,
    ) -> impl Future<Output = bool> + Send;
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `addressBookHasContents`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a
  `serverFail` response

## How it works

### Registration

`register_contacts_handlers` uses `ClosureHandler` (provided by
`jmap-server`) to wrap each handler function and `Arc<B>` into a
`JmapHandler<B::CallerCtx>` and registers it with the dispatcher. The
dispatcher's `CallerCtx` value is forwarded into each closure as `ctx`, then
passed by reference (`&ctx`) into the standard `handle_*` handler bodies,
which themselves forward `caller: &B::CallerCtx` into every `ContactsBackend`
method call. One `Arc::clone` per method name; no heap allocation per request.

### Handler structure

Each handler function (e.g. `handle_address_book_get`, `handle_contact_card_set`)
receives `args: Value` containing the raw JMAP method arguments after
ResultReference resolution, calls `ContactsBackend` methods, and returns the
RFC 8620 response object.

### AddressBook/set — onSuccessSetIsDefault semantics

RFC RFC 9610 §2.3 defines an `onSuccessSetIsDefault` argument that, after
all create/update/destroy operations succeed, designates one AddressBook as
the account default. The handler enforces the single-default invariant:

1. The main set operations run first. If any fail with a `serverFail`, the
   `onSuccessSetIsDefault` step is skipped entirely (guard condition: only runs
   when all main operations succeeded).
2. The handler calls `update_object` to set `isDefault: true` on the designated
   book. The backend is responsible for atomically demoting any previously
   default book.
3. After the update, the handler calls `get_objects` to re-fetch all affected
   address books. Any book whose `isDefault` changed from `true` to `false`
   appears in the `updated` map of the response so the client learns about the
   demotion.

The re-fetch is O(N) in the number of address books. Backends with many books
should implement the single-default invariant atomically inside `update_object`
and return the demoted book in the `Some(obj)` response to avoid the extra
query.

### AddressBook/set — onDestroyRemoveContents cascade

When `onDestroyRemoveContents: true` and the AddressBook being destroyed
contains ContactCards, the handler:

- Destroys ContactCards that belong only to that AddressBook (via
  `destroy_object`).
- Patches `addressBookIds/{id}: null` on cards shared with other address books.

When `onDestroyRemoveContents` is absent or false (the default) and the book
is non-empty, `address_book_has_contents` is called. If it returns `true`, the
destroy is rejected with an `addressBookHasContents` SetError.

### ContactCard/copy — JSON Pointer patch semantics

`ContactCard/copy` copies a card from one account to another. After fetching
the source card, the handler applies any `update` patches supplied in the copy
request. Patches follow RFC 8620 §5.3 JSON Pointer semantics:

- Paths are `/`-separated pointer segments.
- `/` within a segment is encoded as `~1`; `~` is encoded as `~0`.
- Setting a path to `null` removes that property from the card.

The patched card is then passed to `copy_contact_card` for storage in the
destination account.

## Capability URI

Include this in your Session object's `capabilities` map when the contacts
extension is available:

```rust
// Re-exported from jmap-contacts-types:
pub const JMAP_CONTACTS_URI: &str = "urn:ietf:params:jmap:contacts";
```

## CallerCtx

`register_contacts_handlers` registers each method as a `ClosureHandler`
that forwards the dispatcher's `CallerCtx` value into the closure as `ctx`,
which the closure body then passes by reference (`&ctx`) into the standard
`handle_*` handler. The handler in turn forwards `caller: &B::CallerCtx` into
every `ContactsBackend` (and inherited `JmapBackend`) method call.

To use this:

1. Pick a `CallerCtx` type for your backend — e.g. `()` if no auth context is
   needed, or a struct like `AuthCtx { user_id: Id, scopes: Vec<Scope> }`.
2. Implement `JmapBackend` with `type CallerCtx = AuthCtx;` (the bound is
   `Clone + Send + Sync + 'static`).
3. Use the matching `Dispatcher<AuthCtx>` and pass the constructed auth
   context as the second argument to `Dispatcher::dispatch(request, auth, …)`.
4. Inside `ContactsBackend` method bodies, read `caller: &AuthCtx` to apply
   per-user visibility rules, rate limits, etc.

Backends that don't need an auth identity use `type CallerCtx = ();` and a
`Dispatcher<()>`. Both shapes register the same way.

## Gotchas

- `address_book_has_contents` and `copy_contact_card` must be implemented by
  the backend; no default implementation ships with this crate.
- The `onSuccessSetIsDefault` re-fetch (`get_objects` after the update) is O(N)
  in the number of address books. Backends with many books should implement the
  single-default invariant atomically in `update_object` and return the demoted
  book in the `Some(obj)` response.
- No storage backend ships with this crate. The in-memory `MemoryBackend` in
  `tests/` is a test harness only.

## Crate family

```
jmap-types
    ├── jmap-server              Dispatcher this plugs into
    └── jmap-contacts-types      domain types (AddressBook, ContactCard, etc.)
            └── jmap-contacts-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## References

- **[RFC 9610]** — JMAP Contacts (normative for all method
  semantics, AddressBook/set arguments, ContactCard/copy)
- **[RFC 9553]** — JSContact (normative for ContactCard field schema)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference,
  `/set` response shape, `/copy` semantics)

[RFC 9610]: https://www.rfc-editor.org/rfc/rfc9610
[RFC 9553]: https://www.rfc-editor.org/rfc/rfc9553
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[`jmap-server`]: ../crate-jmap-server
