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
//! - [`Did`] — a DID validated against the W3C DID Core §3.1 syntax,
//!   with [`Did::web_domain`] for the `did:web` domain component;
//!   [`DidError`] reports a rejected parse.
//!
//! ## Resolution helpers
//!
//! Pure functions over supplied bytes for the three methods that
//! draft-atwood-jmap-chat-did-00 §Mandatory DID Methods requires. Network
//! fetch and caching belong in a resolver crate, not here.
//!
//! - `did:key`: [`DidKey::from_did`] decodes the key and
//!   [`DidKey::document`] derives the DID document locally.
//! - `did:web`: [`Did::web_document_url`] maps the DID to the HTTPS URL
//!   of its DID document.
//! - `did:plc`: [`verify_plc_audit_log`] verifies a PLC directory audit
//!   log and returns the current [`PlcState`], and
//!   [`PlcState::document`] presents that state as a DID document.

#![forbid(unsafe_code)]

pub mod capability;
pub mod did;
pub mod key;
pub mod plc;

pub use capability::{DidAccountCapability, DidCapability, KeyCustody, JMAP_DID_URI};
pub use did::{Did, DidError};
pub use key::{DidKey, DidKeyError, KeyType};
pub use plc::{verify_plc_audit_log, PlcError, PlcService, PlcState, PlcStatus};
