//! Criterion micro-benchmarks for the `jmap-mime` adapter pipeline.
//!
//! Targets:
//! - `mime_tree::parse`: upstream MIME parser cost on three representative
//!   workload sizes.
//! - [`message_to_jmap_body`]: the recursive tree-walk and RFC 8621 §4.1.4
//!   body-list classification done by this crate.
//!
//! Together these cover the full "raw bytes → JMAP body fields" pipeline
//! that `jmap-mail-server` runs on every `Email/get` body materialisation.
//!
//! Workloads, per bd:JMAP-sc1b.105 acceptance criteria:
//! - Small  (~1 KB): single text/plain body, minimal headers.
//! - Medium (~16 KB): multipart/mixed with three text parts and a base64
//!   filler-block attachment.
//! - Large  (~1 MB): five levels of `multipart/mixed` nesting, with three
//!   text leaves at each level, exercising the recursive tree walker and
//!   the `find_by_id` lookups in `message_to_jmap_body`.
//!
//! All fixtures are hand-built RFC 5322 byte literals — never derived from
//! the code under test. The aim is a stable regression baseline, not
//! absolute throughput numbers.
//!
//! Fixture builders and structural-invariant asserts live in
//! `tests/common/mod.rs` and are shared with `tests/bench_fixtures.rs` so
//! that the structural invariants run under `cargo test` (the workspace
//! pre-commit gate) and not only under `cargo bench`. The asserts here
//! at fixture-setup time are belt-and-suspenders: they also fire when
//! `cargo bench -p jmap-mime` runs.
//!
//! Run all benches:
//!     cargo bench -p jmap-mime
//!
//! Run one bench:
//!     cargo bench -p jmap-mime -- parse_small_plain
//!
//! Workspace tracking: bd:JMAP-sc1b.105.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use jmap_mime::message_to_jmap_body;
use jmap_types::Id;
use mime_tree::{parse, ParsedMessage};

#[path = "../tests/common/mod.rs"]
mod common;

use common::{
    assert_large_fixture, assert_medium_fixture, assert_small_fixture, build_large_deep_multipart,
    build_medium_multipart, build_small_plain,
};

// ---------- parse-only benches ----------

fn bench_parse_small(c: &mut Criterion) {
    let bytes = build_small_plain();
    assert_small_fixture(&bytes);
    c.bench_function("parse_small_plain", |b| {
        b.iter(|| {
            let msg = parse(black_box(&bytes)).expect("small fixture must parse");
            black_box(msg);
        })
    });
}

fn bench_parse_medium(c: &mut Criterion) {
    let bytes = build_medium_multipart();
    assert_medium_fixture(&bytes);
    c.bench_function("parse_medium_multipart", |b| {
        b.iter(|| {
            let msg = parse(black_box(&bytes)).expect("medium fixture must parse");
            black_box(msg);
        })
    });
}

fn bench_parse_large(c: &mut Criterion) {
    let bytes = build_large_deep_multipart();
    assert_large_fixture(&bytes);
    c.bench_function("parse_large_deep_multipart", |b| {
        b.iter(|| {
            let msg = parse(black_box(&bytes)).expect("large fixture must parse");
            black_box(msg);
        })
    });
}

// ---------- adapter (parse + convert) benches ----------

fn bench_pipeline_small(c: &mut Criterion) {
    let bytes = build_small_plain();
    assert_small_fixture(&bytes);
    c.bench_function("pipeline_small_plain", |b| {
        b.iter(|| {
            let msg = parse(black_box(&bytes)).expect("small fixture must parse");
            let fields =
                message_to_jmap_body(black_box(&msg), |p| Id::from(format!("blob-{}", p.part_id)));
            black_box(fields);
        })
    });
}

fn bench_pipeline_medium(c: &mut Criterion) {
    let bytes = build_medium_multipart();
    assert_medium_fixture(&bytes);
    c.bench_function("pipeline_medium_multipart", |b| {
        b.iter(|| {
            let msg = parse(black_box(&bytes)).expect("medium fixture must parse");
            let fields =
                message_to_jmap_body(black_box(&msg), |p| Id::from(format!("blob-{}", p.part_id)));
            black_box(fields);
        })
    });
}

fn bench_pipeline_large(c: &mut Criterion) {
    let bytes = build_large_deep_multipart();
    assert_large_fixture(&bytes);
    c.bench_function("pipeline_large_deep_multipart", |b| {
        b.iter(|| {
            let msg = parse(black_box(&bytes)).expect("large fixture must parse");
            let fields =
                message_to_jmap_body(black_box(&msg), |p| Id::from(format!("blob-{}", p.part_id)));
            black_box(fields);
        })
    });
}

// ---------- convert-only benches (parse hoisted out of the inner loop) ----------

fn bench_convert_small(c: &mut Criterion) {
    let bytes = build_small_plain();
    assert_small_fixture(&bytes);
    let msg: ParsedMessage = parse(&bytes).expect("small fixture must parse");
    c.bench_function("convert_small_plain", |b| {
        b.iter(|| {
            let fields =
                message_to_jmap_body(black_box(&msg), |p| Id::from(format!("blob-{}", p.part_id)));
            black_box(fields);
        })
    });
}

fn bench_convert_medium(c: &mut Criterion) {
    let bytes = build_medium_multipart();
    assert_medium_fixture(&bytes);
    let msg: ParsedMessage = parse(&bytes).expect("medium fixture must parse");
    c.bench_function("convert_medium_multipart", |b| {
        b.iter(|| {
            let fields =
                message_to_jmap_body(black_box(&msg), |p| Id::from(format!("blob-{}", p.part_id)));
            black_box(fields);
        })
    });
}

fn bench_convert_large(c: &mut Criterion) {
    let bytes = build_large_deep_multipart();
    assert_large_fixture(&bytes);
    let msg: ParsedMessage = parse(&bytes).expect("large fixture must parse");
    c.bench_function("convert_large_deep_multipart", |b| {
        b.iter(|| {
            let fields =
                message_to_jmap_body(black_box(&msg), |p| Id::from(format!("blob-{}", p.part_id)));
            black_box(fields);
        })
    });
}

criterion_group!(
    benches,
    bench_parse_small,
    bench_parse_medium,
    bench_parse_large,
    bench_pipeline_small,
    bench_pipeline_medium,
    bench_pipeline_large,
    bench_convert_small,
    bench_convert_medium,
    bench_convert_large,
);
criterion_main!(benches);
