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
//! # Logging safety
//!
//! This crate derives plain [`Debug`] on its PII-bearing types ([`Principal`],
//! [`PrincipalFilterCondition`], [`ShareNotification`], and [`ChangedBy`]).
//! `tracing` or `log` macros that interpolate one of these values with `?`
//! (debug) or `%` (display) emit its full contents to the configured sink:
//! people's names, email addresses, descriptions, metadata, and the name and
//! address of whoever changed a share. That data is typically regulated as
//! personal data under GDPR, CCPA, and similar privacy frameworks.
//!
//! Consumers SHOULD log ids rather than whole values, or wrap PII-bearing
//! values in a redacting newtype before logging:
//!
//! ```ignore
//! // BAD — leaks the principal's name, email, and description.
//! tracing::debug!(principal = ?principal, "resolved principal");
//!
//! // GOOD — emits only the opaque id.
//! tracing::debug!(principal_id = ?principal.id, "resolved principal");
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

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

mod backend;
mod capability;
mod notification;
mod principal;

pub use backend::{PrincipalProperty, ShareNotificationProperty};
pub use capability::{
    JMAP_PRINCIPALS_OWNER_URI, JMAP_PRINCIPALS_URI, PrincipalsCapability, PrincipalsOwnerCapability,
};
pub use notification::{
    ChangedBy, ShareNotification, ShareNotificationComparator, ShareNotificationComparatorProperty,
    ShareNotificationFilter, ShareNotificationFilterCondition,
};
pub use principal::{Principal, PrincipalFilter, PrincipalFilterCondition, PrincipalType};

/// Generic filter algebra from `jmap-types` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-sharing-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the same root
/// re-exports in `jmap-mail-types`, the workspace canonical
/// extension-types template.
pub use jmap_types::{Filter, FilterOperator, Operator};
