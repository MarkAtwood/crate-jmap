//! Shared test infrastructure.
//!
//! Most of the in-memory backend used by these tests now lives in the
//! crate itself as the public reference implementation
//! [`jmap_mail_server::memory::MemoryBackend`]. This module:
//!
//! - re-exports that public reference impl (and [`seed`](memory::seed))
//!   under the historical `common::*` paths, so existing tests can use
//!   `use common::MemoryBackend;` unchanged.
//! - keeps the test-only [`FaultyBackend`] fault-injection wrapper that
//!   forces specific backend operations to return
//!   `BackendSetError::Other(MemoryError(_))`. This is testing
//!   scaffolding (not a reference impl) and so stays here.
//! - keeps small test-fixture byte constants (`VALID_MDN_BLOB` etc.) that
//!   are oracle data, not reference-impl code.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code and unused-import warnings are suppressed because not all
//! items are used in every test binary.
#![allow(dead_code)]
#![allow(unused_imports)]

// Re-exports — keep `use common::MemoryBackend;` working for tests.
pub use jmap_mail_server::memory::{seed, MemoryBackend, MemoryError};

use jmap_mail_server::{
    BackendChangesError, BackendSetError, ChangesResult, GetObject, JmapBackend, JmapObject,
    MailBackend, QueryChangesResult, QueryObject, QueryResult, SetObject,
};
use jmap_types::{Id, State, UTCDate};

// ---------------------------------------------------------------------------
// FaultyBackend — injects BackendSetError::Other on demand
// ---------------------------------------------------------------------------

/// A thin wrapper around [`MemoryBackend`] that can inject
/// `BackendSetError::Other` for specific `(type_name, operation)` pairs.
///
/// Call [`FaultyBackend::inject`] before the operation under test. The first
/// matching call returns `BackendSetError::Other(MemoryError("injected …"))`;
/// the flag is cleared so subsequent calls go to the inner backend normally.
///
/// Test-only — kept in the test harness rather than the public reference
/// implementation to avoid promoting "fault-injection" as part of the
/// stable public API.
pub struct FaultyBackend {
    pub inner: MemoryBackend,
    failures:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<(&'static str, &'static str)>>>,
}

impl FaultyBackend {
    pub fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
            failures: Default::default(),
        }
    }

    /// Schedule a `BackendSetError::Other` for the next call to `op` on `type_name`.
    ///
    /// Calling `inject` twice for the same `(type_name, op)` pair is a no-op —
    /// only one fault is queued; the second call is silently ignored.
    pub fn inject(&self, type_name: &'static str, op: &'static str) {
        self.failures.lock().unwrap().insert((type_name, op));
    }

    /// Remove and return a previously-injected fault (fire-once).
    /// Returns `true` if the fault was present (and is now consumed).
    /// A second call for the same pair returns `false` and has no effect.
    fn take_fault(&self, type_name: &'static str, op: &'static str) -> bool {
        self.failures.lock().unwrap().remove(&(type_name, op))
    }
}

impl JmapBackend for FaultyBackend {
    type Error = MemoryError;
    type CallerCtx = ();

