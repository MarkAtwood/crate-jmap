//! Server rules of draft-atwood-jmap-chat-federation-00, server-to-server
//! delivery between JMAP Chat mailboxes.
//!
//! ## Peer-role access control
//!
//! The `urn:ietf:params:jmap:chat` account capability carries a `role` of
//! `"owner"` or `"peer"` (§Account-Level Capability). A deployment's
//! dispatcher `CallerCtx` implements [`FederationCaller`] to say whether
//! the caller is a peer server operating against a `role: "peer"`
//! account. [`check_method_access`] then applies both directions of the
//! rule:
//!
//! - A peer caller gets `forbidden` for every method not in
//!   [`PEER_METHODS`] (§Account-Level Capability).
//! - Any other caller gets `forbidden` for every method in
//!   [`PEER_METHODS`] (§Server-to-Server Methods).
//!
//! Call it before the method's handler runs, with the method name the
//! dispatcher is about to invoke:
//!
//! ```
//! use jmap_chat_federation_server::{check_method_access, FederationCaller};
//!
//! struct Caller {
//!     peer: bool,
//! }
//!
//! impl FederationCaller for Caller {
//!     fn is_peer(&self) -> bool {
//!         self.peer
//!     }
//! }
//!
//! let peer = Caller { peer: true };
//! assert!(check_method_access(&peer, "Peer/deliver").is_ok());
//! assert_eq!(
//!     check_method_access(&peer, "Chat/get").unwrap_err().error_type,
//!     "forbidden"
//! );
//! ```

#![forbid(unsafe_code)]

use jmap_types::JmapError;

/// The methods of §Server-to-Server Methods: the five required and the
/// three optional `Peer/*` methods.
pub const PEER_METHODS: [&str; 8] = [
    "Peer/deliver",
    "Peer/receipt",
    "Peer/typing",
    "Peer/retract",
    "Peer/groupUpdate",
    "Peer/subscribePresence",
    "Peer/unsubscribePresence",
    "Peer/presence",
];

/// Implemented by a dispatcher `CallerCtx` that can tell a peer server
/// from the mailbox owner.
pub trait FederationCaller {
    /// `true` when the caller is authenticated as a peer server and
    /// operates against an account whose `urn:ietf:params:jmap:chat`
    /// capability has `role: "peer"` (§Account-Level Capability).
    ///
    /// `false` for every other caller, including one whose account has a
    /// role value this crate does not know.
    fn is_peer(&self) -> bool;
}

/// Check that `caller` may invoke `method`.
///
/// # Errors
///
/// Returns `forbidden` when a peer caller invokes a method not in
/// [`PEER_METHODS`], or a caller that is not a peer invokes one that is.
pub fn check_method_access<C: FederationCaller + ?Sized>(
    caller: &C,
    method: &str,
) -> Result<(), JmapError> {
    if caller.is_peer() == PEER_METHODS.contains(&method) {
        Ok(())
    } else {
        Err(JmapError::forbidden())
    }
}
