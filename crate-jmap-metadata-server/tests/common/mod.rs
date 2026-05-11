//! Shared test infrastructure.
//!
//! The in-memory backend used by these tests is the public reference
//! implementation [`jmap_metadata_server::memory::MemoryBackend`]. This
//! module re-exports it (and `MemoryError`) under the
//! `common::*` paths so tests can `use common::MemoryBackend;`.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code and unused-import warnings are suppressed because not all
//! items are used in every test binary.
#![allow(dead_code)]
#![allow(unused_imports)]

pub use jmap_metadata_server::memory::{MemoryBackend, MemoryError};
