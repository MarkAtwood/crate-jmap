//! Pre-commit-gated structural tests for the bench fixtures.
//!
//! The fixture builders and `assert_*_fixture` helpers live in
//! `tests/common/mod.rs`. The bench harness (`benches/mime_pipeline.rs`)
//! consumes the same module via `#[path]` include, so a regression in
//! either the fixture shape or the structural invariant is now caught by
//! `cargo test --workspace` rather than only by `cargo bench`.
//!
//! Workspace tracking: bd:JMAP-t307.1.

#[path = "common/mod.rs"]
mod common;

use common::{
    assert_large_fixture, assert_medium_fixture, assert_small_fixture, build_large_deep_multipart,
    build_medium_multipart, build_small_plain,
};

#[test]
fn small_plain_fixture_parses_to_single_leaf() {
    let bytes = build_small_plain();
    assert_small_fixture(&bytes);
}

#[test]
fn medium_multipart_fixture_has_four_children() {
    let bytes = build_medium_multipart();
    assert_medium_fixture(&bytes);
}

#[test]
fn large_deep_multipart_fixture_nests_five_levels() {
    let bytes = build_large_deep_multipart();
    assert_large_fixture(&bytes);
}
