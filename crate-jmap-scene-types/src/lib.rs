//! JMAP Scene extension types (draft-atwood-jmap-scene-00 and
//! draft-atwood-jmap-scene-wss-00).
//!
//! Normative references:
//!
//! - draft-atwood-jmap-scene-00 — the `urn:ietf:params:jmap:scene`
//!   capability, which models spatial state (regions, the objects in them
//!   and the users present) as JMAP data types. Rendering and simulation
//!   are out of band.
//! - draft-atwood-jmap-scene-wss-00 — the
//!   `urn:ietf:params:jmap:scene:websocket` capability, which pushes
//!   ephemeral Scene events over the JMAP WebSocket.
//!
//! ## Crate posture
//!
//! This is a wire-format type crate, per the workspace AGENTS.md
//! kit-vs-jig posture:
//!
//! - No async dependencies, no I/O, no method semantics.
//! - Depends on RFC 8620 types only; no dependency on the Chat or VTC
//!   type crates. Their ids are plain [`jmap_types::Id`]s.
//! - Forbids `unsafe`.
//!
//! ## Public surface
//!
//! - [`JMAP_SCENE_URI`], [`SceneCapability`] — the capability (draft
//!   §Capability).
//! - [`Position`], [`Orientation`], [`Scale`] and their identity values —
//!   the normative coordinate formats (draft §Coordinate System).
//! - [`SceneBounds`] — the embedded axis-aligned bounding box.
//! - [`SceneAsset`].
//! - [`SceneRegion`], [`GeoAnchor`], [`ViewHint`], [`AccessPolicy`].
//! - [`SceneObject`], [`PhysicsMode`].
//! - [`SceneAvatar`].
//! - [`SceneInteraction`], [`SceneObjectInteractRequest`],
//!   [`SceneObjectInteractResponse`] — the `SceneObject/interact` method
//!   (draft §SceneObject/interact).
//! - [`SceneRegionBroadcast`], [`SceneRegionBroadcastRequest`],
//!   [`SceneRegionBroadcastResponse`] — the optional `SceneRegion/broadcast`
//!   method (draft §SceneRegion/broadcast, jmap-chat-spec 93f2258).
//! - `query` — the `/query` filters, including the `withinRadius` and
//!   `withinBounds` spatial filters, and comparators of the four types.
//! - `set_error` — the `contentPolicy` `SetError` type (draft §Content
//!   Moderation for User-Created Objects).
//! - [`JMAP_SCENE_WEBSOCKET_URI`] — the WebSocket capability
//!   (scene-wss draft §Capability).
//! - [`SceneEvent`], [`SceneAvatarEvent`], [`SceneObjectEvent`],
//!   [`SceneInteractionEvent`], [`SceneRegionEvent`] — the ephemeral events
//!   (scene-wss draft §Ephemeral Event Types).
//! - [`SceneStreamMessage`], [`SceneStreamEnable`], [`SceneStreamDisable`],
//!   [`SceneEventType`], [`SCENE_EVENT_TYPES`] — the stream control frames (scene-wss draft
//!   §Scene Stream Control Messages).
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types
//! ([`SceneAvatar`], [`SceneAvatarEvent`], [`SceneInteraction`], and
//! [`SceneInteractionEvent`]). `tracing` or `log` macros that interpolate one
//! of these values with `?` (debug) or `%` (display) emit its full contents to
//! the configured sink: user ids, display names, avatar positions over time,
//! join and leave times, and interaction payloads. That data is typically
//! regulated as personal data under GDPR, CCPA, and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the user's id, display name, and position.
//! tracing::debug!(avatar = ?avatar, "avatar moved");
//!
//! // GOOD — emits only the opaque ids.
//! tracing::debug!(avatar_id = ?avatar.id, region_id = ?avatar.region_id, "avatar moved");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod asset;
mod avatar;
mod backend;
mod broadcast;
mod capability;
mod coordinates;
mod event;
mod interact;
mod object;
mod query;
mod region;
mod set_error;
mod stream;
mod wire;

pub use asset::SceneAsset;
pub use avatar::SceneAvatar;
pub use broadcast::{
    SceneRegionBroadcast, SceneRegionBroadcastRequest, SceneRegionBroadcastResponse,
};
pub use capability::{JMAP_SCENE_URI, JMAP_SCENE_WEBSOCKET_URI, SceneCapability};
pub use coordinates::{
    IDENTITY_ORIENTATION, IDENTITY_SCALE, Orientation, Position, Scale, SceneBounds,
};
pub use event::{
    AvatarEventKind, ObjectEventKind, SceneAvatarEvent, SceneEvent, SceneInteractionEvent,
    SceneObjectEvent, SceneRegionEvent,
};
pub use interact::{SceneInteraction, SceneObjectInteractRequest, SceneObjectInteractResponse};
pub use object::{PhysicsMode, SceneObject};
pub use query::{
    Filter, FilterOperator, Operator, SceneAssetComparator, SceneAssetFilter,
    SceneAssetFilterCondition, SceneAssetSortProperty, SceneAvatarComparator, SceneAvatarFilter,
    SceneAvatarFilterCondition, SceneAvatarSortProperty, SceneObjectComparator, SceneObjectFilter,
    SceneObjectFilterCondition, SceneObjectSortProperty, SceneRegionComparator, SceneRegionFilter,
    SceneRegionFilterCondition, SceneRegionSortProperty, WithinBounds, WithinRadius,
};
pub use region::{AccessPolicy, GeoAnchor, SceneRegion, ViewHint};
pub use set_error::CONTENT_POLICY;
pub use stream::{
    SCENE_EVENT_TYPES, SceneEventType, SceneStreamDisable, SceneStreamEnable, SceneStreamMessage,
};
