//! jmap-mail-client — RFC 8621 JMAP for Mail method implementations.
//!
//! Depends on jmap-base-client for transport, auth, and session.
//! See PLAN.md for the full implementation plan.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_mail_client::JmapMailExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_mail_session(session);
//! // Fetch Email metadata. None ids = fetch all (typically scoped via /query first).
//! let emails = sc.email_get(None, None, None).await?;
//! # let _ = emails;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, EmailCopyParams, EmailGetParams, EmailImportCreated,
    EmailImportInput, EmailImportResponse, EmailParseParams, EmailParseResponse,
    EmailSubmissionSetParams, GetResponse, MailboxSetParams, QueryChangesResponse, QueryResponse,
    SessionClient, SetError, SetResponse,
};

/// Extension trait adding RFC 8621 (JMAP for Mail) methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_mail_client::JmapMailExt;`
///
/// All JMAP Mail method calls are made through the [`SessionClient`] returned
/// by [`with_mail_session`](JmapMailExt::with_mail_session).
///
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`]. Sealing prevents downstream
/// divergence (e.g. `impl JmapMailExt for MySimulator`) and keeps
/// adding methods to the trait a non-breaking change.
pub trait JmapMailExt: sealed::Sealed {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Mail method calls are made through the returned [`SessionClient`].
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts ANY [`jmap_base_client::Session`],
    /// including one whose advertised capabilities do not include
    /// `urn:ietf:params:jmap:mail` or whose `primaryAccounts` map has
    /// no entry for the mail capability. The constructor performs no
    /// up-front validation and never fails — its return type is the
    /// infallible [`methods::SessionClient`], not a `Result`.
    ///
    /// Capability and primary-account validation is deferred to every
    /// individual method call on the returned [`SessionClient`]. If
    /// the session is unsuitable, those per-method calls return
    /// [`ClientError::InvalidSession`] with a description like
    /// `"no primary account for urn:ietf:params:jmap:mail"`.
    ///
    /// Callers that want to guard at the binding site can pre-check
    /// the session before calling this method:
    ///
    /// ```ignore
    /// if session
    ///     .primary_account_id("urn:ietf:params:jmap:mail")
    ///     .is_none()
    /// {
    ///     // Session does not advertise a primary account for mail;
    ///     // every subsequent SessionClient method call would fail
    ///     // with ClientError::InvalidSession. Refuse here.
    ///     return Err(MyAppError::SessionMissingMailCapability);
    /// }
    /// let sc = client.with_mail_session(session);
    /// ```
    ///
    /// `SessionClient::mail_account_id()` exposes the same check as a
    /// convenience accessor on an already-bound SessionClient.
    fn with_mail_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapMailExt for jmap_base_client::JmapClient {
    fn with_mail_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapMailExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
