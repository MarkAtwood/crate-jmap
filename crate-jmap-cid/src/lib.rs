//! JMAP Blob Content Identifiers
//! ([draft-atwood-jmap-cid-00](https://github.com/MarkAtwood/jmap-chat-spec)):
//! umbrella crate re-exporting the component crate under the family name.
//!
//! - [`types`]: the [`jmap-cid-types`](https://crates.io/crates/jmap-cid-types),
//!   always enabled.
//!
//! The CID family has no `-client` or `-server` crate, so this crate has
//! no `client` or `server` feature.
//!
//! The component crate versions in lockstep with this one.
//!
//! ```
//! use jmap_cid::types::{JMAP_CID_URI, Sha256};
//!
//! assert_eq!(JMAP_CID_URI, "urn:ietf:params:jmap:cid");
//! let digest: Sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
//!     .parse()
//!     .unwrap();
//! assert_eq!(digest.to_cidv1(), "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku");
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub use jmap_cid_types as types;
