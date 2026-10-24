//! `TaskList/set` create fills omitted defaulted properties (bd:JMAP-i98f).
//!
//! Oracle: RFC 8620 §5.3 (rfc8620.txt:1884-1885): "The Foo object type
//! definition may define default values for properties.  Any such property
//! may be omitted by the client." and (rfc8620.txt:2036-2039) `created`
//! holds "any properties that were omitted by the client and thus set to a
//! default by the server."
//!
//! draft-ietf-jmap-tasks-06 §3 (draft-ietf-jmap-tasks-06.txt:440-443):
//! "*isSubscribed*: Boolean Has the user indicated they wish to see this
//! task list in their client?  This SHOULD default to false for task lists
//! in shared accounts the user has access to, and true for any new task
//! list created by the user themselves."

mod common;

use std::sync::Arc;

use jmap_types::Id;

use common::MemoryBackend;
use jmap_server::Dispatcher;
use jmap_tasks_server::register_tasks_handlers;
use jmap_types::{JmapRequest, JmapResponse, State};
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";

async fn create(object: Value) -> JmapResponse {
    let mut dispatcher = Dispatcher::new();
    register_tasks_handlers(
        &mut dispatcher,
        Arc::new(MemoryBackend::new().with_account(&Id::from(ACCOUNT))),
    );
    let request: JmapRequest = serde_json::from_value(json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:tasks"],
        "methodCalls": [["TaskList/set", { "accountId": ACCOUNT, "create": { "c": object } }, "0"]]
    }))
    .expect("request JSON must deserialize");
    dispatcher.dispatch(request, (), State::from("s0")).await
}

fn args(resp: &JmapResponse) -> &Value {
    &resp.method_responses[0].arguments
}

/// A create that omits `isSubscribed` succeeds, and `created` reports the
/// spec default for each.
#[tokio::test]
async fn omitted_defaults_are_filled_and_echoed() {
    let resp = create(json!({ "name": "Home" })).await;
    let created = &args(&resp)["created"]["c"];
    assert!(created["id"].is_string(), "{}", args(&resp));
    assert_eq!(created["isSubscribed"], json!(true), "{}", args(&resp));
}

/// A value the client sends is kept, not replaced by the default.
#[tokio::test]
async fn explicit_value_is_kept() {
    let resp = create(json!({ "name": "Home", "isSubscribed": false })).await;
    assert_eq!(
        args(&resp)["created"]["c"]["isSubscribed"],
        json!(false),
        "{}",
        args(&resp)
    );
}
