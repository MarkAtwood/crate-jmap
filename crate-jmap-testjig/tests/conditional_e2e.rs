//! draft-ietf-jmap-conditional-02 over HTTP (bd JMAP-y1yu.19;
//! docs/design/conditional.md §5.3, §10).
//!
//! Oracles: conditional-02 §2.1 (the capability is advertised only when
//! every Foo/set supports it, as `{}`), example 5.8 (an atomic exchange of
//! two names succeeds; with a conflicting third rename the method fails
//! with `atomicFailure` and nothing is renamed), and
//! docs/design/DECISIONS.md decision 2 (`atomic: true` without the URI in
//! `using` is `invalidArguments`).

use jmap_testjig::{TestjigConfig, TestjigHandle, spawn_in_process};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const FILENODE: &str = "urn:ietf:params:jmap:filenode";
const CONDITIONAL: &str = "urn:ietf:params:jmap:conditional";

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

/// POST one FileNode/set with `using`; the method response.
async fn file_set(jig: &TestjigHandle, account: &str, using: &[&str], mut args: Value) -> Value {
    args["accountId"] = json!(account);
    let resp: Value = reqwest::Client::new()
        .post(format!("http://{}/jmap", jig.addr))
        .bearer_auth(&jig.token)
        .json(&json!({"using": using, "methodCalls": [["FileNode/set", args, "0"]]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    resp["methodResponses"][0].clone()
}

fn account(session: &Value) -> String {
    session["primaryAccounts"][FILENODE]
        .as_str()
        .expect("filenode account")
        .to_owned()
}

#[tokio::test]
async fn session_advertises_conditional() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let s = session(&jig).await;
    assert_eq!(s["capabilities"][CONDITIONAL], json!({}), "{s}");
}

/// Example 5.8 over HTTP.
#[tokio::test]
async fn example_5_8_atomic_swap_over_http() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let using = [CORE, FILENODE, CONDITIONAL];
    let r = file_set(
        &jig,
        &acc,
        &using,
        json!({"create": {
            "a": {"@type": "File", "name": "current.txt", "parentId": null, "blobId": "G1"},
            "b": {"@type": "File", "name": "previous.txt", "parentId": null, "blobId": "G2"},
            "c": {"@type": "File", "name": "other.txt", "parentId": null, "blobId": "G3"}
        }}),
    )
    .await;
    let id = |k: &str| {
        r[1]["created"][k]["id"]
            .as_str()
            .expect("created")
            .to_owned()
    };
    let (a, b, c) = (id("a"), id("b"), id("c"));

    // A third rename onto "current.txt" makes the result conflict.
    let r = file_set(
        &jig,
        &acc,
        &using,
        json!({"atomic": true, "update": {
            &a: {"name": "previous.txt"}, &b: {"name": "current.txt"}, &c: {"name": "current.txt"}
        }}),
    )
    .await;
    assert_eq!(r[0], "error", "{r}");
    assert_eq!(r[1]["type"], "atomicFailure", "{r}");

    let r = file_set(
        &jig,
        &acc,
        &using,
        json!({"atomic": true, "update": {&a: {"name": "previous.txt"}, &b: {"name": "current.txt"}}}),
    )
    .await;
    assert_eq!(r[0], "FileNode/set", "{r}");
    assert!(
        r[1]["updated"].get(&a).is_some() && r[1]["updated"].get(&b).is_some(),
        "{r}"
    );
}

/// Decision 2: without the URI, `atomic: true` is refused.
#[tokio::test]
async fn atomic_without_the_uri_is_invalid_arguments() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let r = file_set(
        &jig,
        &acc,
        &[CORE, FILENODE],
        json!({"atomic": true, "destroy": []}),
    )
    .await;
    assert_eq!(r[0], "error", "{r}");
    assert_eq!(r[1]["type"], "invalidArguments", "{r}");
}
