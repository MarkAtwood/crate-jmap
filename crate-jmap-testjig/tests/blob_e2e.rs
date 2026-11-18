//! Blob over HTTP against an in-process jig (bd JMAP-tna1.2.13): the
//! Session advertises both blob capabilities with their full account
//! objects, and blobext-03 §9.1 and RFC 9404 §4.1.2 run end to end.
//!
//! Oracles: draft-ietf-jmap-blobext-03 §2.1 (the 17 account capability
//! keys) and §9.1; RFC 9404 §3.1 (the 4 keys) and §4.1.2 ("How quick was
//! that?", 19 octets); the memory backend's documented default
//! capability (`maxDataSources` 64, digests sha-256 and sha, tar archive
//! and extract, every other list null). Blob ids are minted and `expires`
//! follows the wall clock, so those are checked by shape only.

use jmap_testjig::{TestjigConfig, TestjigHandle, spawn_in_process};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const BLOB: &str = "urn:ietf:params:jmap:blob";

async fn session(jig: &TestjigHandle) -> Value {
    reqwest::Client::new()
        .get(format!("http://{}/.well-known/jmap", jig.addr))
        .bearer_auth(&jig.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn post(jig: &TestjigHandle, request: Value) -> Value {
    reqwest::Client::new()
        .post(format!("http://{}/jmap", jig.addr))
        .bearer_auth(&jig.token)
        .json(&request)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// The arguments of each method response, after checking its name.
fn results(resp: &Value, names: &[&str]) -> Vec<Value> {
    let invocations = resp["methodResponses"].as_array().unwrap();
    assert_eq!(invocations.len(), names.len(), "{resp}");
    invocations
        .iter()
        .zip(names)
        .map(|(inv, name)| {
            assert_eq!(inv[0], *name, "{resp}");
            inv[1].clone()
        })
        .collect()
}

#[tokio::test]
async fn session_advertises_both_blob_capabilities() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let s = session(&jig).await;
    let account = s["primaryAccounts"][BLOB2].as_str().unwrap();
    assert_eq!(s["primaryAccounts"][BLOB], account);
    assert_eq!(s["capabilities"][BLOB2], json!({}));
    assert_eq!(s["capabilities"][BLOB], json!({}));

    let caps = &s["accounts"][account]["accountCapabilities"];
    // blobext-03 §2.1: the object "MUST contain" all 17 keys.
    assert_eq!(
        caps[BLOB2],
        json!({
            "maxSizeBlobSet": null,
            "maxDataSources": 64,
            "supportedTypeNames": ["Email", "Mailbox", "Thread"],
            "supportedDigestAlgorithms": ["sha-256", "sha"],
            "uploadUrl": null,
            "chunkSize": null,
            "supportedImageReadTypes": null,
            "supportedImageWriteTypes": null,
            "supportedArchiveTypes": ["application/x-tar"],
            "supportedExtractTypes": ["application/x-tar"],
            "supportedCompressTypes": null,
            "supportedDecompressTypes": null,
            "supportedDeltaTypes": null,
            "supportedPatchTypes": null,
            "maxConvertSize": null,
            "maxArchiveEntries": null,
            "maxImageDimension": null
        })
    );
    // RFC 9404 §3.1: the four shared keys.
    assert_eq!(
        caps[BLOB],
        json!({
            "maxSizeBlobSet": null,
            "maxDataSources": 64,
            "supportedTypeNames": ["Email", "Mailbox", "Thread"],
            "supportedDigestAlgorithms": ["sha-256", "sha"]
        })
    );
}

/// blobext-03 §9.1 (with the jig's account in place of `abc`), then
/// Blob/get by `#b1` in the same request and by id in a later one.
#[tokio::test]
async fn blobext_set_then_get() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let account = session(&jig).await["primaryAccounts"][BLOB2]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = post(
        &jig,
        json!({
            "using": [CORE, BLOB2],
            "methodCalls": [
                ["Blob/set", {
                    "accountId": account,
                    "create": {
                        "b1": {
                            "data": [{"data:asText": "Hello, world!"}],
                            "type": "text/plain"
                        }
                    }
                }, "0"],
                ["Blob/get", {
                    "accountId": account,
                    "ids": ["#b1"],
                    "properties": ["data:asText", "digest:sha-256", "size"]
                }, "1"]
            ]
        }),
    )
    .await;
    let [set, get]: [Value; 2] = results(&resp, &["Blob/set", "Blob/get"])
        .try_into()
        .unwrap();
    let created = &set["created"]["b1"];
    assert_eq!(created["type"], "text/plain");
    assert_eq!(created["size"], 13);
    assert!(created["expires"].is_string(), "{set}");
    assert_eq!(set["notCreated"], Value::Null);
    let blob_id = created["id"].as_str().unwrap();
    assert_eq!(
        get["list"],
        json!([{
            "id": blob_id,
            "data:asText": "Hello, world!",
            "digest:sha-256": "MV9b23bQeMQ7isAGTkoBZGErH853yGk0W/yUx1iU7dM=",
            "size": 13
        }])
    );

    let resp = post(
        &jig,
        json!({
            "using": [CORE, BLOB2],
            "methodCalls": [["Blob/get", {"accountId": account, "ids": [blob_id]}, "0"]]
        }),
    )
    .await;
    let [get]: [Value; 1] = results(&resp, &["Blob/get"]).try_into().unwrap();
    assert_eq!(get["list"][0]["data:asText"], "Hello, world!");
}

/// RFC 9404 §4.1.2 under `urn:ietf:params:jmap:blob`, with the jig's
/// account added to each call.
#[tokio::test]
async fn rfc9404_upload_then_get() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let account = session(&jig).await["primaryAccounts"][BLOB]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = post(
        &jig,
        json!({
            "using": [CORE, BLOB],
            "methodCalls": [
                ["Blob/upload", {
                    "accountId": account,
                    "create": {
                        "b4": {"data": [{"data:asText": "The quick brown fox jumped over the lazy dog."}]}
                    }
                }, "S4"],
                ["Blob/upload", {
                    "accountId": account,
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
                    "accountId": account,
                    "properties": ["data:asText", "size"],
                    "ids": ["#cat"]
                }, "G4"]
            ],
            "createdIds": {}
        }),
    )
    .await;
    let [s4, cat, g4]: [Value; 3] = results(&resp, &["Blob/upload", "Blob/upload", "Blob/get"])
        .try_into()
        .unwrap();
    assert_eq!(s4["created"]["b4"]["size"], 45);
    let cat_id = cat["created"]["cat"]["id"].as_str().unwrap();
    assert_eq!(cat["created"]["cat"]["size"], 19);
    assert_eq!(
        g4["list"],
        json!([{"id": cat_id, "data:asText": "How quick was that?", "size": 19}])
    );
    // §4.1: every upload is in createdIds.
    let ids = resp["createdIds"].as_object().unwrap();
    assert_eq!(ids.keys().collect::<Vec<_>>(), ["b4", "cat"]);
    assert_eq!(ids["cat"], cat_id);
}
