//! RFC 9404 (`urn:ietf:params:jmap:blob`) served by the blob handlers
//! (docs/design/DECISIONS.md decisions 11 and 21): every case in
//! docs/design/blobext.md §10 "server, RFC 9404 mode".
//!
//! Whole requests run through a dispatcher, so `#creationId` references
//! between calls go through the request's real creation ids.
//!
//! Oracles: the RFC 9404 §4.1.1, §4.1.2, §4.2.1, §4.2.2 and §4.3.1
//! examples, verbatim apart from what is noted at each test: the blob ids
//! the memory backend mints, `expires` (blobext-03 §7), and `accountId`,
//! which the examples' requests sometimes leave out but RFC 8620 §5.1
//! requires.

use std::sync::Arc;

use jmap_blob_server::memory::MemoryBackend;
use jmap_blob_server::register_blob_handlers;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const BLOB: &str = "urn:ietf:params:jmap:blob";
const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const MAIL: &str = "urn:ietf:params:jmap:mail";
/// 2025-11-15T00:00:00Z; uploads expire an hour later.
const NOW: i64 = 1_763_164_800;
const EXPIRES: &str = "2025-11-15T01:00:00Z";
const FOX: &str = "The quick brown fox jumped over the lazy dog.";

fn id(s: &str) -> Id {
    Id::new_validated(s).unwrap()
}

fn backend() -> Arc<MemoryBackend<()>> {
    let b = MemoryBackend::<()>::default().with_account(&id("account1"));
    b.set_clock_for_test(NOW);
    Arc::new(b)
}

/// Run `calls` with `using` and return the whole response.
async fn request(b: &Arc<MemoryBackend<()>>, using: &[&str], calls: Value) -> Value {
    request_with(b, json!({"using": using, "methodCalls": calls})).await
}

async fn request_with(b: &Arc<MemoryBackend<()>>, request: Value) -> Value {
    let mut d = Dispatcher::new();
    register_blob_handlers(&mut d, Arc::clone(b));
    let req: JmapRequest = serde_json::from_value(request).unwrap();
    serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap()
}

