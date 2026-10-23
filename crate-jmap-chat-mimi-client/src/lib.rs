//! JMAP Chat MIMI provider client (draft-atwood-jmap-chat-mimi-00).
//!
//! Normative reference: draft-atwood-jmap-chat-mimi-00 — the
//! `urn:ietf:params:jmap:chat:mimi` capability, with which an MLS
//! (RFC 9420) client uses a JMAP Chat server as its MIMI provider.
//!
//! ## Public surface
//!
//! - [`JmapMimiExt`] / [`SessionClient`] — the `KeyPackage/get` and
//!   `KeyPackage/set` bindings (draft §KeyPackages), `KeyMaterial/fetch`
//!   (draft §KeyMaterial/fetch), the `Room/create`, `Room/update`,
//!   `Room/send`, `Room/unbind` and `GroupInfo/fetch` bindings (draft
//!   §Rooms) and the account's [`MimiCapability`] (draft §Account-Level
//!   Capability Object).
//! - [`account_has_left`] — whether a commit removed the last of the
//!   account's clients, after which the client SHOULD call `Room/unbind`
//!   (draft §Chat.roomId).
//! - [`verify_group_info_response`], [`group_info_names_hub_sender`],
//!   [`VerifiedGroupInfoResponse`], [`GroupInfoResponseError`] — the
//!   checks a client SHOULD make on a `GroupInfo/fetch` response before an
//!   external join (draft §GroupInfo/fetch).
//!
//! MLS structures are parsed and verified with crates.io `openmls`; the
//! caller supplies the `OpenMlsCrypto` provider.
//!
//! `openmls` 0.x types appear in this crate's public API, so the crate
//! re-exports [`openmls`]. Name it through this crate to get the version it
//! was built with, and expect a major release of this crate whenever it
//! moves to a new `openmls` minor version:
//!
//! ```rust
//! use jmap_chat_mimi_client::openmls::prelude::Ciphersuite;
//!
//! // RFC 9420 §17.1: MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519 is 0x0001.
//! let suite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
//! assert_eq!(u16::from(suite), 0x0001);
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_chat_mimi_client::JmapMimiExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_mimi_session(session);
//! sc.room_unbind("mimi://a.example/r/k5qwmzlqmfsgk3tbnvsxi2lf4a").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Re-exported types
//!
//! [`ClientError`] is a re-export from `jmap-base-client`, and the
//! `KeyPackage/get` and `KeyPackage/set` bindings return the `jmap-types`
//! `GetResponse` and `SetResponse`; none of these is defined here. The
//! `jmap-*` workspace crates are versioned and released together:
//! depend on the same version of this crate, `jmap-base-client` and
//! `jmap-types`.

#![forbid(unsafe_code)]

mod group_info;
mod membership;
mod methods;

pub use group_info::{
    group_info_names_hub_sender, verify_group_info_response, GroupInfoResponseError,
    VerifiedGroupInfoResponse,
};
pub use jmap_base_client::ClientError;
pub use jmap_chat_mimi_types::MimiCapability;
pub use membership::account_has_left;
pub use methods::SessionClient;
pub use openmls;

/// Extension trait adding the MIMI provider methods to
/// [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_chat_mimi_client::JmapMimiExt;`
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`]. Sealing prevents downstream
/// divergence and keeps adding methods to the trait a non-breaking change.
pub trait JmapMimiExt: sealed::Sealed {
    /// Bind this client to a JMAP session for use with MIMI provider
    /// methods.
    ///
    /// The returned [`SessionClient`] captures the session at construction
    /// time. After re-fetching the session, construct a new
    /// `SessionClient` with the updated session.
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts ANY [`jmap_base_client::Session`] and never
    /// fails. Each method call on the returned [`SessionClient`] returns
    /// [`ClientError::InvalidSession`] when the session has no primary
    /// account for `urn:ietf:params:jmap:chat:mimi`.
    fn with_mimi_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapMimiExt for jmap_base_client::JmapClient {
    fn with_mimi_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapMimiExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
