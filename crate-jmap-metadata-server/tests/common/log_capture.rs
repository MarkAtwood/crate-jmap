//! Per-test [`tracing`] subscriber that buffers all emitted log lines so
//! tests can assert no credential-grade secret literal appears in
//! captured output.
//!
//! See bd:JMAP-sc1b.102 and the workspace `AGENTS.md` "Security testing"
//! section for the full pattern rationale.
//!
//! # Usage
//!
//! ```rust,ignore
//! mod common;
//! use common::log_capture::LogCapture;
//!
//! #[test]
//! fn secrets_never_appear_in_log_output() {
//!     let capture = LogCapture::new();
//!     // ... exercise code paths that might log ...
//!     capture.assert_does_not_contain("CANARY-LITERAL");
//! }
//! ```
//!
//! ## Isolation
//!
//! [`LogCapture::new`] installs a thread-local subscriber via
//! [`tracing::subscriber::set_default`]. The returned guard stays in
//! scope for the test's lifetime; once it drops, the subscriber is
//! unregistered. Parallel tests do not interfere because the install
//! is per-thread.

use std::io;
use std::sync::{Arc, Mutex};

use tracing::subscriber::DefaultGuard;
use tracing_subscriber::fmt::MakeWriter;

/// Per-test buffered tracing subscriber.
///
/// Captures every `tracing::*` event emitted on the current thread for
/// the lifetime of this value into an in-memory [`Vec<u8>`] that
/// [`contents`](Self::contents) and the assertion helpers expose.
pub struct LogCapture {
    buffer: Arc<Mutex<Vec<u8>>>,
    // Held to keep the per-thread subscriber installed. Underscore
    // prefix because the guard is not read directly — its drop is what
    // matters.
    _guard: DefaultGuard,
}

impl LogCapture {
    /// Install a buffering subscriber as the thread-local default
    /// [`tracing::Subscriber`] and return a handle whose drop
    /// uninstalls it.
    ///
    /// The subscriber captures every level (TRACE through ERROR) with
    /// ANSI escape sequences disabled so the captured bytes are plain
    /// UTF-8 text suitable for `contains` checks.
    pub fn new() -> Self {
        let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let make_writer = SharedBufferMakeWriter(Arc::clone(&buffer));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(make_writer)
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            // Disable timestamps so captured text is fully deterministic
            // and assertions remain readable across re-runs.
            .without_time()
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        Self {
            buffer,
            _guard: guard,
        }
    }

    /// Return everything captured so far as a `String`.
    ///
    /// The buffer is not cleared by this call. Repeated invocations
    /// during a single test will see the captured prefix grow as more
    /// events are emitted.
    pub fn contents(&self) -> String {
        let bytes = self.buffer.lock().expect("log capture mutex poisoned");
        String::from_utf8(bytes.clone()).expect("captured tracing output must be UTF-8")
    }

    /// Assert that the captured output does not contain `needle`.
    ///
    /// Panics with a diagnostic message that includes the captured
    /// content if the needle is found. The intended use is to assert
    /// that a credential-grade canary literal supplied at test setup
    /// does not appear in any log line emitted during the test.
    pub fn assert_does_not_contain(&self, needle: &str) {
        let contents = self.contents();
        assert!(
            !contents.contains(needle),
            "captured tracing output must not contain {needle:?}; got:\n{contents}"
        );
    }

    /// Assert that the captured output contains `needle`.
    ///
    /// Used by the harness self-test to confirm the capture mechanism
    /// is working before relying on its negative assertions.
    pub fn assert_contains(&self, needle: &str) {
        let contents = self.contents();
        assert!(
            contents.contains(needle),
            "captured tracing output must contain {needle:?}; got:\n{contents}"
        );
    }
}

impl Default for LogCapture {
    fn default() -> Self {
        Self::new()
    }
}

/// `MakeWriter` adapter that hands out clones of a shared `Vec<u8>`
/// behind a `Mutex` so every `tracing` event writes into the same
/// in-memory buffer.
#[derive(Clone)]
struct SharedBufferMakeWriter(Arc<Mutex<Vec<u8>>>);

impl<'a> MakeWriter<'a> for SharedBufferMakeWriter {
    type Writer = SharedBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter(Arc::clone(&self.0))
    }
}

/// `io::Write` adapter that appends to the shared buffer.
struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| io::Error::other("log capture mutex poisoned"))?;
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
