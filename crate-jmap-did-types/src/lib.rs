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
//! draft-atwood-jmap-chat-did-00 §Mandatory DID Methods requires, and for
//! the `did:webvh` method it recommends. Network fetch and caching belong
//! in a resolver crate, not here.
//!
//! - `did:key`: [`DidKey::from_did`] decodes the key and
//!   [`DidKey::document`] derives the DID document locally.
//! - `did:web`: [`Did::web_document_url`] maps the DID to the HTTPS URL
//!   of its DID document.
//! - `did:plc`: [`verify_plc_audit_log`] verifies a PLC directory audit
//!   log and returns the current [`PlcState`], and
//!   [`PlcState::document`] presents that state as a DID document.
//! - `did:webvh`: [`webvh_log_url`] maps the DID to the HTTPS URL of its
//!   DID log, and [`verify_webvh_log`] verifies a DID log and its witness
//!   proofs by the DIF did:webvh v1.0 rules and returns a [`WebvhLog`]
//!   whose valid versions resolve by `versionId`.

#![forbid(unsafe_code)]

mod capability;
pub mod did;
mod jcs;
pub mod key;
pub mod plc;
pub mod webvh;

pub use capability::{DidAccountCapability, DidCapability, KeyCustody, JMAP_DID_URI};
pub use did::{Did, DidError};
pub use key::{DidKey, DidKeyError, KeyType};
pub use plc::{verify_plc_audit_log, PlcError, PlcService, PlcState, PlcStatus};
pub use webvh::{
    verify_webvh_log, webvh_log_url, WebvhError, WebvhLog, WebvhParameters, WebvhVersion,
    WebvhWitness,
};
