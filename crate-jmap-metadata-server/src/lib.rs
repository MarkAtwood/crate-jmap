//! draft-ietf-jmap-metadata-04 server-side helpers.
//!
//! The -02 revision eliminates the standalone `Metadata` object type.
//! Metadata becomes `metadata` and `privateMetadata` properties on
//! each listed JMAP data type. Clients read and write metadata
//! through the existing `Foo/get`, `Foo/set`, `Foo/changes`, and
//! `Foo/query` methods — there are no separate `Metadata/*` methods.
//!
//! This crate provides **pure helper functions** that extension-server
//! backends call to implement the metadata augmentation:
//!
//! - Depth validation ([`value_depth`],
//!   [`exceeds_max_depth`]) and namespace support checking
//!   ([`is_namespace_supported`]).
//! - Property-name classification for the
//!   `ignoreMetadataOnlyChanges` request arg and the
//!   `updatedProperties` response field ([`is_metadata_property`],
//!   [`is_metadata_only_change`]).
//! - Filter predicate helpers for the six
//!   `metadata{Exists,TextContains,TextEquals}` /
//!   `privateMetadata{Exists,TextContains,TextEquals}` filter
//!   conditions ([`metadata_path_exists`], [`metadata_text_contains`],
//!   [`metadata_text_equals`]) and the path check that backs the
//!   `invalidArguments` rejection ([`is_well_formed_metadata_path`]).
//!
//! All functions are stateless and operate on `serde_json` values and
//! [`jmap_metadata_types`] structs. No backend trait, no dispatcher
//! registration, no `memory` feature.
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
    METADATA_PROPERTY, PRIVATE_METADATA_PROPERTY, is_metadata_only_change, is_metadata_property,
};
pub use filter::{
    is_well_formed_metadata_path, metadata_path_exists, metadata_text_contains,
    metadata_text_equals,
};
pub use validate::{exceeds_max_depth, is_namespace_supported, value_depth};

/// Capability URI for `urn:ietf:params:jmap:metadata`.
pub use jmap_metadata_types::JMAP_METADATA_URI;
