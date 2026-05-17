//! jmap-sharing-client — JMAP Sharing method implementations (RFC 9670).
//!
//! Depends on jmap-base-client for transport, auth, and session.
//! See PLAN.md for the full implementation plan.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_sharing_client::JmapSharingExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_sharing_session(session);
//! // List ShareNotifications in the primary account.
//! let notifications = sc.share_notification_get(None, None).await?;
//! # let _ = notifications;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, QueryChangesResponse, QueryResponse, SessionClient,
    SetError, SetResponse,
};

/// Extension trait adding JMAP Sharing methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_sharing_client::JmapSharingExt;`
///
/// All JMAP Sharing method calls are made through the [`SessionClient`] returned
/// by [`with_sharing_session`](JmapSharingExt::with_sharing_session).
pub trait JmapSharingExt {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Sharing method calls are made through the returned [`SessionClient`].
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts ANY [`jmap_base_client::Session`],
    /// including one whose advertised capabilities do not include
    /// `urn:ietf:params:jmap:principals` or whose `primaryAccounts`
    /// map has no entry for the principals capability. The
    /// constructor performs no up-front validation and never fails —
    /// its return type is the infallible
    /// [`methods::SessionClient`], not a `Result`.
    ///
    /// Capability and primary-account validation is deferred to every
    /// individual method call on the returned [`SessionClient`]. If
    /// the session is unsuitable, those per-method calls return
    /// [`ClientError::InvalidSession`] with a description like
    /// `"no primary account for urn:ietf:params:jmap:principals"`.
    ///
    /// Callers that want to guard at the binding site can pre-check
    /// the session before calling this method via
    /// [`session.primary_account_id("urn:ietf:params:jmap:principals")`](jmap_base_client::Session::primary_account_id).
    fn with_sharing_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapSharingExt for jmap_base_client::JmapClient {
    fn with_sharing_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}
