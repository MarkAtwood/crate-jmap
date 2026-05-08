//! RFC 9670 JMAP Sharing data types.
//!
//! Provides [`Principal`], [`ShareNotification`], and supporting types for the
//! [JMAP Sharing extension](https://www.rfc-editor.org/rfc/rfc9670) (RFC 9670).
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` (shared RFC 8620 wire primitives) and
//! `jmap-sharing-server` / `jmap-sharing-client`.
//!
//! All types implement [`serde::Serialize`] and [`serde::Deserialize`] with the
//! camelCase field names required by the JMAP wire format.
//!
//! # Example
//!
//! ```rust
//! use jmap_sharing_types::{Principal, PrincipalType};
//!
//! let json = r#"{
//!     "id": "P123",
//!     "type": "individual",
//!     "name": "Jane Doe",
//!     "description": null,
//!     "email": "jane@example.com",
//!     "timeZone": "Europe/London",
//!     "capabilities": {},
//!     "accounts": null
//! }"#;
//!
//! let p: Principal = serde_json::from_str(json).unwrap();
//! assert_eq!(p.name, "Jane Doe");
//! assert_eq!(p.principal_type, PrincipalType::Individual);
//! ```

#![forbid(unsafe_code)]

pub mod backend;
pub mod capability;
pub mod notification;
pub mod principal;

pub use backend::{PrincipalProperty, ShareNotificationProperty};
pub use capability::{
    PrincipalsCapability, PrincipalsOwnerCapability, JMAP_PRINCIPALS_OWNER_URI, JMAP_PRINCIPALS_URI,
};
pub use notification::{ChangedBy, ShareNotification, ShareNotificationFilterCondition};
pub use principal::{Principal, PrincipalFilterCondition, PrincipalType};
