# jmap-mail-server — Implementation Plan

RFC 8621 (JMAP for Mail) method handlers. Plugs into `jmap-server`'s `Dispatcher`.
Backend-agnostic: defines a `MailBackend` trait; consumers provide the implementation.

## Crate Family Position

```
jmap-types
    ├── jmap-server          dispatcher
    └── jmap-mail-types      data types
            └── jmap-mail-server  ← this crate
```

## What This Crate Is

Method handler implementations for every RFC 8621 JMAP method: Mailbox, Thread,
Email, SearchSnippet, Identity, EmailSubmission, VacationResponse.

Defines a `MailBackend` trait that the application implements. The crate handles
all JMAP protocol semantics (ordering, partial success, singleton enforcement,
ResultReference threading, error type mapping). The backend handles storage.

Known consumer: `stoa` (the Usenet/email JMAP server).

## What This Crate Is Not

- Not a full JMAP server
- Not coupled to any specific storage (SQLite, PostgreSQL, IPFS, in-memory)
- Not handling auth — caller's responsibility before `Dispatcher::dispatch()`
- Not handling MIME parsing — backend's responsibility in `import_email`/`parse_email`
- Not axum-specific — any `http`-based framework works

## In-Scope Extensions (future modules in this crate)

Two small RFC 8621-adjacent extensions belong here rather than in their own crates:

- **Sieve** (`draft-ietf-jmap-sieve-22`): `SieveScript/get`, `/set`, `/query`, `/validate`.
  One object type (`SieveScript`: `id`, `name`, `blobId`, `isActive`). Script content
  stored/retrieved via standard JMAP blob upload/download. Add as `src/sieve.rs` with a
  `sieve` feature flag gating registration.

- **MDN** (`draft-ietf-jmap-mdn-17`): `MDN/send`, `MDN/parse`. No stored objects —
  pure send-and-forget read receipts (RFC 8098). Add as `src/mdn.rs` with an `mdn`
  feature flag.

## Source Material

### Normative

`~/PROJECT/jmap-chat-spec/references/rfc8621.txt` — read the relevant section
before implementing each handler. Wire field names come from the spec, not from
memory or reference code.

### Backend trait pattern — copy this

`~/PROJECT/crate-jmapchat-server/jmapchat-server/src/backend.rs`

`StorageBackend`, `BackendChangesError`, `BackendSetError`, `ChangesResult`,
`QueryResult` are the exact pattern to follow for `MailBackend`. Copy the trait
structure and error types verbatim, then add the mail-specific methods.

### Handler logic reference — read, do not copy

`~/GIT/stalwart-jmap-server/` contains a complete, production RFC 8621 JMAP
server in Rust (Stalwart JMAP Server, archived 2023). It is the best available
reference for what each method handler must do.

**License: AGPL-3.0-only.** Do not copy or translate code from this repo.
Read it to understand the logic; implement independently using the spec as
the authoritative source.

The repo uses a git submodule at `main/` (stalwartlabs/mail-server commit
`53f0222`). All paths below are relative to `~/GIT/stalwart-jmap-server/`.

#### Handler implementations (one-to-one with our modules)

| Our module | Stalwart path | What to study |
|---|---|---|
| `email.rs` (get) | `main/crates/jmap/src/email/get.rs` | property filtering, notFound handling |
| `email.rs` (set) | `main/crates/jmap/src/email/set.rs` | partial success, keyword/mailboxId patch |
| `email.rs` (query) | `main/crates/jmap/src/email/query.rs` | filter/sort wiring to backend |
| `email.rs` (queryChanges) | `main/crates/jmap/src/email/query_changes.rs` | added/removed delta structure |
| `email.rs` (copy) | `main/crates/jmap/src/email/copy.rs` | cross-account semantics |
| `email.rs` (import) | `main/crates/jmap/src/email/import.rs` | blob→Email pipeline, error mapping |
| `email.rs` (parse) | `main/crates/jmap/src/email/parse.rs` | read-only, no state change |
| `snippet.rs` | `main/crates/jmap/src/email/snippet.rs` | filter→snippet wiring |
| `mailbox.rs` | `main/crates/jmap/src/mailbox/` (get, query, set) | onDestroyRemoveEmails logic in set.rs |
| `thread.rs` | `main/crates/jmap/src/thread/get.rs` | simple — thread is just a list of emailIds |
| `identity.rs` | `main/crates/jmap/src/identity/` (get, set) | minimal; no query |
| `submission.rs` | `main/crates/jmap/src/submission/` (get, query, set) | set.rs: how send-trigger is modelled |
| `vacation.rs` | `main/crates/jmap/src/vacation/` | singleton upsert pattern |

