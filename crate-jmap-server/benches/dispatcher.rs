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

use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use jmap_server::{parse_request, resolve_args, Dispatcher, HandlerFuture, JmapHandler};
use jmap_types::{Invocation, JmapRequest};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

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
        // bd:JMAP-wlip.9 — parse_request consumes its Value argument,
        // so each iteration needs a fresh clone. Use iter_batched so
        // the clone runs in the setup closure (outside the timed
        // region); only the parse_request call itself is measured.
        b.iter_batched(
            || body.clone(),
            |body| {
                let req = parse_request(black_box(body), 16).expect("sample body must parse");
                black_box(req);
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_resolve_args_with_ref(c: &mut Criterion) {
    let priors = sample_prior_responses();
    let args = sample_args_with_ref();
    c.bench_function("resolve_args_ids_star", |b| {
        // bd:JMAP-wlip.9 — see bench_parse_request above for rationale.
        b.iter_batched(
            || args.clone(),
            |mut a| {
                resolve_args(&mut a, &priors).expect("resolve must succeed");
                black_box(a);
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_resolve_args_no_ref(c: &mut Criterion) {
    let priors = sample_prior_responses();
    let args = sample_args_no_ref();
    c.bench_function("resolve_args_no_refs_fast_path", |b| {
        // bd:JMAP-wlip.9 — see bench_parse_request above for rationale.
        b.iter_batched(
            || args.clone(),
            |mut a| {
                resolve_args(&mut a, &priors).expect("no-op resolve must succeed");
                black_box(a);
            },
            BatchSize::SmallInput,
        )
    });
}

// -----------------------------------------------------------------------
// Dispatcher::dispatch benches (bd:JMAP-wlip.8)
//
// Surface the per-call spawn overhead documented in bd:JMAP-wlip.8 so
// any future change to the panic-isolation strategy (e.g. replacing
// task::spawn with catch_unwind) can be measured for regression /
// improvement. The handler is an unconditional Ok-returning no-op so
// the bench measures the dispatcher overhead, not handler work.
//
// What this bench DOES catch (bd:JMAP-jfia.16):
//   - task::spawn overhead per method call
//   - JoinError handling cost
//   - method_responses Vec push cost
//   - createdIds extraction cost when /set responses are present
//   - ResultReference resolution cost
//
// What this bench does NOT catch:
//   - serialize_value cost (relevant to handle_get's per-object to_value
//     pattern, bd:JMAP-jfia.10)
//   - backend trait method dispatch cost
//   - any per-handler allocation patterns
//
// A future bench with a realistic-shape handler (e.g. one that runs
// serialize_value over a Vec<MockObject>) would surface the second
// class of regressions. Out of scope for the current bench set.
// -----------------------------------------------------------------------

/// A handler that returns a fixed (empty-object, no-extras) success
/// response. Used to isolate dispatcher overhead.
struct NoopHandler;

impl JmapHandler<()> for NoopHandler {
    fn call(&self, _method: String, _call_id: String, _args: Value, _caller: ()) -> HandlerFuture {
        Box::pin(async move { Ok((json!({}), vec![])) })
    }
}

fn make_dispatcher() -> Dispatcher<()> {
    let mut d: Dispatcher<()> = Dispatcher::new();
    let handler: Arc<dyn JmapHandler<()>> = Arc::new(NoopHandler);
    // Register the same handler under every method name the benches use.
    for method in ["M/one", "M/a", "M/b"] {
        d.register(method, Arc::clone(&handler));
    }
    d
}

/// Single-call request — minimum-overhead dispatch (one method, one
/// spawn, one await).
fn sample_single_call_request() -> JmapRequest {
    JmapRequest::new(
        vec!["urn:ietf:params:jmap:core".into()],
        vec![("M/one".into(), json!({}), "c0".into())],
        None,
    )
}

/// 16-call request — the RFC 8620 §3 default `max_calls` cap. Captures
/// the worst-case spawn overhead inside one parsed request.
fn sample_sixteen_call_request() -> JmapRequest {
    let mut calls: Vec<(String, Value, String)> = Vec::with_capacity(16);
    for i in 0..16 {
        let method = if i % 2 == 0 { "M/a" } else { "M/b" };
        calls.push((method.into(), json!({}), format!("c{i}")));
    }
    JmapRequest::new(vec!["urn:ietf:params:jmap:core".into()], calls, None)
}

fn bench_dispatch_single_call(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let dispatcher = make_dispatcher();
    let request = sample_single_call_request();

    c.bench_function("dispatch_single_call", |b| {
        b.iter_batched(
            || request.clone(),
            |req| {
                rt.block_on(async {
                    let resp = dispatcher.dispatch(req, (), "s0".into()).await;
                    black_box(resp);
                });
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_dispatch_batch_of_sixteen(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let dispatcher = make_dispatcher();
    let request = sample_sixteen_call_request();

    c.bench_function("dispatch_batch_of_sixteen", |b| {
        b.iter_batched(
            || request.clone(),
            |req| {
                rt.block_on(async {
                    let resp = dispatcher.dispatch(req, (), "s0".into()).await;
                    black_box(resp);
                });
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    bench_parse_request,
    bench_resolve_args_with_ref,
    bench_resolve_args_no_ref,
    bench_dispatch_single_call,
    bench_dispatch_batch_of_sixteen,
);
criterion_main!(benches);
