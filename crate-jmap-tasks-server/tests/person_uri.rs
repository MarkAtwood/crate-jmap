//! Person `uri` in Task `checklists` (CheckItem `assignee`) and `comments`
//! (Comment `author`) is an RFC 3986 URI (draft-atwood-jmap-updates-00
//! §7.1, jmap-chat-spec 2b85363: a URI-valued property "SHOULD hold a URI
//! as defined in RFC 3986 Section 3"; "URIs in JMAP are ASCII").
//!
//! Oracle: the draft text; requests and expected SetErrors hand-written.

mod common;

use common::MemoryBackend;
use jmap_tasks_server::handle_task_set;
use jmap_types::Id;
use serde_json::{Value, json};

async fn set(args: Value) -> Value {
    let backend = MemoryBackend::new().with_account(&Id::from("acc1"));
    let mut args = args;
    args["accountId"] = json!("acc1");
    handle_task_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("/set must not fail at method level")
        .primary
}

fn person(uri: &str) -> Value {
    json!({ "@type": "Person", "name": "Ada", "uri": uri })
}

#[tokio::test]
async fn create_rejects_a_non_uri_person_uri() {
    let checklist = json!({ "cl1": { "@type": "Checklist", "checkItems": [
        { "@type": "CheckItem", "title": "x", "sortOrder": 0, "assignee": person("mailto:ada lovelace@example.com") }
    ] } });
    let resp = set(json!({ "create": { "t": { "title": "T", "checklists": checklist } } })).await;
    assert_eq!(
        resp["notCreated"]["t"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notCreated"]["t"]["properties"], json!(["checklists"]));

    let comments = json!({ "c1": { "@type": "Comment", "message": "hi", "author": person("mailto:adä@example.com") } });
    let resp = set(json!({ "create": { "t": { "title": "T", "comments": comments } } })).await;
    assert_eq!(
        resp["notCreated"]["t"]["properties"],
        json!(["comments"]),
        "{resp}"
    );
}

#[tokio::test]
async fn create_accepts_a_uri_person_uri() {
    let comments = json!({ "c1": { "@type": "Comment", "message": "hi", "author": person("mailto:ada@example.com") } });
    let resp = set(json!({ "create": { "t": { "title": "T", "comments": comments } } })).await;
    // Whatever else the backend decides, the Person uri is not the problem.
    assert_ne!(
        resp["notCreated"]["t"]["properties"],
        json!(["comments"]),
        "{resp}"
    );
}

#[tokio::test]
async fn update_rejects_a_non_uri_person_uri() {
    // A pointer straight at the uri.
    let resp = set(json!({ "update": { "T1": {
        "comments/c1/author/uri": "mailto:ada lovelace@example.com"
    } } }))
    .await;
    assert_eq!(
        resp["notUpdated"]["T1"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(resp["notUpdated"]["T1"]["properties"], json!(["comments"]));

    // A value holding the Person.
    let resp = set(json!({ "update": { "T1": {
        "checklists/cl1/checkItems": [
            { "@type": "CheckItem", "title": "x", "sortOrder": 0, "assignee": person("not a uri") }
        ]
    } } }))
    .await;
    assert_eq!(
        resp["notUpdated"]["T1"]["properties"],
        json!(["checklists"]),
        "{resp}"
    );
}
