//! `Blob/set` (blobext-03 §3, §4, §7) against the memory backend: every
//! case in docs/design/blobext.md §10 "server, Blob/set".
//!
//! Oracles: request and response bodies are hand-written from the spec;
//! digests were computed outside the code under test
//! (`printf '%s' … | openssl dgst -sha256 -binary | base64`); expiry times
//! follow the memory backend's documented rules (one hour ahead, touches
//! honoured up to seven days) from a fixed clock; ids follow its `B` +
//! counter scheme.

use std::collections::BTreeMap;
use std::sync::Arc;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::{handle_blob_set, register_blob_handlers};
use jmap_blob_types::BlobAccountCapability;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapError, JmapRequest, State};
use serde_json::{Value, json};

const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const BLOB: &str = "urn:ietf:params:jmap:blob";

/// 2025-11-15T00:00:00Z.
const NOW: i64 = 1_763_164_800;
const NOW_PLUS_1H: &str = "2025-11-15T01:00:00Z";

/// sha-256("Hello, world!").
const HELLO_SHA256: &str = "MV9b23bQeMQ7isAGTkoBZGErH853yGk0W/yUx1iU7dM=";
/// sha-256("world!").
const WORLD_SHA256: &str = "cR6WCTOekrA93AohGCfbpCHzj57YudgG4f/djBX/oD0=";

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

fn backend() -> MemoryBackend {
    let b = MemoryBackend::new().with_account(&id("a1"));
    b.set_clock_for_test(NOW);
    b
}

fn using(uris: &[&str]) -> Vec<String> {
    let mut v = vec!["urn:ietf:params:jmap:core".to_owned()];
    v.extend(uris.iter().map(|u| (*u).to_owned()));
    v
}

async fn call_with(
    b: &MemoryBackend,
    args: Value,
    created: &BTreeMap<Id, Id>,
    uris: &[&str],
) -> Result<Value, JmapError> {
    handle_blob_set(b, &(), args, created, &using(uris))
        .await
        .map(|out| out.primary)
}

async fn set(b: &MemoryBackend, args: Value) -> Value {
    call_with(b, args, &BTreeMap::new(), &[BLOB2])
        .await
        .unwrap()
}

/// Create one blob from `data` and return the response.
async fn create(b: &MemoryBackend, entry: Value) -> Value {
    set(b, json!({"accountId": "a1", "create": {"c": entry}})).await
}

/// The `notCreated["c"]` error of a single-entry create.
async fn create_error(b: &MemoryBackend, entry: Value) -> Value {
    let resp = create(b, entry).await;
    assert_eq!(resp["created"], Value::Null, "{resp}");
    resp["notCreated"]["c"].clone()
}

async fn text_blob(b: &MemoryBackend, text: &str) -> String {
    let resp = create(b, json!({"data": [{"data:asText": text}]})).await;
    resp["created"]["c"]["id"].as_str().unwrap().to_owned()
}

/// Run a whole request through a dispatcher with the blob handlers.
async fn dispatch(b: Arc<MemoryBackend>, request: Value) -> Value {
    let mut d = Dispatcher::new();
    register_blob_handlers(&mut d, b);
    let req: JmapRequest = serde_json::from_value(request).unwrap();
    serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap()
}

fn error_type(r: Result<Value, JmapError>) -> String {
    match r {
        Err(e) => serde_json::to_value(&e).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned(),
        Ok(v) => panic!("expected an error, got {v}"),
    }
}

fn invalid(props: &[&str]) -> Value {
    json!({"type": "invalidProperties", "properties": props})
}

/// The type and properties of a SetError, without its description.
fn kind(e: &Value) -> Value {
    let mut e = e.as_object().unwrap().clone();
    e.remove("description");
    e.remove("notFound");
    e.remove("maxSize");
    Value::Object(e)
}

// ---- create --------------------------------------------------------------

