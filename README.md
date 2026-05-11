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

### Shared sub-types

| Crate | Spec | Role |
|---|---|---|
| `jmap-jscalendar-types` | [RFC 8984] | JSCalendar typed sub-objects: `LocalDateTime`, `Duration`, `RecurrenceRule`, `Location`, `Participant`, `Alert`, etc. Consumed by Calendars and (planned) Tasks. No JMAP dep. |

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
| `jmap-contacts-types` | [RFC 9610], [RFC 9553] | `ContactCard`, `AddressBook`, `AddressBookRights`; JSContact sub-objects as `serde_json::Value` |
| `jmap-contacts-server` | [RFC 9610] | Contacts method handlers; `ContactsBackend` trait |
| `jmap-contacts-client` | [RFC 9610] | Typed client methods |

### Calendars

| Crate | Spec | Role |
|---|---|---|
| `jmap-calendars-types` | [draft-ietf-jmap-calendars], [RFC 8984] | `CalendarEvent`, `Calendar`, `BusyPeriod`, `CalendarAlert`, JSCalendar sub-objects |
| `jmap-calendars-server` | [draft-ietf-jmap-calendars] | Calendars method handlers including `CalendarEvent/parse` and `Principal/getAvailability`; `CalendarsBackend` trait |
| `jmap-calendars-client` | [draft-ietf-jmap-calendars] | Typed client methods |

### Sharing

| Crate | Spec | Role |
|---|---|---|
| `jmap-sharing-types` | [RFC 9670] | `Principal`, `PrincipalType`, `ShareNotification`, `ChangedBy` |
| `jmap-sharing-server` | [RFC 9670] | Sharing method handlers; `SharingBackend` trait |
| `jmap-sharing-client` | [RFC 9670] | Typed client methods |

### Tasks

| Crate | Spec | Role |
|---|---|---|
| `jmap-tasks-types` | [draft-ietf-jmap-tasks], [RFC 8984] | `Task`, `TaskList`, `TaskRights`, `TaskNotification`, `Checklist`, `CheckItem`, `Comment` |
| `jmap-tasks-server` | [draft-ietf-jmap-tasks] | Task method handlers; `TasksBackend` trait |
| `jmap-tasks-client` | [draft-ietf-jmap-tasks] | Typed client methods |

### FileNode

| Crate | Spec | Role |
|---|---|---|
| `jmap-filenode-types` | [draft-ietf-jmap-filenode] | `FileNode`, `NodeType`, `FilesRights`, `FileNodeFilterCondition` |
| `jmap-filenode-server` | [draft-ietf-jmap-filenode] | FileNode method handlers including cycle detection, collision policy, cascade destroy; `FileNodeBackend` trait |
| `jmap-filenode-client` | [draft-ietf-jmap-filenode] | Typed client methods |

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

## Design notes

### JSContact sub-objects are `serde_json::Value`

`ContactCard` in `jmap-contacts-types` stores all JSContact sub-objects — names,
phones, addresses, emails, organizations, etc. — as `serde_json::Value` rather
than typed Rust structs. This is intentional.

[RFC 9553] permits arbitrary vendor extension properties on every sub-object, and
the schema has evolved between versions. Using typed structs would either silently
drop extension fields on round-trip or lock the library to a specific schema
revision. The `Value` approach preserves all data from any server response, at
the cost of compile-time field access. Callers that need to extract a phone number
or address deserialize the relevant `Value` field themselves using RFC 9553 as the
schema.

### `Person`-like attribution types are not unified

Three extension specs each independently define a small struct meaning "the
person responsible for this change":

| Type | Crate | Fields |
|---|---|---|
| `Person` | `jmap-calendars-types` | `name`, `email?`, `principal_id?`, `calendar_address?` |
| `Person` | `jmap-tasks-types` | `@type`, `name?`, `uri?`, `principal_id?` |
| `ChangedBy` | `jmap-sharing-types` | `name`, `email?`, `principal_id?` |

These are semantically the same concept but have different wire fields mandated
by different spec authors in different working groups. They cannot be collapsed
into a single shared type without violating at least one specification.

This reflects a known gap in the JMAP spec suite. The JMAP WG has not yet defined
a shared person/identity reference type that extension specs can normatively
reference — doing so would require cross-WG coordination with CALEXT, the JSContact
authors, SCIM, and others who all have claims on how a "person" should be
represented in a structured format. Until the WG converges on something, the three
structs remain separate. Application code that displays "who made this change"
across object types should extract name and contact information at the application
layer.

### `CallerCtx` forwarding

The `register_*_handlers` functions use `ClosureHandler` internally, which
forwards the `CallerCtx` value from `Dispatcher::dispatch` to every handler closure
as `_ctx: C`. The handlers themselves do not yet act on this parameter — it is
available for applications that implement `JmapHandler<C>` directly and need
per-request auth context (e.g. tenant isolation or row-level security).

---

## Build

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

MSRV: 1.75 (required for async fn in trait).

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
    ├── jmap-calendars-types ── (also consumes jmap-jscalendar-types)
    │       └── jmap-calendars-server
    ├── jmap-sharing-types
    │       └── jmap-sharing-server
    ├── jmap-tasks-types
    │       └── jmap-tasks-server
    └── jmap-filenode-types
            └── jmap-filenode-server

jmap-jscalendar-types  (RFC 8984 JSCalendar typed sub-objects, no JMAP dep)
    └── jmap-calendars-types  (re-exports as `jscalendar` module alias)
