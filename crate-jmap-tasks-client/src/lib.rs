//! JMAP Tasks extension client methods.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_tasks_client::JmapTasksExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_tasks_session(session);
//! let task_lists = sc.task_list_get(None, None).await?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::{
    ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SessionClient, SetResponse,
};

// ---------------------------------------------------------------------------
// JmapTasksExt — the extension trait
// ---------------------------------------------------------------------------

/// Extension trait adding JMAP Tasks methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_tasks_client::JmapTasksExt;`
pub trait JmapTasksExt {
    /// Bind this client to a JMAP session for use with Tasks methods.
    ///
    /// The returned [`SessionClient`] captures the session at construction time.
    /// After re-fetching the session, construct a new `SessionClient` with the
    /// updated session.
    fn with_tasks_session(self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapTasksExt for jmap_base_client::JmapClient {
    fn with_tasks_session(self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self,
            session,
        }
    }
}
