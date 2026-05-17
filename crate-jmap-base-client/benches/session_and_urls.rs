//! Criterion micro-benchmarks for the `jmap-base-client` session+URL hot path.
//!
//! Targets, per bd:JMAP-sc1b.106 acceptance criteria:
//! - [`Session`] JSON deserialization on a typical (RFC 8620 §2.1 example,
//!   2 accounts) payload and a large (100 accounts) payload.
//! - [`expand_url_template`] in three modes: a single-variable substitution
//!   (e.g. `upload_url`), a multi-variable substitution
//!   (`event_source_url` with `types`/`closeafter`/`ping`), and a
//!   substitution where one variable value requires percent-encoding (the
//!   `Cow::Owned` slow path).
//! - [`Session::primary_account_id`] capability URI lookup against a
//!   typical session capabilities map, and [`Session::websocket_capability`]
//!   for the RFC 8887 capability sub-object deserialize cost.
//!
//! All fixtures are hand-built JSON strings transcribed from RFC 8620 §2.1
//! and RFC 8887 — never derived from the code under test. The aim is a
//! stable regression baseline as the session/URL paths evolve.
//!
//! Run all benches:
//!     cargo bench -p jmap-base-client --bench session_and_urls
//!
//! Run one bench:
//!     cargo bench -p jmap-base-client --bench session_and_urls -- session_deserialize_typical
//!
//! Workspace tracking: bd:JMAP-sc1b.106.

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use jmap_base_client::{expand_url_template, Session};

// ---------- Fixture builders ----------

/// RFC 8620 §2.1 example Session JSON, hand-transcribed from spec text.
///
/// Two accounts (A13824 personal + A97813 shared), four capabilities,
/// three URL templates, one state token. ~1.4 KB.
fn typical_session_json() -> &'static str {
    r#"{
        "capabilities": {
            "urn:ietf:params:jmap:core": {
                "maxSizeUpload": 50000000,
                "maxConcurrentUpload": 8,
                "maxSizeRequest": 10000000,
                "maxConcurrentRequest": 8,
                "maxCallsInRequest": 32,
                "maxObjectsInGet": 256,
                "maxObjectsInSet": 128,
                "collationAlgorithms": [
                    "i;ascii-numeric",
                    "i;ascii-casemap",
                    "i;unicode-casemap"
                ]
            },
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:contacts": {},
            "urn:ietf:params:jmap:websocket": {
                "url": "wss://jmap.example.com/ws/",
                "supportsPush": true
            },
            "https://example.com/apis/foobar": {
                "maxFoosFinangled": 42
            }
        },
        "accounts": {
            "A13824": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:mail": {
                        "maxMailboxesPerEmail": null,
                        "maxMailboxDepth": 10
                    },
                    "urn:ietf:params:jmap:contacts": {}
                }
            },
            "A97813": {
                "name": "jane@example.com",
                "isPersonal": false,
                "isReadOnly": true,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:mail": {
                        "maxMailboxesPerEmail": 1,
                        "maxMailboxDepth": 10
                    }
                }
            }
        },
        "primaryAccounts": {
            "urn:ietf:params:jmap:mail": "A13824",
            "urn:ietf:params:jmap:contacts": "A13824"
        },
        "username": "john@example.com",
        "apiUrl": "https://jmap.example.com/api/",
        "downloadUrl": "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "https://jmap.example.com/upload/{accountId}/",
        "eventSourceUrl": "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
        "state": "75128aab4b1b"
    }"#
}

/// Build a synthetic Session JSON with 100 accounts to exercise the
/// `HashMap` allocation and deserialize cost on the upper end of what
/// large multi-tenant deployments would see.
///
/// Per-account shape matches the §2.1 example; account IDs are
/// `A000`..`A099`. Capability map and URL templates match the typical
/// fixture so the bench delta is purely the accounts-map size.
fn large_session_json(num_accounts: usize) -> String {
    let mut accounts = String::with_capacity(num_accounts * 256);
    for i in 0..num_accounts {
        if i > 0 {
            accounts.push(',');
        }
        // write! formats straight into the buffer; avoids the per-iteration
        // String allocation of push_str(&format!(...)) (bd:JMAP-6r7c.55).
        // Inline format args follow the workspace inline-args sweep
        // (bd:JMAP-6r7c.56).
        write!(
            accounts,
            r#""A{i:03}": {{
                "name": "user{i}@example.com",
                "isPersonal": false,
                "isReadOnly": false,
                "accountCapabilities": {{
                    "urn:ietf:params:jmap:mail": {{
                        "maxMailboxesPerEmail": null,
                        "maxMailboxDepth": 10
                    }}
                }}
            }}"#
        )
        .expect("writing to a String never fails");
    }
    format!(
        r#"{{
            "capabilities": {{
                "urn:ietf:params:jmap:core": {{}},
                "urn:ietf:params:jmap:mail": {{}}
            }},
            "accounts": {{ {} }},
            "primaryAccounts": {{
                "urn:ietf:params:jmap:mail": "A000"
            }},
            "username": "admin@example.com",
            "apiUrl": "https://jmap.example.com/api/",
            "downloadUrl": "https://jmap.example.com/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}",
            "uploadUrl": "https://jmap.example.com/upload/{{accountId}}/",
            "eventSourceUrl": "https://jmap.example.com/eventsource/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}",
            "state": "large-session-state-token"
        }}"#,
        accounts
    )
}

