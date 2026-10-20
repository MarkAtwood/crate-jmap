//! Server rules of draft-atwood-jmap-chat-federation-00, server-to-server
//! delivery between JMAP Chat mailboxes.
//!
//! ## Peer/deliver
//!
//! [`handle_peer_deliver`] is the `Peer/deliver` handler: §New Message
//! Validation steps 1 to 14, §Edit and Reaction Validation, and the
//! direct-chat race of §Direct Chat Simultaneous Initiation. It runs over
//! a [`FederationBackend`], a `jmap_chat_server::ChatBackend` that also
//! names the authenticated peer ([`AuthenticatedPeer`]), the `@everyone`
//! permission floor, attachment fetching ([`FetchedBlob`]), and direct
//! Chats stored under a peer-chosen id.
//!
//! ## Peer/receipt, Peer/typing, Peer/retract, Peer/groupUpdate
//!
//! [`handle_peer_receipt`] records a recipient's delivery and read
//! receipt on the owner's Message, subject to the owner's
//! `receiptSharing`. [`handle_peer_typing`] checks membership, blocking
//! and `receiveTypingIndicators`, then hands the typing event to
//! [`FederationBackend::forward_typing`]. [`handle_peer_retract`]
//! tombstones a Message for its original sender. [`handle_peer_group_update`]
//! creates a group Chat or applies a membership, role or metadata change,
//! with the draft's admin checks.
//!
//! ## Peer/subscribePresence, Peer/unsubscribePresence, Peer/presence
//!
//! The optional presence methods run over a [`FederationPresenceBackend`],
//! which keeps the subscriptions and pushes to the owner's clients.
//! [`handle_peer_subscribe_presence`] records a subscription and answers
//! with the owner's presence, never `"invisible"`;
//! [`handle_peer_unsubscribe_presence`] cancels one. [`handle_peer_presence`]
//! updates the sender's ChatContact `presence`, `lastActiveAt`,
//! `statusText` and `statusEmoji`, drops the update silently for a blocked
//! contact, and fires the local presence event. A deployment that does not
//! offer them leaves them unregistered, so callers get `unknownMethod`.
//!
//! Every handler checks the caller-identity field against the
//! authenticated peer first, and rejects before its first write.
//!
//! ## Sending
//!
//! A backend's `ChatBackend::on_outbound_event` turns each
//! `jmap_chat_server::OutboundEvent` into `Peer/*` calls. For a new
//! Message, [`outbound_reference`] gives the `senderMsgId` to send for its
//! `replyTo` and `threadRootId` (§Outbound Message Preparation). For a
//! retraction, `Peer/retract` goes to each of [`other_participants`]
//! (§Peer/retract "Sender Behavior"). For a group change,
//! [`peer_group_update_args`] builds the `Peer/groupUpdate` arguments,
//! mapping local ids to identity strings through [`SenderIdentities`].
//! [`is_transient`] classifies a failed call for retry ([`DeliveryFailure`],
//! §Retry and Backoff), and [`aggregate_delivery_state`] combines the
//! recipients' states into the Message's `deliveryState` (§Delivery State
//! Transitions).
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

mod deliver;
mod group_update;
mod outbound;
mod presence;
mod receipt;
mod retract;
mod typing;

pub use deliver::{handle_peer_deliver, AuthenticatedPeer, FederationBackend, FetchedBlob};
pub use group_update::handle_peer_group_update;
pub use outbound::{
    aggregate_delivery_state, is_transient, other_participants, outbound_reference,
    peer_group_update_args, DeliveryFailure, OutboundError, SenderIdentities,
};
pub use presence::{
    handle_peer_presence, handle_peer_subscribe_presence, handle_peer_unsubscribe_presence,
    FederationPresenceBackend,
};
pub use receipt::handle_peer_receipt;
pub use retract::handle_peer_retract;
pub use typing::handle_peer_typing;

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
