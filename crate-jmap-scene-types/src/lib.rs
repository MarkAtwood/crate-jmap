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
//! - `query` — the `/query` filters, including the `withinRadius` and
//!   `withinBounds` spatial filters, and comparators of the four types.
//! - `set_error` — the `contentPolicy` SetError type (draft §Content
//!   Moderation for User-Created Objects).
//! - [`JMAP_SCENE_WEBSOCKET_URI`] — the WebSocket capability
//!   (scene-wss draft §Capability).
//! - [`SceneEvent`], [`SceneAvatarEvent`], [`SceneObjectEvent`],
//!   [`SceneInteractionEvent`] — the ephemeral events (scene-wss draft
//!   §Ephemeral Event Types).
//! - [`SceneStreamMessage`], [`SceneStreamEnable`], [`SceneStreamDisable`],
//!   [`SceneEventType`], [`SCENE_EVENT_TYPES`] — the stream control frames (scene-wss draft
//!   §Scene Stream Control Messages).

#![forbid(unsafe_code)]

mod asset;
mod avatar;
mod backend;
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
pub use capability::{SceneCapability, JMAP_SCENE_URI, JMAP_SCENE_WEBSOCKET_URI};
pub use coordinates::{
    Orientation, Position, Scale, SceneBounds, IDENTITY_ORIENTATION, IDENTITY_SCALE,
};
pub use event::{
    AvatarEventKind, ObjectEventKind, SceneAvatarEvent, SceneEvent, SceneInteractionEvent,
    SceneObjectEvent,
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
    SceneEventType, SceneStreamDisable, SceneStreamEnable, SceneStreamMessage, SCENE_EVENT_TYPES,
};
