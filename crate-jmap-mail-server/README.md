# jmap-mail-server

## What it is

RFC 8621 (JMAP for Mail) method handlers and the `MailBackend` trait. Plugs
into [`jmap-server`]'s `Dispatcher`. Implements all 26 RFC 8621 method names.
Storage-agnostic — consumers implement the `MailBackend` trait for their
own data layer.

Two optional extensions are available behind Cargo features:

- `mdn` — RFC 9007 Message Disposition Notifications (`MDN/send`, `MDN/parse`)
- `sieve` — RFC 9661 Sieve Script management (`SieveScript/get`, `set`, `query`,
  `validate`)

Each extension exposes its own `register_*_handlers` entry point and its own
backend trait extension.

## What it's for

Implements RFC 8621 (Email, Mailbox, Thread, EmailSubmission, Identity,
SearchSnippet, VacationResponse) plus optional MDN and Sieve extensions, so
consumers can wire JMAP-for-Mail method dispatch into their own HTTP transport
without re-deriving wire format or `/set` semantics. This crate is the
**canonical template** for every extension `*-server` crate in the workspace
(chat, calendars, tasks, contacts, filenode, sharing, metadata); siblings
mirror its module layout and trait shape, and all of them inter-depend through
the workspace `JmapHandler` trait in `jmap-server`. The consumer supplies a
`MailBackend` impl (storage), a `CallerCtx` type (auth identity), and wires
the dispatcher into HTTP/SSE/WebSocket transport themselves.

## How to use

```rust,ignore
use std::sync::Arc;
use jmap_mail_server::{MailBackend, register_mail_handlers};
use jmap_server::Dispatcher;

// 1. Implement MailBackend for your storage layer. The impl must also
//    satisfy the JmapBackend supertrait (account_exists, get_objects,
//    get_state, get_changes, query_objects, query_changes) and declare
//    `type Error` and `type CallerCtx`. See the MailBackend trait
//    section below for the full surface.

// 2. Construct a dispatcher parameterised on your CallerCtx type, then
//    register all 26 RFC 8621 methods in one call.
let mut dispatcher: Dispatcher<MyCallerCtx> = Dispatcher::new();
register_mail_handlers(&mut dispatcher, Arc::new(my_backend));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, caller_ctx, session_state).await;
```

`Dispatcher<C>` requires `B::CallerCtx = C`; `register_mail_handlers`
enforces the match at the type level. For a single-user dev server use
`Dispatcher<()>` with a backend whose `CallerCtx = ()`. For
multi-tenant production, define a typed caller context (e.g. a
principal-id newtype) and parameterise both sides on that type.

