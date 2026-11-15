//! RFC 9425 JMAP Quotas client: typed `Quota/get`, `/changes`, `/query` and
//! `/queryChanges` calls over `jmap-base-client`.
//!
//! Bind a session with [`JmapQuotasExt::with_quotas_session`] and call the
//! methods on the returned [`SessionClient`].
//!
//! A server returns, in a Quota's `types`, only the types whose capability
//! the request's `using` lists, and no Quota with none of them (RFC 9425
//! §4.1). So `using` carries more than `core` and `quota`: by default every
//! capability the Session advertises, so that every Quota is visible.
//! [`SessionClient::with_type_capabilities`] narrows it to the data types
//! the caller handles.
//!
//! # Example
//!
//! ```no_run
//! # async fn demo(client: jmap_base_client::JmapClient, session: jmap_base_client::Session)
//! # -> Result<(), jmap_base_client::ClientError> {
//! use jmap_quotas_client::JmapQuotasExt;
//!
//! let quotas = client.with_quotas_session(session);
//! for q in quotas.quota_get(None).await?.list {
//!     println!("{}: {} of {}", q.name, q.used, q.hard_limit);
//! }
//! # Ok(()) }
//! ```

#![forbid(unsafe_code)]

mod methods;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SessionClient,
};

/// Extension trait adding JMAP Quotas methods to
/// [`jmap_base_client::JmapClient`].
///
/// This trait is **sealed**: the crate implements it only for
/// [`jmap_base_client::JmapClient`], so adding methods stays non-breaking.
pub trait JmapQuotasExt: sealed::Sealed {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// The session is not checked here. Each method call returns
    /// [`ClientError::InvalidSession`] when the session has no primary
    /// account for `urn:ietf:params:jmap:quota`.
    fn with_quotas_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapQuotasExt for jmap_base_client::JmapClient {
    fn with_quotas_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
            type_capabilities: None,
            i18n: None,
        }
    }
}

mod sealed {
    /// Sealing trait for [`super::JmapQuotasExt`].
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
