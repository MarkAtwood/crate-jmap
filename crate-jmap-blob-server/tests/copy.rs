//! RFC 8620 §6.3 `Blob/copy` through the dispatcher over the memory
//! backend (bd JMAP-tna1.2.14).
//!
//! Oracle: RFC 8620 §6.3: "Binary data may be copied *between* two
//! different accounts"; `copied` is "A map of the blobId in the
//! fromAccount to the id for the blob in the account it was copied to, or
//! null if none were successfully copied"; `notCopied` "or null if none";
//! "the "notFound" SetError error may be returned if the blobId to be
//! copied cannot be found"; method-level "fromAccountNotFound". §5.4: the
//! target account "MUST be different to the "fromAccountId"". §5.3:
//! creation ids may stand for ids created earlier in the request.

use std::sync::Arc;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::register_blob_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const BLOB2: &str = "urn:ietf:params:jmap:blob2";

fn backend() -> Arc<MemoryBackend> {
    Arc::new(
        MemoryBackend::new()
            .with_account(&Id::from("A1"))
            .with_account(&Id::from("A2")),
    )
}

async fn dispatch(b: Arc<MemoryBackend>, using: &[&str], calls: Value) -> Vec<Value> {
    let mut d = Dispatcher::new();
    register_blob_handlers(&mut d, b);
    let req: JmapRequest =
        serde_json::from_value(json!({"using": using, "methodCalls": calls})).unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    resp["methodResponses"].as_array().unwrap().clone()
}

/// The arguments of response `i`, which must be named `name`.
fn args<'a>(resp: &'a [Value], i: usize, name: &str) -> &'a Value {
    assert_eq!(resp[i][0], name, "{}", resp[i]);
    &resp[i][1]
}

/// Blob/get of `id` in A2 (a later request on the same store).
async fn get_in_a2(b: &Arc<MemoryBackend>, id: &Value) -> Value {
    let resp = dispatch(
        Arc::clone(b),
        &[CORE, BLOB2],
        json!([["Blob/get", {"accountId": "A2", "ids": [id],
            "properties": ["data:asText", "size"]}, "0"]]),
    )
    .await;
    args(&resp, 0, "Blob/get")["list"][0].clone()
}

/// A blob created in A1 (one inline source and a range of it, so the
/// memory backend stores it as chunks), copied to A2 by `#creationId`,
/// reads back the same octets and type there; a missing blob and an
/// unknown reference are `notFound`; a repeated id is copied once.
#[tokio::test]
async fn copies_between_accounts_and_reports_missing_blobs() {
    let b = backend();
    let resp = dispatch(
        Arc::clone(&b),
        &[CORE, BLOB2],
        json!([
            ["Blob/set", {"accountId": "A1", "create": {
                "leaf": {"data": [{"data:asText": "Hello, "}]},
                "b": {"data": [{"blobId": "#leaf"}, {"data:asText": "world!"}],
                      "type": "text/plain"}
            }}, "0"],
            ["Blob/copy", {"fromAccountId": "A1", "accountId": "A2",
                "blobIds": ["#b", "#b", "missing", "#nope"]}, "1"],
            ["Blob/get", {"accountId": "A1", "ids": ["#b"],
                "properties": ["data:asText"]}, "2"]
        ]),
    )
    .await;
    let copy = args(&resp, 1, "Blob/copy");
    assert_eq!(copy["fromAccountId"], "A1");
    assert_eq!(copy["accountId"], "A2");
    let copied = copy["copied"].as_object().unwrap();
    assert_eq!(copied.keys().collect::<Vec<_>>(), ["#b"], "{copy}");
    let source = args(&resp, 2, "Blob/get")["list"][0]["id"].clone();
    assert_ne!(copied["#b"], source, "a new id in the target account");
    for key in ["missing", "#nope"] {
        assert_eq!(copy["notCopied"][key]["type"], "notFound", "{copy}");
    }
    let got = get_in_a2(&b, &copied["#b"]).await;
    assert_eq!(got["id"], copied["#b"]);
    assert_eq!(got["data:asText"], "Hello, world!");
    assert_eq!(got["size"], 13);
}

/// Blob/copy is core: `using` with only core is enough, and both maps are
/// `null` when empty.
#[tokio::test]
async fn needs_only_core_and_writes_empty_maps_as_null() {
    let resp = dispatch(
        backend(),
        &[CORE],
        json!([["Blob/copy", {"fromAccountId": "A1", "accountId": "A2",
            "blobIds": []}, "0"]]),
    )
    .await;
    let copy = args(&resp, 0, "Blob/copy");
    assert_eq!(copy["copied"], Value::Null, "{copy}");
    assert_eq!(copy["notCopied"], Value::Null, "{copy}");
}

/// The copy is independent of its source: destroying the source leaves
/// it readable.
#[tokio::test]
async fn the_copy_outlives_its_source() {
    let b = backend();
    let resp = dispatch(
        Arc::clone(&b),
        &[CORE, BLOB2],
        json!([
            ["Blob/set", {"accountId": "A1", "create": {
                "b": {"data": [{"data:asText": "x"}]}}}, "0"],
            ["Blob/copy", {"fromAccountId": "A1", "accountId": "A2",
                "blobIds": ["#b"]}, "1"],
            ["Blob/set", {"accountId": "A1", "destroy": ["#b"]}, "2"]
        ]),
    )
    .await;
    assert_eq!(
        args(&resp, 2, "Blob/set")["destroyed"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let copy = args(&resp, 1, "Blob/copy")["copied"]["#b"].clone();
    assert_eq!(get_in_a2(&b, &copy).await["data:asText"], "x");
}

async fn method_error(from: &str, to: &str, blob_ids: Value) -> String {
    let resp = dispatch(
        backend(),
        &[CORE],
        json!([["Blob/copy", {"fromAccountId": from, "accountId": to,
            "blobIds": blob_ids}, "0"]]),
    )
    .await;
    args(&resp, 0, "error")["type"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn method_level_errors() {
    // §6.3: "fromAccountNotFound"; RFC 8620 §3.6.2 accountNotFound.
    assert_eq!(
        method_error("Z9", "A2", json!([])).await,
        "fromAccountNotFound"
    );
    assert_eq!(method_error("A1", "Z9", json!([])).await, "accountNotFound");
    // §6.3 "*between* two different accounts"; §5.4 "MUST be different".
    assert_eq!(
        method_error("A1", "A1", json!(["b"])).await,
        "invalidArguments"
    );
    assert_eq!(
        method_error("A1", "A2", json!("b")).await,
        "invalidArguments"
    );
    assert_eq!(
        method_error("A1", "A2", json!([1])).await,
        "invalidArguments"
    );
}
