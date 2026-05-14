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

    /// Import a raw message blob as an Email (RFC 8621 §5.7).
    ///
    /// The blob must already be stored (uploaded via JMAP blob upload). Returns
    /// the assigned id and the created Email object.
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

    /// Return the thread id of the first stored [`Email`](jmap_mail_types::Email) whose
    /// `messageId` list intersects `message_ids`, or `None` if no match exists.
    ///
    /// **Persistent backends MUST override this method.** The default `next_id`
    /// generator used when this returns `None` is seeded from system-clock
    /// nanoseconds at process startup. Two processes that start within the same
    /// nanosecond (common in containers and test harnesses) will produce
    /// identical ID sequences, silently corrupting thread graphs across
    /// restarts. A persistent backend must derive thread IDs from durable
    /// storage — for example, by looking up a content-addressed hash of the
    /// message-id header — so that thread identity survives process boundaries.
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

    /// Returns true if this account supports the given JMAP object type.
    /// Called by the server consumer (e.g. the session capability builder) —
    /// NOT called internally by the handler library. Backends that support all
    /// types unconditionally can return `true` always.
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