/// The arguments of each method response, in order.
fn results(resp: &Value) -> Vec<Value> {
    resp["methodResponses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|inv| inv[1].clone())
        .collect()
}

/// The single method response's arguments.
async fn one(b: &Arc<MemoryBackend<()>>, using: &[&str], call: Value) -> Value {
    results(&request(b, using, json!([call])).await).remove(0)
}

// ---- Blob/upload (§4.1) --------------------------------------------------

/// §4.1.1, verbatim apart from the minted id and `expires`.
#[tokio::test]
async fn upload_simple_example() {
    let b = backend();
    let resp = one(
        &b,
        &[CORE, BLOB],
        json!(["Blob/upload", {
            "accountId": "account1",
            "create": {
                "1": {
                    "data": [{
                        "data:asBase64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABAQMAAAAl21bKAAAAA1BMVEX/AAAZ4gk3AAAAAXRSTlN/gFy0ywAAAApJREFUeJxjYgAAAAYAAzY3fKgAAAAASUVORK5CYII="
                    }],
                    "type": "image/png"
                }
            }
        }, "R1"]),
    )
    .await;
    assert_eq!(
        resp,
        json!({
            "accountId": "account1",
            "created": {
                "1": {
                    "id": "B1",
                    "type": "image/png",
                    "size": 95,
                    "expires": EXPIRES
                }
            },
            "notCreated": null
        })
    );
}

/// §4.1.2: the second upload reads ranges of the first through `#b4`, and
/// Blob/get reads the result through `#cat`. The responses carry no
/// `oldState` (decision 21(iii)), and the uploads were given no type, so
/// `type` is null where the example's server chose
/// `application/octet-stream` (§4.1: "the calculated value,
/// "application/octet-string", or null").
#[tokio::test]
async fn upload_complex_example() {
    let b = backend();
    let resp = request(
        &b,
        &[CORE, BLOB],
        json!([
            ["Blob/upload", {
                "accountId": "account1",
                "create": {"b4": {"data": [{"data:asText": FOX}]}}
            }, "S4"],
            ["Blob/upload", {
                "accountId": "account1",
                "create": {
                    "cat": {
                        "data": [
                            {"data:asText": "How"},
                            {"blobId": "#b4", "length": 7, "offset": 3},
                            {"data:asText": "was t"},
                            {"blobId": "#b4", "length": 1, "offset": 1},
                            {"data:asBase64": "YXQ/"}
                        ]
                    }
                }
            }, "CAT"],
            ["Blob/get", {
                "accountId": "account1",
                "properties": ["data:asText", "size"],
                "ids": ["#cat"]
            }, "G4"]
        ]),
    )
    .await;
    let [s4, cat, g4]: [Value; 3] = results(&resp).try_into().unwrap();
    assert_eq!(
        s4,
        json!({
            "accountId": "account1",
            "created": {"b4": {"id": "B1", "size": 45, "type": null, "expires": EXPIRES}},
            "notCreated": null
        })
    );
    assert_eq!(
        cat,
        json!({
            "accountId": "account1",
            // B2-B4: the memory backend stores each inline part of a
            // mixed blob as a blob of its own.
            "created": {"cat": {"id": "B5", "size": 19, "type": null, "expires": EXPIRES}},
            "notCreated": null
        })
    );
    assert_eq!(
        g4,
        json!({
            "accountId": "account1",
            "list": [{"id": "B5", "data:asText": "How quick was that?", "size": 19}],
            "notFound": []
        })
    );
}

/// §4.1: servers "MUST add an entry to the createdIds map" for every
/// upload.
#[tokio::test]
async fn every_upload_reaches_created_ids() {
    let b = backend();
    let resp = request_with(
        &b,
        json!({
            "using": [CORE, BLOB],
            "methodCalls": [["Blob/upload", {
                "accountId": "account1",
                "create": {
                    "x": {"data": [{"data:asText": "x"}]},
                    "y": {"data": [{"data:asText": "y"}], "type": "text/plain"},
                    "empty": {"data": []}
                }
            }, "0"]],
            "createdIds": {}
        }),
    )
    .await;
    assert_eq!(
        resp["createdIds"],
        json!({"empty": "B1", "x": "B2", "y": "B3"})
    );
}

/// §4.1: "only create is allowed" (decision 21(iv)); `create` is required.
#[tokio::test]
async fn upload_method_errors() {
    let b = backend();
    for args in [
        json!({"accountId": "account1", "create": {}, "update": {"B1": {}}}),
        json!({"accountId": "account1", "create": {}, "destroy": ["B1"]}),
        json!({"accountId": "account1", "create": {}, "destroy": "B1"}),
        json!({"accountId": "account1"}),
        json!({"accountId": "account1", "create": null}),
        json!({"accountId": "account1", "create": []}),
        json!({"create": {}}),
    ] {
        let resp = one(&b, &[CORE, BLOB], json!(["Blob/upload", args.clone(), "0"])).await;
        assert_eq!(resp["type"], "invalidArguments", "{args}");
    }
    // Empty update and destroy lose nothing.
    for args in [
        json!({"accountId": "account1", "create": {}, "update": {}, "destroy": []}),
        json!({"accountId": "account1", "create": {}, "update": null, "destroy": null}),
    ] {
        let resp = one(&b, &[CORE, BLOB], json!(["Blob/upload", args.clone(), "0"])).await;
        assert_eq!(resp["accountId"], "account1", "{args}");
    }
    let resp = one(
        &b,
        &[CORE, BLOB],
        json!(["Blob/upload", {"accountId": "nope", "create": {}}, "0"]),
    )
    .await;
    assert_eq!(resp["type"], "accountNotFound");
}

/// §4.1: an UploadObject has `data` and `type` only; invalid sources,
/// references and creation ids fail like a Blob/set create.
#[tokio::test]
async fn upload_entry_errors() {
    let b = backend();
    let resp = one(
        &b,
        &[CORE, BLOB],
        json!(["Blob/upload", {
            "accountId": "account1",
            "create": {
                "size": {"data": [{"data:asText": "x"}], "size": 1},
                "digest": {"data": [{"data:asText": "x"}], "digest:sha": null},
                "persist": {"data": [{"data:asText": "x"}], "noPersist": false},
                "noData": {"type": "text/plain"},
                "badBase64": {"data": [{"data:asBase64": "!!"}]},
                "badRange": {"data": [{"blobId": "#ok", "offset": 5}]},
                "missing": {"data": [{"blobId": "B99"}]},
                "nothing": {"data": [{"blobId": "#nothing"}]},
                "ok": {"data": [{"data:asText": "abc"}]},
                "not an id": {"data": []}
            }
        }, "0"]),
    )
    .await;
    let not_created = &resp["notCreated"];
    for (cid, error, property) in [
        ("size", "invalidProperties", Some("size")),
        ("digest", "invalidProperties", Some("digest:sha")),
        ("persist", "invalidProperties", Some("noPersist")),
        ("noData", "invalidProperties", Some("data")),
        ("badBase64", "invalidProperties", Some("data")),
        ("badRange", "invalidProperties", Some("data")),
        ("nothing", "invalidProperties", Some("data")),
        ("missing", "notFound", None),
        ("not an id", "invalidProperties", Some("id")),
    ] {
        assert_eq!(not_created[cid]["type"], error, "{cid}: {resp}");
        if let Some(p) = property {
            assert_eq!(not_created[cid]["properties"], json!([p]), "{cid}");
        }
    }
    assert_eq!(resp["created"]["ok"]["size"], 3);
}

/// Like Blob/set and Blob/convert, Blob/upload runs in a blob2 request.
#[tokio::test]
async fn upload_in_a_blob2_request() {
    let b = backend();
    let resp = one(
        &b,
        &[CORE, BLOB2],
        json!(["Blob/upload", {"accountId": "account1", "create": {"x": {"data": []}}}, "0"]),
    )
    .await;
    assert_eq!(resp["created"]["x"]["size"], 0);
}

// ---- Blob/get (§4.2) -----------------------------------------------------

/// §4.2.1, verbatim apart from the minted id: the upload replaces the
/// example's existing blob.
#[tokio::test]
async fn get_simple_example() {
    let b = backend();
    let resp = request(
        &b,
        &[CORE, BLOB],
        json!([
            ["Blob/upload", {"accountId": "account1", "create": {"fox": {"data": [{"data:asText": FOX}]}}}, "S"],
            ["Blob/get", {
                "accountId": "account1",
                "ids": ["B1", "not-a-blob"],
                "properties": ["data:asText", "digest:sha", "size"]
            }, "R1"],
            ["Blob/get", {
                "accountId": "account1",
                "ids": ["B1"],
                "properties": ["data:asText", "digest:sha", "digest:sha-256", "size"],
                "offset": 4,
                "length": 9
            }, "R2"]
        ]),
    )
    .await;
    let [_, r1, r2]: [Value; 3] = results(&resp).try_into().unwrap();
    assert_eq!(
        r1,
        json!({
            "accountId": "account1",
            "list": [{
                "id": "B1",
                "data:asText": FOX,
                "digest:sha": "wIVPufsDxBzOOALLDSIFKebu+U4=",
                "size": 45
            }],
            "notFound": ["not-a-blob"]
        })
    );
    assert_eq!(
        r2,
        json!({
            "accountId": "account1",
            "list": [{
                "id": "B1",
                "data:asText": "quick bro",
                "digest:sha": "QiRAPtfyX8K6tm1iOAtZ87Xj3Ww=",
                "digest:sha-256": "gdg9INW7lwHK6OQ9u0dwDz2ZY/gubi0En0xlFpKt0OA=",
                "size": 45
            }],
            "notFound": []
        })
    );
}

/// §4.2.2 G1-G5, verbatim apart from the minted ids, `accountId`, G2's
/// explicit `"data:asText": null` (see there), and the
/// upload's `type` values, which the example's response swaps (b1 was
/// sent without a type, b2 as `text/plain`): the requested types are
/// asserted. G4 and G5 send offset and length without `properties`,
/// which RFC 9404 allows (decision 11; blobext-03 gap g does not apply).
#[tokio::test]
async fn get_range_and_encoding_example() {
    let b = backend();
    let ids = json!(["#b1", "#b2"]);
    let resp = request(
        &b,
        &[CORE, BLOB],
        json!([
            ["Blob/upload", {
                "accountId": "account1",
                "create": {
                    "b1": {"data": [{"data:asBase64": "VGhlIHF1aWNrIGJyb3duIGZveCBqdW1wZWQgb3ZlciB0aGUggYEgZG9nLg=="}]},
                    "b2": {"data": [{"data:asText": "hello world"}], "type": "text/plain"}
                }
            }, "S1"],
            ["Blob/get", {"accountId": "account1", "ids": ids}, "G1"],
            ["Blob/get", {"accountId": "account1", "ids": ids, "properties": ["data:asText", "size"]}, "G2"],
            ["Blob/get", {"accountId": "account1", "ids": ids, "properties": ["data:asBase64", "size"]}, "G3"],
            ["Blob/get", {"accountId": "account1", "offset": 0, "length": 5, "ids": ids}, "G4"],
            ["Blob/get", {"accountId": "account1", "offset": 20, "length": 100, "ids": ids}, "G5"]
        ]),
    )
    .await;
    let [s1, g1, g2, g3, g4, g5]: [Value; 6] = results(&resp).try_into().unwrap();
    assert_eq!(
        s1["created"],
        json!({
            "b1": {"id": "B1", "size": 43, "type": null, "expires": EXPIRES},
            "b2": {"id": "B2", "size": 11, "type": "text/plain", "expires": EXPIRES}
        })
    );
    let list =
        |a: Value, b: Value| json!({"accountId": "account1", "list": [a, b], "notFound": []});
    let b1_base64 = "VGhlIHF1aWNrIGJyb3duIGZveCBqdW1wZWQgb3ZlciB0aGUggYEgZG9nLg==";
    assert_eq!(
        g1,
        list(
            json!({"id": "B1", "isEncodingProblem": true, "data:asBase64": b1_base64, "size": 43}),
            json!({"id": "B2", "data:asText": "hello world", "size": 11})
        )
    );
    assert_eq!(
        g2,
        list(
            // The example leaves data:asText out; the §4.2 text says "the
            // data:asText response value MUST be null".
            json!({"id": "B1", "isEncodingProblem": true, "data:asText": null, "size": 43}),
            json!({"id": "B2", "data:asText": "hello world", "size": 11})
        )
    );
    assert_eq!(
        g3,
        list(
            json!({"id": "B1", "data:asBase64": b1_base64, "size": 43}),
            json!({"id": "B2", "data:asBase64": "aGVsbG8gd29ybGQ=", "size": 11})
        )
    );
    assert_eq!(
        g4,
        list(
            json!({"id": "B1", "data:asText": "The q", "size": 43}),
            json!({"id": "B2", "data:asText": "hello", "size": 11})
        )
    );
    assert_eq!(
        g5,
        list(
            json!({
                "id": "B1",
                "isTruncated": true,
                "isEncodingProblem": true,
                "data:asBase64": "anVtcGVkIG92ZXIgdGhlIIGBIGRvZy4=",
                "size": 43
            }),
            json!({"id": "B2", "isTruncated": true, "data:asText": "", "size": 11})
        )
    );
}

/// RFC 9404 has no `chunks` or `imageData` (decision 11): unknown
/// properties are `invalidArguments`.
#[tokio::test]
async fn blobext_properties_are_unknown() {
    let b = backend();
    for property in ["chunks", "imageData"] {
        let resp = one(
            &b,
            &[CORE, BLOB],
            json!(["Blob/get", {"accountId": "account1", "ids": [], "properties": [property]}, "0"]),
        )
        .await;
        assert_eq!(resp["type"], "invalidArguments", "{property}");
    }
}

// ---- Blob/lookup (§4.3) --------------------------------------------------

/// §4.3.1, apart from the minted id and `accountId`; `not-a-blob` gets a
/// row with empty arrays and there is no `notFound`, following the §4.3
/// text over the example (decision 21(ii)).
#[tokio::test]
async fn lookup_example() {
    let b = backend();
    let mut cap = MemoryBackend::default_capability();
    cap.supported_type_names = ["Mailbox", "Thread", "Email"].map(str::to_owned).to_vec();
    b.advertise_for_test(&id("account1"), cap);
    let resp = request(
        &b,
        &[CORE, BLOB],
        json!([["Blob/upload", {"accountId": "account1", "create": {"m": {"data": [{"data:asText": "mail"}]}}}, "S"]]),
    )
    .await;
    assert_eq!(results(&resp)[0]["created"]["m"]["id"], "B1");
    for (t, o) in [
        ("Mailbox", "M54e97373"),
        ("Mailbox", "Mcbe6b662"),
        ("Thread", "T1530616e"),
        ("Email", "E16e70a73eb4"),
        ("Email", "E84b0930cf16"),
    ] {
        b.add_reference_for_test(&id("account1"), t, &id(o), &id("B1"));
    }
    let resp = one(
        &b,
        &[CORE, BLOB, MAIL],
        json!(["Blob/lookup", {
            "accountId": "account1",
            "typeNames": ["Mailbox", "Thread", "Email"],
            "ids": ["B1", "not-a-blob"]
        }, "R1"]),
    )
    .await;
    assert_eq!(
        resp,
        json!({
            "accountId": "account1",
            "list": [
                {
                    "id": "B1",
                    "matchedIds": {
                        "Mailbox": ["M54e97373", "Mcbe6b662"],
                        "Thread": ["T1530616e"],
                        "Email": ["E16e70a73eb4", "E84b0930cf16"]
                    }
                },
                {
                    "id": "not-a-blob",
                    "matchedIds": {"Mailbox": [], "Thread": [], "Email": []}
                }
            ]
        })
    );
}

// ---- both capabilities (decision 21(i)) ----------------------------------

/// blobext-03 §2.1: a client "MUST NOT include both"; every Blob method in
/// such a request answers `invalidArguments`.
#[tokio::test]
async fn both_capabilities_refused_by_every_method() {
    let b = backend();
    let resp = request(
        &b,
        &[CORE, BLOB, BLOB2],
        json!([
            ["Blob/get", {"accountId": "account1", "ids": []}, "0"],
            ["Blob/set", {"accountId": "account1"}, "1"],
            ["Blob/upload", {"accountId": "account1", "create": {}}, "2"],
            ["Blob/lookup", {"accountId": "account1", "typeNames": [], "ids": []}, "3"],
            ["Blob/convert", {"accountId": "account1", "create": {}}, "4"]
        ]),
    )
    .await;
    let names: Vec<&str> = resp["methodResponses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|inv| inv[0].as_str().unwrap())
        .collect();
    assert_eq!(names, ["error"; 5]);
    for r in results(&resp) {
        assert_eq!(r["type"], "invalidArguments");
    }
}
