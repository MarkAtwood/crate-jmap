//! JMAP Blob Management method handlers for the `jmap-server` dispatcher:
//! draft-ietf-jmap-blobext-03 (`urn:ietf:params:jmap:blob2`) and RFC 9404
//! (`urn:ietf:params:jmap:blob`).
//!
//! # Status
//!
//! Scaffold only (bd JMAP-tna1.2.1). The public surface is empty until the
//! `BlobBackend` trait and `register_blob_handlers` land in
//! bd JMAP-tna1.2.4; see `PLAN.md` for the planned modules. The `memory`
//! and `test-helpers` features exist but gate nothing yet.

#![forbid(unsafe_code)]
// bd:JMAP-uqhn.6 — narrowing and sign-changing `as` casts are banned;
// use `try_from`.
#![warn(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
