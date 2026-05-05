// jmap-mail-client — RFC 8621 JMAP for Mail method implementations.
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

#![forbid(unsafe_code)]

pub mod methods;

pub use methods::{
    AddedItem, ChangesResponse, EmailCopyParams, EmailGetParams, GetResponse, MailboxSetParams,
    QueryChangesResponse, QueryResponse, SessionClient, SetError, SetResponse,
};

/// Extension trait adding RFC 8621 (JMAP for Mail) methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_mail_client::JmapMailExt;`
///
/// All JMAP Mail method calls are made through the [`SessionClient`] returned
/// by [`with_mail_session`](JmapMailExt::with_mail_session).
pub trait JmapMailExt {
    /// Create a [`SessionClient`] bound to this client and session.
    ///
    /// All JMAP Mail method calls are made through the returned [`SessionClient`].
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