/// blobext-03 §9.1, with the minted id, the null states and the
/// `expires` §7 requires (gap a).
#[tokio::test]
async fn section_9_1_inline_upload() {
    let b = backend();
    let resp = set(
        &b,
        json!({
            "accountId": "a1",
            "create": {
                "b1": {
                    "data": [{"data:asText": "Hello, world!"}],
                    "type": "text/plain"
                }
            }
        }),
    )
    .await;
    assert_eq!(
        resp,
        json!({
            "accountId": "a1",
            "oldState": null,
            "newState": null,
            "created": {
                "b1": {
                    "id": "B1",
                    "type": "text/plain",
                    "size": 13,
                    "expires": NOW_PLUS_1H
                }
            },
            "updated": null,
            "destroyed": null,
            "notCreated": null,
            "notUpdated": null,
            "notDestroyed": null
        })
    );
}

#[tokio::test]
async fn non_null_if_in_state_is_invalid_arguments() {
    let b = backend();
    let r = call_with(
        &b,
        json!({"accountId": "a1", "ifInState": "s1", "create": {}}),
        &BTreeMap::new(),
        &[BLOB2],
    )
    .await;
    assert_eq!(error_type(r), "invalidArguments");
    // null is accepted.
    let resp = set(&b, json!({"accountId": "a1", "ifInState": null})).await;
    assert_eq!(resp["created"], Value::Null);
}

#[tokio::test]
async fn both_blob_capabilities_is_invalid_arguments() {
    let b = backend();
    let r = call_with(
        &b,
        json!({"accountId": "a1"}),
        &BTreeMap::new(),
        &[BLOB, BLOB2],
    )
    .await;
    assert_eq!(error_type(r), "invalidArguments");
}

/// One test per §3 cause of an invalid DataSourceObject: each is
/// `invalidProperties` `["data"]` (the server "MUST NOT guess").
#[tokio::test]
async fn invalid_data_sources_are_invalid_properties_data() {
    let b = backend();
    let ten = text_blob(&b, "0123456789").await;
    let cases = [
        (
            "two data keys",
            json!([{"data:asText": "a", "data:asBase64": "YQ=="}]),
        ),
        (
            "text and blobId",
            json!([{"data:asText": "a", "blobId": ten}]),
        ),
        ("no data key", json!([{"offset": 0}])),
        (
            "all keys null (gap f)",
            json!([{"data:asText": null, "blobId": null}]),
        ),
        ("bad base64", json!([{"data:asBase64": "not base64!"}])),
        ("unpadded base64", json!([{"data:asBase64": "YQ"}])),
        ("offset past end", json!([{"blobId": ten, "offset": 11}])),
        (
            "length past end",
            json!([{"blobId": ten, "offset": 5, "length": 6}]),
        ),
        (
            "inline length past end",
            json!([{"data:asText": "abc", "length": 4}]),
        ),
        ("wrong size", json!([{"blobId": ten, "size": 9}])),
        (
            "wrong position",
            json!([{"data:asText": "ab"}, {"data:asText": "cd", "position": 1}]),
        ),
        (
            "wrong source digest",
            json!([{"data:asText": "Hello, world!", "digest:sha-256": "AAAA"}]),
        ),
        (
            "unadvertised source digest",
            json!([{"data:asText": "x", "digest:md5": "AAAA"}]),
        ),
        (
            "unknown source property",
            json!([{"data:asText": "x", "colour": 1}]),
        ),
        ("source not an object", json!(["x"])),
    ];
    for (why, data) in cases {
        let e = create_error(&b, json!({"data": data})).await;
        assert_eq!(kind(&e), invalid(&["data"]), "{why}: {e}");
    }
}

