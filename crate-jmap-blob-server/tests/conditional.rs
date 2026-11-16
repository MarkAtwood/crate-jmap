//! Conditional Blob/set (draft-ietf-jmap-conditional-02) through
//! `register_blob_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-tna1.2.12; docs/design/blobext.md §9, decisions 1, 2,
//! 17 and 19).
//!
//! Oracles: the conditional-02 rules and the blob's own content. The
//! seeded blob is "Hello, world!" (13 octets); its `digest:sha-256` was
//! computed outside the crate (Python `hashlib`).

use std::sync::Arc;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::{register_blob_conditional, register_blob_handlers};
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const CONDITIONAL: &str = "urn:ietf:params:jmap:conditional";
const HELLO_SHA256: &str = "MV9b23bQeMQ7isAGTkoBZGErH853yGk0W/yUx1iU7dM=";

fn account() -> Id {
    Id::new_validated("a1").unwrap()
}

/// Dispatchers with the blob handlers; `conditional` adds the opt-in.
fn dispatcher(b: &Arc<MemoryBackend>, conditional: bool) -> Dispatcher<()> {
    let mut d = Dispatcher::new();
    register_blob_handlers(&mut d, Arc::clone(b));
    if conditional {
        register_blob_conditional(&mut d, Arc::clone(b)).unwrap();
    }
    d
}

/// A backend holding B1 = "Hello, world!".
async fn seeded() -> Arc<MemoryBackend> {
    let b = Arc::new(MemoryBackend::new().with_account(&account()));
    let r = call(
        &dispatcher(&b, true),
        &[CORE, BLOB2],
        json!(["Blob/set", {"accountId": "a1", "create": {
            "c1": {"data": [{"data:asText": "Hello, world!"}]}
        }}, "0"]),
    )
    .await;
    assert_eq!(r.1["created"]["c1"]["id"], "B1", "{r:?}");
    assert_eq!(r.1["created"]["c1"]["size"], 13);
    b
}

/// Run one call; its response name and arguments.
async fn call(d: &Dispatcher<()>, using: &[&str], call: Value) -> (String, Value) {
    let req: JmapRequest =
        serde_json::from_value(json!({"using": using, "methodCalls": [call]})).unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// Blob/set destroying B1 under `if_unchanged_by`.
fn destroy_b1_if(if_unchanged_by: Value) -> Value {
    json!(["Blob/set", {
        "accountId": "a1",
        "destroy": ["B1"],
        "ifUnchangedBy": if_unchanged_by
    }, "1"])
}

/// Which of `ids` Blob/get still finds.
async fn existing(b: &Arc<MemoryBackend>, ids: &[&str]) -> Vec<String> {
    let (_, r) = call(
        &dispatcher(b, false),
        &[CORE, BLOB2],
        json!(["Blob/get", {"accountId": "a1", "ids": ids, "properties": ["size"]}, "g"]),
    )
    .await;
    r["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn size_precondition_passes_and_fails() {
    let b = seeded().await;
    let d = dispatcher(&b, true);
    let using = [CORE, BLOB2, CONDITIONAL];

    let (name, r) = call(&d, &using, destroy_b1_if(json!({"B1": {"size": 14}}))).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["B1"]}))
    );
    assert_eq!(existing(&b, &["B1"]).await, ["B1"], "not destroyed");

    let (name, r) = call(&d, &using, destroy_b1_if(json!({"B1": {"size": 13}}))).await;
    assert_eq!(name, "Blob/set", "{r:?}");
    assert_eq!(r["destroyed"], json!(["B1"]));
    assert!(existing(&b, &["B1"]).await.is_empty());
}

/// `null` holds only when the object does not exist; `{}` only when it
/// does (conditional-02 §3.1).
#[tokio::test]
async fn null_and_empty_preconditions() {
    let b = seeded().await;
    let d = dispatcher(&b, true);
    let using = [CORE, BLOB2, CONDITIONAL];

    let (name, r) = call(&d, &using, destroy_b1_if(json!({"B1": null}))).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["B1"]}))
    );

    // B9 does not exist: `null` holds for it, alongside `{}` for B1.
    let (name, r) = call(&d, &using, destroy_b1_if(json!({"B1": {}, "B9": null}))).await;
    assert_eq!(name, "Blob/set", "{r:?}");
    assert_eq!(r["destroyed"], json!(["B1"]));

    // Now B1 is gone, so `{}` fails.
    let (name, r) = call(&d, &using, destroy_b1_if(json!({"B1": {}}))).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["B1"]}))
    );
}

#[tokio::test]
async fn digest_precondition() {
    let b = seeded().await;
    let d = dispatcher(&b, true);
    let using = [CORE, BLOB2, CONDITIONAL];

    let stale = json!({"B1": {"digest:sha-256": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}});
    let (name, r) = call(&d, &using, destroy_b1_if(stale)).await;
    assert_eq!(
        (name.as_str(), r),
        ("error", json!({"type": "stateMismatch", "failed": ["B1"]}))
    );

    let fresh = json!({"B1": {"digest:sha-256": HELLO_SHA256}});
    let (name, r) = call(&d, &using, destroy_b1_if(fresh)).await;
    assert_eq!(name, "Blob/set", "{r:?}");
    assert_eq!(r["destroyed"], json!(["B1"]));
}

