//! Tripwire integration test using the [`common::log_capture::LogCapture`]
//! harness: future `tracing::*` instrumentation that interpolates a
//! `Metadata` value via `?` or `{:?}` must NOT leak the per-user-private
//! content of `Metadata.extra` (or any other client-supplied field) into
//! captured log output.
//!
//! Tracks bd:JMAP-826m.53.
//!
//! ## Why this exists when no `tracing::*` call sites live in
//! `crate-jmap-metadata-server/src/` today
//!
//! The crate currently emits zero `tracing::*` events (verified by
//! `rg 'tracing::|info!|debug!|warn!|error!' crate-jmap-metadata-server/src/`).
//! A naïve reading of this file is "trivially passing because no logging
//! ever happens". That IS the entire test — it is a tripwire for the
//! FUTURE state where someone adds debug instrumentation:
//!
//! 1. [`log_capture_captures_traced_output`] is the harness self-test.
//!    It proves the [`LogCapture`] mechanism actually captures what
//!    `tracing` emits before any negative assertion relies on it.
//!
//! 2. [`log_capture_detects_a_leak_when_one_exists`] is the negative
//!    control. It models a hypothetical future leak by emitting a
//!    locally-constructed `Leaky` value through `tracing::info!`,
//!    proving the LogCapture surface would catch a real leak.
//!
//! 3. [`metadata_extra_never_appears_in_handler_path_logs`] is the
//!    metadata-specific canary. It seeds a `Metadata::Annotation` with
//!    a unique canary literal in its `extra` vendor-fields map, then
//!    exercises every Metadata/* handler path (get / changes / set
//!    create-update-destroy / query / queryChanges). The assertion is
//!    that the canary literal does not appear in any captured tracing
//!    output. The test fails the moment a future contributor adds
//!    `tracing::info!(record = ?stored_obj, ...)` or any similar shape
//!    that interpolates a `Metadata` value's `Debug` form.
//!
//! Unlike `crate-jmap-base-client`'s log-redaction tests — which assert
//! that the type-level `Debug` impl redacts secrets — `Metadata` and its
//! sub-types have **standard derived `Debug`** that DOES interpolate
//! `extra` content verbatim. This test does not (and cannot) verify
//! type-level redaction; it verifies that the crate's own code does not
//! interpolate that `Debug` into log output. When `tracing::*` is added,
//! the contributor must either (a) use field selection that excludes
//! `extra`, (b) wrap the value in a redacting newtype, or (c) avoid
//! emitting at INFO level. The canary test ensures the violation is
//! visible in CI rather than shipping to production.
//!
//! ## Scope and limits
//!
//! - **In scope**: assert no canary literal appears in `tracing::*`
//!   captured during handler dispatch.
//! - **Out of scope**: assert that `Metadata::Debug` itself redacts.
//!   It does not, and changing that would be a workspace-wide policy
//!   shift well beyond this bead.
//! - **Workspace-canonical-template note**: `jmap-mail-server` does
//!   not have an equivalent canary test (the bead's analysis notes the
//!   gap). Email body content has a weaker spec-level confidentiality
//!   contract than `isPrivate` metadata, so this test stays
//!   metadata-server-specific for now. A workspace-wide sweep would
//!   land the canary on every extension-server `*-server` crate.

mod common;

use std::sync::Arc;

use common::{log_capture::LogCapture, MemoryBackend};
use jmap_metadata_server::{register_metadata_handlers, MetadataBackend, JMAP_METADATA_URI};
use jmap_metadata_types::Metadata;
use jmap_server::{Dispatcher, JmapRequest, State};
use jmap_types::Id;
use serde_json::json;

// ---------------------------------------------------------------------------
// Harness self-tests
// ---------------------------------------------------------------------------

/// Sanity check: the [`LogCapture`] harness actually captures the
/// output of `tracing::*` calls emitted while it is installed.
///
/// Without this self-test, the negative-assertion tests below would be
/// vacuously true if [`LogCapture::new`] silently failed to install
/// the subscriber.
#[test]
fn log_capture_captures_traced_output() {
    let capture = LogCapture::new();
    tracing::info!("sentinel-message-metadata-826m-53 emitted by harness self-test");
    capture.assert_contains("sentinel-message-metadata-826m-53");
}

