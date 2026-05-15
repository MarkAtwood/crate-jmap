//! jmap-metadata-client — JMAP Object Metadata extension method implementations.
//!
//! Implements the client-side method bindings for
//! [draft-ietf-jmap-metadata-01](https://datatracker.ietf.org/doc/draft-ietf-jmap-metadata/).
//! Depends on `jmap-base-client` for transport, auth, and session, and on
//! `jmap-metadata-types` for the wire types.
//!
//! Cookie-cut from `jmap-mail-client` (canonical extension-client template
//! per workspace AGENTS.md).
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_metadata_client::JmapMetadataExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_metadata_session(session);
//! // Fetch all Metadata objects in the account.
//! let metadata = sc.metadata_get(None, None, None).await?;
//! # let _ = metadata;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, MetadataChangesParams, MetadataGetParams,
    MetadataQueryChangesParams, MetadataQueryParams, MetadataSetParams, QueryChangesResponse,
    QueryResponse, SessionClient, SetError, SetResponse,
};

/// Extension trait adding JMAP Object Metadata (draft-ietf-jmap-metadata-01)
/// methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_metadata_client::JmapMetadataExt;`
///
/// All JMAP Metadata method calls are made through the [`SessionClient`]
/// returned by [`with_metadata_session`](JmapMetadataExt::with_metadata_session).
pub trait JmapMetadataExt {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Metadata method calls are made through the returned
    /// [`SessionClient`].
    fn with_metadata_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapMetadataExt for jmap_base_client::JmapClient {
    fn with_metadata_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}
