# jmap-*

A Rust workspace implementing the JMAP protocol suite as a set of composable,
backend-agnostic library crates. Covers:

- [RFC 8620] (JMAP Core), [RFC 8621] (JMAP for Mail), [RFC 8887] (JMAP over
  WebSocket), [RFC 9670] (JMAP Sharing).
- [RFC 9007] (Message Disposition Notifications) and [RFC 9661] (Sieve Scripts)
  as feature-gated extensions on the Mail server.
- [RFC 8984] JSCalendar and [RFC 9553] JSContact typed sub-types as standalone
  no-async crates consumed by Calendars/Tasks and Contacts respectively.
- The JMAP Contacts ([RFC 9610]), Calendars ([draft-ietf-jmap-calendars]),
  Tasks ([draft-ietf-jmap-tasks]), and FileNode ([draft-ietf-jmap-filenode])
  extensions.
- The JMAP Object Metadata extension ([draft-ietf-jmap-metadata]), the
  workspace-recommended IETF-track path for vendor data that must be queryable.
- The JMAP Content Identifier extension ([draft-atwood-jmap-cid]) for blob
  integrity hashes.
- The JMAP Chat extension ([draft-atwood-jmap-chat]).

This is a **protocol library kit**, not a server application. You bring storage,
authentication, and HTTP. The library handles JMAP wire framing, ResultReference
resolution, batch dispatch, type-safe request/response shapes, and the
specification-mandated validation logic for each object type. Reference
`MemoryBackend` implementations gated behind a `memory` feature exist on every
extension server crate for testing — they are intentionally demonstration-only
and not for production. A separate `jmap-testjig` workspace member (publish =
false) wires the kit into a runnable HTTP/SSE/WS process for the workspace's
own integration testing.

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
| `jmap-jscontact-types` | [RFC 9553] | JSContact typed sub-objects: `Name`, `EmailAddress`, `Phone`, `Address`, `Organization`, `Anniversary`, `PersonalInfo`, etc. Consumed by Contacts. No JMAP dep. |
| `jmap-cid-types` | [draft-atwood-jmap-cid] | `CidCapability` (`urn:ietf:params:jmap:cid`) and the `Sha256` typed wire shape (lowercase hex, 64 chars). Feeds Blob upload responses and FileNode integrity fields. No async. |

### Mail

| Crate | Spec | Role |
|---|---|---|
| `jmap-mail-types` | [RFC 8621] | `Email`, `Mailbox`, `Thread`, `Identity`, `EmailSubmission`, `VacationResponse` |
| `jmap-mime` | [RFC 5322] | Adapter: `mime_tree` parsed output → `jmap-mail-types` body structures |
| `jmap-mail-server` | [RFC 8621], [RFC 9007], [RFC 9661] | All 26 RFC 8621 method handlers via `register_mail_handlers`; `MailBackend` trait. Feature-gated `register_mdn_handlers` (RFC 9007, `mdn` feature) and `register_sieve_handlers` (RFC 9661, `sieve` feature) with their own backend traits. Reference `MemoryBackend` under the `memory` feature. |
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
| `jmap-contacts-types` | [RFC 9610], [RFC 9553] | `ContactCard`, `AddressBook`, `AddressBookRights`; JSContact sub-objects as `serde_json::Value` on the wire with typed access via `jmap-jscontact-types` re-exported as the `jscontact` module |
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

### Metadata

| Crate | Spec | Role |
|---|---|---|
| `jmap-metadata-types` | [draft-ietf-jmap-metadata] | `Metadata`, `Annotation`, `ImapMetadata`, `WebDavMetadata`, `MetadataFilterCondition`, `MetadataCapability`. The IETF-track escape for vendor data that must be queryable. |
| `jmap-metadata-server` | [draft-ietf-jmap-metadata] | `Metadata/get/changes/set/query/queryChanges`; `MetadataBackend` trait |
| `jmap-metadata-client` | [draft-ietf-jmap-metadata] | Typed client methods |

### Workspace-only

| Crate | Role |
|---|---|
| `jmap-testjig` | Workspace integration test jig. Wires the dispatcher + all extension handlers + reference `MemoryBackend`s into a runnable HTTP/SSE/WS process. `publish = false`. Single-user, hardcoded bearer auth, in-memory only — explicitly NOT FOR PRODUCTION. |

---

## Backend traits

