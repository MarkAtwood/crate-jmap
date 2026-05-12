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
//!   binary attachment shaped after a small PDF.
//! - Large  (~1 MB): five levels of `multipart/mixed` nesting, with three
//!   text leaves at each level, exercising the recursive tree walker and
//!   the `find_by_id` lookups in `message_to_jmap_body`.
//!
//! All fixtures are hand-built RFC 5322 byte literals — never derived from
//! the code under test. The aim is a stable regression baseline, not
//! absolute throughput numbers.
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

// ---------- Fixture builders ----------

/// Build a ~1 KB single text/plain email.
///
/// Total layout:
/// - ~200 bytes of headers (From, To, Subject, Date, Message-ID, Content-Type).
/// - ~800 bytes of repeated Lorem Ipsum padding.
fn build_small_plain() -> Vec<u8> {
    let mut s = String::with_capacity(1100);
    s.push_str("From: alice@example.com\r\n");
    s.push_str("To: bob@example.com\r\n");
    s.push_str("Subject: Small bench fixture (~1 KB text/plain)\r\n");
    s.push_str("Date: Mon, 11 May 2026 12:00:00 +0000\r\n");
    s.push_str("Message-ID: <small-bench-1@example.com>\r\n");
    s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    s.push_str("\r\n");
    while s.len() < 1024 {
        s.push_str("Lorem ipsum dolor sit amet, consectetur adipiscing elit. ");
    }
    s.push_str("\r\n");
    s.into_bytes()
}

/// Build a ~16 KB multipart/mixed message with three text parts plus a
/// base64-encoded attachment shaped like a small PDF blob.
///
/// Layout:
/// - Outer multipart/mixed envelope (boundary = "b1").
/// - Three text/plain parts (~1 KB each) — exercises classification.
/// - One application/pdf base64 attachment (~12 KB encoded) — exercises
///   the attachment branch and the `attachments` list path.
///
/// The PDF body is filler ASCII, not a valid PDF. `mime_tree::parse`
/// does not validate attachment content, so this is a faithful workload
/// for the adapter pipeline.
fn build_medium_multipart() -> Vec<u8> {
    let mut s = String::with_capacity(17 * 1024);
    s.push_str("From: alice@example.com\r\n");
    s.push_str("To: bob@example.com\r\n");
    s.push_str("Subject: Medium bench fixture (~16 KB multipart/mixed)\r\n");
    s.push_str("Date: Mon, 11 May 2026 12:00:00 +0000\r\n");
    s.push_str("Message-ID: <medium-bench-1@example.com>\r\n");
    s.push_str("MIME-Version: 1.0\r\n");
    s.push_str("Content-Type: multipart/mixed; boundary=\"b1\"\r\n");
    s.push_str("\r\n");

    for (idx, label) in ["alpha", "beta", "gamma"].iter().enumerate() {
        s.push_str("--b1\r\n");
        s.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        s.push_str(&format!("Content-ID: <part-{}@example.com>\r\n", idx));
        s.push_str("\r\n");
        let target = s.len() + 1024;
        while s.len() < target {
            s.push_str("Body text for the ");
            s.push_str(label);
            s.push_str(" leaf part with stable filler content. ");
        }
        s.push_str("\r\n");
    }

    s.push_str("--b1\r\n");
    s.push_str("Content-Type: application/pdf\r\n");
    s.push_str("Content-Disposition: attachment; filename=\"report.pdf\"\r\n");
    s.push_str("Content-Transfer-Encoding: base64\r\n");
    s.push_str("\r\n");
    // Filler base64: 76-char lines of repeated ASCII. Not real base64
    // alphabet edge cases — mime-tree does not validate the content,
    // only the surrounding structural framing.
    let line = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    while s.len() < 16 * 1024 {
        s.push_str(line);
        s.push_str("\r\n");
    }
    s.push_str("\r\n--b1--\r\n");
    s.into_bytes()
}

