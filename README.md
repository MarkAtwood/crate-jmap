# jmap-*

A Rust workspace implementing the JMAP protocol suite as a set of composable,
backend-agnostic library crates. Covers [RFC 8620] (JMAP Core), [RFC 8621] (JMAP
for Mail), [RFC 8887] (JMAP over WebSocket), [RFC 9670] (JMAP Sharing), the
JSCalendar-based Calendars and Tasks extensions, the JMAP Chat extension, JMAP
FileNode, and JMAP Contacts.

This is a **protocol library**, not a server application. You bring storage,
authentication, and HTTP. The library handles JMAP wire framing, ResultReference
resolution, batch dispatch, type-safe request/response shapes, and the
specification-mandated validation logic for each object type.

---

## Architecture

Each protocol area follows a three-crate pattern:

```
*-types    — serde wire types; no async, no handlers, no network
*-server   — method handlers + Backend trait; storage-agnostic
*-client   — typed async methods over jmap-base-client
```

The server and client crates are independent. A project that only sends JMAP
requests imports the client crates. A project that only serves JMAP requests
imports the server crates. Nothing forces you to use both.

All server crates plug into a single `jmap-server::Dispatcher`:

```rust
use std::sync::Arc;
use jmap_server::Dispatcher;
use jmap_mail_server::register_mail_handlers;
use jmap_calendars_server::register_calendars_handlers;

let mut dispatcher: Dispatcher<AuthCtx> = Dispatcher::new();
register_mail_handlers(&mut dispatcher, Arc::new(my_mail_backend));
register_calendars_handlers(&mut dispatcher, Arc::new(my_cal_backend));

// In your HTTP handler:
let response = dispatcher.dispatch(request, auth_ctx, session_state).await;
```

`CallerCtx` (the `AuthCtx` parameter above) is cloned into every handler call.
Use it to carry per-request auth identity, tenant ID, or rate-limit tokens.

---

## Crates

### Foundation

| Crate | Spec | Role |
|---|---|---|
| `jmap-types` | [RFC 8620] | `Id`, `State`, `JmapError`, `JmapRequest/Response`, `ResultReference`, `Argument<T>` |
| `jmap-server` | [RFC 8620] | `Dispatcher`, ResultReference resolution, generic `/get` `/changes` `/query` `/queryChanges` handlers |
| `jmap-base-client` | [RFC 8620], [RFC 8887] | Auth, session fetch, blob upload/download, SSE stream, WebSocket |

### Mail

| Crate | Spec | Role |
|---|---|---|
| `jmap-mail-types` | [RFC 8621] | `Email`, `Mailbox`, `Thread`, `Identity`, `EmailSubmission`, `VacationResponse` |
| `jmap-mime` | [RFC 5322] | Adapter: `mime_tree` parsed output → `jmap-mail-types` body structures |
| `jmap-mail-server` | [RFC 8621] | All 26 RFC 8621 method handlers; `MailBackend` trait |
| `jmap-mail-client` | [RFC 8621] | Typed client methods for all 26 methods |

### Chat

| Crate | Spec | Role |
|---|---|---|
| `jmap-chat-types` | [draft-atwood-jmap-chat] | `Chat`, `Message`, `Space`, `ReadPosition`, and push/WebSocket event types |
| `jmap-chat-server` | [draft-atwood-jmap-chat] | Chat method handlers; `ChatBackend` trait |
| `jmap-chat-client` | [draft-atwood-jmap-chat] | Typed client methods for all Chat methods |

### Contacts

| Crate | Spec | Role |
|---|---|---|
| `jmap-contacts-types` | [draft-ietf-jmap-contacts-10], [RFC 9553] | `ContactCard`, `AddressBook`, `AddressBookRights`; JSContact sub-objects as `serde_json::Value` |
| `jmap-contacts-server` | [draft-ietf-jmap-contacts-10] | Contacts method handlers; `ContactsBackend` trait |
| `jmap-contacts-client` | [draft-ietf-jmap-contacts-10] | Typed client methods |

### Calendars

