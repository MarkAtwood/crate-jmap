//! JMAP Object History helpers for server backends
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes,
//! docs/design/object-history.md §2a).
//!
//! # Status
//!
//! Scaffold only (bd JMAP-tna1.3.1). The pure helpers that reference
//! backends use to keep history — version selection, the per-object
//! version log with retention, and the `Foo/changes` fold for restored
//! objects — arrive with bd JMAP-tna1.3.2. They work on the wire types of
//! `jmap-object-history-types`. The handlers and the
//! `ObjectHistoryBackend` trait live in `jmap-server`.

#![forbid(unsafe_code)]
