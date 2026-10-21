//! MimiBackend trait and supporting types for the MIMI provider handlers.
//!
//! Read-side operations (`get_objects`, `get_state`) are defined on the
//! [`jmap_server::JmapBackend`] supertrait. This module adds the MLS crypto
//! provider, the account-level capability object and the KeyPackage
//! writes.
//!
//! The `pub use` block re-exports the [`jmap_server`] backend types so a
//! downstream impl can `use jmap_chat_mimi_server::backend::*`; each
//! re-export pins the upstream type's major-version contract into this
//! crate.

pub use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesResult, QueryObject, QueryResult, SetError, SetErrorType, SetObject,
};

use jmap_chat_mimi_types::{Base64Url, MimiCapability};
use openmls::prelude::OpenMlsCrypto;

/// A KeyPackage that passed the per-KeyPackage checks of
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set, ready for
/// [`MimiBackend::create_key_package`].
///
/// Built by [`crate::check_key_package_create`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewKeyPackage {
    /// The client's `keyPackage`, verbatim (§Conventions and Definitions:
    /// "Servers MUST NOT re-encode these octets").
    pub key_package: Base64Url,
    /// The KeyPackageRef of `key_package` (RFC 9420 §5.2), with its
    /// length prefix.
    pub key_package_ref: Base64Url,
    /// The `lastResort` value derived from `key_package`.
    pub last_resort: bool,
    /// The leaf node's signature key, which "identifies the client and
    /// binds the KeyPackage to the account".
    pub signature_key: Vec<u8>,
}

/// The `invalidProperties` SetError for a KeyPackage whose signature key
/// is bound to another account.
///
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set: "The server SHOULD
/// reject, with `invalidProperties` naming `keyPackage`, a KeyPackage whose
/// signature key is bound to another account."
pub fn signature_key_bound_elsewhere() -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties(["keyPackage"])
        .with_description("The KeyPackage signature key is bound to another account.")
}

/// The `invalidProperties` SetError for a KeyPackage whose KeyPackageRef
/// is already in use.
///
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set: "The server MUST reject,
/// with `invalidProperties` naming `keyPackage`, a KeyPackage whose
/// KeyPackageRef equals that of a KeyPackage it stores or has handed out".
pub fn key_package_ref_in_use() -> SetError {
    SetError::new(SetErrorType::InvalidProperties)
        .with_properties(["keyPackage"])
        .with_description("A KeyPackage with this KeyPackageRef is stored or was handed out.")
}

/// The `overQuota` SetError for a create beyond `maxKeyPackages`.
///
/// draft-atwood-jmap-chat-mimi-00 §KeyPackage/set: the server "MUST reject
/// a create that would exceed `maxKeyPackages` with `overQuota`".
pub fn key_package_over_quota() -> SetError {
    SetError::new(SetErrorType::OverQuota)
        .with_description("The account already stores maxKeyPackages KeyPackages.")
}

/// Storage backend for the MIMI provider handlers
/// (draft-atwood-jmap-chat-mimi-00).
///
/// This trait is not object-safe by design (associated type, generic
/// supertrait methods). Use `Arc<impl MimiBackend>` when sharing across
/// tasks.
pub trait MimiBackend: JmapBackend {
    /// The MLS crypto provider that verifies KeyPackage signatures and
    /// computes KeyPackageRefs, for example `openmls_rust_crypto::RustCrypto`.
    type Crypto: OpenMlsCrypto;

    /// The crypto provider. It needs no key material: the handlers only
    /// verify and hash.
    fn mls_crypto(&self) -> &Self::Crypto;

    /// The account-level `urn:ietf:params:jmap:chat:mimi` capability object
    /// (draft-atwood-jmap-chat-mimi-00 §Account-Level Capability Object) the
    /// consumer advertises for `account_id` to this caller.
    ///
    /// `KeyPackage/set` create reads `userUri`, `cipherSuites` and
    /// `maxKeyPackages` from it. Return the value the Session object
    /// carries; a different value makes the handler contradict the
    /// advertised capability.
    fn mimi_capability(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<MimiCapability, Self::Error>> + Send;

    /// Store a KeyPackage record.
    ///
    /// The handler has run the per-KeyPackage checks. The backend MUST, in
    /// one atomic step, applying the first rule that fails:
    ///
    /// - refuse with [`signature_key_bound_elsewhere`] when
    ///   `key_package.signature_key` is bound to an account other than
    ///   `account_id`; otherwise bind it to `account_id` ("the first
    ///   KeyPackage the server accepts with a given signature key binds
    ///   that signature key to the account"). The draft says SHOULD; a
    ///   deployment that skips this check documents why;
    /// - refuse with [`key_package_ref_in_use`] when
    ///   `key_package.key_package_ref` equals that of a KeyPackage it
    ///   stores, in any account, or has handed out;
    /// - when `key_package.last_resort` is `true`, destroy the stored
    ///   last-resort record with the same signature key in `account_id`,
    ///   if any ("Creating a last-resort KeyPackage for a client destroys
    ///   that client's previous last-resort KeyPackage"), recording a
    ///   `destroyed` change;
    /// - refuse with [`key_package_over_quota`] when the account would then
    ///   hold more than `max_key_packages` records, last-resort records
    ///   included; nothing is destroyed or bound in that case;
    /// - assign the record `id` and store the record.
    ///
    /// Returns the assigned id. The state advances only when the create
    /// commits, as for the canonical `jmap-mail-server` `create_object`.
    fn create_key_package(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        key_package: NewKeyPackage,
        max_key_packages: u64,
    ) -> impl std::future::Future<Output = Result<jmap_types::Id, BackendSetError<Self::Error>>> + Send;

    /// Destroy a KeyPackage record by id. Unknown `id` is `notFound`; the
    /// state advances only when the destroy commits. The signature-key
    /// binding stays.
    fn destroy_key_package(
        &self,
        caller: &Self::CallerCtx,
        account_id: &jmap_types::Id,
        id: &jmap_types::Id,
    ) -> impl std::future::Future<Output = Result<(), BackendSetError<Self::Error>>> + Send;
}
