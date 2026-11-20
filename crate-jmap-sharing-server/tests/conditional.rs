//! Conditional sharing Foo/set (draft-ietf-jmap-conditional-02) through
//! `register_sharing_conditional` and the memory backend's `ScopedState`
//! scopes (bd JMAP-y1yu.13; docs/design/conditional.md §5.1, §7, §9).
//!
//! Oracles: conditional-02 §3.1 (a pointer into a nested property compares
//! only that value; example 5.3) and §4.2 ("the server MUST NOT make any
//! of the changes"). The crate has no cross-object rule or cap for an
//! atomic commit to check, so rollback is what is tested.

mod common;

use std::sync::Arc;

use common::MemoryBackend;
use jmap_server::Dispatcher;
use jmap_sharing_server::SharingBackend;
use jmap_sharing_server::{register_sharing_conditional, register_sharing_handlers};
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:principals",
    "urn:ietf:params:jmap:conditional",
];

/// A create body; `capabilities` and `accounts` are server-set (RFC 9670
/// §2), so a client cannot send them.
fn principal(name: &str) -> Value {
    json!({"type": "individual", "name": name, "email": null, "description": null,
           "timeZone": null})
}

fn dispatcher() -> (Dispatcher<()>, Arc<MemoryBackend<()>>) {
    let b = Arc::new(MemoryBackend::<()>::default().with_account(&Id::from("acc1")));
    let mut d = Dispatcher::new();
    register_sharing_handlers(&mut d, Arc::clone(&b));
    register_sharing_conditional(&mut d, Arc::clone(&b));
    (d, b)
}

async fn call(d: &Dispatcher<()>, method: &str, mut args: Value) -> (String, Value) {
    args["accountId"] = json!("acc1");
    let req: JmapRequest =
        serde_json::from_value(json!({"using": USING, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    (inv[0].as_str().unwrap().to_owned(), inv[1].clone())
}

/// A Principal with a server-set capability holding nested rights,
/// stored through the backend.
async fn seed(b: &MemoryBackend<()>) -> String {
    let mut p = principal("Alice");
    p["capabilities"] = json!({"urn:example:share": {"mayRead": true, "mayWrite": false}});
    p["accounts"] = Value::Null;
    let (id, _) = b
        .create_object(&(), &Id::from("acc1"), "seed", move |id: &Id| {
            p["id"] = json!(id);
            serde_json::from_value::<jmap_sharing_types::Principal>(p).expect("fixture")
        })
        .await
        .expect("seed");
    id.as_ref().to_owned()
}

/// A nested pointer, as in example 5.3: only `mayRead` under the one
/// capability is compared, so a concurrent change to another property of
/// the same object does not fail it.
#[tokio::test]
async fn nested_pointer_precondition() {
    let (d, b) = dispatcher();
    let id = seed(&b).await;
    let (_, r) = call(
        &d,
        "Principal/set",
        json!({"update": {&id: {"name": "Alice S."}}}),
    )
    .await;
    assert!(r["updated"].get(&id).is_some(), "{r}");

    let rename_if = |may_read: bool| {
        json!({
            "ifUnchangedBy": {&id: {"capabilities/urn:example:share/mayRead": may_read}},
            "update": {&id: {"description": "updated"}}
        })
    };
    let (name, r) = call(&d, "Principal/set", rename_if(false)).await;
    assert_eq!(
        (name.as_str(), r),
        (
            "error",
            json!({"type": "stateMismatch", "failed": [id.clone()]})
        )
    );
    let (name, r) = call(&d, "Principal/set", rename_if(true)).await;
    assert_eq!(name, "Principal/set", "{r}");
    assert!(r["updated"].get(&id).is_some(), "{r}");
}

#[tokio::test]
async fn atomic_failure_rolls_back() {
    let (d, b) = dispatcher();
    let id = seed(&b).await;
    let (_, before) = call(&d, "Principal/get", json!({})).await;

    let (name, r) = call(
        &d,
        "Principal/set",
        json!({
            "atomic": true,
            "create": {"q": principal("Bob")},
            "update": {&id: {"name": "Renamed"}, "nope": {"name": "x"}}
        }),
    )
    .await;
    assert_eq!(name, "error", "{r}");
    assert_eq!(r["type"], "atomicFailure", "{r}");
    assert_eq!(r["notUpdated"]["nope"]["type"], "notFound", "{r}");
    let (_, after) = call(&d, "Principal/get", json!({})).await;
    assert_eq!(after, before, "nothing created or renamed; state unchanged");
}

#[test]
fn ready_after_handlers_and_conditional() {
    let b = Arc::new(MemoryBackend::<()>::default());
    let mut d = Dispatcher::new();
    register_sharing_handlers(&mut d, Arc::clone(&b));
    assert_eq!(d.conditional_gaps().len(), 2, "{:?}", d.conditional_gaps());
    register_sharing_conditional(&mut d, b);
    assert!(
        d.conditional_gaps().is_empty(),
        "{:?}",
        d.conditional_gaps()
    );
    assert!(d.conditional_ready());
}