    async fn account_exists(&self, _caller: &(), account_id: &Id) -> Result<bool, Self::Error> {
        self.inner.account_exists(&(), account_id).await
    }

    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        ids: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), Self::Error> {
        self.inner
            .get_objects::<O>(&(), account_id, ids, properties)
            .await
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<State, Self::Error> {
        self.inner.get_state::<O>(&(), account_id).await
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_state: &State,
        max_changes: Option<u64>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .get_changes::<O>(&(), account_id, since_state, max_changes)
            .await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        limit: Option<u64>,
        position: i64,
    ) -> Result<QueryResult, Self::Error> {
        self.inner
            .query_objects::<O>(&(), account_id, filter, sort, limit, position)
            .await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        since_query_state: &State,
        filter: Option<&O::Filter>,
        sort: Option<&[O::Comparator]>,
        max_changes: Option<u64>,
        up_to_id: Option<&Id>,
        collapse_threads: bool,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner
            .query_changes::<O>(
                &(),
                account_id,
                since_query_state,
                filter,
                sort,
                max_changes,
                up_to_id,
                collapse_threads,
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// MailBackend impl for FaultyBackend (write-side and mail-specific)
// ---------------------------------------------------------------------------

impl MailBackend for FaultyBackend {
    async fn create_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        create_id: &str,
        obj: O,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "create") {
            return Err(BackendSetError::Other(MemoryError(
                "injected create error".to_owned(),
            )));
        }
        self.inner
            .create_object::<O>(&(), account_id, create_id, obj)
            .await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "update") {
            return Err(BackendSetError::Other(MemoryError(
                "injected update error".to_owned(),
            )));
        }
        self.inner
            .update_object::<O>(&(), account_id, id, patch)
            .await
    }

    async fn destroy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
    ) -> Result<(), BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "destroy") {
            return Err(BackendSetError::Other(MemoryError(
                "injected destroy error".to_owned(),
            )));
        }
        self.inner.destroy_object::<O>(&(), account_id, id).await
    }

    async fn import_email(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&jmap_types::UTCDate>,
    ) -> Result<(Id, jmap_mail_types::Email), BackendSetError<Self::Error>> {
        if self.take_fault("Email", "import") {
            return Err(BackendSetError::Other(MemoryError(
                "injected import error".to_owned(),
            )));
        }
        self.inner
            .import_email(&(), account_id, blob_id, mailbox_ids, keywords, received_at)
            .await
    }

    async fn find_thread_by_message_ids(
        &self,
        _caller: &(),
        account_id: &Id,
        message_ids: &[&str],
    ) -> Result<Option<Id>, Self::Error> {
        self.inner
            .find_thread_by_message_ids(&(), account_id, message_ids)
            .await
    }

    async fn blob_exists(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<bool, Self::Error> {
        if self.take_fault("", "blob_exists") {
            return Err(MemoryError("injected blob_exists failure".to_owned()));
        }
        self.inner.blob_exists(&(), account_id, blob_id).await
    }

    async fn parse_email(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<jmap_mail_types::Email, Self::Error> {
        self.inner.parse_email(&(), account_id, blob_id).await
    }

    async fn copy_email(
        &self,
        _caller: &(),
        from_account_id: &Id,
        email_id: &Id,
        to_account_id: &Id,
        mailbox_ids: &[Id],
        keywords: &[jmap_mail_types::Keyword],
        received_at: Option<&UTCDate>,
    ) -> Result<(Id, jmap_mail_types::Email), BackendSetError<Self::Error>> {
        self.inner
            .copy_email(
                &(),
                from_account_id,
                email_id,
                to_account_id,
                mailbox_ids,
                keywords,
                received_at,
            )
            .await
    }

    async fn search_snippets(
        &self,
        _caller: &(),
        account_id: &Id,
        email_ids: &[Id],
        filter: Option<&jmap_mail_types::EmailFilterCondition>,
    ) -> Result<Vec<jmap_mail_types::SearchSnippet>, Self::Error> {
        self.inner
            .search_snippets(&(), account_id, email_ids, filter)
            .await
    }

    fn supports_type<O: JmapObject>(&self) -> bool {
        self.inner.supports_type::<O>()
    }
}

// ---------------------------------------------------------------------------
// MDN test fixture constants (feature = "mdn")
// ---------------------------------------------------------------------------

/// Minimal valid MDN blob for use in MDN/parse tests.
///
/// Hand-written from RFC 9007 §3.3 + RFC 8098 §9 example.
/// This is the independent oracle for parse tests — do not generate from code under test.
#[cfg(feature = "mdn")]
pub const VALID_MDN_BLOB: &[u8] = b"\
From: Joe Recipient <Joe_Recipient@example.com>\r\n\
To: Jane Sender <Jane_Sender@example.org>\r\n\
Subject: Disposition notification\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/report; report-type=disposition-notification; boundary=\"RAA14128\"\r\n\
\r\n\
--RAA14128\r\n\
Content-Type: text/plain\r\n\
\r\n\
The message has been displayed on your recipient's computer.\r\n\
\r\n\
--RAA14128\r\n\
Content-Type: message/disposition-notification\r\n\
\r\n\
Reporting-UA: joes-pc.cs.example.com; Foomail 97.1\r\n\
Original-Recipient: rfc822;Joe_Recipient@example.com\r\n\
Final-Recipient: rfc822;Joe_Recipient@example.com\r\n\
Original-Message-ID: <199509192301.23456@example.org>\r\n\
Disposition: manual-action/MDN-sent-manually; displayed\r\n\
\r\n\
--RAA14128--\r\n\
";

/// Invalid blob (not a multipart/report MDN) for notParsable tests.
///
/// Does not contain a `Disposition:` field — the minimal parsability heuristic
/// used by `MemoryBackend::parse_mdns` will classify this as notParsable.
#[cfg(feature = "mdn")]
pub const INVALID_MDN_BLOB: &[u8] = b"This is just a plain text file, not an MDN.\r\n";

// ---------------------------------------------------------------------------
// Sieve test fixture constants (feature = "sieve")
// ---------------------------------------------------------------------------

/// A minimal valid Sieve script for test fixtures.
/// Source: RFC 5228 §8 "Formal Syntax" — "keep;" is the simplest valid script.
#[cfg(feature = "sieve")]
pub const VALID_SIEVE_SCRIPT: &[u8] = b"keep;";

/// An invalid/empty Sieve script — triggers `validate_sieve_script` to return Some(err).
#[cfg(feature = "sieve")]
pub const INVALID_SIEVE_SCRIPT: &[u8] = b"";
