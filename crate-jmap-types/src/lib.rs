//! Shared JMAP wire types for RFC 8620.
//!
//! Provides [`Id`], [`UTCDate`], [`Date`], [`State`], [`JmapError`],
//! [`JmapRequest`], [`JmapResponse`], [`Filter`], and [`ResultReference`] —
//! the primitives shared by all crates in the `jmap-*` family.
//!
//! No async, no network I/O. Depends only on `serde`, `serde_json`,
//! and `thiserror`.

#![forbid(unsafe_code)]

pub mod error;
pub mod id;
pub mod query;
pub mod resultref;
pub mod wire;

pub use error::JmapError;
pub use id::{Date, Id, State, UTCDate};
pub use query::{Filter, FilterOperator, Operator};
pub use resultref::{Argument, ResultReference};
pub use wire::{Invocation, JmapRequest, JmapResponse};
