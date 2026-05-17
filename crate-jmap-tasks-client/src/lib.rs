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
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SessionClient,
    SetError, SetResponse, TaskListSetParams,
};

// ---------------------------------------------------------------------------
// JmapTasksExt — the extension trait
// ---------------------------------------------------------------------------

/// Extension trait adding JMAP Tasks methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_tasks_client::JmapTasksExt;`
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`]. Sealing prevents downstream
/// divergence (e.g. `impl JmapTasksExt for MySimulator`) and keeps
/// adding methods to the trait a non-breaking change.
pub trait JmapTasksExt: sealed::Sealed {
    /// Bind this client to a JMAP session for use with Tasks methods.
    ///
    /// The returned [`SessionClient`] captures the session at construction time.
    /// After re-fetching the session, construct a new `SessionClient` with the
    /// updated session.
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts ANY [`jmap_base_client::Session`],
    /// including one whose advertised capabilities do not include
    /// `urn:ietf:params:jmap:tasks` or whose `primaryAccounts` map has
    /// no entry for the tasks capability. The constructor performs no
    /// up-front validation and never fails — its return type is the
    /// infallible [`methods::SessionClient`], not a `Result`.
    ///
    /// Capability and primary-account validation is deferred to every
    /// individual method call on the returned [`SessionClient`]. If
    /// the session is unsuitable, those per-method calls return
    /// [`ClientError::InvalidSession`] with a description like
    /// `"no primary account for urn:ietf:params:jmap:tasks"`.
    ///
    /// Callers that want to guard at the binding site can pre-check
    /// the session before calling this method via
    /// [`session.primary_account_id("urn:ietf:params:jmap:tasks")`](jmap_base_client::Session::primary_account_id).
    fn with_tasks_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapTasksExt for jmap_base_client::JmapClient {
    fn with_tasks_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapTasksExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