Each server crate defines a `*Backend` trait that is the only integration surface
between the library and your storage layer. The read side (`get_objects`,
`get_state`, `get_changes`, `query_objects`, `query_changes`, `account_exists`,
`principal_id`) is defined on the `JmapBackend` supertrait in `jmap-server`.
Extension-specific write and structural operations live on the extension trait.

Every backend method takes `caller: &Self::CallerCtx` as its first argument
after `&self`. This is the foundation seam for caller identity:
`JmapBackend::principal_id(caller) -> Option<&Id>` is the canonical way the
JMAP layer asks "who is the caller", and backends are canonical for permission
enforcement using that value. See workspace `AGENTS.md` "Caller identity
(foundation seam)" for the full rule set.

Example (`MailBackend`, abbreviated):

```rust
pub trait MailBackend: JmapBackend {
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>>;

    fn import_email(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[Keyword],
        received_at: Option<&UTCDate>,
    ) -> impl Future<Output = Result<(Id, Email), BackendSetError<Self::Error>>>;

    fn find_thread_by_message_ids(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        message_ids: &[&str],
    ) -> impl Future<Output = Result<Option<Id>, Self::Error>>;

    fn search_snippets(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        email_ids: &[Id],
        filter: Option<&EmailFilterCondition>,
    ) -> impl Future<Output = Result<Vec<SearchSnippet>, Self::Error>>;

    // ... (update, destroy, copy, parse, blob_exists, and optional overrides)
}
```

All traits use Rust's native async fn in trait (AFIT). No `#[async_trait]`
macro.

---

## Design notes

### JSContact and JSCalendar sub-objects: hybrid sloppy-value pattern

`ContactCard` in `jmap-contacts-types` and `CalendarEvent` in
`jmap-calendars-types` store their RFC 9553 / RFC 8984 sub-objects (names,
phones, addresses, organizations, recurrence rules, participants, alerts, etc.)
as `serde_json::Value` on the wire. This is intentional: both specs permit
arbitrary vendor extension properties on every sub-object, and the schemas
evolve between revisions. Storing the wire field as `Value` preserves all
data from any server response across deserialize/serialize round-trips at the
cost of compile-time field access.

For callers that want typed access, the workspace ships standalone typed
sub-type crates that mirror the spec's object model:

- `jmap-jscontact-types` — typed RFC 9553 sub-types, re-exported by
  `jmap-contacts-types` as the `jscontact` module.
- `jmap-jscalendar-types` — typed RFC 8984 sub-types, re-exported by
  `jmap-calendars-types` as the `jscalendar` module.

The hybrid pattern is: the wire field stays `Value` (round-trip fidelity);
typed access is opt-in via `serde_json::from_value::<jscontact::Name>(...)`.
See `crate-jmap-calendars-types/PLAN.md` for the per-field rationale.

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

### `CallerCtx` forwarding and identity enforcement

The `register_*_handlers` functions use `ClosureHandler` internally, which
forwards the `CallerCtx` value from `Dispatcher::dispatch` to every handler
closure as `&ctx`. Handlers pass `caller` through to every `*Backend` method
call as the first argument after `&self` — this is the foundation seam
codified in workspace `AGENTS.md` "Caller identity (foundation seam)".

Identity flows as follows:

1. HTTP / auth middleware builds the `CallerCtx` value (the type your
   `MailBackend::CallerCtx` associated type names).
2. `Dispatcher::dispatch(request, caller, session_state).await` clones it
   into every handler invocation.
3. Each handler forwards `&caller` to every backend method.
4. The backend asks `JmapBackend::principal_id(caller) -> Option<&Id>` for
   the canonical identity, and is the sole authority on permission
   enforcement (handlers may pre-check defensively, but the backend MUST
   re-verify atomically with the mutation).

A backend that returns `None` from `principal_id` is signalling that this
deployment does not honor identity-dependent JMAP semantics — fine for
single-user dev jigs and tests, but production multi-user deployments MUST
override.

### Extras-preservation policy

Every public deserialize struct on the JMAP wire carries an `extra:
serde_json::Map<String, serde_json::Value>` flatten field; every wire-format
result string enum (e.g. `MailboxRole`, `NodeType`, `ChatKind`,
`ParticipantRole`) carries an `Unknown(String)` variant. Together these
preserve vendor / site / private-extension fields and unrecognised result
values across deserialize / serialize round-trip. Filter algebra types,
control enums (`Operator`, `ComparatorProperty`), and externally-owned
classifier strings (RFC 9553 `kind` fields, RFC 8984 `kind` fields) are
explicitly excluded — see workspace `AGENTS.md` for the per-axis rationale.

