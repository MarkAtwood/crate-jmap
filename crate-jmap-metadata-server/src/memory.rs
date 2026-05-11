//! In-memory reference implementation of [`MetadataBackend`].
//!
//! # This is a reference implementation, not production
//!
//! `MemoryBackend` is intended for three audiences:
//!
//! 1. **Workspace integration tests** — every `tests/*.rs` integration test
//!    in this crate exercises method handlers against this backend.
//! 2. **Downstream contributors** — a documented, complete, source-readable
//!    implementation of the [`MetadataBackend`] trait to study when writing
//!    a real (database-backed) backend.
//! 3. **Examples and smoke tests** — boot a real JMAP-for-Metadata
//!    dispatcher with one line of code, without standing up a database.
//!
//! It is **not** suitable for production: all state is held in `HashMap`s
//! behind a `std::sync::Mutex`, persistence is not implemented, and a
//! number of draft-ietf-jmap-metadata-01 edge cases are simplified (see
//! source comments).
//!
//! # Feature flag and API stability
//!
//! This module is gated behind `feature = "memory"` and is **not** enabled
//! by default. Its public API stability is opt-in: it may break across
//! minor versions while the crate is pre-1.0.
//!
//! # Status (JMAP-06zp.3.1)
//!
//! The full backend implementation lands under JMAP-06zp.3.4. This file
//! is a scaffolding placeholder that exposes the `MemoryBackend` struct
//! so `cargo doc --all-features` and `cargo check --all-features` succeed
//! workspace-wide while the implementation is in progress.

#![deny(clippy::await_holding_lock)]

/// In-memory implementation of [`crate::MetadataBackend`] (placeholder
/// pending JMAP-06zp.3.4).
///
/// # Construction
///
/// Use [`MemoryBackend::new`] to get a fresh empty instance. Account
/// registration and `MetadataBackend` impls land under JMAP-06zp.3.4.
#[derive(Clone, Default)]
pub struct MemoryBackend;

impl MemoryBackend {
    /// Construct an empty `MemoryBackend`. Equivalent to [`Self::default`].
    pub fn new() -> Self {
        Self
    }
}
