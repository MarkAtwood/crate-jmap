// jmap-sharing-client — JMAP Sharing method implementations (RFC 9670).
// Depends on jmap-base-client for transport, auth, and session.
// See PLAN.md for the full implementation plan.

#![forbid(unsafe_code)]

pub mod methods;

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
