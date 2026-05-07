//! jmap-contacts-client — JMAP Contacts method implementations.
//!
//! Depends on jmap-base-client for transport, auth, and session.
//! See PLAN.md for the full implementation plan.

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
pub trait JmapContactsExt {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Contacts method calls are made through the returned
    /// [`SessionClient`].
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
