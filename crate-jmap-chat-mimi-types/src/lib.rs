//! JMAP Chat MIMI provider extension types
//! (draft-atwood-jmap-chat-mimi-00).
//!
//! Normative reference: draft-atwood-jmap-chat-mimi-00 — the
//! `urn:ietf:params:jmap:chat:mimi` capability, with which an MLS
//! (RFC 9420) client uses a JMAP Chat server as its MIMI provider. The
//! server routes MLS messages between the client and the MIMI hub and
//! never decrypts them.
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies, no I/O.
//! - No MLS or TLS-codec dependency: byte-string properties stay as
//!   validated [`Base64Url`] strings and are never decoded or re-encoded
//!   here.
//! - Forbids `unsafe`.
//!
//! ## Public surface
//!
//! - [`JMAP_CHAT_MIMI_URI`] — the capability URI (draft §Capability).
//! - [`MimiCapability`] — the account-level capability object.
//! - [`MIMI_FANOUT_BODY_TYPE`], [`MLS_MESSAGE_BODY_TYPE`] — the
//!   `bodyType` values of draft §MLS Messages.
//! - [`KeyPackage`], [`KeyPackageCreate`], [`KeyPackageCreated`],
//!   [`KeyPackageProperty`] — the KeyPackage object and the `KeyPackage/get`
//!   and `KeyPackage/set` argument shapes (draft §KeyPackages).
//! - [`KeyMaterialFetchRequest`], [`KeyMaterialFetchResponse`],
//!   [`ClientKeyMaterial`], [`KeyMaterialUserCode`], [`KeyMaterialClientCode`]
//!   — the `KeyMaterial/fetch` arguments (draft §KeyMaterial/fetch).
//! - [`Base64Url`] / [`Base64UrlError`] — the unpadded base64url wire
//!   shape of every byte-string property (draft §Conventions).

#![forbid(unsafe_code)]

pub mod base64url;
pub mod capability;
pub mod keymaterial;
pub mod keypackage;

pub use base64url::{Base64Url, Base64UrlError};
pub use capability::{
    MimiCapability, JMAP_CHAT_MIMI_URI, MIMI_FANOUT_BODY_TYPE, MLS_MESSAGE_BODY_TYPE,
};
pub use keymaterial::{
    ClientKeyMaterial, KeyMaterialClientCode, KeyMaterialFetchRequest, KeyMaterialFetchResponse,
    KeyMaterialUserCode,
};
pub use keypackage::{KeyPackage, KeyPackageCreate, KeyPackageCreated, KeyPackageProperty};