#### Wire types and method structs

| Item | Stalwart path | Notes |
|---|---|---|
| Method request/response types | `main/crates/jmap-proto/src/method/` | One file per object type; shows all fields we must parse |
| Core wire primitives | `main/crates/jmap-proto/src/types/` | Id, State, Keyword, Property, etc. |
| Object definitions | `main/crates/jmap-proto/src/object/` | How serde shapes are declared |

#### Integration tests (best oracle for expected behaviour)

The `main/tests/src/jmap/` directory has one test file per method group.
These are the most valuable reference for edge cases and expected JSON shapes.
Use them to derive independent test oracles — do not copy test code.

| Test file | Covers |
|---|---|
| `email_get.rs` | property filtering, notFound, partial get |
| `email_set.rs` | create/update/destroy, keyword patch, mailboxId patch |
| `email_query.rs` | filter combinations, sort, pagination |
| `email_query_changes.rs` | added/removed deltas, cannotCalculateChanges |
| `email_copy.rs` | cross-account copy |
| `email_parse.rs` | parse from blobId, property subset |
| `email_search_snippet.rs` | snippet extraction |
| `mailbox.rs` | onDestroyRemoveEmails, mailboxHasEmail |
| `thread_get.rs` | thread emailIds ordering |
| `email_submission.rs` | submission create → send trigger |
| `vacation_response.rs` | singleton get/set, no create/destroy |

#### stoa (secondary — ignore storage details)

`~/PROJECT/stoa/crates/mail/src/jmap/dispatch.rs` — shows which methods stoa
actually registers, useful for confirming scope.
`~/PROJECT/stoa/crates/mail/src/email/`, `mailbox/`, `thread/` — handler
shape for what each handler receives; ignore IPFS/SQLite coupling.

## Dependencies

```toml
jmap-types      = { path = "../crate-jmap-types" }
jmap-mail-types = { path = "../crate-jmap-mail-types" }
jmap-server     = { path = "../crate-jmap-server" }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"
tokio      = { version = "1", features = ["rt"] }
```

No MIME parsing libraries. No HTTP client. No database drivers.

## RFC 8621 Method Coverage

| Object | Methods | RFC §§ | Backend path |
|---|---|---|---|
| Mailbox | get, changes, query, queryChanges, set | §2 | generic CRUD + query |
| Thread | get, changes | §3 | generic get + changes |
| Email | get, changes, query, queryChanges, set | §4.2–4.6 | generic CRUD + query |
| Email | copy | §4.7 | `copy_email` |
| Email | import | §4.8 | `import_email` |
| Email | parse | §4.9 | `parse_email` (read-only) |
| SearchSnippet | get | §5.1 | `search_snippets` |
| Identity | get, changes, set | §6 | generic CRUD |
| EmailSubmission | get, changes, query, queryChanges, set | §7 | generic CRUD + query; create triggers send |
| VacationResponse | get, set | §8 | generic get + update (singleton) |

Total: 26 method registrations.

## Key Design Decisions

### 1. MailBackend follows StorageBackend exactly for generic CRUD

Same AFIT pattern, same `BackendChangesError`/`BackendSetError` error types,
same `ChangesResult`/`QueryResult` structs. Implementors who have already built
a `StorageBackend` for jmapchat-server will find the contract identical.

`MailBackend` is NOT object-safe (it has generic methods). The dispatcher and
all handlers are generic over `B: MailBackend`, monomorphized at compile time.
No `#[async_trait]` macro — AFIT is stable since Rust 1.75.

### 2. queryChanges as a separate backend method

RFC 8621 adds `queryChanges` for Mailbox, Email, and EmailSubmission. This
requires a backend method beyond what jmapchat-server needed. A new
`query_changes<O: QueryObject>` method returns `QueryChangesResult`
(with `removed: Vec<Id>` and `added: Vec<AddedItem>`).

Backends that cannot compute incremental query deltas should return
`Err(BackendChangesError::TooManyChanges { limit: 0 })`, which the handler
maps to `cannotCalculateChanges` per RFC 8620 §5.5.

### 3. Email/import and Email/parse as separate backend methods

Both require reading a raw RFC 5322 blob from the account's blob store — an
operation with no equivalent in the generic CRUD interface.

- `import_email`: reads a blob, parses it as RFC 5322, stores it as an Email.
  Returns the created Email with server-assigned ID. Backend owns MIME parsing.
- `parse_email`: reads a blob, parses it as RFC 5322. Read-only; no state change.
  Backend owns MIME parsing. Returns Email fields matching `properties` argument.