| Crate | Spec | Role |
|---|---|---|
| `jmap-calendars-types` | [draft-ietf-jmap-calendars-26], [RFC 8984] | `CalendarEvent`, `Calendar`, `BusyPeriod`, `CalendarAlert`, JSCalendar sub-objects |
| `jmap-calendars-server` | [draft-ietf-jmap-calendars-26] | Calendars method handlers including `CalendarEvent/parse` and `Principal/getAvailability`; `CalendarsBackend` trait |
| `jmap-calendars-client` | [draft-ietf-jmap-calendars-26] | Typed client methods |

### Sharing

| Crate | Spec | Role |
|---|---|---|
| `jmap-sharing-types` | [RFC 9670] | `Principal`, `PrincipalType`, `ShareNotification`, `ChangedBy` |
| `jmap-sharing-server` | [RFC 9670] | Sharing method handlers; `SharingBackend` trait |
| `jmap-sharing-client` | [RFC 9670] | Typed client methods |

### Tasks

| Crate | Spec | Role |
|---|---|---|
| `jmap-tasks-types` | [draft-ietf-jmap-tasks-06], [RFC 8984] | `Task`, `TaskList`, `TaskRights`, `TaskNotification`, `Checklist`, `CheckItem`, `Comment` |
| `jmap-tasks-server` | [draft-ietf-jmap-tasks-06] | Task method handlers; `TasksBackend` trait |
| `jmap-tasks-client` | [draft-ietf-jmap-tasks-06] | Typed client methods |

### FileNode

| Crate | Spec | Role |
|---|---|---|
| `jmap-filenode-types` | [draft-ietf-jmap-filenode-13] | `FileNode`, `NodeType`, `FilesRights`, `FileNodeFilterCondition` |
| `jmap-filenode-server` | [draft-ietf-jmap-filenode-13] | FileNode method handlers including cycle detection, collision policy, cascade destroy; `FileNodeBackend` trait |
| `jmap-filenode-client` | [draft-ietf-jmap-filenode-13] | Typed client methods |

---

## Backend traits

Each server crate defines a `*Backend` trait that is the only integration surface
between the library and your storage layer. The read side (`get_objects`,
`get_state`, `get_changes`, `query_objects`, `query_changes`, `account_exists`)
is defined on the `JmapBackend` supertrait in `jmap-server`. Extension-specific
write and structural operations live on the extension trait.

Example (`MailBackend`, abbreviated):

```rust
pub trait MailBackend: JmapBackend {
    fn create_object<O: SetObject>(&self, account_id: &Id, create_id: &str, obj: O)
        -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>>;

    fn import_email(&self, account_id: &Id, blob_id: &Id, mailbox_ids: &[Id],
        keywords: &[Keyword], received_at: Option<&UTCDate>)
        -> impl Future<Output = Result<(Id, Email), BackendSetError<Self::Error>>>;

    fn find_thread_by_message_ids(&self, account_id: &Id, message_ids: &[&str])
        -> impl Future<Output = Result<Option<Id>, Self::Error>>;

    fn search_snippets(&self, account_id: &Id, email_ids: &[Id],
        filter: Option<&EmailFilterCondition>)
        -> impl Future<Output = Result<Vec<SearchSnippet>, Self::Error>>;

    // ... (update, destroy, copy, parse, blob_exists, and optional overrides)
}
```

All traits use Rust's native async fn in trait (AFIT, stable since 1.75). No
`#[async_trait]` macro.

---

## Extension draft status

Several crates implement specifications that are IETF Internet-Drafts rather than
published RFCs. Draft status at the time of writing:

| Draft | Status | Notes |
|---|---|---|
| `draft-ietf-jmap-calendars-26` | Expired draft | Substantively stable; used in production |
| `draft-ietf-jmap-contacts-10` | Active draft | Field names may still change |
| `draft-ietf-jmap-tasks-06` | Expired (2023) | Best-judgment interpretation of ambiguous sections |
| `draft-ietf-jmap-filenode-13` | Active draft | |
| `draft-atwood-jmap-chat-*` | Personal drafts | Not IETF-submitted |

---

## Design notes

### JSContact sub-objects are `serde_json::Value`

`ContactCard` in `jmap-contacts-types` stores all JSContact sub-objects — names,
phones, addresses, emails, organizations, etc. — as `serde_json::Value` rather
than typed Rust structs. This is intentional:

