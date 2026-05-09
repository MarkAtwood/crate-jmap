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
