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
//! - [`Sha256DigestError`] — parse error reported by
//!   [`Sha256::from_hex`] and the [`Sha256`] `Deserialize` impl.
//!   Single-tier enum (no wrapper struct); `#[non_exhaustive]` at
//!   the type level plus per-variant `#[non_exhaustive]` keeps both
//!   variant additions and per-variant field additions
//!   semver-additive.
//! - [`JMAP_CID_URI`] — the `urn:ietf:params:jmap:cid` capability
//!   URI constant (draft §3).
//! - [`CidCapability`] — the value object of the
//!   `urn:ietf:params:jmap:cid` capability (draft §3). Currently
//!   empty per the draft; `#[non_exhaustive]` with an `extra`
//!   field per workspace extras-preservation policy.
//!
//! ## Wiring into the rest of the kit
//!
//! The Blob upload-response binding and `Session::supports_cid()`
//! advertisement check live in `jmap-base-client` (landed under
//! closed beads bd:JMAP-v9py.13 and bd:JMAP-v9py.14):
//!
//! - `jmap_base_client::blob::BlobUploadResponse.sha256:
//!   Option<jmap_cid_types::Sha256>` — typed digest carried on the
//!   blob upload response when the server advertises CID.
//! - `jmap_base_client::Session::supports_cid() -> bool` — checks
//!   the session-level capability map for `urn:ietf:params:jmap:cid`.
//!
//! The [`CidCapability`] value-object shape (draft §3) lives in
//! the [`capability`] module alongside [`JMAP_CID_URI`].

#![forbid(unsafe_code)]

pub mod capability;
pub mod digest;

pub use capability::{CidCapability, JMAP_CID_URI};
pub use digest::{Sha256, Sha256DigestError};
