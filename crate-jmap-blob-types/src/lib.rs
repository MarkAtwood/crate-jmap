//! JMAP Blob Management data types: draft-ietf-jmap-blobext-03
//! (`urn:ietf:params:jmap:blob2`) and RFC 9404 (`urn:ietf:params:jmap:blob`).
//!
//! Provides the capability objects, [`DigestAlgorithm`] with built-in
//! digesters, [`BlobRef`] (a blobId or `#creationId`), [`DataSourceObject`],
//! and the arguments and responses of `Blob/set`, `Blob/get`,
//! `Blob/lookup` and `Blob/convert`, plus RFC 9404 `Blob/upload` and RFC
//! 8620 §6.3 `Blob/copy`.
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` (shared RFC 8620 wire primitives) and
//! `jmap-blob-server` / `jmap-blob-client`. The JMAP Data Types registry
//! that `Blob/lookup` reads is `jmap_types::data_types`.
//!
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types
//! ([`BlobGetItem`], [`DataSourceObject`], [`ImageData`], and [`ImageGps`]).
//! `tracing` or `log` macros that interpolate one of these values with `?`
//! (debug) or `%` (display) emit its full contents to the configured sink: blob
//! contents read as text, and image metadata such as GPS coordinates, capture
//! time, and embedded comments. That data is typically regulated as personal
//! data under GDPR, CCPA, and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the blob text and where the photo was taken.
//! tracing::debug!(item = ?item, "Blob/get item");
//!
//! // GOOD — emits only the opaque id.
//! tracing::debug!(blob_id = ?item.id, "Blob/get item");
//! ```
//!
//! Plain `Debug` plus this section is the workspace policy for PII-bearing
//! types (decision 53, bd:JMAP-5rmi). Redacting `Debug` impls and log-capture
//! canaries are reserved for credential-grade secrets, such as `BearerAuth` and
//! `BasicAuth` in `jmap-base-client`.
//!
//! # Example
//!
//! ```rust
//! use jmap_blob_types::{BlobSetArgs, DigestAlgorithm};
//!
//! // blobext-03 §9.1, with a digest of the whole blob.
//! let args: BlobSetArgs = serde_json::from_str(r#"{
//!     "accountId": "abc",
//!     "create": {
//!         "b1": {
//!             "data": [{"data:asText": "Hello, world!"}],
//!             "type": "text/plain",
//!             "digest:sha-256": "MV9b23bQeMQ7isAGTkoBZGErH853yGk0W/yUx1iU7dM="
//!         }
//!     }
//! }"#).unwrap();
//!
//! let b1 = &args.create.unwrap()["b1"];
//! assert_eq!(b1.data[0].data_as_text.as_deref(), Some("Hello, world!"));
//!
//! let mut d = DigestAlgorithm::Sha256.builtin_digester().unwrap();
//! d.update(b"Hello, world!");
//! assert_eq!(d.finish(), b1.digests[&DigestAlgorithm::Sha256]);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod blob_ref;
mod capability;
mod convert;
mod copy;
mod digest;
pub mod errors;
mod get;
mod lookup;
mod rfc9404;
mod serde_util;
mod set;
mod source;

pub use blob_ref::BlobRef;
pub use capability::{
    BlobAccountCapability, BlobCapability, CapabilityError, JMAP_BLOB_URI, JMAP_BLOB2_URI,
    MIN_MAX_DATA_SOURCES, Rfc9404AccountCapability,
};
pub use convert::{
    ArchiveEntry, ArchiveRecipe, BlobConvertArgs, BlobConvertResponse, ColorSpace, CompressRecipe,
    CompressionMethod, ConvertRequest, ConvertedBlob, DecompressRecipe, DeltaRecipe, EntryType,
    ExtractRecipe, ImageConvertRecipe, PatchRecipe, Recipe, compress_level_range,
};
pub use copy::{BlobCopyArgs, BlobCopyResponse};
pub use digest::{Digest, DigestAlgorithm};
pub use get::{
    BlobGetArgs, BlobGetItem, BlobGetResponse, BlobProperty, DataSourceProperty, ImageData,
    ImageGps,
};
pub use lookup::{BlobInfo, BlobLookupArgs, BlobLookupResponse};
pub use rfc9404::{BlobUploadArgs, BlobUploadCallResponse, Rfc9404LookupResponse, UploadObject};
pub use set::{BlobCreateObject, BlobObject, BlobSetArgs, BlobSetResponse, BlobUpdated};
pub use source::DataSourceObject;