```

---

## JMAP specification references

### Published RFCs

| RFC | Title |
|---|---|
| [RFC 8620] | The JSON Meta Application Protocol (JMAP) — Core |
| [RFC 8621] | The JSON Meta Application Protocol (JMAP) for Mail |
| [RFC 8887] | A JSON Meta Application Protocol (JMAP) Subprotocol for WebSocket |
| [RFC 8984] | JSCalendar: A JSON Representation of Calendar Data |
| [RFC 9007] | Handling Message Disposition Notification with JMAP |
| [RFC 9219] | S/MIME Signature Verification Extension to JMAP |
| [RFC 9404] | JSON Meta Application Protocol (JMAP) Blob Management Extension |
| [RFC 9425] | JSON Meta Application Protocol (JMAP) for Quotas |
| [RFC 9553] | JSContact: A JSON Representation of Contact Data |
| [RFC 9610] | JSON Meta Application Protocol (JMAP) for Contacts |
| [RFC 9661] | The JSON Meta Application Protocol (JMAP) for Sieve Scripts |
| [RFC 9670] | JSON Meta Application Protocol (JMAP) Sharing |
| [RFC 9749] | Use of VAPID in JSON Meta Application Protocol (JMAP) Push |

### Active IETF drafts

| Draft | Title | Datatracker |
|---|---|---|
| [draft-ietf-jmap-calendars] | JMAP for Calendars | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-calendars/) |
| [draft-ietf-jmap-tasks] | JMAP for Tasks | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-tasks/) |
| [draft-ietf-jmap-filenode] | JMAP File Node | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-filenode/) |
| [draft-ietf-jmap-blobext] | JMAP Blob Management Extension | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/) |
| [draft-ietf-jmap-essential] | JMAP Essential Extensions | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-essential/) |
| [draft-ietf-jmap-metadata] | JMAP for Message Metadata | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/) |
| [draft-ietf-jmap-emailpush] | JMAP Email Push | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-emailpush/) |
| [draft-ietf-jmap-refplus] | JMAP Reference Pointer Extensions | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-refplus/) |
| [draft-ietf-jmap-mail-sharing] | JMAP for Mail Sharing | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-mail-sharing/) |
| [draft-ietf-jmap-portability-extensions] | JMAP Portability Extensions | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-portability-extensions/) |

### Related standards

| Spec | Title |
|---|---|
| [RFC 5228] | Sieve: An Email Filtering Language |
| [RFC 5322] | Internet Message Format |
| [RFC 5545] | iCalendar |
| [RFC 6350] | vCard Format Specification |
| [RFC 9420] | The Messaging Layer Security (MLS) Protocol |

### Working group and community

| Resource | URL |
|---|---|
| JMAP WG home | https://datatracker.ietf.org/wg/jmap/about/ |
| JMAP WG documents | https://datatracker.ietf.org/wg/jmap/documents/ |
| jmap.io | https://jmap.io/ |
| JMAP discussion list | https://www.ietf.org/mailman/listinfo/jmap |
| draft-atwood-jmap-chat | https://github.com/MarkAtwood/jmap-chat-spec |

---

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[RFC 8887]: https://www.rfc-editor.org/rfc/rfc8887
[RFC 8984]: https://www.rfc-editor.org/rfc/rfc8984
[RFC 9007]: https://www.rfc-editor.org/rfc/rfc9007
[RFC 9219]: https://www.rfc-editor.org/rfc/rfc9219
[RFC 9404]: https://www.rfc-editor.org/rfc/rfc9404
[RFC 9425]: https://www.rfc-editor.org/rfc/rfc9425
[RFC 9553]: https://www.rfc-editor.org/rfc/rfc9553
[RFC 9610]: https://www.rfc-editor.org/rfc/rfc9610
[RFC 9661]: https://www.rfc-editor.org/rfc/rfc9661
[RFC 9670]: https://www.rfc-editor.org/rfc/rfc9670
[RFC 9749]: https://www.rfc-editor.org/rfc/rfc9749
[RFC 5228]: https://www.rfc-editor.org/rfc/rfc5228
[RFC 5322]: https://www.rfc-editor.org/rfc/rfc5322
[RFC 5545]: https://www.rfc-editor.org/rfc/rfc5545
[RFC 6350]: https://www.rfc-editor.org/rfc/rfc6350
[RFC 9420]: https://www.rfc-editor.org/rfc/rfc9420
[draft-ietf-jmap-calendars]: https://www.ietf.org/archive/id/draft-ietf-jmap-calendars-26.txt
[draft-ietf-jmap-tasks]: https://www.ietf.org/archive/id/draft-ietf-jmap-tasks-06.txt
[draft-ietf-jmap-filenode]: https://www.ietf.org/archive/id/draft-ietf-jmap-filenode-13.txt
[draft-ietf-jmap-blobext]: https://www.ietf.org/archive/id/draft-ietf-jmap-blobext-01.txt
[draft-ietf-jmap-essential]: https://datatracker.ietf.org/doc/draft-ietf-jmap-essential/
[draft-ietf-jmap-metadata]: https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/
[draft-ietf-jmap-emailpush]: https://datatracker.ietf.org/doc/draft-ietf-jmap-emailpush/
[draft-ietf-jmap-refplus]: https://datatracker.ietf.org/doc/draft-ietf-jmap-refplus/
[draft-ietf-jmap-mail-sharing]: https://datatracker.ietf.org/doc/draft-ietf-jmap-mail-sharing/
[draft-ietf-jmap-portability-extensions]: https://datatracker.ietf.org/doc/draft-ietf-jmap-portability-extensions/
[draft-atwood-jmap-chat]: https://github.com/MarkAtwood/jmap-chat-spec

## License

MIT OR Apache-2.0
