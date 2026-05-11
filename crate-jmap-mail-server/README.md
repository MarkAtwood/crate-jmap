# jmap-mail-server

RFC 8621 (JMAP for Mail) method handlers for Rust. Plugs into
[`jmap-server`]'s `Dispatcher`. Implements all 26 RFC 8621 method names.
Storage-agnostic — consumers implement the `MailBackend` trait for their
own data layer.

Two optional extensions are available behind Cargo features:

- `mdn` — RFC 9007 Message Disposition Notifications (`MDN/send`, `MDN/parse`)
- `sieve` — RFC 9661 Sieve Script management (`SieveScript/get`, `set`, `query`,
  `validate`)

Each extension exposes its own `register_*_handlers` entry point and its own
backend trait extension.

## Usage

```rust
use std::sync::Arc;
use jmap_mail_server::{MailBackend, register_mail_handlers};
use jmap_server::Dispatcher;

// 1. Implement MailBackend for your storage layer (see trait section below).
struct MyBackend { /* db pool, etc. */ }
impl MailBackend for MyBackend { /* ... */ }

// 2. Wire all 26 RFC 8621 methods into a Dispatcher in one call.
let mut dispatcher: Dispatcher<()> = Dispatcher::new();
register_mail_handlers(&mut dispatcher, Arc::new(MyBackend { /* ... */ }));

// 3. Dispatch JMAP requests (in your HTTP handler).
// let response = dispatcher.dispatch(request, (), session_state).await;
```

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

```rust
pub trait MailBackend: JmapBackend {
    // --- Write operations ---

    /// Create a new object. Returns (assigned_id, created_object).
    /// For singleton types (VacationResponse), MUST be idempotent:
    /// if an object already exists for the key, return it rather than
    /// creating a duplicate.
    fn create_object<O: SetObject>(&self, account_id: &Id, create_id: &str, obj: O)
        -> impl Future<Output = Result<(Id, O), BackendSetError<Self::Error>>> + Send;

    /// Apply a partial patch to an existing object.
    /// Returns Some(updated) if the backend modified server-set fields beyond
    /// the patch (RFC 8620 §5.3 echo); None if the patch was applied verbatim.
    /// Handlers include the Some(O) in the /set response — do not discard it.
    fn update_object<O: SetObject>(&self, account_id: &Id, id: &Id, patch: O::Patch)
        -> impl Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject>(&self, account_id: &Id, id: &Id)
        -> impl Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // --- Mail-specific operations ---

    /// Import a raw message blob as an Email (RFC 8621 §5.7).
    /// The blob must already be present in the blob store.
    fn import_email(&self, account_id: &Id, blob_id: &Id, mailbox_ids: &[Id],
        keywords: &[Keyword], received_at: Option<&UTCDate>)
        -> impl Future<Output = Result<(Id, Email), BackendSetError<Self::Error>>> + Send;

    /// Copy an Email from one account to another (RFC 8621 §6.3 / RFC 8620 §6.3).
    fn copy_email(&self, from_account_id: &Id, email_id: &Id, to_account_id: &Id,
        mailbox_ids: &[Id], keywords: &[Keyword], received_at: Option<&UTCDate>)
        -> impl Future<Output = Result<(Id, Email), BackendSetError<Self::Error>>> + Send;

    /// Parse a blob as a message and return an Email without storing it
    /// (RFC 8621 §5.8 — Email/parse).
    fn parse_email(&self, account_id: &Id, blob_id: &Id)
        -> impl Future<Output = Result<Email, Self::Error>> + Send;

    /// Return true if blob_id exists in account_id's blob store.
    /// Used by Email/parse to distinguish notFound (blob absent) from
    /// notParsable (blob present but not a valid message).
    fn blob_exists(&self, account_id: &Id, blob_id: &Id)
        -> impl Future<Output = bool> + Send;

    /// Return the thread id of the first stored Email whose messageId list
    /// intersects message_ids, or None if no match.
    ///
    /// Persistent backends MUST override this. The default ID generator is
    /// seeded from system-clock nanoseconds at startup; two processes starting
    /// within the same nanosecond (common in containers and test harnesses)
    /// will produce identical ID sequences and silently corrupt thread graphs
    /// across restarts.
    fn find_thread_by_message_ids(&self, account_id: &Id, message_ids: &[&str])
        -> impl Future<Output = Result<Option<Id>, Self::Error>> + Send;

    /// Return search snippets for the given Email ids (RFC 8621 §5.9).
    fn search_snippets(&self, account_id: &Id, email_ids: &[Id],
        filter: Option<&EmailFilterCondition>)
        -> impl Future<Output = Result<Vec<SearchSnippet>, Self::Error>> + Send;

    /// Return true if this account supports the given JMAP object type.
    /// Not called internally — used by the session capability builder.
    fn supports_type<O: JmapObject>(&self) -> bool;

    // --- Optional overrides (have sensible defaults) ---

    /// Max bytes of body value text per EmailBodyPart (0 = unlimited).
    fn max_body_value_bytes(&self, account_id: &Id) -> u64 { 0 }

    /// Max seconds in the future that sendAt may be for EmailSubmission
    /// (0 = no delayed send).
    fn max_delayed_send_seconds(&self, account_id: &Id) -> u64 { 0 }

    /// Return true if this backend can compute Mailbox/queryChanges
    /// (RFC 8620 §5.6 canCalculateChanges). Defaults to false.
    fn can_calculate_mailbox_query_changes(&self, account_id: &Id) -> bool { false }
}
```

`BackendSetError<E>` is an enum over two variants:

- `BackendSetError::SetError(SetError)` — a semantic RFC 8620 SetError
  (`notFound`, `invalidProperties`, `forbidden`, `mailboxHasChild`, etc.)
- `BackendSetError::Other(E)` — a storage-layer error that becomes a
  `serverFail` response

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
pub const JMAP_MAIL_URI: &str            = "urn:ietf:params:jmap:mail";
pub const JMAP_SUBMISSION_URI: &str      = "urn:ietf:params:jmap:submission";
pub const JMAP_VACATION_RESPONSE_URI: &str = "urn:ietf:params:jmap:vacationresponse";
```

Which URIs to advertise depends on which object types your backend supports.
Use `MailBackend::supports_type::<O>()` to check at session-build time.

## Crate family

```
jmap-types
    ├── jmap-server          Dispatcher this plugs into
    └── jmap-mail-types      domain types (Email, Mailbox, etc.)
            └── jmap-mail-server  ← this crate
```

Path dependencies between crates use `path = "../crate-jmap-*"` and will
remain that way until the family is published to crates.io.

## Known Limitations

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

## License

MIT OR Apache-2.0