/// Build a ~1 MB message with five levels of `multipart/mixed` nesting.
///
/// At each of the outer 4 levels: three text/plain leaves plus one
/// nested multipart child. The 5th (deepest) level has three text/plain
/// leaves and no nested child, terminating the recursion.
///
/// Total: 15 text leaves + 5 multipart wrappers = 20 parts, with body
/// padding sized to bring the encoded message to ~1 MB. This exercises
/// both the recursive `part_to_jmap_inner` and the `find_by_id` lookups
/// done by `message_to_jmap_body` for `text_body`/`html_body`/`attachments`.
fn build_large_deep_multipart() -> Vec<u8> {
    // Per-leaf body padding: ~70 KB × 15 leaves ≈ 1.05 MB body, plus MIME
    // framing comfortably exceeds 1 MB.
    const LEAF_PAD_BYTES: usize = 70 * 1024;
    const NESTING_DEPTH: usize = 5;

    let boundaries: Vec<String> = (0..NESTING_DEPTH)
        .map(|i| format!("bnd{}", i + 1))
        .collect();

    let mut s = String::with_capacity(1_100_000);
    s.push_str("From: alice@example.com\r\n");
    s.push_str("To: bob@example.com\r\n");
    s.push_str("Subject: Large bench fixture (~1 MB deeply nested multipart)\r\n");
    s.push_str("Date: Mon, 11 May 2026 12:00:00 +0000\r\n");
    s.push_str("Message-ID: <large-bench-1@example.com>\r\n");
    s.push_str("MIME-Version: 1.0\r\n");
    s.push_str(&format!(
        "Content-Type: multipart/mixed; boundary=\"{}\"\r\n",
        boundaries[0]
    ));
    s.push_str("\r\n");

    write_level(&mut s, &boundaries, 0, LEAF_PAD_BYTES);
    s.into_bytes()
}

/// Recursively write one level of the deeply-nested fixture into `out`.
///
/// At `level < NESTING_DEPTH - 1` writes three text leaves plus one
/// nested multipart child. At the deepest level writes three text
/// leaves only and closes.
fn write_level(out: &mut String, boundaries: &[String], level: usize, leaf_pad: usize) {
    let boundary = &boundaries[level];
    let is_deepest = level + 1 == boundaries.len();

    for leaf_idx in 0..3 {
        out.push_str("--");
        out.push_str(boundary);
        out.push_str("\r\n");
        out.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        out.push_str(&format!(
            "Content-ID: <L{}-leaf-{}@example.com>\r\n",
            level + 1,
            leaf_idx
        ));
        out.push_str("\r\n");
        let target = out.len() + leaf_pad;
        while out.len() < target {
            out.push_str("Padding line for stable bench fixture content. ");
        }
        out.push_str("\r\n");
    }

    if !is_deepest {
        let inner = &boundaries[level + 1];
        out.push_str("--");
        out.push_str(boundary);
        out.push_str("\r\n");
        out.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{}\"\r\n",
            inner
        ));
        out.push_str("\r\n");
        write_level(out, boundaries, level + 1, leaf_pad);
    }

    out.push_str("--");
    out.push_str(boundary);
    out.push_str("--\r\n");
}

/// Assert that the small fixture is ~1 KB and parses to a single leaf.
///
/// Runs once per bench function during fixture setup. Bench targets use
/// `harness = false`, so `#[test]` items inside this file would never
/// be collected; inline assertions are the equivalent guarantee.
fn assert_small_fixture(bytes: &[u8]) {
    assert!(
        (1024..1500).contains(&bytes.len()),
        "small fixture should be ~1 KB, got {} bytes",
        bytes.len()
    );
    let msg = parse(bytes).expect("small fixture must parse");
    assert!(
        msg.part_index.children.is_empty(),
        "small fixture should be a single leaf",
    );
}

/// Assert that the medium fixture is ~16 KB and has exactly four
/// children (3 text + 1 attachment).
fn assert_medium_fixture(bytes: &[u8]) {
    assert!(
        (16 * 1024..18 * 1024).contains(&bytes.len()),
        "medium fixture should be ~16 KB, got {} bytes",
        bytes.len()
    );
    let msg = parse(bytes).expect("medium fixture must parse");
    assert_eq!(
        msg.part_index.children.len(),
        4,
        "medium fixture should have 3 text + 1 attachment = 4 children",
    );
}

/// Assert that the large fixture is ~1 MB and nests exactly 5 levels.
fn assert_large_fixture(bytes: &[u8]) {
    assert!(
        (1_000_000..1_300_000).contains(&bytes.len()),
        "large fixture should be ~1 MB, got {} bytes",
        bytes.len()
    );
    let msg = parse(bytes).expect("large fixture must parse");
    let mut depth = 1usize;
    let mut node = &msg.part_index;
    loop {
        let multipart_child = node.children.iter().find(|c| !c.children.is_empty());
        match multipart_child {
            Some(c) => {
                depth += 1;
                node = c;
            }
            None => break,
        }
    }
    assert_eq!(depth, 5, "large fixture should nest 5 levels deep");
}

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