---

## Build

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

MSRV: 1.88. The actual floor is driven by transitive dependencies (`icu_*@2.2.0`
requires 1.86; `hashbrown@0.17.0` needs the edition2024 Cargo feature stabilized
in 1.85); 1.88 is the conservative pick with a margin against further dep churn.

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
    │       ├── jmap-filenode-client
    │       └── jmap-metadata-client
    ├── jmap-mail-types
    │       ├── jmap-mime
    │       └── jmap-mail-server
    ├── jmap-chat-types
    │       └── jmap-chat-server
    ├── jmap-contacts-types ── (also consumes jmap-jscontact-types)
    │       └── jmap-contacts-server
    ├── jmap-calendars-types ── (also consumes jmap-jscalendar-types)
    │       └── jmap-calendars-server
    ├── jmap-sharing-types
    │       └── jmap-sharing-server
    ├── jmap-tasks-types ── (will consume jmap-jscalendar-types)
    │       └── jmap-tasks-server
    ├── jmap-filenode-types
    │       └── jmap-filenode-server
    ├── jmap-metadata-types
    │       └── jmap-metadata-server
    └── jmap-cid-types  (CidCapability + Sha256 typed wire shape; feeds
                         jmap-base-client BlobUploadResponse and future
                         FileNode integrity fields)

jmap-jscalendar-types  (RFC 8984 JSCalendar typed sub-objects, no JMAP dep)
    ├── jmap-calendars-types  (re-exports as `jscalendar` module alias)
    └── jmap-tasks-types      (planned consumer)

jmap-jscontact-types   (RFC 9553 JSContact typed sub-objects, no JMAP dep)
    └── jmap-contacts-types   (re-exports as `jscontact` module alias)

jmap-testjig  (publish = false; depends on every server crate plus axum
               and tokio-tungstenite for the integration test process)
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
| [draft-ietf-jmap-blobext] | JMAP Blob Management Extension (obsoletes RFC 9404 if approved; introduces `urn:ietf:params:jmap:blob2`) | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-blobext/) |
| [draft-ietf-jmap-essential] | JMAP Essential Extensions | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-essential/) |
| [draft-ietf-jmap-metadata] | JMAP for Object Metadata | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/) |
| [draft-ietf-jmap-emailpush] | JMAP Email Push | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-emailpush/) |
| [draft-ietf-jmap-refplus] | JMAP Reference Pointer Extensions | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-refplus/) |
| [draft-ietf-jmap-mail-sharing] | JMAP for Mail Sharing | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-mail-sharing/) |
| [draft-ietf-jmap-portability-extensions] | JMAP Portability Extensions | [tracker](https://datatracker.ietf.org/doc/draft-ietf-jmap-portability-extensions/) |

### Independent drafts

| Draft | Title | Source |
|---|---|---|
| [draft-atwood-jmap-cid] | JMAP Content Identifier (`urn:ietf:params:jmap:cid`) | [spec repo](https://github.com/MarkAtwood/jmap-chat-spec) |
| [draft-atwood-jmap-chat] | JMAP Chat | [spec repo](https://github.com/MarkAtwood/jmap-chat-spec) |
| draft-atwood-jmap-chat-push | JMAP Chat: Push payloads | (in jmap-chat-spec repo) |
| draft-atwood-jmap-chat-wss | JMAP Chat: WebSocket ephemeral events | (in jmap-chat-spec repo) |
| draft-atwood-jmap-chat-federation | JMAP Chat: Federation (not yet implemented in this workspace) | (in jmap-chat-spec repo) |
| draft-atwood-jmap-chat-filenode | JMAP Chat: File attachment objects (not yet implemented) | (in jmap-chat-spec repo) |
| draft-atwood-jmap-chat-calendars | JMAP Chat: Calendar integration (not yet implemented) | (in jmap-chat-spec repo) |
| draft-atwood-jmap-chat-tasks | JMAP Chat: Task integration (not yet implemented) | (in jmap-chat-spec repo) |

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
[draft-atwood-jmap-cid]: https://github.com/MarkAtwood/jmap-chat-spec
