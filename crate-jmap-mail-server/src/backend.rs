//! MailBackend trait and supporting types for RFC 8621 method handlers.
//!
//! Consumers implement [`MailBackend`] for their storage system. The method
//! handlers in sibling modules call into the backend through this trait.
//!
//! The read-side operations (`get_objects`, `get_state`, `get_changes`,
//! `query_objects`, `query_changes`) are defined on the [`jmap_server::JmapBackend`]
//! supertrait. Only write operations and mail-specific operations are here.
//!
//! Marker traits and property selector enums live in `jmap-types` and
//! `jmap-mail-types` respectively; they are re-exported here for convenience.
//!
//! # isUnread convention
//!
//! When computing [`Mailbox::unread_emails`](jmap_mail_types::Mailbox::unread_emails)
//! and [`Mailbox::unread_threads`](jmap_mail_types::Mailbox::unread_threads),
//! backends MUST use the following definition (RFC 8621 §2 / jmapio/jmap-js
//! `Message.js` lines 803–805):
//!
//! ```text
//! isUnread = NOT keywords.$seen  AND  NOT keywords.$draft
//! ```
//!
//! Draft messages — those with the `$draft` keyword — are **never** counted as
//! unread regardless of their `$seen` state. A message is unread only if it
//! lacks both `$seen` and `$draft`.
//!
//! `unread_threads` counts threads that contain at least one unread (by the
//! above definition) email in the mailbox.

