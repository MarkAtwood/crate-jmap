//! Shared JMAP wire types for RFC 8620.
//!
//! Provides [`Id`], [`UTCDate`], [`State`], [`JmapError`],
//! [`JmapRequest`], [`JmapResponse`], [`Filter`], [`ResultReference`], and
//! the RFC 8620 §7 push types ([`StateChange`], [`PushSubscription`]), and
//! the draft-atwood-jmap-object-uri-00 object reference [`JmapUri`] —
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
//! assert_eq!(req.method_calls[0].name, "Mailbox/get");
//! ```

#![forbid(unsafe_code)]

mod backend;
mod error;
mod id;
mod jmap_uri;
mod methods;
mod patch;
mod push;
mod query;
mod resultref;
mod set_error;
mod wire;

// `string_enum` is a private module that hosts the [`impl_string_enum!`]
// declarative macro. The macro itself is `#[macro_export]`-ed, so it is
// reachable as `jmap_types::impl_string_enum!` regardless of this `mod`'s
// visibility. See bd JMAP-wk77.
mod string_enum;

pub use backend::{GetObject, JmapObject, QueryObject, SetObject};
pub use error::{JmapError, MethodErrorType};
pub use id::{Date, Id, State, UTCDate, ValidationError};
pub use jmap_uri::{JmapUri, JmapUriError};
pub use methods::{
    AddedItem, ChangesResponse, CopyResponse, GetResponse, PartialObject, QueryChangesResponse,
    QueryResponse, SetResponse,
};
pub use patch::PatchObject;
pub use push::{PushKeys, PushSubscription, PushVerification, StateChange, Urgency};
pub use query::{Filter, FilterOperator, Operator};
pub use resultref::{Argument, ResultReference};
pub use set_error::{ReservedExtrasKey, SetError, SetErrorType, RESERVED_SET_ERROR_WIRE_NAMES};
pub use wire::{Invocation, JmapRequest, JmapResponse};