RFC 9553 (JSContact) permits arbitrary vendor extension properties on every
sub-object, and the schema has evolved between draft versions. Using typed structs
would either silently drop extension fields on round-trip or lock the library to
a specific schema revision. The `Value` approach preserves all data from any
server response, at the cost of compile-time field access. Callers that need to
extract a phone number or address deserialize the relevant `Value` field
themselves using RFC 9553 as the schema.

### `Person`-like attribution types are not unified

Three extension drafts each independently define a small struct meaning "the
person responsible for this change":

| Type | Crate | Fields |
|---|---|---|
| `Person` | `jmap-calendars-types` | `name`, `email?`, `principal_id?`, `calendar_address?` |
| `Person` | `jmap-tasks-types` | `@type`, `name?`, `uri?`, `principal_id?` |
| `ChangedBy` | `jmap-sharing-types` | `name`, `email?`, `principal_id?` |

These are semantically the same concept but have different wire fields mandated
by different spec authors from different working groups. They cannot be collapsed
into a shared type without violating at least one specification.

This is a weakness in the IETF drafts rather than in this library. The JMAP WG
has not yet defined a shared person/identity reference type that extension drafts
can reference — and doing so would require coordination across CALEXT, SCIM,
OpenID Connect, and others who all have claims on how a "person" should be
represented. Until the WG converges, the three structs remain separate. Application
code that displays "who made this change" across object types should extract
name and contact information at the application layer.

### `CallerCtx` forwarding

The `register_*_handlers` functions use `ClosureHandlerWithCtx` internally, which
forwards the `CallerCtx` value from `Dispatcher::dispatch` to every handler closure
as `_ctx: C`. The handlers themselves do not yet consume this parameter — it is
available for applications that implement `JmapHandler<C>` directly and need
per-request auth context (e.g. tenant isolation or row-level security). The
`_ctx` parameter can be used in closures registered via `dispatcher.register()`
without going through `register_*_handlers`.

---

## Build

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

MSRV: 1.75 (required for async fn in trait, stable).

---

## Dependency graph

```
jmap-types
    ├── jmap-server
    ├── jmap-base-client
    │       ├── jmap-mail-client
    │       ├── jmap-chat-client
    │       ├── jmap-contacts-client
    │       ├── jmap-calendars-client
    │       ├── jmap-sharing-client
    │       ├── jmap-tasks-client
    │       └── jmap-filenode-client
    ├── jmap-mail-types
    │       ├── jmap-mime
    │       └── jmap-mail-server
    ├── jmap-chat-types
    │       └── jmap-chat-server
    ├── jmap-contacts-types
    │       └── jmap-contacts-server
    ├── jmap-calendars-types
    │       └── jmap-calendars-server
    ├── jmap-sharing-types
    │       └── jmap-sharing-server
    ├── jmap-tasks-types
    │       └── jmap-tasks-server
    └── jmap-filenode-types
            └── jmap-filenode-server
```

All `*-client` crates depend on `jmap-base-client` and their corresponding
`*-types` crate. All `*-server` crates depend on `jmap-server` and their
corresponding `*-types` crate. Type crates have no async dependencies.

---

## References

- [RFC 8620] — JMAP Core
- [RFC 8621] — JMAP for Mail
- [RFC 8887] — JMAP over WebSocket
- [RFC 8984] — JSCalendar
- [RFC 9553] — JSContact
- [RFC 9670] — JMAP Sharing
- [draft-ietf-jmap-calendars-26](https://www.ietf.org/archive/id/draft-ietf-jmap-calendars-26.txt)
- [draft-ietf-jmap-contacts-10](https://www.ietf.org/archive/id/draft-ietf-jmap-contacts-10.txt)
- [draft-ietf-jmap-tasks-06](https://www.ietf.org/archive/id/draft-ietf-jmap-tasks-06.txt)
- [draft-ietf-jmap-filenode-13](https://www.ietf.org/archive/id/draft-ietf-jmap-filenode-13.txt)
- [draft-atwood-jmap-chat](https://github.com/MarkAtwood/jmap-chat-spec)

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[RFC 8887]: https://www.rfc-editor.org/rfc/rfc8887
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 9553]: https://www.rfc-editor.org/rfc/rfc9553
[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[draft-atwood-jmap-chat]: https://github.com/MarkAtwood/jmap-chat-spec

---

## License

MIT OR Apache-2.0
