//! jmap-filenode-client — JMAP FileNode method implementations.
//!
//! Depends on jmap-base-client for transport, auth, and session.
//! See PLAN.md for the full implementation plan.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_filenode_client::JmapFileNodeExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_filenode_session(session);
//! // Fetch FileNode metadata. None ids = fetch all in the primary account.
//! let nodes = sc.file_node_get(None, None, None).await?;
//! # let _ = nodes;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::filenode::{FileNodeCopyParams, FileNodeOnExists, FileNodeSetParams};
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SessionClient,
    SetError, SetResponse,
};

/// Extension trait adding JMAP FileNode methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_filenode_client::JmapFileNodeExt;`
///
/// All JMAP FileNode method calls are made through the [`SessionClient`] returned
/// by [`with_filenode_session`](JmapFileNodeExt::with_filenode_session).
pub trait JmapFileNodeExt {
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

impl JmapFileNodeExt for jmap_base_client::JmapClient {
    fn with_filenode_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}
