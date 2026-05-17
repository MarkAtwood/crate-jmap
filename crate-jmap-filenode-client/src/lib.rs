//! jmap-filenode-client — JMAP FileNode method implementations.
//!
//! Depends on jmap-base-client for transport, auth, and session.
//! See PLAN.md for the full implementation plan.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_filenode_client::JmapFilenodeExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_filenode_session(session);
//! // Fetch FileNode metadata. None ids = fetch all in the primary account.
//! let nodes = sc.filenode_get(None, None, None).await?;
//! # let _ = nodes;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::filenode::{
    FileNodeCopyParams, FileNodeGetParams, FileNodeOnExists, FileNodeSetParams,
};
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SessionClient,
    SetError, SetResponse,
};

/// Extension trait adding JMAP FileNode methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_filenode_client::JmapFilenodeExt;`
///
/// All JMAP FileNode method calls are made through the [`SessionClient`] returned
/// by [`with_filenode_session`](JmapFilenodeExt::with_filenode_session).
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`]. Sealing prevents downstream
/// divergence (e.g. `impl JmapFilenodeExt for MySimulator`) and keeps
/// adding methods to the trait a non-breaking change.
pub trait JmapFilenodeExt: sealed::Sealed {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP FileNode method calls are made through the returned [`SessionClient`].
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts ANY [`jmap_base_client::Session`],
    /// including one whose advertised capabilities do not include
    /// `urn:ietf:params:jmap:filenode` or whose `primaryAccounts` map
    /// has no entry for the filenode capability. The constructor
    /// performs no up-front validation and never fails — its return
    /// type is the infallible [`methods::SessionClient`], not a
    /// `Result`.
    ///
    /// Capability and primary-account validation is deferred to every
    /// individual method call on the returned [`SessionClient`]. If
    /// the session is unsuitable, those per-method calls return
    /// [`ClientError::InvalidSession`] with a description like
    /// `"no primary account for urn:ietf:params:jmap:filenode"`.
    ///
    /// Callers that want to guard at the binding site can pre-check
    /// the session before calling this method via
    /// [`session.primary_account_id("urn:ietf:params:jmap:filenode")`](jmap_base_client::Session::primary_account_id).
    fn with_filenode_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapFilenodeExt for jmap_base_client::JmapClient {
    fn with_filenode_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapFilenodeExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