pub use jmap_mail_types::backend::{
    EmailProperty, EmailSubmissionProperty, IdentityProperty, MailboxProperty,
    SearchSnippetProperty, ThreadProperty, VacationResponseProperty,
};
pub use jmap_server::{
    AddedItem, BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

// ---------------------------------------------------------------------------
// MailBackend trait
// ---------------------------------------------------------------------------

/// Storage backend for RFC 8621 JMAP Mail method handlers.
///
/// Implementors provide the actual data access; the method handler modules
/// in this crate translate between JMAP wire protocol and backend calls.
///
/// Read-side operations (`get_objects`, `get_state`, `get_changes`,
/// `query_objects`, `query_changes`) are inherited from [`JmapBackend`].
///
/// This trait is not object-safe by design (generic methods). Use
/// `Arc<impl MailBackend>` when sharing across tasks.
pub trait MailBackend: JmapBackend {
    // -----------------------------------------------------------------------
    // Write operations
    // -----------------------------------------------------------------------

    /// Create a new object.
    ///
    /// Returns `(assigned_id, created_object)` on success. `create_id` is the
    /// client-side creation id used in the `/set` request.
    ///
    /// # Sentinel fields the backend MUST replace
    ///
    /// The method handlers in this crate pass partially-constructed objects
    /// with sentinel values that the backend MUST replace with real values
    /// before storing:
    ///
    /// - **`id`**: The `id` field in the input object is always set to
    ///   `"placeholder"`. The backend MUST replace it with a real, unique,
    ///   account-scoped ID and return that ID as the first element of the
    ///   result tuple.
    ///
    /// - **`blob_id`** (Email only): The `blob_id` field is set to
    ///   `"placeholder-blob"`. The backend MUST replace it with the real
    ///   blob ID that corresponds to the stored message bytes. For backends
    ///   that do not store raw bytes on the `Email/set` create path (e.g.,
    ///   `MemoryBackend`), any stable unique ID derived from the stored object
    ///   is acceptable.
    ///
    /// - **`size`** (Email only): Set to `0`. The backend MUST update this
    ///   to the actual byte size of the stored blob (or the serialized object
    ///   size as a proxy) before returning.
    ///
    /// Failing to replace these sentinels will cause the client to receive
    /// invalid wire values (`"placeholder"` / `"placeholder-blob"` / `0`).
    ///
    /// # Singleton types
    ///
    /// For types where only one instance may exist per account (e.g.,
    /// `VacationResponse`), `create_object` MUST be idempotent: if an object
    /// already exists for the given key, the implementation MUST return the
    /// existing object rather than creating a duplicate. The `vacation.rs`
    /// handler relies on this guarantee to avoid a TOCTOU race between
    /// concurrent upsert requests.
    fn create_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        create_id: &str,
        obj: O,
    ) -> impl std::future::Future<Output = Result<(jmap_types::Id, O), BackendSetError<Self::Error>>>
           + Send;

    /// Apply a partial update (patch) to an existing object.
    ///
    /// Returns `Some(updated_object)` if the backend modified any properties
    /// beyond what the client requested (RFC 8620 §5.3 server-set field echo),
    /// or `None` if the patch was applied verbatim.
    ///
    /// **Callers must handle the `Some` case.** When the return value is
    /// `Some(O)`, the handler should serialize the updated object and include
    /// the server-modified fields in the `updated` map of the `/set` response
    /// (RFC 8620 §5.3). Discarding the return value causes server-modified
    /// fields to be silently omitted from the response. Per-request auth
    /// context is available via the `caller` parameter, which the
    /// `register_mail_handlers` closures forward unchanged from
    /// [`jmap_server::Dispatcher::dispatch`].
    fn update_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
        patch: O::Patch,
    ) -> impl std::future::Future<Output = Result<Option<O>, BackendSetError<Self::Error>>> + Send;

    /// Destroy an existing object by id.
    fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;

    // -----------------------------------------------------------------------
    // Mail-specific methods
    // -----------------------------------------------------------------------

    /// Import a raw message blob as an Email (RFC 8621 §4.8 — `Email/import`).
    ///
    /// The blob must already be stored (uploaded via JMAP blob upload). Returns
    /// the assigned id and the created Email object.
    ///
    /// # Contract
    ///
    /// The handler in
    /// [`handle_email_import`](crate::handle_email_import) parses and
    /// validates the request, then calls this method once per
    /// successfully-validated `EmailImport` entry. The contract is
    /// designed so production backends (PostgreSQL, S3, Maildir,
    /// IMAP-bridge, etc.) have a single canonical answer for each
    /// question below.
    ///
    /// ## Argument preconditions
    ///
    /// The handler has already validated all of the following before
    /// calling. Backends MAY assume these preconditions hold and SHOULD
    /// NOT re-validate them (defense-in-depth checks that return
    /// `BackendSetError::Other` on violation are acceptable but
    /// redundant).
    ///
    /// - **`mailbox_ids` is non-empty.** RFC 8621 §4.8 requires at
    ///   least one mailbox. The handler rejects an empty `mailboxIds`
    ///   wire value with `invalidProperties` before reaching this
    ///   method. The slice will never be empty at this entry point.
    /// - **`mailbox_ids` syntax is valid.** Each id is a syntactically
    ///   well-formed `Id` per RFC 8620 §1.2. The handler does NOT
    ///   verify that each id refers to an existing Mailbox in
    ///   `account_id` — the backend MUST do that and reject with
    ///   `BackendSetError::SetError(SetError::new(SetErrorType::InvalidProperties))`
    ///   (with `properties: ["mailboxIds"]`) if any referenced
    ///   Mailbox is missing.
    /// - **`keywords` syntax is valid.** Each `Keyword` has been
    ///   parsed through `jmap_mail_types::Keyword` (RFC 8621 §4.1.1)
    ///   and normalised to lowercase. The slice MAY be empty —
    ///   empty `keywords` means the Email has no keywords set
    ///   (RFC 8621 §4.8 default `{}`), NOT that the backend should
    ///   apply a default like `$received`.
    ///
    /// ## `received_at: None` semantics
    ///
    /// RFC 8621 §4.8 specifies the default as "time of most recent
    /// Received header, or time of import on server if none". When
    /// the caller did not supply a `receivedAt` value, the handler
    /// passes `None` through and leaves the policy decision to the
    /// backend. A spec-compliant production backend SHOULD:
    ///
    /// 1. Parse the most recent `Received:` header from the blob
    ///    and use its timestamp, if present and parseable.
    /// 2. Otherwise, use the current server clock.
    ///
    /// The reference `MemoryBackend` (gated behind `feature = "memory"`)
    /// uses the epoch (`1970-01-01T00:00:00Z`) as a deterministic
    /// stand-in for tests; this is **not** spec-compliant production
    /// behaviour. Production backends MUST NOT copy this fallback.
    ///
    /// ## Thread assignment
    ///
    /// The handler does NOT call
    /// [`find_thread_by_message_ids`](MailBackend::find_thread_by_message_ids)
    /// before this method — there is no `thread_id` argument and no
    /// pre-computed thread hint. The backend is responsible for
    /// parsing the blob's `Message-ID` / `In-Reply-To` /
    /// `References` headers and joining or creating a thread.
    /// Implementations SHOULD share the same thread-assignment logic
    /// they use from `create_object::<Email>` so that the two entry
    /// points produce identical thread graphs for the same input.
    ///
    /// ## Sentinel-replacement contract
    ///
    /// Unlike [`create_object`](MailBackend::create_object), this
    /// method does NOT receive a partially-constructed `Email` with
    /// placeholder values. The backend builds the full `Email` from
    /// the raw blob and the four argument fields, and returns
    /// `(assigned_id, email)` directly. The returned `Email` MUST
    /// have correct `id`, `blob_id`, `thread_id`, and `size` —
    /// these are the four server-set fields RFC 8621 §4.8 requires
    /// in the `created` response map. The handler reads them out
    /// of the returned struct and forwards them to the client.
    ///
    /// ## Error mapping
    ///
    /// The handler maps the returned `Result` to the wire as follows:
    ///
    /// | Return | Wire response |
    /// |---|---|
    /// | `Ok((id, email))` | `created[creationId] = { id, blobId, threadId, size }` |
    /// | `Err(BackendSetError::SetError(e))` | `notCreated[creationId] = <SetError JSON>` |
    /// | `Err(BackendSetError::Other(e))` | `notCreated[creationId] = server_fail_value_from_backend(&e)` (a `serverFail` with the fixed `"internal error"` description per the bd:JMAP-wlip.2 / bd:JMAP-jfia.1 redaction contract — the backend `Display` output is **never** interpolated into the wire `description`, to prevent credential / blob / PII leaks) |
    ///
    /// Spec-defined `SetError` variants the backend SHOULD use:
    ///
    /// - **`BlobNotFound`** (`SetErrorType::BlobNotFound`) — the
    ///   `blob_id` is not present in the account's blob store.
    /// - **`AlreadyExists`** (`SetErrorType::AlreadyExists` with
    ///   `existing_id`) — RFC 8621 §4.8 permits the server to
    ///   forbid duplicate `Message-ID` values within an account.
    ///   Backends that enforce this MUST include the existing
    ///   Email id via
    ///   `SetError::new(SetErrorType::AlreadyExists).with_existing_id(...)`.
    /// - **`InvalidProperties`** (`SetErrorType::InvalidProperties`)
    ///   — a referenced Mailbox id does not exist, or the blob is
    ///   in the store but referenced fields are otherwise invalid.
    /// - **`OverQuota`** (`SetErrorType::OverQuota`) — the import
    ///   would push the account over its quota.
    /// - **`InvalidEmail`** (`SetErrorType::InvalidEmail`) — the
    ///   blob is not a valid RFC 5322 message and the backend
    ///   declined to repair it.
    ///
    /// `BackendSetError::Other(e)` is reserved for unexpected
    /// internal failures (disk I/O, deserialisation, etc.) that
    /// should reach the client as `serverFail` so the client
    /// knows to retry. Returning `Other` for a deterministic
    /// failure that has a spec-defined `SetError` variant
    /// surfaces as a non-retryable error and misleads the
    /// client — prefer the spec variant whenever it applies.
    fn import_email(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
        mailbox_ids: &[jmap_types::Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&jmap_types::UTCDate>,
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_mail_types::Email), BackendSetError<Self::Error>>,
    > + Send;

    /// Look up the thread id of the first stored
    /// [`Email`](jmap_mail_types::Email) whose `messageId` list intersects
    /// `message_ids`, or `None` if no match exists.
    ///
    /// The handler uses this method during `Email/set` / `Email/import` to
    /// reuse an existing thread id when an incoming message has the same
    /// `Message-ID` / `References` as a stored message. If this method
    /// returns `None`, the handler calls
    /// [`create_object`](MailBackend::create_object)`::<Email>` which
    /// generates a fresh thread id via the backend's own ID generator.
    ///
    /// # Persistence guidance
    ///
    /// Backends with durable storage SHOULD derive thread ids from a
    /// content-addressed hash of the `Message-ID` header (or a stable
    /// per-account index of message-id → thread-id) so that thread
    /// identity survives process restarts. Backends that generate
    /// thread ids from a process-startup-seeded counter (e.g.
    /// the reference `MemoryBackend`) will produce different thread
    /// graphs across restarts: acceptable for tests, unacceptable
    /// for production.
    fn find_thread_by_message_ids(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        message_ids: &[&str],
    ) -> impl std::future::Future<Output = Result<Option<jmap_types::Id>, Self::Error>> + Send;

    /// Return `true` if `blob_id` exists in `account_id`'s blob store.
    ///
    /// **RFC 8621 §5.8 requirement**: `Email/parse` MUST distinguish two failure
    /// cases — `notFound` (blob ID is not in the store) and `notParsable` (blob is
    /// in the store but cannot be interpreted as an RFC 5322 message). If this
    /// method always returns `true`, every parse failure will be reported to the
    /// client as `notParsable` even when the blob does not exist, which makes it
    /// impossible for the client to distinguish "wrong blob ID" from "valid blob,
    /// unreadable message".
    ///
    /// There is no default implementation: requiring an explicit implementation
    /// forces each backend author to confront this distinction. A default of `true`
    /// would silently produce non-conformant behavior for backends where blobs can
    /// be absent.
    ///
    /// # Three-way result
    ///
    /// The return type is `Result<bool, Self::Error>` to distinguish three
    /// states that callers actually need to tell apart:
    ///
    /// - `Ok(true)` — the blob is definitely present and reachable.
    /// - `Ok(false)` — the blob is definitely absent. The handler maps this
    ///   to `invalidProperties` ("blob not found") on a create, or to a
    ///   `notFound` entry on `Email/parse`.
    /// - `Err(_)` — connectivity/transient failure. The handler maps this
    ///   to `serverFail` so the client knows to retry. Returning `Ok(false)`
    ///   for a transient backend failure is a bug: it surfaces as a
    ///   deterministic-looking error and the client will not retry.
    fn blob_exists(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send;

    /// Parse a raw message blob and return an Email object without storing it
    /// (RFC 8621 §5.8 — `Email/parse`).
    fn parse_email(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        blob_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<jmap_mail_types::Email, Self::Error>> + Send;

    /// Copy an Email from one account to another (RFC 8620 §6.3).
    ///
    /// Returns the new id and the created Email in `to_account_id`.
    #[allow(clippy::too_many_arguments)]
    fn copy_email(
        &self,
        caller: &Self::CallerCtx,
        from_account_id: &jmap_types::Id,
        email_id: &jmap_types::Id,
        to_account_id: &jmap_types::Id,
        mailbox_ids: &[jmap_types::Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&jmap_types::UTCDate>,
    ) -> impl std::future::Future<
        Output = Result<(jmap_types::Id, jmap_mail_types::Email), BackendSetError<Self::Error>>,
    > + Send;

    /// Return search snippets for the given Email ids (RFC 8621 §5.9 — `SearchSnippet/get`).
    fn search_snippets(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        email_ids: &[jmap_types::Id],
        filter: Option<&jmap_mail_types::EmailFilterCondition>,
    ) -> impl std::future::Future<Output = Result<Vec<jmap_mail_types::SearchSnippet>, Self::Error>> + Send;

    /// Returns `true` if this backend implementation supports the given
    /// JMAP object type.
    ///
    /// # Contract
    ///
    /// This is a **global, stateless backend-capability check** — it asks
    /// "did this implementation wire up the methods needed for type
    /// `O`?", not "does this user's account have this type enabled?".
    /// That is why the method is synchronous and takes no `caller` or
    /// `account_id` arguments. Per-account capability variation belongs
    /// in the consumer's session-capability builder (workspace AGENTS.md
    /// library-kit posture), NOT here.
    ///
    /// # Types in scope
    ///
    /// Implementors should answer for the JMAP object types this
    /// trait covers (RFC 8621 plus opt-in extensions): `Email`,
    /// `Mailbox`, `Thread`, `Identity`, `EmailSubmission`,
    /// `VacationResponse`, `SearchSnippet`, and (under the
    /// corresponding feature flags) `SieveScript` and MDN types.
    /// Backends MAY answer for object types from sibling extensions
    /// (`Calendar`, `Chat`, etc.) if they also implement those
    /// traits, but the typical pattern is one backend impl per
    /// extension family. A backend that does not recognise `O`
    /// SHOULD return `false`.
    ///
    /// # Callers
    ///
    /// The handler library calls this method when handling optional
    /// methods. For example,
    /// [`handle_search_snippet_get`](crate::handle_search_snippet_get)
    /// returns `accountNotSupportedByMethod` when
    /// `supports_type::<SearchSnippet>()` is `false`, so that backends
    /// without snippet support can short-circuit before any per-method
    /// state is touched. The server consumer also calls this method
    /// when building the JMAP session capability response.
    ///
    /// # Default
    ///
    /// Backends that support every type in this trait unconditionally
    /// can return `true` always.
    fn supports_type<O: JmapObject>(&self) -> bool;

    /// Maximum number of email IDs to fetch from the backend when
    /// `collapseThreads=true`. Fetching stops at this limit; the response
    /// will omit `total` when this limit is reached.
    ///
    /// Default: 65536. Override to lower the per-account limit, e.g. for
    /// multi-tenant deployments where adversarial clients could otherwise
    /// trigger large in-memory scans.
    fn max_collapse_threads_emails(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> usize {
        65_536
    }

    /// Maximum bytes of body value text to return per `EmailBodyPart`.
    ///
    /// A value of `0` means unlimited. Used with `maxBodyValueBytes` in
    /// `Email/get` and `Email/parse`. Override in your implementation to
    /// enforce per-account limits.
    fn max_body_value_bytes(&self, _caller: &Self::CallerCtx, _account_id: &jmap_types::Id) -> u64 {
        0 // unlimited by default
    }

    /// Maximum seconds in the future that `sendAt` may be in an `EmailSubmission`.
    ///
    /// A value of `0` means no delayed send support. Used to validate `sendAt`
    /// in `EmailSubmission/set`. Override in your implementation to advertise
    /// this server capability.
    fn max_delayed_send_seconds(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> u64 {
        0 // no delayed send by default
    }

    /// Return `true` if this backend can compute `Mailbox/queryChanges` for
    /// the given account (RFC 8620 §5.6 — `canCalculateChanges`).
    ///
    /// The default is `false` because the in-process query filter in
    /// `handle_mailbox_query` cannot guarantee that the backend tracks
    /// per-query result sets. Override to `true` only if the backend
    /// maintains a stable, query-result-aware change log for Mailbox objects.
    fn can_calculate_mailbox_query_changes(
        &self,
        _caller: &Self::CallerCtx,
        _account_id: &jmap_types::Id,
    ) -> bool {
        false
    }

    /// Destroy multiple [`Email`](jmap_mail_types::Email) objects in a single backend operation.
    ///
    /// The default implementation calls [`Self::destroy_object::<Email>`] once per ID.
    /// Override for batch efficiency (e.g., a single SQL `DELETE … IN` or a single
    /// lock acquire in `MemoryBackend`).
    ///
    /// Returns one entry per input ID: `None` means destroyed successfully;
    /// `Some(e)` means the destroy failed with the given error. The ordering
    /// matches the input slice.
    fn batch_destroy_emails(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        email_ids: &[jmap_types::Id],
    ) -> impl std::future::Future<Output = Vec<(jmap_types::Id, Option<BackendSetError<Self::Error>>)>>
           + Send
    where
        Self: Sized,
    {
        async move {
            let mut results = Vec::with_capacity(email_ids.len());
            for id in email_ids {
                let err = self
                    .destroy_object::<jmap_mail_types::Email>(caller, account_id, id)
                    .await
                    .err();
                results.push((id.clone(), err));
            }
            results
        }
    }
}
