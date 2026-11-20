//! JMAP Object History client bindings over `jmap-base-client`
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes,
//! docs/design/object-history.md §2a, §8).
//!
//! [`JmapObjectHistoryExt::with_object_history_session`] gives a
//! [`SessionClient`] whose calls work for any data type with history:
//! [`get_history`](SessionClient::get_history) (`Foo/get` with the §3
//! arguments), [`query_destroyed`](SessionClient::query_destroyed) and
//! [`query_destroyed_changes`](SessionClient::query_destroyed_changes)
//! (`Foo/query` and `Foo/queryChanges` with `onlyDestroyed`, §4), and
//! [`restore`](SessionClient::restore) (`Foo/set` with `restore`, §5.1, on
//! the base client's generic `SetCall`). Every call adds
//! `urn:ietf:params:jmap:object-history` to `using`. The wire types are
//! those of `jmap-object-history-types`; this crate does not re-export
//! them.
//!
//! Two cautions from the draft for applications: history is best effort
//! (§3.1): do not rely on `expires`, `hasOlder` or version numbers across
//! requests, since the server "may purge history at any time"; and do not
//! present a destroy as permanent (§8.4) when the server keeps history.
//!
//! # Example
//!
//! ```no_run
//! # async fn demo(client: jmap_base_client::JmapClient, session: jmap_base_client::Session)
//! # -> Result<(), jmap_base_client::ClientError> {
//! use jmap_object_history_client::{HistoryGetCall, JmapObjectHistoryExt};
//! use jmap_object_history_types::HistoryGetArguments;
//! use jmap_types::Id;
//! use serde_json::{Map, Value};
//!
//! let history = client.with_object_history_session(session);
//! let mut args = HistoryGetArguments::new();
//! args.include_replaced = true;
//! let call = HistoryGetCall::new(Id::from("abc"), "urn:ietf:params:jmap:contacts", args)
//!     .ids(Some(vec![Id::from("contact1")]));
//! let resp = history.get_history::<Map<String, Value>>("ContactCard", &call).await?;
//! for entry in &resp.list {
//!     println!("version {}", entry.object_history.version);
//! }
//! # Ok(()) }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod calls;

pub use calls::{
    DestroyedQueryCall, DestroyedQueryChangesCall, HistoryGetCall, SessionClient, has_older,
    older_page_before,
};

/// Extension trait adding draft-ietf-jmap-object-history calls to
/// [`jmap_base_client::JmapClient`].
///
/// This trait is **sealed**: the crate implements it only for
/// [`jmap_base_client::JmapClient`], so adding methods stays non-breaking.
pub trait JmapObjectHistoryExt: sealed::Sealed {
    /// A [`SessionClient`] bound to this client and session.
    fn with_object_history_session(&self, session: jmap_base_client::Session) -> SessionClient;
}

impl JmapObjectHistoryExt for jmap_base_client::JmapClient {
    fn with_object_history_session(&self, session: jmap_base_client::Session) -> SessionClient {
        SessionClient::new(self.clone(), session)
    }
}

mod sealed {
    /// Sealing trait for [`super::JmapObjectHistoryExt`].
    pub trait Sealed {}
    impl Sealed for ::jmap_base_client::JmapClient {}
}
