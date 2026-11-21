//! JMAP Chat federation extension types
//! (draft-atwood-jmap-chat-federation-00).
//!
//! Normative reference: draft-atwood-jmap-chat-federation-00 — server-to-
//! server delivery between JMAP Chat mailboxes. It extends the
//! `urn:ietf:params:jmap:chat` capability and defines no capability URI of
//! its own.
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies, no I/O.
//! - The `jmap-chat-*` types stay unchanged: federation fields on chat
//!   objects are read from their `extra` maps.
//! - Forbids `unsafe`.
//!
//! ## Public surface
//!
//! - [`contact_server_url`], [`contact_direct_address`] — the
//!   `ChatContact.serverUrl` and `ChatContact.directAddress` extension
//!   fields, read from `ChatContact.extra` (draft §`ChatContact` Type
//!   Extensions for Federation).
//! - `peer` — request and response arguments of the eight `Peer/*`
//!   methods (draft §Server-to-Server Methods). User references are
//!   identity strings (draft §User References in Peer Methods).
//! - [`is_jmap_object_uri`] — whether a `MessageAction` or Endpoint `uri`
//!   is one a receiving server removes from peer data (same section).
//! - [`FederationSession`], [`anonymous_well_known_session`] — the
//!   `ownerUserId`, `peerSigningKeys` and `ownerDirectAddress` Session
//!   fields, and the anonymous `/.well-known/jmap` Session object (draft
//!   §Session Object Extensions for Federation, §Key Publication).
//! - [`owner_endpoints`] — the `endpoints` of a `ChatContact`, from the
//!   `ownerEndpoints` its server's Session publishes
//!   (draft-atwood-jmap-chat-00 §`ChatContact`).
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types
//! ([`PeerMessage`], [`PeerEdit`], [`PeerMention`], [`PeerAttachment`],
//! [`PeerAvatar`], [`PeerChatMember`], [`PeerDeliverArgs`],
//! [`PeerReceiptArgs`], [`PeerTypingArgs`], [`PeerPresenceArgs`], and
//! [`FederationSession`]). `tracing` or `log` macros that interpolate one of
//! these values with `?` (debug) or `%` (display) emit its full contents to the
//! configured sink: message bodies, sender and recipient user ids, direct
//! addresses, read receipts, and typing and presence signals. That data is
//! typically regulated as personal data under GDPR, CCPA, and similar privacy
//! frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the message body.
//! tracing::debug!(msg = ?peer_message, "peer deliver");
//!
//! // GOOD — emits only the opaque message id.
//! tracing::debug!(msg_id = ?peer_message.sender_msg_id, "peer deliver");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod contact;
mod peer;
mod session;

pub use contact::{contact_direct_address, contact_server_url};
pub use peer::{
    GroupUpdateAction, PeerAck, PeerAttachment, PeerAvatar, PeerChatMember, PeerDeliverArgs,
    PeerDeliverResponse, PeerEdit, PeerGroupUpdateArgs, PeerMention, PeerMessage, PeerPresenceArgs,
    PeerReactionUpdate, PeerReceiptArgs, PeerRetractArgs, PeerRetractResponse, PeerRoleUpdate,
    PeerSubscribePresenceArgs, PeerSubscribePresenceResponse, PeerTypingArgs,
    PeerUnsubscribePresenceArgs, ReactionAction, is_jmap_object_uri,
};
pub use session::{
    AnonymousSessionError, FederationSession, anonymous_well_known_session, owner_endpoints,
};
