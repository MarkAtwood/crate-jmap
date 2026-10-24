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
//!   `BackendSetError::Other(MemoryError)`. This is testing
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
pub use jmap_mail_server::memory::{MemoryBackend, MemoryError, seed};

use jmap_mail_server::MailBackend;
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult,
    SetObject,
};
use jmap_types::{Id, State, UTCDate};

// ---------------------------------------------------------------------------
// FaultyBackend — injects BackendSetError::Other on demand
// ---------------------------------------------------------------------------

/// Canary literal embedded in every injected `BackendSetError::Other`
/// payload (bd:JMAP-ic0j.68). Tests assert this string does NOT appear in
/// the wire-format `/set` response, which proves the per-id `serverFail`
/// Value path redacts backend-error Display text through
/// [`jmap_server::server_fail_value_from_backend`] rather than echoing it
/// onto the wire. Shaped like a leaked credential so a future contributor
/// who breaks the redaction sees the visible footgun in the test failure.
pub const FAULTY_BACKEND_CANARY: &str = "BACKEND-CANARY-LEAK-DO-NOT-WIRE-7f3a2";

/// A thin wrapper around [`MemoryBackend`] that can inject
/// `BackendSetError::Other` for specific `(type_name, operation)` pairs.
///
/// Call [`FaultyBackend::inject`] before the operation under test. The first
/// matching call returns `BackendSetError::Other(MemoryError::new("injected … <canary>".to_owned()))`;
/// the flag is cleared so subsequent calls go to the inner backend normally.
///
/// The injected error text always contains [`FAULTY_BACKEND_CANARY`] so a
/// redaction-leak assertion can be written generically.
///
/// The `(O::TYPE_NAME, "query")` pair is different: the next
/// `query_objects` call for type `O` returns `BackendQueryError::Method`
/// carrying `unsupportedFilter` with description
/// [`INJECTED_QUERY_METHOD_ERROR_DESCRIPTION`] (RFC 8620 §5.5).
///
/// Test-only — kept in the test harness rather than the public reference
/// implementation to avoid promoting "fault-injection" as part of the
/// stable public API.
/// Description of the `unsupportedFilter` an injected `"query"` fault
/// returns; seeing this text on the wire proves the backend's error passed
/// through the handler.
pub const INJECTED_QUERY_METHOD_ERROR_DESCRIPTION: &str = "backend cannot process this filter";

pub struct FaultyBackend {
    pub inner: MemoryBackend,
    failures:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<(&'static str, &'static str)>>>,
    blob_reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl FaultyBackend {
    pub fn new() -> Self {
        Self {
            inner: MemoryBackend::new(),
            failures: Default::default(),
            blob_reads: Default::default(),
        }
    }

    /// Number of `MailBackend::read_blob` calls made so far.
    pub fn blob_reads(&self) -> usize {
        self.blob_reads.load(std::sync::atomic::Ordering::SeqCst)
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
        params: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<Self::Error>> {
        self.inner.get_changes::<O>(&(), account_id, params).await
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "query") {
            let mut err = jmap_types::JmapError::unsupported_filter();
            err.description = Some(INJECTED_QUERY_METHOD_ERROR_DESCRIPTION.into());
            return Err(BackendQueryError::Method(err));
        }
        self.inner.query_objects::<O>(&(), account_id, params).await
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        params: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<Self::Error>> {
        self.inner.query_changes::<O>(&(), account_id, params).await
    }
}

// ---------------------------------------------------------------------------
// MailBackend impl for FaultyBackend (write-side and mail-specific)
// ---------------------------------------------------------------------------

impl MailBackend for FaultyBackend {
    async fn create_object<O, F>(
        &self,
        _caller: &(),
        account_id: &Id,
        create_id: &str,
        build: F,
    ) -> Result<(Id, O), BackendSetError<Self::Error>>
    where
        O: SetObject + Send + Sync,
        F: FnOnce(&Id) -> O + Send,
    {
        if self.take_fault(O::TYPE_NAME, "create") {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected create error {FAULTY_BACKEND_CANARY}"
            ))));
        }
        self.inner
            .create_object(&(), account_id, create_id, build)
            .await
    }

    async fn create_email<F>(
        &self,
        _caller: &(),
        account_id: &Id,
        create_id: &str,
        message: Vec<u8>,
        build: F,
    ) -> Result<(Id, jmap_mail_types::Email), BackendSetError<Self::Error>>
    where
        F: FnOnce(&Id, &Id, &[u8]) -> jmap_mail_types::Email + Send,
    {
        if self.take_fault("Email", "create") {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected create error {FAULTY_BACKEND_CANARY}"
            ))));
        }
        self.inner
            .create_email(&(), account_id, create_id, message, build)
            .await
    }

    async fn read_blob(
        &self,
        _caller: &(),
        account_id: &Id,
        blob_id: &Id,
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        self.blob_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.take_fault("", "read_blob") {
            return Err(MemoryError::new(format!(
                "injected read_blob failure {FAULTY_BACKEND_CANARY}"
            )));
        }
        self.inner.read_blob(&(), account_id, blob_id).await
    }

    async fn update_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        account_id: &Id,
        id: &Id,
        patch: O::Patch,
    ) -> Result<Option<O>, BackendSetError<Self::Error>> {
        if self.take_fault(O::TYPE_NAME, "update") {
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected update error {FAULTY_BACKEND_CANARY}"
            ))));
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
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected destroy error {FAULTY_BACKEND_CANARY}"
            ))));
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
            return Err(BackendSetError::Other(MemoryError::new(format!(
                "injected import error {FAULTY_BACKEND_CANARY}"
            ))));
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
            return Err(MemoryError::new(format!(
                "injected blob_exists failure {FAULTY_BACKEND_CANARY}"
            )));
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

    async fn copy_object<O: SetObject + Send + Sync>(
        &self,
        _caller: &(),
        from_account_id: &Id,
        to_account_id: &Id,
        obj: O,
        create: &serde_json::Value,
    ) -> Result<(Id, O), BackendSetError<Self::Error>> {
        self.inner
            .copy_object(&(), from_account_id, to_account_id, obj, create)
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

    fn supports_type<O: JmapObject>(&self, _caller: &(), account_id: &Id) -> bool {
        self.inner.supports_type::<O>(&(), account_id)
    }
}

// ---------------------------------------------------------------------------
// SieveBackend impl for FaultyBackend (feature = "sieve")
// ---------------------------------------------------------------------------

/// Delegates every method to the inner `MemoryBackend`.
#[cfg(feature = "sieve")]
impl jmap_mail_server::SieveBackend for FaultyBackend {
    async fn validate_sieve_script(
        &self,
        _caller: &(),
        account_id: &Id,
        script: &[u8],
    ) -> Result<Option<String>, Self::Error> {
        self.inner
            .validate_sieve_script(&(), account_id, script)
            .await
    }

    async fn max_sieve_script_bytes(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<Option<u64>, Self::Error> {
        self.inner.max_sieve_script_bytes(&(), account_id).await
    }

    async fn max_sieve_scripts_per_account(
        &self,
        _caller: &(),
        account_id: &Id,
    ) -> Result<usize, Self::Error> {
        self.inner
            .max_sieve_scripts_per_account(&(), account_id)
            .await
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
