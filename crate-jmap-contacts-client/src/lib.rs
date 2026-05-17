//! jmap-contacts-client — JMAP Contacts method implementations.
//!
//! Depends on jmap-base-client for transport, auth, and session.
//! See PLAN.md for the full implementation plan.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_contacts_client::JmapContactsExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_contacts_session(session);
//! // List all address books in the primary account.
//! let address_books = sc.address_book_get(None, None).await?;
//! # let _ = address_books;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod methods;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, AddressBookSetParams, ChangesResponse, GetResponse, QueryChangesResponse,
    QueryResponse, SessionClient, SetError, SetResponse,
};

/// Extension trait adding JMAP Contacts methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_contacts_client::JmapContactsExt;`
///
/// All JMAP Contacts method calls are made through the [`SessionClient`]
/// returned by [`with_contacts_session`](JmapContactsExt::with_contacts_session).
/// This trait is **sealed**: implementations outside this crate are not
/// permitted. The crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`]. Sealing prevents downstream
/// divergence (e.g. `impl JmapContactsExt for MySimulator`) and keeps
/// adding methods to the trait a non-breaking change.
pub trait JmapContactsExt: sealed::Sealed {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Contacts method calls are made through the returned
    /// [`SessionClient`].
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts ANY [`jmap_base_client::Session`],
    /// including one whose advertised capabilities do not include
    /// `urn:ietf:params:jmap:contacts` or whose `primaryAccounts` map
    /// has no entry for the contacts capability. The constructor
    /// performs no up-front validation and never fails — its return
    /// type is the infallible [`methods::SessionClient`], not a
    /// `Result`.
    ///
    /// Capability and primary-account validation is deferred to every
    /// individual method call on the returned [`SessionClient`]. If
    /// the session is unsuitable, those per-method calls return
    /// [`ClientError::InvalidSession`] with a description like
    /// `"no primary account for urn:ietf:params:jmap:contacts"`.
    ///
    /// Callers that want to guard at the binding site can pre-check
    /// the session before calling this method via
    /// [`session.primary_account_id("urn:ietf:params:jmap:contacts")`](jmap_base_client::Session::primary_account_id).
    fn with_contacts_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapContactsExt for jmap_base_client::JmapClient {
    fn with_contacts_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapContactsExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