The handler does not know how blobs are stored; the backend does.

### 4. Email/copy as a separate backend method

Cross-account blob movement has no generic CRUD equivalent. `copy_email`
receives a `from_account_id`, `from_email_id`, `to_account_id`, `mailbox_ids`,
and `keywords`. The backend handles duplication; the handler handles the RFC
8621 §4.7 response structure (`created`, `notCreated`).

### 5. EmailSubmission/set create triggers actual email sending

`create_object<EmailSubmission>` is the path that initiates outbound delivery.
The backend is responsible for both recording the submission and triggering the
send (SMTP, relay, or queue). The handler does not know how delivery works.

RFC 8621 §7.5 requires that if the send fails immediately, the submission object
must not be created. The backend must not commit the record if delivery fails.

### 6. VacationResponse is a singleton — handler enforces, backend is unaware

RFC 8621 §8.2 mandates that VacationResponse/set only supports `update` (no
`create`, no `destroy`). The fixed ID is `"singleton"`. The handler rejects
any create or destroy attempt with `invalidArguments` before touching the backend.

The backend uses the same `get_objects<VacationResponse>` and
`update_object<VacationResponse>` methods as any other type. It needs no
singleton-specific logic.

### 7. Mailbox/set onDestroyRemoveEmails handled in the handler layer

RFC 8621 §2.5: when `onDestroyRemoveEmails` is true in the destroy arguments,
the handler must destroy all emails in the mailbox before destroying it. This
requires calling `query_objects<Email>` (filter by mailboxId) then
`destroy_object<Email>` for each result, then `destroy_object<Mailbox>`.

When false (default): if the mailbox contains emails, the destroy MUST fail with
`mailboxHasEmail`. The handler calls `query_objects<Email>` to check.

All of this is handler logic. The backend has no `onDestroyRemoveEmails` concept.

### 8. SearchSnippet/get gated by supports_type

SearchSnippet requires full-text search infrastructure. Backends without a
search index return `false` from `supports_type::<SearchSnippet>()`. The handler
maps this to `accountNotSupportedByMethod` per RFC 8620 §5.1.

### 9. register_mail_handlers is the entry point

One function registers all 26 method handlers with the caller's
`jmap-server::Dispatcher<C>`. The backend is wrapped in `Arc<B>` and cloned
into each handler closure — same pattern as jmap-chat-server.

## Planned Public API

```rust
/// Implement for your mail storage system.
///
/// Uses AFIT (async fn in trait, stable since Rust 1.75). Not object-safe;
/// always monomorphized at compile time.
///
/// Implementor invariants (same as jmapchat-server StorageBackend):
/// 1. State monotonicity: get_state returns a different token after every
///    successful mutation. Token does not change on failure.
/// 2. Initial state: "0" is always the valid initial state sentinel.
/// 3. supports_type consistency: if false, the corresponding methods are
///    never called and must not be implemented.
/// 4. Partial set success: per-object failures do not roll back other objects
///    in the same /set call (RFC 8620 §5.3).
/// 5. EmailSubmission atomicity: if create_object<EmailSubmission> returns Ok,
///    the email has been queued or sent. If it returns Err, nothing was stored.
#[allow(async_fn_in_trait)]
pub trait MailBackend: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    // ── Standard CRUD (mirrors StorageBackend in jmapchat-server) ──────────

    fn get_objects<O: GetObject + Send + Sync>(
        &self,
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[O::Property]>,
    ) -> impl Future<Output = Result<(Vec<O>, Vec<Id>), Self::Error>> + Send;

    fn create_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    fn update_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    fn get_state<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
    ) -> impl Future<Output = Result<State, Self::Error>> + Send;

    fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> impl Future<Output = Result<ChangesResult, BackendChangesError<Self::Error>>> + Send;

    fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> impl Future<Output = Result<QueryResult, Self::Error>> + Send;

    // ── queryChanges (RFC 8620 §5.5) ────────────────────────────────────────

    fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
    ) -> impl Future<Output = Result<QueryChangesResult, BackendChangesError<Self::Error>>> + Send;

    // ── Mail-specific ────────────────────────────────────────────────────────

    /// Email/import (RFC 8621 §4.8): parse blob as RFC 5322, store as Email.
    /// `blob_id` must exist in the account's blob store.
    /// Backend owns MIME parsing; returns the created Email with server-assigned Id.
    fn import_email(
        &self,
        account_id: &Id,
        blob_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[Keyword],
        received_at: Option<&UTCDate>,
    ) -> impl Future<Output = Result<(Id, Email), BackendSetError<Self::Error>>> + Send;

    /// Email/parse (RFC 8621 §4.9): parse blob as RFC 5322. Read-only.
    /// `blob_id` must exist in the account's blob store.
    /// Backend owns MIME parsing; returns Email fields matching `properties`.
    fn parse_email(
        &self,
        account_id: &Id,
        blob_id: &Id,
        properties: Option<&[EmailProperty]>,
    ) -> impl Future<Output = Result<Email, Self::Error>> + Send;

    /// Email/copy (RFC 8621 §4.7): duplicate an email into another account.
    fn copy_email(
        &self,
        from_account_id: &Id,
        email_id: &Id,
        to_account_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[Keyword],
    ) -> impl Future<Output = Result<(Id, Email), BackendSetError<Self::Error>>> + Send;

    /// SearchSnippet/get (RFC 8621 §5.1): return search snippets for email IDs.
    /// Only called when supports_type::<SearchSnippet>() returns true.
    fn search_snippets(
        &self,
        account_id: &Id,
        email_ids: &[Id],
        filter: Option<&EmailFilterCondition>,
    ) -> impl Future<Output = Result<Vec<SearchSnippet>, Self::Error>> + Send;

    /// Whether this backend supports operations on the given object type.
    /// Return false for SearchSnippet if no full-text search is available.
    fn supports_type<O: JmapObject>(&self) -> bool;
}

/// Register all RFC 8621 JMAP Mail handlers with a jmap-server Dispatcher.
///
/// After calling this, the dispatcher handles all 26 RFC 8621 method names.
/// Wrap `backend` in `Arc` before passing — it is cloned into each handler.
pub fn register_mail_handlers<B, C>(dispatcher: &mut Dispatcher<C>, backend: Arc<B>)
where
    B: MailBackend + 'static,
    C: Clone + Send + 'static;

pub use backend::{
    BackendChangesError, BackendSetError,
    ChangesResult, QueryResult, QueryChangesResult, AddedItem,
};
```

