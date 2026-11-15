//! JMAP Blob Management data types: draft-ietf-jmap-blobext-03
//! (`urn:ietf:params:jmap:blob2`) and RFC 9404 (`urn:ietf:params:jmap:blob`).
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` (shared RFC 8620 wire primitives) and
//! `jmap-blob-server` / `jmap-blob-client`.
//!
//! # Status
//!
//! Scaffold only (bd JMAP-tna1.2.1). The public surface is empty until the
//! wire types land; see `PLAN.md` for the planned modules:
//!
//! - capability objects, digest algorithms, `BlobRef`, data source objects,
//!   `Blob/set` and `Blob/get` types — bd JMAP-tna1.2.2;
//! - `Blob/convert` recipes and `Blob/lookup` types — bd JMAP-tna1.2.3;
//! - RFC 9404 `Blob/upload` types — bd JMAP-tna1.2.11.

#![forbid(unsafe_code)]
