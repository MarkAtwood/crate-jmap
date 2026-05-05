//! Shared JMAP wire types for RFC 8620.
//!
//! Provides [`Id`], [`UTCDate`], [`State`], [`JmapError`],
//! [`JmapRequest`], [`JmapResponse`], [`Filter`], and [`ResultReference`] —
//! the primitives shared by all crates in the `jmap-*` family.
//!
//! This crate is types-only: no method handlers, no async, no network I/O.
//! It is the root dependency for `jmap-mail-types`, `jmap-chat-types`, and
//! all server and client crates in the family.
//!
//! All types implement [`serde::Serialize`] and [`serde::Deserialize`] with the
//! camelCase field names required by the JMAP wire format.
//!
//! # Example
//!
//! ```rust
//! use jmap_types::JmapRequest;
//!
//! let json = r#"{
//!     "using": ["urn:ietf:params:jmap:core"],
//!     "methodCalls": [["Mailbox/get", {"accountId": "a1"}, "c0"]]
//! }"#;
//!
//! let req: JmapRequest = serde_json::from_str(json).unwrap();
//! assert_eq!(req.using[0], "urn:ietf:params:jmap:core");
//! assert_eq!(req.method_calls[0].0, "Mailbox/get");
//! ```

#![forbid(unsafe_code)]

pub mod backend;
pub mod error;
pub mod id;
pub mod query;
pub mod resultref;
pub mod wire;

pub use backend::{GetObject, JmapObject, QueryObject, SetObject};
pub use error::JmapError;
pub use id::{Date, Id, State, UTCDate, ValidationError};
pub use query::{Filter, FilterOperator, Operator};
pub use resultref::{Argument, ResultReference};
pub use wire::{Invocation, JmapRequest, JmapResponse};
