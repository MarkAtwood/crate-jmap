//! Shared test infrastructure.
//!
//! The in-memory backend used by these tests now lives in the crate itself
//! as the public reference implementation
//! [`jmap_sharing_server::memory::MemoryBackend`]. This module re-exports
//! it (and `MemoryError`) under the historical `common::*` paths so
//! existing tests can use `use common::MemoryBackend;` unchanged.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code and unused-import warnings are suppressed because not all
//! items are used in every test binary.
#![allow(dead_code)]
#![allow(unused_imports)]

pub use jmap_sharing_server::memory::{MemoryBackend, MemoryError};
