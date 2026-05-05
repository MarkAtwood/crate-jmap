# jmap-*

A Rust workspace implementing the JMAP protocol family
([RFC 8620](https://www.rfc-editor.org/rfc/rfc8620)).

## Crate map

| Crate | Role |
|---|---|
| `jmap-types` | RFC 8620 base wire types: `Id`, `State`, `JmapError`, `JmapRequest/Response`, `ResultReference` |
| `jmap-server` | Dispatcher, `parse_request`, ResultReference resolution, generic handlers |
| `jmap-base-client` | Auth, session fetch, blob, SSE, WebSocket |
| `jmap-mail-types` | RFC 8621 data types: Email, Mailbox, Thread, etc. |
| `jmap-mail-server` | RFC 8621 method handlers, `MailBackend` trait |
| `jmap-mail-client` | RFC 8621 client methods |
| `jmap-mime` | MIME adapter: mime-tree → jmap-mail-types |
| `jmap-chat-types` | JMAP Chat extension data types |
| `jmap-chat-server` | JMAP Chat method handlers |
| `jmap-chat-client` | JMAP Chat client methods |
| `jmap-contacts-types` | draft-ietf-jmap-contacts-10 + RFC 9553 data types |
| `jmap-contacts-server` | Contacts method handlers, `ContactsBackend` trait |
| `jmap-contacts-client` | Contacts client methods |
| `jmap-calendars-types` | draft-ietf-jmap-calendars-26 + RFC 8984 data types |
| `jmap-calendars-server` | Calendars method handlers, `CalendarsBackend` trait |
| `jmap-calendars-client` | Calendars client methods |
| `jmap-sharing-types` | RFC 9670 data types: Principal, ShareNotification |
| `jmap-sharing-server` | RFC 9670 method handlers, `SharingBackend` trait |
| `jmap-sharing-client` | RFC 9670 client methods |
| `jmap-tasks-types` | draft-ietf-jmap-tasks-06 data types |
| `jmap-tasks-server` | Tasks method handlers, `TasksBackend` trait |
| `jmap-tasks-client` | Tasks client methods |
| `jmap-filenode-types` | draft-ietf-jmap-filenode-13 data types |
| `jmap-filenode-server` | FileNode method handlers, `FileNodeBackend` trait |
| `jmap-filenode-client` | FileNode client methods |

## Architecture

Each extension follows a three-crate pattern:

```
*-types    — serde types only, no async, no handlers
*-server   — method handlers + Backend trait; storage-agnostic
*-client   — typed async methods over jmap-base-client
```

Server crates provide a `register_*_handlers` function that wires all method
names into a `jmap-server::Dispatcher`. Backends implement the relevant
`*Backend` trait for their storage layer.

## Known design limitations

### `Person`-like attribution types are not unified

Several JMAP extension drafts independently define a small inline struct
representing "the person responsible for a change or event":

| Type | Crate | Spec |
|---|---|---|
| `Person` | `jmap-calendars-types` | draft-ietf-jmap-calendars-26 §7 |
| `Person` | `jmap-tasks-types` | draft-ietf-jmap-tasks-06 §4.2.3 |
| `ChangedBy` | `jmap-sharing-types` | RFC 9670 §3 |

All three carry a name, an optional principal ID, and optional contact
information, but each spec draft defines its own slightly different field set:
`calendarAddress` appears only in the calendars version; `uri` and an `@type`
discriminator appear only in the tasks version; `email` is absent from tasks.
Because the wire formats differ, they cannot be collapsed into a single shared
type without violating at least one of the specs.

This is a known weakness in the IETF specifications themselves. Identity and
person representation is one of the harder cross-WG coordination problems —
vCard/JSContact (CALEXT), SCIM, WebFinger, OpenID Connect, and others all have
legitimate but incompatible claims on how a "person" should be represented,
and JMAP extension drafts have so far avoided taking normative dependencies on
any of them. The result is each draft inventing its own minimal attribution
object.

**We are waiting for the JMAP WG to decide whether to define a shared
person/identity reference type** that extension drafts can reference. Until
that happens, the three structs remain separate, each in its own `*-types`
crate. Application code that needs to display "who made this change" across
extensions should extract `name` and optional contact information at the
application layer rather than through a shared library type.

Tracked upstream: https://github.com/nicowillis/jmap

## Dependency tree

```
jmap-types
    ├── jmap-server
    ├── jmap-base-client
    ├── jmap-mail-types
    │       ├── jmap-mime
    │       ├── jmap-mail-server
    │       └── jmap-mail-client
    ├── jmap-chat-types
    │       ├── jmap-chat-server
    │       └── jmap-chat-client
    ├── jmap-contacts-types
    │       ├── jmap-contacts-server
    │       └── jmap-contacts-client
    ├── jmap-calendars-types
    │       ├── jmap-calendars-server
    │       └── jmap-calendars-client
    ├── jmap-sharing-types
    │       ├── jmap-sharing-server
    │       └── jmap-sharing-client
    ├── jmap-tasks-types
    │       ├── jmap-tasks-server
    │       └── jmap-tasks-client
    └── jmap-filenode-types
            ├── jmap-filenode-server
            └── jmap-filenode-client
```

## Build

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

## License

MIT OR Apache-2.0
