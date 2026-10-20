//! JMAP Scene extension types (draft-atwood-jmap-scene-00).
//!
//! Normative reference: draft-atwood-jmap-scene-00 — the
//! `urn:ietf:params:jmap:scene` capability, which models spatial state
//! (regions, the objects in them and the users present) as JMAP data
//! types. Rendering and simulation are out of band.
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
//! - [`query`] — the `/query` filters, including the `withinRadius` and
//!   `withinBounds` spatial filters, and comparators of the four types.
//! - [`set_error`] — the `contentPolicy` SetError type and the
//!   `retryAfter` property of `rateLimit` (draft §Content Moderation for
//!   User-Created Objects).

#![forbid(unsafe_code)]

pub mod asset;
pub mod avatar;
mod backend;
pub mod capability;
pub mod coordinates;
pub mod object;
pub mod query;
pub mod region;
pub mod set_error;
mod wire;

pub use asset::SceneAsset;
pub use avatar::SceneAvatar;
pub use capability::{SceneCapability, JMAP_SCENE_URI};
pub use coordinates::{
    Orientation, Position, Scale, SceneBounds, IDENTITY_ORIENTATION, IDENTITY_SCALE,
};
pub use object::{PhysicsMode, SceneObject};
pub use region::{AccessPolicy, GeoAnchor, SceneRegion, ViewHint};
pub use set_error::{retry_after, RetryAfterError, CONTENT_POLICY, RETRY_AFTER};