/// Assert that the typical-session fixture parses to the expected
/// shape. Inline assertion runs once per bench setup; harness=false
/// would skip a #[test] block.
fn assert_typical_session(raw: &str) {
    let session: Session = serde_json::from_str(raw).expect("typical session must parse");
    assert_eq!(session.accounts.len(), 2);
    assert_eq!(session.capabilities.len(), 5);
    assert_eq!(session.username, "john@example.com");
}

/// Assert that the large-session fixture parses and contains the
/// expected number of accounts.
fn assert_large_session(raw: &str, expected_accounts: usize) {
    let session: Session = serde_json::from_str(raw).expect("large session must parse");
    assert_eq!(session.accounts.len(), expected_accounts);
}

// ---------- Session deserialization benches ----------

fn bench_session_deserialize_typical(c: &mut Criterion) {
    let raw = typical_session_json();
    assert_typical_session(raw);
    c.bench_function("session_deserialize_typical", |b| {
        b.iter(|| {
            let session: Session =
                serde_json::from_str(black_box(raw)).expect("typical session must parse");
            black_box(session);
        })
    });
}

fn bench_session_deserialize_large_100_accounts(c: &mut Criterion) {
    let raw = large_session_json(100);
    assert_large_session(&raw, 100);
    c.bench_function("session_deserialize_large_100_accounts", |b| {
        b.iter(|| {
            let session: Session =
                serde_json::from_str(black_box(raw.as_str())).expect("large session must parse");
            black_box(session);
        })
    });
}

// ---------- URL template expansion benches ----------

fn bench_expand_url_template_single_var(c: &mut Criterion) {
    let template = "https://jmap.example.com/upload/{accountId}/";
    let vars: [(&str, &str); 1] = [("accountId", "A13824")];
    c.bench_function("expand_url_template_single_var", |b| {
        b.iter(|| {
            let url = expand_url_template(black_box(template), black_box(&vars))
                .expect("template must expand");
            black_box(url);
        })
    });
}

fn bench_expand_url_template_multi_var(c: &mut Criterion) {
    let template =
        "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}";
    let vars: [(&str, &str); 3] = [
        ("types", "Email,Mailbox,Thread"),
        ("closeafter", "state"),
        ("ping", "0"),
    ];
    c.bench_function("expand_url_template_multi_var", |b| {
        b.iter(|| {
            let url = expand_url_template(black_box(template), black_box(&vars))
                .expect("template must expand");
            black_box(url);
        })
    });
}

fn bench_expand_url_template_with_encoding(c: &mut Criterion) {
    // download_url template with a `name` value that requires
    // percent-encoding (space + slash). This exercises the
    // Cow::Owned slow path in `percent_encode`.
    let template = "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}";
    let vars: [(&str, &str); 4] = [
        ("accountId", "A13824"),
        ("blobId", "Bd83fde6"),
        ("name", "annual report 2026/Q1.pdf"),
        ("type", "application/pdf"),
    ];
    c.bench_function("expand_url_template_with_encoding", |b| {
        b.iter(|| {
            let url = expand_url_template(black_box(template), black_box(&vars))
                .expect("template must expand");
            black_box(url);
        })
    });
}

// ---------- Capability lookup benches ----------

fn bench_primary_account_id_hit(c: &mut Criterion) {
    let raw = typical_session_json();
    let session: Session = serde_json::from_str(raw).expect("typical session must parse");
    c.bench_function("session_primary_account_id_hit", |b| {
        b.iter(|| {
            let id = session.primary_account_id(black_box("urn:ietf:params:jmap:mail"));
            black_box(id);
        })
    });
}

fn bench_websocket_capability_deserialize(c: &mut Criterion) {
    let raw = typical_session_json();
    let session: Session = serde_json::from_str(raw).expect("typical session must parse");
    c.bench_function("session_websocket_capability_deserialize", |b| {
        b.iter(|| {
            // black_box both the input session and the output capability so
            // the optimizer cannot hoist the websocket_capability() call out
            // of the iteration loop. Consistent with every other bench in
            // this file (bd:JMAP-6r7c.12).
            let cap = black_box(&session)
                .websocket_capability()
                .expect("ws capability must parse");
            black_box(cap);
        })
    });
}

criterion_group!(
    benches,
    bench_session_deserialize_typical,
    bench_session_deserialize_large_100_accounts,
    bench_expand_url_template_single_var,
    bench_expand_url_template_multi_var,
    bench_expand_url_template_with_encoding,
    bench_primary_account_id_hit,
    bench_websocket_capability_deserialize,
);
criterion_main!(benches);
