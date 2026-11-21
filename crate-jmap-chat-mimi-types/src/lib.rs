//! JMAP Chat MIMI provider extension types
//! (draft-atwood-jmap-chat-mimi-00).
//!
//! Normative reference: draft-atwood-jmap-chat-mimi-00 — the
//! `urn:ietf:params:jmap:chat:mimi` capability, with which an MLS
//! (RFC 9420) client uses a JMAP Chat server as its MIMI provider. The
//! server routes MLS messages between the client and the MIMI hub and
//! never decrypts them.
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies, no I/O.
//! - No MLS or TLS-codec dependency: byte-string properties stay as
//!   validated [`Base64Url`] strings and are never decoded or re-encoded
//!   here.
//! - Forbids `unsafe`.
//!
//! ## Public surface
//!
//! - [`JMAP_CHAT_MIMI_URI`] — the capability URI (draft §Capability).
//! - [`MimiCapability`] — the account-level capability object.
//! - [`MIMI_FANOUT_BODY_TYPE`], [`MLS_MESSAGE_BODY_TYPE`] — the
//!   `bodyType` values of draft §MLS Messages.
//! - [`KeyPackage`], [`KeyPackageCreate`], [`KeyPackageCreated`],
//!   [`KeyPackageProperty`] — the [`KeyPackage`] object and the `KeyPackage/get`
//!   and `KeyPackage/set` argument shapes (draft §`KeyPackages`).
//! - [`KeyMaterialFetchRequest`], [`KeyMaterialFetchResponse`],
//!   [`ClientKeyMaterial`], [`KeyMaterialUserCode`], [`KeyMaterialClientCode`]
//!   — the `KeyMaterial/fetch` arguments (draft §KeyMaterial/fetch).
//! - [`chat_room_id`] — the `Chat.roomId` extension field, read from
//!   `Chat.extra` (draft §Chat.roomId).
//! - [`RoomCreateRequest`], [`RoomCreateResponse`], [`RoomUpdateRequest`],
//!   [`RoomUpdateResponse`], [`UpdateResponseCode`], [`RoomSendRequest`],
//!   [`RoomSendResponse`], [`SubmitResponseCode`], [`RoomUnbindRequest`],
//!   [`RoomUnbindResponse`] — the `Room/create`, `Room/update`, `Room/send`
//!   and `Room/unbind` arguments (draft §Rooms).
//! - [`GroupInfoFetchRequest`], [`GroupInfoFetchResponse`], [`GroupInfoCode`]
//!   — the `GroupInfo/fetch` arguments (draft §GroupInfo/fetch).
//! - [`Base64Url`] / [`Base64UrlError`] — the unpadded base64url wire
//!   shape of every byte-string property (draft §Conventions).
//! - `convert` — the converting-party rules: [`sender_msg_id`],
//!   [`RefKey`], [`check_ref_key`] / [`RefKeyCollision`],
//!   [`may_record_alias`], [`topic_id`], [`is_media_reaction`] and
//!   [`never_sets_read`] (draft §Message Identifiers through §Message
//!   Status, §Security Considerations).
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types
//! ([`MimiCapability`], [`ClientKeyMaterial`], and
//! [`KeyMaterialFetchResponse`]). `tracing` or `log` macros that interpolate
//! one of these values with `?` (debug) or `%` (display) emit its full contents
//! to the configured sink: MIMI user and client URIs, which identify people
//! and are typically regulated as personal data under GDPR, CCPA, and similar
//! privacy frameworks. (Key packages and MLS messages are opaque base64url
//! here.)
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the user URI.
//! tracing::debug!(resp = ?response, "fetched key material");
//!
//! // GOOD — emits only the status code.
//! tracing::debug!(status = ?response.user_status, "fetched key material");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod base64url;
mod capability;
mod convert;
mod groupinfo;
mod keymaterial;
mod keypackage;
mod room;

pub use base64url::{Base64Url, Base64UrlError};
pub use capability::{
    JMAP_CHAT_MIMI_URI, MIMI_FANOUT_BODY_TYPE, MLS_MESSAGE_BODY_TYPE, MimiCapability,
};
pub use convert::{
    MimiMessageId, RefKey, RefKeyCollision, check_ref_key, is_media_reaction, may_record_alias,
    never_sets_read, sender_msg_id, topic_id, ulid,
};
pub use groupinfo::{GroupInfoCode, GroupInfoFetchRequest, GroupInfoFetchResponse};
pub use keymaterial::{
    ClientKeyMaterial, KeyMaterialClientCode, KeyMaterialFetchRequest, KeyMaterialFetchResponse,
    KeyMaterialUserCode,
};
pub use keypackage::{KeyPackage, KeyPackageCreate, KeyPackageCreated, KeyPackageProperty};
pub use room::{
    RoomCreateRequest, RoomCreateResponse, RoomSendRequest, RoomSendResponse, RoomUnbindRequest,
    RoomUnbindResponse, RoomUpdateRequest, RoomUpdateResponse, SubmitResponseCode,
    UpdateResponseCode, chat_room_id,
};
