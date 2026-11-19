//! JMAP search: a reserved umbrella crate with **no public items yet**.
//!
//! The planned extension is a generic filter-expression language for the
//! `/query` method of every JMAP data type: JSON Pointer field tests into
//! the `/get` representation, comparison, array and logical operators,
//! collation and free-text matching, added as a `where` filter property
//! under a capability of its own. It is to be lifted from the
//! graph-independent part of draft-atwood-jmap-graph-search-00. When its
//! `-types`, `-client` and `-server` crates exist, this crate re-exports
//! them the way the other `jmap-*` umbrellas (for example `jmap-quotas`)
//! do: `types` always, `client` and `server` behind features.
//!
//! Until then, the only search language in the workspace is Graph
//! search (`urn:ietf:params:jmap:graph:search`), in the `search` modules
//! of [`jmap-graph-types`](https://crates.io/crates/jmap-graph-types) and
//! [`jmap-graph-server`](https://crates.io/crates/jmap-graph-server); the
//! [`jmap-graph`](https://crates.io/crates/jmap-graph) umbrella
//! re-exports both. This crate does not re-export them, because their
//! API is Graph-specific and will change when the type-neutral core
//! moves out.
//!
//! Version 0.5.0 replaces the 0.0.1 name-claim placeholder and keeps the
//! name on the shared `jmap-*` version line.

#![forbid(unsafe_code)]
#![no_std]
