//! JMAP Object History client bindings over `jmap-base-client`
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes,
//! docs/design/object-history.md §2a).
//!
//! # Status
//!
//! Scaffold only (bd JMAP-tna1.3.1). History `Foo/get`, destroyed
//! `Foo/query` and `restore` on the base client's generic `SetCall` arrive
//! with bd JMAP-tna1.3.12 (docs/design/object-history.md §8).

#![forbid(unsafe_code)]
