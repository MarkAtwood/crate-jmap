//! JMAP Blob Content Identifiers extension types
//! (draft-atwood-jmap-cid-00).
//!
//! Normative reference: draft-atwood-jmap-cid-00 — the
//! `urn:ietf:params:jmap:cid` JMAP capability. When a server
//! advertises this capability, it extends the blob upload response
//! defined in RFC 8620 §6.1 with a `sha256` field carrying the
//! SHA-256 digest of the uploaded content as a lowercase hex string
//! of exactly 64 characters. When the JMAP FileNode extension
//! (draft-ietf-jmap-filenode) is also supported, a `sha256` property
//! is added to FileNode objects.
//!
//! CID is independent of any single consumer extension. It is a
//! Blob/FileNode-level extension that any JMAP deployment can
//! advertise, and the `sha256` field defined here is also referenced
//! by draft-atwood-jmap-chat-00 (which defers to this document as
//! the normative definition).
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies.
//! - No JMAP-server / handler-library dependency.
//! - Forbids `unsafe`.
//!
//! ## Crate family position
//!
//! ```text
//! jmap-types
//!     └── jmap-cid-types  ← this crate (capability + sha256 type)
//! ```
//!
//! ## Public surface
//!
//! - [`Sha256`] — the 64-character lowercase-hex `sha256-value`
//!   from draft §2, with parse-time ABNF validation on construction
//!   and on deserialize.
//! - [`Sha256DigestError`] / [`Sha256DigestErrorKind`] — parse error
//!   reported by [`Sha256::from_hex`] and the [`Sha256`]
//!   `Deserialize` impl.
//! - [`JMAP_CID_URI`] — the `urn:ietf:params:jmap:cid` capability
//!   URI constant (draft §3).
//!
//! ## What this crate is not (yet)
//!
//! Blob upload-response wiring and the `supports_cid()` Session
//! advertisement detection are tracked separately as follow-up beads
//! (bd:JMAP-v9py.13 and bd:JMAP-v9py.14 — see `PLAN.md`).

#![forbid(unsafe_code)]

pub mod capability;
pub mod digest;

pub use capability::JMAP_CID_URI;
pub use digest::{Sha256, Sha256DigestError, Sha256DigestErrorKind};