## Module Layout

```
src/
  lib.rs          re-exports; register_mail_handlers
  backend.rs      MailBackend trait; BackendChangesError, BackendSetError,
                  ChangesResult, QueryResult, QueryChangesResult, AddedItem
  mailbox.rs      Mailbox/get, /changes, /query, /queryChanges, /set
                  (includes onDestroyRemoveEmails logic)
  thread.rs       Thread/get, /changes
  email.rs        Email/get, /changes, /query, /queryChanges, /set,
                  /copy, /import, /parse
  snippet.rs      SearchSnippet/get
  identity.rs     Identity/get, /changes, /set
  submission.rs   EmailSubmission/get, /changes, /query, /queryChanges, /set
  vacation.rs     VacationResponse/get, /set (singleton enforcement here)
```

## Test Strategy

A `MemoryBackend` in `tests/common/mod.rs` provides an in-memory `HashMap`
implementation of `MailBackend`. This serves as both the test harness and the
canonical example for implementors.

Test files per object group:

```
tests/
  common/
    mod.rs          MemoryBackend implementation
  mailbox_tests.rs
  thread_tests.rs
  email_tests.rs
  snippet_tests.rs
  identity_tests.rs
  submission_tests.rs
  vacation_tests.rs
```

Test oracles come from RFC 8621 example JSON (the spec includes full
request/response pairs for each method). Extract them verbatim from the spec
and hardcode as `serde_json::json!({...})` literals. Never derive expected
values from the implementation under test.

Each test calls `register_mail_handlers` with the `MemoryBackend`, constructs a
`JmapRequest` matching the RFC example, calls `Dispatcher::dispatch`, and
asserts the response matches the RFC example response.

### Non-trivial test cases to include

- Mailbox/set: `onDestroyRemoveEmails: true` destroys emails before mailbox
- Mailbox/set: destroy with emails and `onDestroyRemoveEmails: false` → `mailboxHasEmail`
- Email/set: create in multiple mailboxes; update `mailboxIds`; add/remove keywords
- Email/set: destroy with `destroy: ["#id"]` ResultReference from prior /query
- Email/import: import via blobId; verify Email appears in Mailbox/query
- Email/parse: parse does not create Email; state unchanged after call
- Email/copy: email appears in destination account; source unchanged
- VacationResponse/set: create attempt → `invalidArguments`; destroy → `invalidArguments`
- VacationResponse/set: update patches existing singleton
- SearchSnippet/get: `supports_type = false` → `accountNotSupportedByMethod`
- EmailSubmission/set: create returns submission + triggers send (stubbed in MemoryBackend)
- queryChanges: `cannotCalculateChanges` when backend returns TooManyChanges
