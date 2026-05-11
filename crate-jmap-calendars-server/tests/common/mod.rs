//! Shared test infrastructure.
//!
//! The in-memory backend used by these tests lives in the crate itself
//! as the public reference implementation
//! [`jmap_calendars_server::memory::MemoryBackend`]. This module re-exports
//! it under the historical `common::*` path so tests can use
//! `use common::MemoryBackend;` and match the cookie-cutter pattern
//! established by sibling server crates.
//!
//! Each integration test binary includes this module with `mod common;`.
//! Dead-code and unused-import warnings are suppressed because not all
//! items are used in every test binary.
#![allow(dead_code)]
#![allow(unused_imports)]

pub use jmap_calendars_server::memory::{MemoryBackend, MemoryError};
