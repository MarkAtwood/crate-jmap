//! JMAP Decentralized Identifiers extension types
//! (draft-atwood-jmap-did-00).
//!
//! Normative reference: draft-atwood-jmap-did-00 — the
//! `urn:ietf:params:jmap:did` JMAP capability. A server advertising it
//! binds each account to a DID (W3C DID Core) and publishes the binding
//! in the account-level capability object.
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies, no network I/O.
//! - No JMAP-server / handler-library dependency.
//! - Forbids `unsafe`.
//!
//! ## Crate family position
//!
//! ```text
//! jmap-types
//!     └── jmap-did-types  ← this crate
//! ```
//!
//! ## Public surface
//!
//! - [`JMAP_DID_URI`] — the `urn:ietf:params:jmap:did` capability URI
//!   (draft §Capability).
//! - [`DidCapability`] — the session-level capability object.
//! - [`DidAccountCapability`] — the account-level capability object.
//! - [`KeyCustody`] — the `keyCustody` value of the account-level
//!   capability object.

#![forbid(unsafe_code)]

pub mod capability;

pub use capability::{DidAccountCapability, DidCapability, KeyCustody, JMAP_DID_URI};
