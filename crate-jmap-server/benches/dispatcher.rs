//! Criterion micro-benchmarks for the `jmap-server` dispatcher hot path.
//!
//! Targets:
//! - [`parse_request`]: decoding a JMAP wire request into [`JmapRequest`].
//! - [`resolve_args`]: in-place ResultReference resolution against prior
//!   responses, with and without the RFC 8620 §3.7 `*` wildcard.
//!
//! Inputs are hand-built `serde_json::Value`s shaped after RFC 8620 §3
//! examples and the Fastmail "top-ten" sample (jmap-samples-fastmail
//! python3/top-ten.py — see workspace memory
//! `jmap-ids-result-reference-path-fastmail-official-samples`). The aim
//! is not to maximize throughput numbers but to keep a baseline that
//! reveals regressions in the parse + resolve pipeline as the
//! dispatcher evolves.
//!
//! Run all benches:
//!     cargo bench -p jmap-server
//!
//! Run one bench:
//!     cargo bench -p jmap-server -- parse_request_three_calls
//!
//! Workspace tracking: bd:JMAP-sc1b.18.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use jmap_server::{parse_request, resolve_args};
use jmap_types::Invocation;
use serde_json::{json, Value};

/// A representative 3-call JMAP request body matching the canonical
/// "list recent messages then fetch their threads" pattern from
/// RFC 8620 §3.7 and the Fastmail samples.
fn sample_three_call_body() -> Value {
    json!({
        "using": [
            "urn:ietf:params:jmap:core",
            "urn:ietf:params:jmap:mail"
        ],
        "methodCalls": [
            [
                "Email/query",
                {
                    "accountId": "a1",
                    "filter": { "inMailbox": "inbox" },
                    "sort": [{ "property": "receivedAt", "isAscending": false }],
                    "collapseThreads": true,
                    "position": 0,
                    "limit": 10,
                    "calculateTotal": true
                },
                "c0"
            ],
            [
                "Email/get",
                {
                    "accountId": "a1",
                    "#ids": {
                        "resultOf": "c0",
                        "name": "Email/query",
                        "path": "/ids/*"
                    },
                    "properties": [
                        "threadId", "subject", "from", "to",
                        "receivedAt", "preview"
                    ]
                },
                "c1"
            ],
            [
                "Thread/get",
                {
                    "accountId": "a1",
                    "#ids": {
                        "resultOf": "c1",
                        "name": "Email/get",
                        "path": "/list/*/threadId"
                    }
                },
                "c2"
            ]
        ]
    })
}

/// A representative prior-response list emulating `Email/query` having
/// just returned a 10-id list. Subsequent `resolve_args` benches use
/// this to resolve `#ids` references with path `/ids/*` (RFC 8620 §3.7
/// wildcard) against it.
fn sample_prior_responses() -> Vec<Invocation> {
    vec![(
        "Email/query".to_owned(),
        json!({
            "accountId": "a1",
            "queryState": "q-state-1",
            "canCalculateChanges": true,
            "position": 0,
            "ids": [
                "e1", "e2", "e3", "e4", "e5",
                "e6", "e7", "e8", "e9", "e10"
            ],
            "total": 10
        }),
        "c0".to_owned(),
    )]
}

/// Args object containing a single `#ids` ResultReference to be
/// resolved against `sample_prior_responses`.
fn sample_args_with_ref() -> Value {
    json!({
        "accountId": "a1",
        "#ids": {
            "resultOf": "c0",
            "name": "Email/query",
            "path": "/ids/*"
        },
        "properties": ["threadId", "subject", "from", "receivedAt"]
    })
}

/// Args object with no ResultReferences — exercises the early-return
/// path in `resolve_args`.
fn sample_args_no_ref() -> Value {
    json!({
        "accountId": "a1",
        "ids": ["e1", "e2", "e3"],
        "properties": ["threadId", "subject"]
    })
}

fn bench_parse_request(c: &mut Criterion) {
    let body = sample_three_call_body();
    c.bench_function("parse_request_three_calls", |b| {
        b.iter(|| {
            // Clone per iteration: parse_request takes Value by value
            // and consumes it. The clone cost is unavoidable but
            // stable across runs, so the bench remains a fair
            // regression baseline.
            let req = parse_request(black_box(body.clone()), 16).expect("sample body must parse");
            black_box(req);
        })
    });
}

fn bench_resolve_args_with_ref(c: &mut Criterion) {
    let priors = sample_prior_responses();
    let args = sample_args_with_ref();
    c.bench_function("resolve_args_ids_star", |b| {
        b.iter(|| {
            let mut a = black_box(args.clone());
            resolve_args(&mut a, &priors).expect("resolve must succeed");
            black_box(a);
        })
    });
}

fn bench_resolve_args_no_ref(c: &mut Criterion) {
    let priors = sample_prior_responses();
    let args = sample_args_no_ref();
    c.bench_function("resolve_args_no_refs_fast_path", |b| {
        b.iter(|| {
            let mut a = black_box(args.clone());
            resolve_args(&mut a, &priors).expect("no-op resolve must succeed");
            black_box(a);
        })
    });
}

criterion_group!(
    benches,
    bench_parse_request,
    bench_resolve_args_with_ref,
    bench_resolve_args_no_ref
);
criterion_main!(benches);
