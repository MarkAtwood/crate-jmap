//! JMAP VTC extension client methods (draft-atwood-jmap-vtc-00).
//!
//! Binds every method of draft-atwood-jmap-vtc-00 §Methods — the
//! `/get`, `/changes`, `/set`, `/query` and `/queryChanges` methods of
//! `VTCCall`, `VTCParticipant`, `VTCRecording` and `VTCLivestream` — on top
//! of `jmap-base-client`. Wire types come from `jmap-vtc-types`.
//!
//! # Usage
//!
//! ```rust,no_run
//! # use jmap_vtc_client::JmapVtcExt;
//! # async fn example(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
//! let session = client.fetch_session().await?;
//! let sc = client.with_vtc_session(session);
//! let calls = sc.vtc_call_get(None, None).await?;
//! # Ok(())
//! # }
//! ```
//!
//! [`VtcWsExt`] adds draft-atwood-jmap-vtc-wss-00 ephemeral VTC events to
//! a `jmap_base_client::WsSession`: send `VTCStreamEnable` and
//! `VTCStreamDisable` frames, and receive [`VtcWsFrame`]s, with every
//! non-VTC frame passed through unchanged.

#![forbid(unsafe_code)]

mod methods;
mod ws;

pub use jmap_base_client::ClientError;
pub use methods::{
    AddedItem, ChangesResponse, GetResponse, PartialObject, QueryChangesResponse, QueryResponse,
    SessionClient, SetError, SetResponse,
};
pub use ws::{parse_vtc_ws_frame, VtcWsExt, VtcWsFrame};

// ---------------------------------------------------------------------------
// JmapVtcExt — the extension trait
// ---------------------------------------------------------------------------

/// Extension trait adding JMAP VTC methods to [`jmap_base_client::JmapClient`].
///
/// Import this trait to use: `use jmap_vtc_client::JmapVtcExt;`
/// This trait is **sealed**: the crate adds an `impl` only for
/// [`jmap_base_client::JmapClient`], which keeps adding methods to the
/// trait a non-breaking change.
pub trait JmapVtcExt: sealed::Sealed {
    /// Bind this client to a JMAP session for use with VTC methods.
    ///
    /// The returned [`SessionClient`] captures the session at construction
    /// time. After re-fetching the session, construct a new `SessionClient`.
    ///
    /// # Deferred session-capability validation
    ///
    /// This constructor accepts any [`jmap_base_client::Session`] and never
    /// fails. Each method call on the returned [`SessionClient`] returns
    /// [`ClientError::InvalidSession`] when the session has no primary
    /// account for `urn:ietf:params:jmap:vtc`. Pre-check with
    /// [`session.primary_account_id("urn:ietf:params:jmap:vtc")`](jmap_base_client::Session::primary_account_id)
    /// to guard at the binding site.
    fn with_vtc_session(&self, session: jmap_base_client::Session) -> methods::SessionClient;
}

impl JmapVtcExt for jmap_base_client::JmapClient {
    fn with_vtc_session(&self, session: jmap_base_client::Session) -> methods::SessionClient {
        methods::SessionClient {
            client: self.clone(),
            session,
            account_id: None,
        }
    }
}

mod sealed {
    /// Sealing-trait for [`super::JmapVtcExt`] — see the trait's rustdoc.
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