#[tokio::test]
async fn valid_data_source_checks_pass() {
    let b = backend();
    let ten = text_blob(&b, "0123456789").await;
    let resp = create(
        &b,
        json!({"data": [
            {"data:asText": "ab", "position": 0},
            {"blobId": ten, "offset": 2, "length": 3, "size": 10, "position": 2},
            // offset = size and a null length: zero octets, not past the end.
            {"blobId": ten, "offset": 10, "position": 5},
            // The source digest covers the contributed octets, "world!".
            {"data:asBase64": "SGVsbG8sIHdvcmxkIQ==", "offset": 7,
             "digest:sha-256": WORLD_SHA256}
        ]}),
    )
    .await;
    let new = resp["created"]["c"]["id"].as_str().unwrap();
    assert_eq!(resp["created"]["c"]["size"], 11, "{resp}");
    let got = jmap_blob_server::handle_blob_get(
        &b,
        &(),
        json!({"accountId": "a1", "ids": [new]}),
        &BTreeMap::new(),
        &using(&[BLOB2]),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(got["list"][0]["data:asText"], "ab234world!");
}

#[tokio::test]
async fn missing_source_blob_is_not_found() {
    let b = backend();
    let e = create_error(&b, json!({"data": [{"blobId": "nope"}]})).await;
    assert_eq!(e["type"], "notFound");
    // A #ref with no creation anywhere (gap e).
    let e = create_error(&b, json!({"data": [{"blobId": "#ghost"}]})).await;
    assert_eq!(e["type"], "notFound");
}

#[tokio::test]
async fn too_many_sources_is_too_large() {
    let b = backend();
    let data: Vec<Value> = (0..65).map(|_| json!({"data:asText": "x"})).collect();
    let e = create_error(&b, json!({"data": data})).await;
    assert_eq!(e["type"], "tooLarge");
    // 64 is the advertised maxDataSources.
    let data: Vec<Value> = (0..64).map(|_| json!({"data:asText": "x"})).collect();
    let resp = create(&b, json!({"data": data})).await;
    assert_eq!(resp["created"]["c"]["size"], 64);
}

#[tokio::test]
async fn over_max_size_blob_set_is_too_large() {
    let b = backend();
    let mut cap: BlobAccountCapability = MemoryBackend::default_capability();
    cap.max_size_blob_set = Some(4);
    b.advertise_for_test(&id("a1"), cap);
    let e = create_error(&b, json!({"data": [{"data:asText": "12345"}]})).await;
    assert_eq!(e["type"], "tooLarge");
    assert_eq!(e["maxSize"], 4);
    let resp = create(&b, json!({"data": [{"data:asText": "1234"}]})).await;
    assert_eq!(resp["created"]["c"]["size"], 4);
}

/// A whole `size` or digest mismatch rejects the entry and creates no
/// blob: the next blob gets the next id in sequence.
#[tokio::test]
async fn whole_size_and_digest_mismatch_create_nothing() {
    let b = backend();
    let e = create_error(
        &b,
        json!({"data": [{"data:asText": "Hello, world!"}], "size": 12}),
    )
    .await;
    assert_eq!(kind(&e), invalid(&["size"]));
    let e = create_error(
        &b,
        json!({"data": [{"data:asText": "Hello, world!"}],
               "digest:sha-256": "8U8obKQ10fo7nYBB6PBqoK96so6o7c1+Ef1IWhALYys="}),
    )
    .await;
    assert_eq!(kind(&e), invalid(&["digest:sha-256"]));
    // The matching size and digest pass, and the blob is the first one.
    let resp = create(
        &b,
        json!({"data": [{"data:asText": "Hello, "}, {"data:asText": "world!"}],
               "size": 13, "digest:sha-256": HELLO_SHA256}),
    )
    .await;
    assert_eq!(resp["created"]["c"]["id"], "B1", "{resp}");
}

#[tokio::test]
async fn unknown_and_unadvertised_create_properties() {
    let b = backend();
    let e = create_error(&b, json!({"data": [], "colour": "red"})).await;
    assert_eq!(kind(&e), invalid(&["colour"]));
    let e = create_error(&b, json!({"data": [], "digest:md5": "AAAA"})).await;
    assert_eq!(kind(&e), invalid(&["digest:md5"]));
    let e = create_error(&b, json!({"data": [], "size": "13"})).await;
    assert_eq!(kind(&e), invalid(&["size"]));
    let e = create_error(&b, json!({"type": "text/plain"})).await;
    assert_eq!(kind(&e), invalid(&["data"]));
    // Null optional properties are their defaults.
    let resp = create(
        &b,
        json!({"data": [], "type": null, "size": null, "noPersist": null,
               "digest:md5": null}),
    )
    .await;
    assert_eq!(resp["created"]["c"]["size"], 0);
}

#[tokio::test]
async fn zero_sources_make_an_empty_blob() {
    let b = backend();
    let resp = create(&b, json!({"data": []})).await;
    assert_eq!(
        resp["created"]["c"],
        json!({"id": "B1", "type": null, "size": 0, "expires": NOW_PLUS_1H})
    );
}

/// `a` names `#b`, which sorts after it: `b` is created first.
#[tokio::test]
async fn ref_within_the_call_in_reverse_map_order() {
    let b = backend();
    let resp = set(
        &b,
        json!({"accountId": "a1", "create": {
            "a": {"data": [{"blobId": "#b", "offset": 1}]},
            "b": {"data": [{"data:asText": "xyz"}]}
        }}),
    )
    .await;
    assert_eq!(resp["created"]["b"]["id"], "B1", "{resp}");
    assert_eq!(resp["created"]["a"]["id"], "B2");
    assert_eq!(resp["created"]["a"]["size"], 2);
}

#[tokio::test]
async fn ref_to_a_failed_creation_is_not_found() {
    let b = backend();
    let mut earlier = BTreeMap::new();
    // An earlier call's `b` must not stand in for this call's failed one.
    earlier.insert(id("b"), id("B9"));
    let resp = call_with(
        &b,
        json!({"accountId": "a1", "create": {
            "a": {"data": [{"blobId": "#b"}]},
            "b": {"data": [{"data:asBase64": "!"}]}
        }}),
        &earlier,
        &[BLOB2],
    )
    .await
    .unwrap();
    assert_eq!(resp["notCreated"]["a"]["type"], "notFound", "{resp}");
    assert_eq!(kind(&resp["notCreated"]["b"]), invalid(&["data"]));
}

#[tokio::test]
async fn ref_across_calls() {
    let b = Arc::new(backend());
    let resp = dispatch(
        Arc::clone(&b),
        json!({
            "using": using(&[BLOB2]),
            "methodCalls": [
                ["Blob/set", {"accountId": "a1", "create": {
                    "b1": {"data": [{"data:asText": "Hello, "}]}
                }}, "0"],
                ["Blob/set", {"accountId": "a1", "create": {
                    "b2": {"data": [{"blobId": "#b1"}, {"data:asText": "world!"}]}
                }}, "1"],
                ["Blob/get", {"accountId": "a1", "ids": ["#b2"]}, "2"]
            ]
        }),
    )
    .await;
    // The memory backend stores the inline "world!" as a leaf blob of its
    // own, so b2's id is not the next in sequence: take it from call 1.
    let b2 = &resp["methodResponses"][1][1]["created"]["b2"]["id"];
    let get = &resp["methodResponses"][2][1];
    assert_eq!(
        get["list"][0],
        json!({"id": b2, "data:asText": "Hello, world!", "size": 13}),
        "{resp}"
    );
}

#[tokio::test]
async fn creation_cycle_rejects_every_member() {
    let b = backend();
    let resp = set(
        &b,
        json!({"accountId": "a1", "create": {
            "x": {"data": [{"blobId": "#y"}]},
            "y": {"data": [{"blobId": "#x"}]},
            "s": {"data": [{"blobId": "#s"}]},
            "ok": {"data": [{"data:asText": "fine"}]}
        }}),
    )
    .await;
    for cid in ["x", "y", "s"] {
        assert_eq!(kind(&resp["notCreated"][cid]), invalid(&["data"]), "{cid}");
    }
    assert_eq!(resp["created"]["ok"]["size"], 4);
}

/// Blob/get `chunks` with the default dataSourceProperties feed straight
/// back into Blob/set.
#[tokio::test]
async fn chunks_fed_back_into_blob_set() {
    let b = backend();
    let one = text_blob(&b, "0123456789").await;
    let two = text_blob(&b, "abcdef").await;
    let joined = create(
        &b,
        json!({"data": [{"blobId": one, "offset": 2}, {"blobId": two, "length": 3}]}),
    )
    .await["created"]["c"]["id"]
        .clone();
    let get = jmap_blob_server::handle_blob_get(
        &b,
        &(),
        json!({"accountId": "a1", "ids": [joined], "properties": ["chunks"]}),
        &BTreeMap::new(),
        &using(&[BLOB2]),
    )
    .await
    .unwrap()
    .primary;
    let chunks = get["list"][0]["chunks"].clone();
    let copy = create(&b, json!({"data": chunks, "size": 11})).await;
    let copy_id = copy["created"]["c"]["id"].clone();
    let read = jmap_blob_server::handle_blob_get(
        &b,
        &(),
        json!({"accountId": "a1", "ids": [copy_id]}),
        &BTreeMap::new(),
        &using(&[BLOB2]),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(read["list"][0]["data:asText"], "23456789abc");
}

#[tokio::test]
async fn no_persist_is_reported_unless_the_backend_omits_it() {
    let request = json!({
        "using": using(&[BLOB2]),
        "methodCalls": [
            ["Blob/set", {"accountId": "a1", "create": {
                "tmp": {"data": [{"data:asText": "scratch"}], "noPersist": true},
                "keep": {"data": [{"blobId": "#tmp"}]}
            }}, "0"]
        ],
        "createdIds": {}
    });
    let b = Arc::new(backend());
    let resp = dispatch(Arc::clone(&b), request.clone()).await;
    let created = &resp["methodResponses"][0][1]["created"];
    // noPersist expires in five minutes.
    assert_eq!(created["tmp"]["expires"], "2025-11-15T00:05:00Z", "{resp}");
    assert_eq!(resp["createdIds"], json!({"tmp": "B1", "keep": "B2"}));

    let b = Arc::new(backend());
    b.omit_no_persist_for_test(true);
    let resp = dispatch(Arc::clone(&b), request).await;
    let created = &resp["methodResponses"][0][1]["created"];
    // Omitted from created and createdIds, yet usable within the call.
    assert_eq!(created.get("tmp"), None, "{resp}");
    assert_eq!(created["keep"]["size"], 7);
    assert_eq!(resp["createdIds"], json!({"keep": "B2"}));
}

// ---- update --------------------------------------------------------------

#[tokio::test]
async fn touch_honoured_and_capped() {
    let b = backend();
    let blob = text_blob(&b, "hello").await;
    let resp = set(
        &b,
        json!({"accountId": "a1", "update": {
            blob.clone(): {"expires": "2025-11-16T00:00:00Z"}
        }}),
    )
    .await;
    assert_eq!(resp["updated"], json!({blob.clone(): null}), "{resp}");
    // Beyond seven days the backend applies its cap and says so.
    let resp = set(
        &b,
        json!({"accountId": "a1", "update": {
            blob.clone(): {"expires": "2026-01-01T00:00:00Z"}
        }}),
    )
    .await;
    assert_eq!(
        resp["updated"],
        json!({blob.clone(): {"expires": "2025-11-22T00:00:00Z"}})
    );
}

#[tokio::test]
async fn touch_other_properties_only_at_current_values() {
    let b = backend();
    let blob = text_blob(&b, "hello").await;
    let resp = set(
        &b,
        json!({"accountId": "a1", "update": {
            blob.clone(): {"size": 5, "type": null, "id": blob.clone(),
                           "expires": "2025-11-16T00:00:00Z"}
        }}),
    )
    .await;
    assert_eq!(resp["updated"], json!({blob.clone(): null}), "{resp}");
    for (patch, prop) in [
        (json!({"size": 6}), "size"),
        (json!({"type": "text/plain"}), "type"),
        (json!({"data": []}), "data"),
        (json!({"expires": "tomorrow"}), "expires"),
    ] {
        let resp = set(
            &b,
            json!({"accountId": "a1", "update": {blob.clone(): patch}}),
        )
        .await;
        assert_eq!(kind(&resp["notUpdated"][&blob]), invalid(&[prop]), "{prop}");
    }
    let resp = set(&b, json!({"accountId": "a1", "update": {blob.clone(): 1}})).await;
    assert_eq!(resp["notUpdated"][&blob]["type"], "invalidPatch");
}

#[tokio::test]
async fn touch_missing_is_not_found() {
    let b = backend();
    let resp = set(
        &b,
        json!({"accountId": "a1", "update": {"nope": {"expires": null}}}),
    )
    .await;
    assert_eq!(resp["notUpdated"]["nope"]["type"], "notFound");
    let resp = set(
        &b,
        json!({"accountId": "a1", "update": {"#ghost": {"expires": null}}}),
    )
    .await;
    assert_eq!(resp["notUpdated"]["#ghost"]["type"], "notFound");
}

#[tokio::test]
async fn touch_and_destroy_by_creation_ref() {
    let b = backend();
    let resp = set(
        &b,
        json!({"accountId": "a1",
               "create": {"t": {"data": [{"data:asText": "x"}]}},
               "update": {"#t": {"expires": "2025-11-16T00:00:00Z"}},
               "destroy": ["#t"]}),
    )
    .await;
    assert_eq!(resp["updated"], json!({"B1": null}), "{resp}");
    assert_eq!(resp["destroyed"], json!(["B1"]));
}

// ---- destroy -------------------------------------------------------------

#[tokio::test]
async fn destroy_referenced_is_blob_has_reference() {
    let b = backend();
    let blob = text_blob(&b, "attached").await;
    b.add_reference_for_test(&id("a1"), "Email", &id("M1"), &id(&blob));
    let resp = set(
        &b,
        json!({"accountId": "a1", "destroy": [blob.clone(), "nope"]}),
    )
    .await;
    assert_eq!(resp["notDestroyed"][&blob]["type"], "blobHasReference");
    assert_eq!(resp["notDestroyed"]["nope"]["type"], "notFound");
    assert_eq!(resp["destroyed"], Value::Null);
}

#[tokio::test]
async fn destroy_then_create_from_it_in_order() {
    // Creates run before destroys, so a create may still read a blob the
    // same call destroys.
    let b = backend();
    let blob = text_blob(&b, "old").await;
    let resp = set(
        &b,
        json!({"accountId": "a1",
               "create": {"n": {"data": [{"blobId": blob.clone()}]}},
               "destroy": [blob.clone()]}),
    )
    .await;
    assert_eq!(resp["created"]["n"]["size"], 3, "{resp}");
    // The leaf is now a chunk of the new blob, so the memory backend
    // refuses to destroy it.
    assert_eq!(resp["notDestroyed"][&blob]["type"], "blobHasReference");
}

#[tokio::test]
async fn malformed_arguments_are_invalid_arguments() {
    let b = backend();
    for args in [
        json!({"accountId": "a1", "create": []}),
        json!({"accountId": "a1", "update": ["x"]}),
        json!({"accountId": "a1", "destroy": "x"}),
        json!({"accountId": "a1", "destroy": [1]}),
    ] {
        let r = call_with(&b, args.clone(), &BTreeMap::new(), &[BLOB2]).await;
        assert_eq!(error_type(r), "invalidArguments", "{args}");
    }
}