/// Negative control: prove the harness can actually catch a leak.
///
/// Uses a locally-defined type with a derived `Debug` impl (no
/// redaction) to model what a buggy contributor would add. Verifies
/// that emitting it through `tracing::info!(field = ?leaky, ...)`
/// causes the canary literal to appear in captured output. Without
/// this test the positive `assert_does_not_contain` calls in the
/// metadata canary below could pass vacuously for a reason other
/// than the absence of leaks (e.g., tracing not being wired up).
#[test]
fn log_capture_detects_a_leak_when_one_exists() {
    // The inner `&str` is only read through the derived `Debug` impl;
    // clippy excludes Debug from dead-code analysis, so without
    // `#[expect(dead_code)]` the field would be flagged.
    #[derive(Debug)]
    struct Leaky(#[expect(dead_code, reason = "consumed only via Debug formatting")] &'static str);

    const CANARY: &str = "CANARY-NEGATIVE-CONTROL-LEAK-METADATA-826m";
    let capture = LogCapture::new();
    let leaky = Leaky(CANARY);

    tracing::info!(secret = ?leaky, "leaky type formatted via Debug");

    let contents = capture.contents();
    assert!(
        contents.contains(CANARY),
        "negative control: expected the canary literal to appear in captured output; \
         if this fires, the harness is not capturing Debug-formatted args and the \
         positive assertions below pass vacuously. Got:\n{contents}"
    );
}

// ---------------------------------------------------------------------------
// Metadata canary
// ---------------------------------------------------------------------------

/// Build a minimal [`JmapRequest`] with a single method call carrying the
/// Metadata capability URI.
fn single_call(method: &str, args: serde_json::Value, call_id: &str) -> JmapRequest {
    JmapRequest::new(
        vec![JMAP_METADATA_URI.into()],
        vec![(method.into(), args, call_id.into())],
        None,
    )
}

/// Tripwire: when a future contributor adds `tracing::*` instrumentation
/// to a method handler or backend method that interpolates a `Metadata`
/// value via `?` or `{:?}`, the per-user-private `extra` vendor-fields
/// content leaks into log output. This test seeds a canary literal in
/// the `extra` map, exercises every handler path that touches the
/// stored record (`/get`, `/changes`, `/set` create/update/destroy,
/// `/query`, `/queryChanges`), and asserts the canary does not appear
/// in any captured `tracing::*` event.
///
/// Today this test trivially passes because the crate has zero
/// `tracing::*` call sites in `src/`. The value is forward-looking:
/// the test is a tripwire that fires the moment a contributor adds a
/// leaky `tracing::info!(rec = ?stored_obj, ...)`-shaped call.
///
/// See the file-level doc comment for the full rationale and the
/// canonical pattern source (`crate-jmap-base-client/tests/log_redaction.rs`).
#[tokio::test]
async fn metadata_extra_never_appears_in_handler_path_logs() {
    const CANARY: &str = "REDACTION-CANARY-METADATA-826m-53-DO-NOT-LEAK";

    let capture = LogCapture::new();

    // Seed an Annotation carrying the canary in a vendor extra field
    // and in a freeform metadata payload. Both shapes are the kinds of
    // per-user-private content a future `tracing::info!(rec = ?obj)`
    // would interpolate verbatim.
    let backend = Arc::new(MemoryBackend::new_with_accounts(&["acc1"]));
    let seed = Metadata::deserialize_for_test(json!({
        "@type": "Annotation",
        "relatedType": "Email",
        "relatedId": "EM1",
        "isPrivate": true,
        "acme.example.com:secretComment": CANARY,
    }));
    let (created_id, _) = backend
        .create_object::<Metadata>(&(), &Id::from("acc1"), "seed", seed)
        .await
        .expect("seed create_object must succeed");

    // Exercise every read and write handler path the future-leak risk
    // analysis (bd:JMAP-826m.53) names: /get, /changes, /set
    // create/update/destroy, /query, /queryChanges.
    let mut dispatcher: Dispatcher<()> = Dispatcher::new();
    register_metadata_handlers(&mut dispatcher, Arc::clone(&backend));

    // /get — typical read path.
    let req = single_call(
        "Metadata/get",
        json!({"accountId": "acc1", "ids": null}),
        "c0",
    );
    let _ = dispatcher.dispatch(req, (), State::from("s0")).await;

    // /changes — change-log walk.
    let req = single_call(
        "Metadata/changes",
        json!({"accountId": "acc1", "sinceState": "0"}),
        "c1",
    );
    let _ = dispatcher.dispatch(req, (), State::from("s1")).await;

    // /set update — patches against the seeded id. Patch value also
    // carries the canary so the update path with a wire-supplied
    // canary literal is exercised.
    let req = single_call(
        "Metadata/set",
        json!({
            "accountId": "acc1",
            "update": {
                created_id.as_ref(): {
                    "acme.example.com:secretComment": CANARY,
                }
            }
        }),
        "c2",
    );
    let _ = dispatcher.dispatch(req, (), State::from("s2")).await;

    // /query — filter by relatedType so the filter validator path
    // is exercised too.
    let req = single_call(
        "Metadata/query",
        json!({"accountId": "acc1", "filter": {"relatedType": "Email"}}),
        "c3",
    );
    let _ = dispatcher.dispatch(req, (), State::from("s3")).await;

    // /queryChanges — same shape but the changes oracle.
    let req = single_call(
        "Metadata/queryChanges",
        json!({
            "accountId": "acc1",
            "filter": {"relatedType": "Email"},
            "sinceQueryState": "0"
        }),
        "c4",
    );
    let _ = dispatcher.dispatch(req, (), State::from("s4")).await;

    // /set destroy — destroy path.
    let req = single_call(
        "Metadata/set",
        json!({"accountId": "acc1", "destroy": [created_id.as_ref()]}),
        "c5",
    );
    let _ = dispatcher.dispatch(req, (), State::from("s5")).await;

    // /set create with a fresh canary — the create path with a
    // wire-supplied canary literal.
    let req = single_call(
        "Metadata/set",
        json!({
            "accountId": "acc1",
            "create": {
                "new1": {
                    "@type": "Annotation",
                    "relatedType": "Email",
                    "relatedId": "EM2",
                    "isPrivate": true,
                    "acme.example.com:secretComment": CANARY,
                }
            }
        }),
        "c6",
    );
    let _ = dispatcher.dispatch(req, (), State::from("s6")).await;

    capture.assert_does_not_contain(CANARY);
}

/// Helper alias so the canary test reads naturally. `Metadata` does not
/// itself expose a `deserialize_for_test` constructor; this trait-style
/// helper wraps `serde_json::from_value` with a panic-on-error contract
/// for test fixtures.
trait MetadataForTest {
    fn deserialize_for_test(v: serde_json::Value) -> Self;
}

impl MetadataForTest for Metadata {
    fn deserialize_for_test(v: serde_json::Value) -> Self {
        serde_json::from_value(v).expect("test fixture must deserialize as Metadata")
    }
}
