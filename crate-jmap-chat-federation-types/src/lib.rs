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
//!   fields, read from `ChatContact.extra` (draft §ChatContact Type
//!   Extensions for Federation).
//! - [`peer`] — request and response arguments of the eight `Peer/*`
//!   methods (draft §Server-to-Server Methods). User references are
//!   identity strings (draft §User References in Peer Methods).
//! - [`FederationSession`], [`anonymous_well_known_session`] — the
//!   `ownerUserId`, `peerSigningKeys` and `ownerDirectAddress` Session
//!   fields, and the anonymous `/.well-known/jmap` Session object (draft
//!   §Session Object Extensions for Federation, §Key Publication).

#![forbid(unsafe_code)]

pub mod contact;
pub mod peer;
pub mod session;

pub use contact::{contact_direct_address, contact_server_url};
pub use peer::{
    GroupUpdateAction, PeerAck, PeerAttachment, PeerChatMember, PeerDeliverArgs,
    PeerDeliverResponse, PeerEdit, PeerGroupUpdateArgs, PeerMention, PeerMessage, PeerPresenceArgs,
    PeerReactionUpdate, PeerReceiptArgs, PeerRetractArgs, PeerRetractResponse, PeerRoleUpdate,
    PeerSubscribePresenceArgs, PeerSubscribePresenceResponse, PeerTypingArgs,
    PeerUnsubscribePresenceArgs, ReactionAction,
};
pub use session::{anonymous_well_known_session, AnonymousSessionError, FederationSession};