For a runnable, type-checked variant of this example, see the
[crate-level documentation](https://docs.rs/jmap-mail-server) — the
doctest in `lib.rs` is verified by `cargo test --doc`.

After `register_mail_handlers` returns, the dispatcher handles every method
name listed in the [Registered methods](#registered-methods) section below.
The same `Arc<MyBackend>` can be shared with other parts of your application.

## Registered methods

All 26 method names from RFC 8621 §1.3 are registered:

| Object | Methods |
|---|---|
| `Mailbox` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `Thread` | `get`, `changes` |
| `Email` | `get`, `changes`, `query`, `queryChanges`, `set`, `copy`, `import`, `parse` |
| `SearchSnippet` | `get` |
| `Identity` | `get`, `changes`, `set` |
| `EmailSubmission` | `get`, `changes`, `query`, `queryChanges`, `set` |
| `VacationResponse` | `get`, `set` |

## MailBackend trait

Implement this trait to connect the handlers to your storage system. The
read-side methods (`get_objects`, `get_state`, `get_changes`, `query_objects`,
`query_changes`) are defined on the `JmapBackend` supertrait (from
`jmap-server`). `MailBackend` adds write operations and mail-specific
operations.

```rust,ignore
pub trait MailBackend: JmapBackend {
    // --- Write operations ---

    /// Create a new object. `build` receives the backend-assigned id and
    /// returns the object to store. Returns (assigned_id, created_object).
    /// For singleton types (VacationResponse), MUST be idempotent:
    /// if an object already exists for the key, return it rather than
    /// creating a duplicate.
    fn create_object<O, F>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send;

    /// Apply a partial patch to an existing object.
    /// Returns Some(updated) if the backend modified server-set fields beyond
    /// the patch (RFC 8620 §5.3 echo); None if the patch was applied verbatim.
    /// Handlers include the Some(O) in the /set response — do not discard it.
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        id: &Id,
    ) -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // --- Mail-specific operations ---

    /// Import a raw message blob as an Email (RFC 8621 §5.7).
    /// The blob must already be present in the blob store.
    fn import_email(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[Keyword],
        received_at: Option<&UTCDate>,
    ) -> impl Future<Output = Result<(Id, Email), BackendSetError<Self::Error>>> + Send;

    /// Return the thread id of the first stored Email whose messageId list
    /// intersects message_ids, or None if no match.
    fn find_thread_by_message_ids(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        message_ids: &[&str],
    ) -> impl Future<Output = Result<Option<Id>, Self::Error>> + Send;

    /// Return true if blob_id exists in account_id's blob store.
    /// Used by Email/parse to distinguish notFound (blob absent) from
    /// notParsable (blob present but not a valid message).
    fn blob_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;

    /// Parse a blob as a message and return an Email without storing it
    /// (RFC 8621 §5.8 — Email/parse).
    fn parse_email(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        blob_id: &Id,
    ) -> impl Future<Output = Result<Email, Self::Error>> + Send;

    /// Store a record read from `from_account_id` as a new record in
    /// `to_account_id` (RFC 8620 §5.4 /copy; the handler applied the
    /// overrides). Returns the new id and the stored record.
    fn copy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
    ) -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Return search snippets for the given Email ids (RFC 8621 §5.9).
    fn search_snippets(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        email_ids: &[Id],
        filter: Option<&EmailFilterCondition>,
    ) -> impl Future<Output = Result<Vec<SearchSnippet>, Self::Error>> + Send;

    /// Returns `true` if this backend supports JMAP object type `O` for
    /// `account_id`, as seen by `caller`.
    /// Used by the session capability builder and by SearchSnippet/get.
    fn supports_type<O: JmapObject>(&self, caller: &Self::CallerCtx, account_id: &Id) -> bool;

    // --- Optional overrides (have sensible defaults) ---

    /// Max email ids fetched when `collapseThreads=true` (default 65536).
    fn max_collapse_threads_emails(&self, caller: &Self::CallerCtx, account_id: &Id) -> usize {
        65_536
    }

    /// Max bytes of body value text per EmailBodyPart (0 = unlimited).
    fn max_body_value_bytes(&self, caller: &Self::CallerCtx, account_id: &Id) -> u64 { 0 }

    /// Max seconds in the future that sendAt may be for EmailSubmission
    /// (0 = no delayed send).
    fn max_delayed_send_seconds(&self, caller: &Self::CallerCtx, account_id: &Id) -> u64 { 0 }

    /// Return true if this backend can compute Mailbox/queryChanges
    /// (RFC 8620 §5.6 canCalculateChanges). Defaults to false.
    fn can_calculate_mailbox_query_changes(&self, caller: &Self::CallerCtx, account_id: &Id)
        -> bool { false }

    /// Destroy several Emails; one result per input id, in order.
    /// The default calls destroy_object::<Email> per id; an override MUST
    /// keep the per-id write-access check.
    fn batch_destroy_emails(
        &self,
        caller: &Self::CallerCtx,
        account_id: &Id,
        email_ids: &[Id],
    ) -> impl Future<Output = Vec<(Id, Option<BackendSetError<Self::Error>>)>> + Send
    where
        Self: Sized { /* ... */ }
}
```

Every method receives a `caller: &Self::CallerCtx` reference as its
first parameter after `&self`. The handler closures registered by
`register_mail_handlers` forward the caller context unchanged from
`jmap_server::Dispatcher::dispatch`, so the backend sees exactly the
value the HTTP transport layer supplied.

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `mailboxHasChild`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a
  `serverFail` response

## Permission enforcement responsibilities

The handlers in this crate do **no** permission checking. Per the
workspace AGENTS.md "Caller identity (foundation seam)" rule, backends
are canonical for authorization: the handler computes the candidate
mutation and the backend's `/set` impl is the canonical point of
enforcement. Caller identity is read via
`JmapBackend::principal_id` on the `caller: &Self::CallerCtx`
parameter every backend method receives. The full contract is the
"Permission enforcement (backend canonical)" rustdoc on `MailBackend`.

The reference `memory::MemoryBackend` does **not** implement any of
the checks below — it returns `None` from `principal_id` and stores
whatever the handler hands it. Production backends MUST implement each
of the following or they will silently accept JMAP requests that
[RFC 8621] mandates the server to reject. ACL denials are `forbidden`
SetErrors unless noted.

### Mailbox/set ([RFC 8621] §2, §2.5)

1. **Create requires `mayCreateChild` on the parent**, or the
   account's `mayCreateTopLevelMailbox` ([RFC 8621] §1.3.1) when
   `parentId` is null.

2. **Update of `name` or `parentId` requires `mayRename`.**

3. **Destroy requires `mayDelete`.**

4. **`role` uniqueness is atomic in the backend.** "There MUST NOT be
   two Mailboxes in the same account with the same role" ([RFC 8621]
   §2). The handler checks this against a pre-mutation snapshot, which
   concurrent requests can race past; the backend MUST re-verify it
   atomically with the write.

5. **`isSubscribed` is per user.** It "MUST be stored separately per
   user where multiple users have access to a shared Mailbox"
   ([RFC 8621] §2), keyed on `principal_id`.

### Email/set, Email/import and Email/copy ([RFC 8621] §2, §4.6–§4.8)

6. **Create, import and copy require `mayAddItems` on every Mailbox in
   `mailboxIds`.** `Email/copy` also requires that the caller can read
   the source Email in `fromAccountId`; an unreadable source is
   `notFound`.

7. **Update of `mailboxIds` requires `mayAddItems` on each Mailbox
   added and `mayRemoveItems` on each one removed.**

8. **Keyword updates require the right on *all* of the Email's
   Mailboxes:** `maySetSeen` to change `$seen`, `maySetKeywords` to
   change any other keyword.

9. **Destroy requires `mayRemoveItems` on every Mailbox the Email is
   in**, including the per-id overrides of `batch_destroy_emails`.

### Reads ([RFC 8621] §2, §9.5)

10. **Partial account access is invisible.** "Email objects may be
    fetched if they are in *at least one* Mailbox with" `mayReadItems`
    ([RFC 8621] §2), and the server "MUST treat any data the user does
    not have permission to access the same as if it did not exist"
    (§9.5). `get_objects` and `query_objects` leave out Mailboxes,
    Emails and Thread members the caller cannot read.

### Identity/set ([RFC 8621] §6, §6.3, §9.6)

11. **Create requires permission to send from `email`.** The server
    "MUST reject it with the appropriate error if the user does not
    have permission to use that email address to send from" (§9.6):
    the `forbiddenFrom` SetError (§6.3).

12. **Destroy honours `mayDelete`.** The handler refuses to destroy an
    Identity whose stored `mayDelete` is false; that pre-check is not
    atomic with the write, so the backend MUST re-verify it.

### EmailSubmission/set ([RFC 8621] §7.5, §9.6)

13. **The Identity must belong to the caller**, and servers "SHOULD
    reject submissions where the From header field of the message does
    not correspond to the associated Identity" (§9.6): `forbiddenFrom`.

14. **Envelope `mailFrom` restrictions.** The server "MUST also
    enforce appropriate restrictions on the MAIL FROM address" (§9.6):
    `forbiddenMailFrom`.

15. **Send policy.** A caller not permitted to send at all right now
    gets `forbiddenToSend` (§7.5).

### VacationResponse/set ([RFC 8621] §8)

16. **Update requires write access to the account.** The handler
    enforces the singleton shape (`singleton` id, no create or
    destroy); the backend's `update_object` owns the ACL.

### Caller-identity-less backends

The reference `MemoryBackend` returns `None` from `principal_id` and
is intentionally permissive (fail-open, bd:JMAP-bghk) — it is suitable
for single-user dev servers, fixtures, and the workspace's own
integration tests, but NOT for any deployment where more than one
principal can reach the JMAP dispatcher. A deployment that needs ACL
enforcement MUST override `JmapBackend::principal_id` and implement
each of the checks above; the workspace ships no production-grade
reference impl.

## How it works

### Registration

This crate exposes three entry points; each is independent and may be called
in any order on the same dispatcher:

| Function | Cargo feature | Methods registered | Backend trait |
|---|---|---|---|
| `register_mail_handlers` | always available | 26 RFC 8621 methods (see [Registered methods](#registered-methods)) | `MailBackend` |
| `register_mdn_handlers` | `mdn` (RFC 9007) | `MDN/send`, `MDN/parse` | `MailBackend + MdnBackend` |
| `register_sieve_handlers` | `sieve` (RFC 9661) | `SieveScript/get`, `set`, `query`, `validate` | `MailBackend + SieveBackend` |

All three use `ClosureHandler` (provided by `jmap-server`) to wrap each
handler function and `Arc<B>` into a `JmapHandler<B::CallerCtx>` and register it
with the dispatcher. The dispatcher's `CallerCtx` value is forwarded into each
closure as `ctx`, then passed by reference (`&ctx`) into the standard
`handle_*` handler bodies, which themselves forward `caller: &B::CallerCtx`
into every `MailBackend` method call. One `Arc::clone` per method name; no heap
allocation per request.

`register_mdn_handlers` takes a third argument, `max_blob_ids: usize`, which
caps the number of blob IDs accepted by a single `MDN/parse` request. Use
`mdn::MDN_PARSE_MAX_BLOB_IDS` for the default (16). Callers are responsible
for advertising the corresponding capability URI in the JMAP Session — the
handlers do not inspect the `using` field themselves. The capability URI
constants are re-exported from `jmap_mail_types`:

- `JMAP_MDN_URI = "urn:ietf:params:jmap:mdn"` (when `mdn` is enabled)
- `JMAP_SIEVE_SCRIPTS_URI = "urn:ietf:params:jmap:sieve"` (when `sieve` is
  enabled)

### Handler structure

Each handler function (e.g. `handle_email_get`, `handle_mailbox_set`) receives
`args: Value` containing the raw JMAP method arguments after ResultReference
resolution, calls `MailBackend` methods, and returns the RFC 8621 response
object. No framework magic — just straightforward async functions.

### Mailbox/query — in-process filtering

`Mailbox/query` fetches all mailboxes for the account and filters them in
process. This is correct for typical account sizes. Sort requests are rejected
with `unsupportedSort` rather than silently ignored. `sortAsTree` and
`filterAsTree` are also rejected because tree-mode traversal is not
implemented; returning wrong results silently is worse than an error.

### Mailbox/set — role uniqueness

RFC 8621 §2 requires that at most one mailbox per account holds each standard
role (`inbox`, `sent`, `trash`, etc.). The `Mailbox/set` handler enforces this
with a two-pass update loop:

1. Pass 1 runs patches that set `role: null` (vacating a role).
2. Pass 2 runs everything else, checking against the pre-request snapshot
   minus roles freed by successful pass-1 vacates.

A same-request role swap (A vacates "sent", B claims "sent") always succeeds
regardless of map iteration order. To swap roles, use a single `/set` request
that vacates in one `update` entry and claims in another.

### VacationResponse — singleton semantics

There is exactly one `VacationResponse` per account; its id is always
`"singleton"`. The handler enforces this directly:

- `create` entries are always rejected with `SetErrorType::Singleton`.
- `destroy` entries are always rejected with `SetErrorType::Singleton`.
- `update "singleton"` is the only valid mutation. If no VacationResponse
  exists yet the handler performs an upsert (calls `create_object` then
  `update_object`). Concurrent upserts are safe only if `create_object` is
  idempotent for the singleton key — the handler holds no shared state and
  cannot add locking.

### Email/copy and EmailSubmission/set — call_id forwarding

`Email/copy` and `EmailSubmission/set` are the only two handlers that receive
the `call_id` value. `Email/copy` passes it through to the backend to support
idempotency tracking on cross-account copy operations. `EmailSubmission/set`
uses it when generating implicit `onSuccessUpdateEmail` invocations.

### Mailbox/set — onDestroyRemoveEmails cascade

When `onDestroyRemoveEmails: true` and the mailbox being destroyed contains
emails, the handler:

- Destroys emails that exist only in that mailbox.
- Patches `mailboxIds/{id}: null` on emails that also belong to other
  mailboxes.

The cascade makes N+2 backend calls per destroyed mailbox (one query, one
get, N email operations). A batch backend method would reduce this to O(1)
calls — that is a known gap in the current `MailBackend` API.

## CallerCtx

`register_mail_handlers` registers each method as a `ClosureHandler`
that forwards the dispatcher's `CallerCtx` value into the closure as `ctx`,
which the closure body then passes by reference (`&ctx`) into the standard
`handle_*` handler. The handler in turn forwards `caller: &B::CallerCtx` into
every `MailBackend` (and inherited `JmapBackend`) method call.

To use this:

1. Pick a `CallerCtx` type for your backend — e.g. `()` if no auth context is
   needed, or a struct like `AuthCtx { user_id: Id, scopes: Vec<Scope> }`.
2. Implement `JmapBackend` with `type CallerCtx = AuthCtx;` (the bound is
   `Clone + Send + Sync + 'static`).
3. Use the matching `Dispatcher<AuthCtx>` and pass the constructed auth
   context as the second argument to `Dispatcher::dispatch(request, auth, …)`.
4. Inside `MailBackend` method bodies, read `caller: &AuthCtx` to apply
   per-user visibility rules, rate limits, etc.

Backends that don't need an auth identity use `type CallerCtx = ();` and a
`Dispatcher<()>`. Both shapes register the same way.

## Capability URIs

Include these in your Session object's `capabilities` map:

```rust
// Declared in jmap-mail-types and re-exported here:
use jmap_mail_server::{JMAP_MAIL_URI, JMAP_SUBMISSION_URI, JMAP_VACATIONRESPONSE_URI};
assert_eq!(JMAP_MAIL_URI, "urn:ietf:params:jmap:mail");
assert_eq!(JMAP_SUBMISSION_URI, "urn:ietf:params:jmap:submission");
assert_eq!(JMAP_VACATIONRESPONSE_URI, "urn:ietf:params:jmap:vacationresponse");
```

Which URIs to advertise depends on which object types your backend supports.
Use `MailBackend::supports_type::<O>(caller, account_id)` to check each
account at session-build time.

## Crate family

```
jmap-types
    ├── jmap-server          Dispatcher this plugs into
    └── jmap-mail-types      domain types (Email, Mailbox, etc.)
            └── jmap-mail-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## Gotchas

- **`search_snippets` has no default implementation.** `MailBackend::search_snippets` must be implemented; there is no default. The in-crate test `MemoryMailBackend` returns empty snippets for all calls, which means `SearchSnippet/get` returns empty results in test mode. A real implementation requires full-text indexing or delegation to a search service.
- **`find_thread_by_message_ids` default is unsuitable for persistent backends.** The default implementation uses a thread-ID generator seeded from system-clock nanoseconds at process startup. Two processes starting within the same nanosecond (common in containers and CI) produce identical ID sequences. More critically, the thread graph is lost on restart — emails that share a `Message-ID` / `References` chain will be silently assigned new thread IDs after a restart. Persistent backends MUST override this method and store thread assignments durably.
- **`onDestroyRemoveEmails` cascade default is O(N) per destroyed mailbox.** When `onDestroyRemoveEmails: true`, the handler queries all emails in the mailbox, fetches their `mailboxIds`, and issues individual updates or destroys per email. The `MailBackend` trait provides an optional `batch_destroy_emails` method (with a default implementation that loops) — backends that need O(1) cascade behavior should override `batch_destroy_emails` with a single bulk-delete operation; the handler already calls this method rather than looping itself.
- **`Email/import` and `Email/parse` require backend cooperation.** The handler validates argument shape and calls `backend.import_email` / `backend.parse_email`; the actual RFC 5322 parsing is entirely the backend's responsibility. Use `jmap-mime` to convert `mime_tree` output to `jmap-mail-types` body structures.
- **Singleton upsert for `VacationResponse` is not concurrency-safe.** If two requests simultaneously create a `VacationResponse` for the same account, both may succeed at the storage layer before either can detect the other. The handler uses optimistic create-then-update; backends that support conditional writes should enforce singleton semantics atomically in `create_object`.

## References

- **[RFC 8621]** — JMAP for Mail (normative for all method semantics)
- **[RFC 8620]** — JMAP Core (request format, SetError, ResultReference,
  `/set` response shape, `canCalculateChanges`)
- **[RFC 5322]** — Internet Message Format (message structure referenced by
  `Email/import` and `Email/parse`)

[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 5322]: https://www.rfc-editor.org/rfc/rfc5322
[`jmap-server`]: ../crate-jmap-server
