//! RFC 9425 JMAP Quotas data types.
//!
//! Provides [`Quota`], [`Scope`], [`ResourceType`] and the `Quota/query`
//! filter and comparator for [JMAP for Quotas](https://www.rfc-editor.org/rfc/rfc9425)
//! (RFC 9425).
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It sits between `jmap-types` (shared RFC 8620 wire primitives) and
//! `jmap-quotas-server` / `jmap-quotas-client`.
//!
//! All types implement [`serde::Serialize`] and [`serde::Deserialize`] with the
//! camelCase field names required by the JMAP wire format.
//!
//! # Example
//!
//! ```rust
//! use jmap_quotas_types::{Quota, ResourceType, Scope};
//!
//! let json = r#"{
//!     "id": "q1",
//!     "resourceType": "octets",
//!     "used": 1024,
//!     "hardLimit": 4096,
//!     "scope": "account",
//!     "name": "bob@example.com",
//!     "types": ["Email"]
//! }"#;
//!
//! let q: Quota = serde_json::from_str(json).unwrap();
//! assert_eq!(q.resource_type, ResourceType::Octets);
//! assert_eq!(q.scope, Scope::Account);
//! ```

#![forbid(unsafe_code)]

mod backend;
mod capability;
mod quota;

pub use backend::QuotaProperty;
pub use capability::{JMAP_QUOTA_URI, QuotaCapability};
pub use quota::{
    Quota, QuotaComparator, QuotaComparatorProperty, QuotaFilter, QuotaFilterCondition,
    ResourceType, Scope,
};

/// Generic filter algebra from `jmap-types` (RFC 8620 §5.5).
///
/// Re-exported here so callers of `jmap-quotas-types` do not need a
/// direct dependency on `jmap-types`. Mirrors the same root
/// re-exports in `jmap-mail-types`, the workspace canonical
/// extension-types template.
pub use jmap_types::{Filter, FilterOperator, Operator};
