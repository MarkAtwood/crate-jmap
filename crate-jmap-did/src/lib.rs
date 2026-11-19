//! JMAP Decentralized Identifiers
//! ([draft-atwood-jmap-did-00](https://github.com/MarkAtwood/jmap-chat-spec)):
//! umbrella crate re-exporting the component crate under the family name.
//!
//! - [`types`]: the [`jmap-did-types`](https://crates.io/crates/jmap-did-types),
//!   always enabled.
//!
//! The DID family has no `-client` or `-server` crate, so this crate has
//! no `client` or `server` feature. DID resolution, which does network
//! I/O, is the separate support crate
//! [`jmap-did-resolver`](https://crates.io/crates/jmap-did-resolver),
//! which this crate does not re-export.
//!
//! The component crate versions in lockstep with this one.
//!
//! ```
//! use jmap_did::types::{DidAccountCapability, JMAP_DID_URI, KeyCustody};
//!
//! assert_eq!(JMAP_DID_URI, "urn:ietf:params:jmap:did");
//! let cap: DidAccountCapability = serde_json::from_str(r#"{
//!     "did": "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK",
//!     "didDocumentUri": null, "previousDid": null, "keyCustody": "user"}"#)
//!     .unwrap();
//! assert_eq!(cap.key_custody, Some(KeyCustody::User));
//! ```

#![forbid(unsafe_code)]

pub use jmap_did_types as types;
