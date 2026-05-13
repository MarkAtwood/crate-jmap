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
//! ## What this crate is not (yet)
//!
//! This is the **skeleton** crate created by bd:JMAP-v9py.11. The
//! `Sha256` typed shape with parse-time 64-hex-char validation,
//! Blob upload-response wiring, and `supports_cid()` Session
//! advertisement detection are tracked separately as follow-up beads
//! (bd:JMAP-v9py.12, .13, .14 — see `PLAN.md`).

#![forbid(unsafe_code)]