/// Decision 19: Blob/get never returns `type` or `expires`, so pointers
/// to them are `invalidArguments`, not `forbidden`, and nothing changes.
#[tokio::test]
async fn type_and_expires_pointers_are_invalid_arguments() {
    let b = seeded().await;
    let d = dispatcher(&b, true);
    let using = [CORE, BLOB2, CONDITIONAL];
    for pre in [
        json!({"B1": {"type": "text/plain"}}),
        json!({"B1": {"expires": null}}),
    ] {
        let (name, r) = call(&d, &using, destroy_b1_if(pre.clone())).await;
        assert_eq!(name, "error", "{pre}: {r:?}");
        assert_eq!(r["type"], "invalidArguments", "{pre}: {r:?}");
    }
    assert_eq!(existing(&b, &["B1"]).await, ["B1"]);
}

/// An atomic Blob/set with one failing create creates nothing: the
/// created blob (a `noPersist` one) and the id counter roll back.
#[tokio::test]
async fn atomic_failure_rolls_back_created_blobs() {
    let b = seeded().await;
    let d = dispatcher(&b, true);
    let create = json!({
        "c1": {"data": [{"data:asText": "kept?"}], "noPersist": true},
        "c2": {"data": [{"blobId": "B9"}]}
    });

    let (name, r) = call(
        &d,
        &[CORE, BLOB2, CONDITIONAL],
        json!(["Blob/set", {"accountId": "a1", "create": create, "atomic": true}, "1"]),
    )
    .await;
    assert_eq!(name, "error", "{r:?}");
    assert_eq!(r["type"], "atomicFailure", "{r:?}");
    let not_created = r["notCreated"].as_object().unwrap();
    assert_eq!(not_created.keys().collect::<Vec<_>>(), ["c2"], "{r:?}");
    assert!(existing(&b, &["B2"]).await.is_empty(), "c1 rolled back");

    // The same call without `atomic` keeps c1; it gets B2 again because
    // the rollback restored the id counter too.
    let (name, r) = call(
        &d,
        &[CORE, BLOB2, CONDITIONAL],
        json!(["Blob/set", {"accountId": "a1", "create": create}, "2"]),
    )
    .await;
    assert_eq!(name, "Blob/set", "{r:?}");
    assert_eq!(r["created"]["c1"]["id"], "B2");
    assert!(r["notCreated"]["c2"].is_object());
}

#[tokio::test]
async fn atomic_success_commits_every_change() {
    let b = seeded().await;
    let d = dispatcher(&b, true);
    let (name, r) = call(
        &d,
        &[CORE, BLOB2, CONDITIONAL],
        json!(["Blob/set", {
            "accountId": "a1",
            "create": {"c1": {"data": [{"data:asText": "new"}]}},
            "destroy": ["B1"],
            "ifUnchangedBy": {"B1": {"size": 13}},
            "atomic": true
        }, "1"]),
    )
    .await;
    assert_eq!(name, "Blob/set", "{r:?}");
    assert_eq!(r["created"]["c1"]["id"], "B2");
    assert_eq!(r["destroyed"], json!(["B1"]));
    assert_eq!(existing(&b, &["B1", "B2"]).await, ["B2"]);
}

/// Decision 2: without the conditional URI, non-default conditional
/// arguments are `invalidArguments`, opted in or not; defaults pass.
#[tokio::test]
async fn conditional_arguments_without_the_uri() {
    let b = seeded().await;
    for opted_in in [false, true] {
        let d = dispatcher(&b, opted_in);
        for args in [
            json!({"atomic": true}),
            json!({"ifUnchangedBy": {"B1": {}}}),
        ] {
            let mut set = json!({"accountId": "a1", "destroy": ["B1"]});
            set.as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            let (name, r) = call(&d, &[CORE, BLOB2], json!(["Blob/set", set, "1"])).await;
            assert_eq!(
                (name.as_str(), &r["type"]),
                ("error", &json!("invalidArguments")),
                "opted_in={opted_in} {args}: {r:?}"
            );
        }
    }
    assert_eq!(existing(&b, &["B1"]).await, ["B1"]);

    let d = dispatcher(&b, false);
    let (name, r) = call(
        &d,
        &[CORE, BLOB2],
        json!(["Blob/set", {
            "accountId": "a1", "destroy": ["B1"], "atomic": false, "ifUnchangedBy": null
        }, "1"]),
    )
    .await;
    assert_eq!(name, "Blob/set", "{r:?}");
    assert_eq!(r["destroyed"], json!(["B1"]));
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = Arc::new(MemoryBackend::new());
    let plain = dispatcher(&b, false);
    assert_eq!(plain.conditional_gaps(), ["Blob/set"]);
    assert!(!plain.conditional_ready());

    let d = dispatcher(&b, true);
    assert!(d.conditional_gaps().is_empty());
    assert!(d.conditional_ready());

    // Before the handlers, there is no Blob/set to attach to.
    let mut empty = Dispatcher::new();
    assert!(register_blob_conditional(&mut empty, b).is_err());
}
