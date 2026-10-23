//! draft-ietf-jmap-metadata-04 server-side helpers.
//!
//! Since -02, the draft has no standalone `Metadata` object type.
//! Metadata becomes `metadata` and `privateMetadata` properties on
//! each listed JMAP data type. Clients read and write metadata
//! through the existing `Foo/get`, `Foo/set`, `Foo/changes`, and
//! `Foo/query` methods — there are no separate `Metadata/*` methods.
//!
//! This crate provides helpers that extension-server backends call to
//! implement the metadata augmentation:
//!
//! - Depth validation ([`value_depth`],
//!   [`exceeds_max_depth`]) and namespace support checking
//!   ([`is_namespace_supported`]).
//! - `/set` validation of the §3.2 rules 2 to 4 ([`check_metadata_write`]
//!   before applying a write, [`check_metadata_depth`] after).
//! - Property-name classification for the
//!   `ignoreMetadataOnlyChanges` request arg and the
//!   `updatedProperties` response field ([`is_metadata_property`],
//!   [`is_metadata_only_change`]), plus a per-id change tracker
//!   ([`changed_properties`], [`ChangedProperties`],
//!   [`MetadataOnlyChanges`]).
//! - Filter predicate helpers for the six
//!   `metadata{Exists,TextContains,TextEquals}` /
//!   `privateMetadata{Exists,TextContains,TextEquals}` filter
//!   conditions ([`metadata_path_exists`], [`metadata_text_contains`],
//!   [`metadata_text_equals`]), the path check that backs the
//!   `invalidArguments` rejection ([`is_well_formed_metadata_path`]), and
//!   [`MetadataCondition`], which takes the six fields off a decoded
//!   `FilterCondition` and evaluates them.
//!
//! Everything operates on `serde_json` values and
//! [`jmap_metadata_types`] structs, so a backend that stores objects as
//! JSON gets a working metadata baseline from a few calls. No backend
//! trait, no dispatcher registration, no `memory` feature.
//!
//! # Migration from -01
//!
//! The -01 crate exposed `MetadataBackend`, `register_metadata_handlers`,
//! `MemoryBackend`, and five `handle_metadata_*` method handlers. All of
//! those are removed in -02. Extension-server crates that previously
//! depended on this crate for a standalone `Metadata/*` handler set
//! should instead integrate the helper functions into their own
//! `Foo/get`, `Foo/set`, `Foo/changes`, and `Foo/query` handlers.

#![forbid(unsafe_code)]

mod changes;
mod filter;
mod validate;

pub use changes::{
    ChangedProperties, METADATA_PROPERTY, MetadataOnlyChanges, PRIVATE_METADATA_PROPERTY,
    changed_properties, is_metadata_only_change, is_metadata_property,
};
pub use filter::{
    MetadataCondition, MetadataFilterError, is_well_formed_metadata_path, metadata_path_exists,
    metadata_text_contains, metadata_text_equals,
};
pub use validate::{
    InvalidMetadataWrite, SetKind, check_metadata_depth, check_metadata_write, exceeds_max_depth,
    is_namespace_supported, value_depth,
};

/// Capability URI for `urn:ietf:params:jmap:metadata`.
pub use jmap_metadata_types::JMAP_METADATA_URI;
