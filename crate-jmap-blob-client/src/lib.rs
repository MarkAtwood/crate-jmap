//! JMAP Blob Management client: typed `Blob/set`, `Blob/get`,
//! `Blob/lookup` and `Blob/convert` calls (draft-ietf-jmap-blobext-03,
//! `urn:ietf:params:jmap:blob2`) over `jmap-base-client`.
//!
//! # Status
//!
//! Scaffold only (bd JMAP-tna1.2.1). The public surface is empty until the
//! `JmapBlobExt` extension trait and its `SessionClient` land; see
//! `PLAN.md`.

#![forbid(unsafe_code)]
