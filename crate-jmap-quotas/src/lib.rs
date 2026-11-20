//! JMAP for Quotas ([RFC 9425](https://www.rfc-editor.org/rfc/rfc9425)):
//! umbrella crate re-exporting the component crates, so one dependency
//! covers them.
//!
//! - [`types`]: the [`jmap-quotas-types`](https://crates.io/crates/jmap-quotas-types),
//!   always enabled.
//! - `client`: [`jmap-quotas-client`](https://crates.io/crates/jmap-quotas-client),
//!   behind the `client` feature.
//! - `server`: [`jmap-quotas-server`](https://crates.io/crates/jmap-quotas-server),
//!   behind the `server` feature; the `memory` feature adds its in-memory
//!   reference backend.
//!
//! The component crates version in lockstep with this one.
//!
//! ```
//! use jmap_quotas::types::{JMAP_QUOTA_URI, Quota};
//!
//! assert_eq!(JMAP_QUOTA_URI, "urn:ietf:params:jmap:quota");
//! let q: Quota = serde_json::from_str(r#"{"id": "q1", "resourceType": "count",
//!     "used": 1, "hardLimit": 10, "scope": "account", "name": "n", "types": []}"#)
//!     .unwrap();
//! assert_eq!(q.hard_limit, 10);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]

pub use jmap_quotas_types as types;

#[cfg(feature = "client")]
pub use jmap_quotas_client as client;

#[cfg(feature = "server")]
pub use jmap_quotas_server as server;
