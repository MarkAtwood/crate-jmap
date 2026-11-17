//! Object history for Mailbox, Identity and VacationResponse through the
//! dispatcher and the memory backend (bd JMAP-tna1.3.4).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3 (selection), §3.1
//! (`replaced`, `hasOlder`, `expires`), §4 (onlyDestroyed, destruction time
//! descending), §5.1 (restore keeps the id; a current id is `notFound`;
//! "the server MUST order the restores ... a Mailbox is restored before a
//! child Mailbox whose parentId refers to it"), §5.2 (a missing container
//! is rejected "as it would an update"; conflicts use the Foo/set error),
//! §8.2 (no version from before a rights change), and the -04 editor's
//! copy (§3.1 the newest retained version is final; §4 queryChanges takes
//! onlyDestroyed; §5.1 Foo/changes compares sinceState with now). RFC 8621
//! §2.5 for the Mailbox name rule. Times are hand-computed from the fixed
//! clock.

use std::sync::Arc;
use std::time::Duration;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{register_mail_handlers, register_mail_handlers_with_history};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const HISTORY: &str = "urn:ietf:params:jmap:object-history";
const A: &str = "a1";
/// 2026-03-01T00:00:00Z.
const T0: i64 = 1_772_323_200_000;

fn backend() -> MemoryBackend {
    let b = MemoryBackend::new()
        .with_account(&Id::from(A))
        .with_object_history(
            serde_json::from_value(json!({
                "maxHistoryDuration": 2_592_000,
                "supportedTypes": ["Mailbox", "Identity", "VacationResponse"]
            }))
            .unwrap(),
            HistoryRetention::new().max_duration(Some(Duration::from_secs(2_592_000))),
        );
    b.set_history_clock_for_test(T0);
    b
}

struct Server {
    b: MemoryBackend,
    d: Dispatcher<()>,
}

impl Server {
    fn new() -> Self {
        let b = backend();
        let mut d = Dispatcher::new();
        register_mail_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    /// Run `calls` with core, mail and object-history in `using`; the
    /// responses' arguments, each checked to be named `name` where given.
    async fn run(&self, calls: Value) -> Vec<Value> {
        run(&self.d, &[CORE, MAIL, HISTORY], calls).await
    }

    fn at(&self, minutes: i64) {
        self.b.set_history_clock_for_test(T0 + minutes * 60_000);
    }
}

async fn run(d: &Dispatcher<()>, using: &[&str], calls: Value) -> Vec<Value> {
    let req: JmapRequest =
        serde_json::from_value(json!({"using": using, "methodCalls": calls})).unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    resp["methodResponses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            assert_ne!(r[0], "error", "{r}");
            r[1].clone()
        })
        .collect()
}

async fn one(s: &Server, method: &str, args: Value) -> Value {
    let mut args = args;
    args["accountId"] = json!(A);
    s.run(json!([[method, args, "0"]])).await.remove(0)
}

async fn error(s: &Server, method: &str, args: Value) -> String {
    let mut args = args;
    args["accountId"] = json!(A);
    let req: JmapRequest = serde_json::from_value(
        json!({"using": [CORE, MAIL, HISTORY], "methodCalls": [[method, args, "0"]]}),
    )
    .unwrap();
    let resp = serde_json::to_value(s.d.dispatch(req, (), State::from("s")).await).unwrap();
    let r = &resp["methodResponses"][0];
    assert_eq!(r[0], "error", "{r}");
    r[1]["type"].as_str().unwrap().to_owned()
}

async fn create_mailbox(s: &Server, name: &str, parent: Option<&str>) -> String {
    let out = one(
        s,
        "Mailbox/set",
        json!({"create": {"m": {"name": name, "parentId": parent}}}),
    )
    .await;
    out["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{out}"))
        .to_owned()
}

async fn state(s: &Server, t: &str) -> String {
    one(s, &format!("{t}/get"), json!({"ids": []})).await["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// §3/§3.1: three versions of a renamed Mailbox, ascending, with
/// `replaced` from the clock, `expires` = replaced + 2592000 s, and the
/// live version last with nulls.
#[tokio::test]
async fn replaced_versions_of_a_mailbox() {
    let s = Server::new();
    let id = create_mailbox(&s, "One", None).await;
    s.at(1);
    one(&s, "Mailbox/set", json!({"update": {&id: {"name": "Two"}}})).await;
    s.at(2);
    one(
        &s,
        "Mailbox/set",
        json!({"update": {&id: {"name": "Three"}}}),
    )
    .await;
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "includeReplaced": true, "properties": ["name"]}),
    )
    .await;
    assert_eq!(
        got["list"],
        json!([
            {"id": id, "name": "One", "objectHistory": {"version": 1,
              "replaced": "2026-03-01T00:01:00Z", "hasOlder": false,
              "expires": "2026-03-31T00:01:00Z"}},
            {"id": id, "name": "Two", "objectHistory": {"version": 2,
              "replaced": "2026-03-01T00:02:00Z", "hasOlder": true,
              "expires": "2026-03-31T00:02:00Z"}},
            {"id": id, "name": "Three", "objectHistory": {"version": 3,
              "replaced": null, "hasOlder": true, "expires": null}}
        ])
    );
    // Without a flag: the plain Mailbox/get, no objectHistory.
    let plain = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "properties": ["name"]}),
    )
    .await;
    assert_eq!(plain["list"], json!([{"id": id, "name": "Three"}]));
}

/// Destroy, find, restore: §3 includeDestroyed, §4 onlyDestroyed and its
/// queryChanges (-04), §5.1 restore under the same id, and -04 §5.1
/// Foo/changes.
#[tokio::test]
async fn destroy_find_and_restore_a_mailbox() {
    let s = Server::new();
    let id = create_mailbox(&s, "Old", None).await;
    let before_destroy = state(&s, "Mailbox").await;
    s.at(5);
    one(&s, "Mailbox/set", json!({"destroy": [&id]})).await;
    let after_destroy = state(&s, "Mailbox").await;

    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "properties": ["name"]}),
    )
    .await;
    assert_eq!(got["notFound"], json!([&id]));
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "includeDestroyed": true, "properties": ["name"]}),
    )
    .await;
    assert_eq!(got["list"][0]["name"], "Old");
    assert_eq!(
        got["list"][0]["objectHistory"]["replaced"],
        "2026-03-01T00:05:00Z"
    );

    let q = one(&s, "Mailbox/query", json!({"onlyDestroyed": true})).await;
    assert_eq!(q["ids"], json!([&id]));
    let dq_state = q["queryState"].as_str().unwrap().to_owned();
    let q = one(
        &s,
        "Mailbox/query",
        json!({"onlyDestroyed": true, "destroyedAfter": "2026-03-01T00:05:00Z"}),
    )
    .await;
    assert_eq!(q["ids"], json!([]), "strictly after");

    // A current id cannot be restored.
    let live = create_mailbox(&s, "Live", None).await;
    let out = one(
        &s,
        "Mailbox/set",
        json!({"restore": {&live: null, &id: {}}}),
    )
    .await;
    assert_eq!(out["notRestored"][&live]["type"], "notFound", "{out}");
    assert_eq!(out["restored"][&id], Value::Null, "{out}");
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "properties": ["name"]}),
    )
    .await;
    assert_eq!(got["list"], json!([{"id": id, "name": "Old"}]));

    // -04 §4: the destroyed query loses it.
    let qc = one(
        &s,
        "Mailbox/queryChanges",
        json!({"sinceQueryState": dq_state, "onlyDestroyed": true}),
    )
    .await;
    assert_eq!(qc["removed"], json!([&id]));
    assert_eq!(qc["added"], json!([]));

    // -04 §5.1: existed at sinceState and now ⇒ updated; not at
    // sinceState ⇒ created.
    let ch = one(&s, "Mailbox/changes", json!({"sinceState": before_destroy})).await;
    assert!(
        ch["updated"].as_array().unwrap().contains(&json!(id)),
        "{ch}"
    );
    assert!(
        !ch["destroyed"].as_array().unwrap().contains(&json!(id)),
        "{ch}"
    );
    let ch = one(&s, "Mailbox/changes", json!({"sinceState": after_destroy})).await;
    assert!(
        ch["created"].as_array().unwrap().contains(&json!(id)),
        "{ch}"
    );

    // History continues: the destroyed version is now a previous one.
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "includeReplaced": true, "properties": ["name"]}),
    )
    .await;
    let versions: Vec<u64> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["objectHistory"]["version"].as_u64().unwrap())
        .collect();
    assert_eq!(versions, [1, 2]);
}

/// -04 §5.1: a child restored with its parent in one call, in either key
/// order; alone, it fails as an update naming a missing parent would.
#[tokio::test]
async fn parent_and_child_restore_in_either_order() {
    let s = Server::new();
    let parent = create_mailbox(&s, "P", None).await;
    let child = create_mailbox(&s, "C", Some(&parent)).await;
    one(&s, "Mailbox/set", json!({"destroy": [&child]})).await;
    one(&s, "Mailbox/set", json!({"destroy": [&parent]})).await;

    let out = one(&s, "Mailbox/set", json!({"restore": {&child: null}})).await;
    assert_eq!(out["notRestored"][&child]["type"], "invalidProperties");
    assert_eq!(
        out["notRestored"][&child]["properties"],
        json!(["parentId"])
    );

    // Keys sorted so the child comes first whichever id is smaller.
    let (first, second) = if child < parent {
        (&child, &parent)
    } else {
        (&parent, &child)
    };
    let mut restore = serde_json::Map::new();
    restore.insert(first.clone(), Value::Null);
    restore.insert(second.clone(), Value::Null);
    let out = one(&s, "Mailbox/set", json!({"restore": restore})).await;
    assert_eq!(out["notRestored"], Value::Null, "{out}");
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&child], "properties": ["parentId"]}),
    )
    .await;
    assert_eq!(got["list"][0]["parentId"], json!(parent));
}

/// §5.2: a restore that conflicts with a current object gets the Foo/set
/// error, here RFC 8621 §2.5's `alreadyExists` for a sibling name.
#[tokio::test]
async fn name_clash_is_already_exists() {
    let s = Server::new();
    let id = create_mailbox(&s, "Same", None).await;
    one(&s, "Mailbox/set", json!({"destroy": [&id]})).await;
    create_mailbox(&s, "Same", None).await;
    let out = one(&s, "Mailbox/set", json!({"restore": {&id: null}})).await;
    assert_eq!(out["notRestored"][&id]["type"], "alreadyExists");
    // Renaming in the patch avoids it.
    let out = one(
        &s,
        "Mailbox/set",
        json!({"restore": {&id: {"name": "Other"}}}),
    )
    .await;
    assert_eq!(out["restored"][&id], Value::Null, "{out}");
}

/// -04 §3.1: with the version current at destruction discarded, the older
/// retained version is the final one, and its `replaced` stands for the
/// destruction time.
#[tokio::test]
async fn discarded_final_version_falls_back_to_the_older_one() {
    let s = Server::new();
    let id = create_mailbox(&s, "First", None).await;
    s.at(1);
    one(
        &s,
        "Mailbox/set",
        json!({"update": {&id: {"name": "Second"}}}),
    )
    .await;
    s.at(9);
    one(&s, "Mailbox/set", json!({"destroy": [&id]})).await;
    s.b.discard_version_for_test("Mailbox", &Id::from(A), &Id::from(id.as_str()), 2);
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "includeDestroyed": true, "properties": ["name"]}),
    )
    .await;
    assert_eq!(got["list"][0]["name"], "First");
    assert_eq!(
        got["list"][0]["objectHistory"]["replaced"],
        "2026-03-01T00:01:00Z"
    );
    let q = one(
        &s,
        "Mailbox/query",
        json!({"onlyDestroyed": true, "destroyedBefore": "2026-03-01T00:02:00Z"}),
    )
    .await;
    assert_eq!(q["ids"], json!([&id]));
    let out = one(&s, "Mailbox/set", json!({"restore": {&id: null}})).await;
    assert_eq!(out["restored"][&id], Value::Null, "{out}");
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "properties": ["name"]}),
    )
    .await;
    assert_eq!(got["list"][0]["name"], "First");
}

/// §8.2: versions from before a rights change are not returned.
#[tokio::test]
async fn rights_cut_hides_older_versions() {
    let s = Server::new();
    let id = create_mailbox(&s, "A", None).await;
    s.at(1);
    one(&s, "Mailbox/set", json!({"update": {&id: {"name": "B"}}})).await;
    s.at(3);
    one(&s, "Mailbox/set", json!({"update": {&id: {"name": "C"}}})).await;
    s.b.set_rights_changed_at_for_test("Mailbox", &Id::from(A), T0 + 2 * 60_000);
    let got = one(
        &s,
        "Mailbox/get",
        json!({"ids": [&id], "includeReplaced": true, "properties": ["name"]}),
    )
    .await;
    let names: Vec<&str> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["B", "C"]);
}

/// Identity restores; VacationResponse is never destroyed, so its restore
/// is `notFound`, while its replaced versions are history.
#[tokio::test]
async fn identity_and_vacation_response() {
    let s = Server::new();
    let out = one(
        &s,
        "Identity/set",
        json!({"create": {"i": {"name": "Me", "email": "me@example.com"}}}),
    )
    .await;
    let id = out["created"]["i"]["id"].as_str().unwrap().to_owned();
    one(&s, "Identity/set", json!({"destroy": [&id]})).await;
    let out = one(&s, "Identity/set", json!({"restore": {&id: null}})).await;
    assert!(out["restored"].get(&id).is_some(), "{out}");

    one(
        &s,
        "VacationResponse/set",
        json!({"update": {"singleton": {"isEnabled": true}}}),
    )
    .await;
    let out = one(
        &s,
        "VacationResponse/set",
        json!({"restore": {"singleton": null}}),
    )
    .await;
    assert_eq!(out["notRestored"]["singleton"]["type"], "notFound");
    let got = one(
        &s,
        "VacationResponse/get",
        json!({"ids": ["singleton"], "includeReplaced": true, "properties": ["isEnabled"]}),
    )
    .await;
    let enabled: Vec<&Value> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["isEnabled"])
        .collect();
    assert_eq!(enabled, [&json!(false), &json!(true)], "{got}");
}

/// The registration marks every Mailbox, Identity and VacationResponse
/// method that takes history arguments.
#[test]
fn no_gaps_after_registration() {
    let mut d = Dispatcher::new();
    register_mail_handlers_with_history(&mut d, Arc::new(backend()));
    assert!(
        d.object_history_gaps(&["Mailbox", "Identity", "VacationResponse"])
            .is_empty()
    );
    let mut d = Dispatcher::new();
    register_mail_handlers(&mut d, Arc::new(backend()));
    assert_eq!(
        d.object_history_gaps(&["Identity"]),
        ["Identity/get", "Identity/set"]
    );
}

/// The plain registration answers a call without history arguments the
/// same as the history-aware one, byte for byte.
#[tokio::test]
async fn plain_and_history_registration_agree_without_history_arguments() {
    let calls = json!([
        ["Identity/set", {"accountId": A, "create": {"i": {"name": "Me", "email": "me@example.com"}}}, "0"],
        ["Identity/get", {"accountId": A, "ids": null, "properties": ["name", "email"]}, "1"],
        ["VacationResponse/get", {"accountId": A, "ids": ["singleton"]}, "2"],
        ["Mailbox/query", {"accountId": A}, "3"]
    ]);
    let mut out = Vec::new();
    for history in [false, true] {
        let b = backend();
        let mut d = Dispatcher::new();
        if history {
            register_mail_handlers_with_history(&mut d, Arc::new(b));
        } else {
            register_mail_handlers(&mut d, Arc::new(b));
        }
        let mut r = run(&d, &[CORE, MAIL], calls.clone()).await;
        // Identity ids are random.
        r[1]["list"][0]["id"] = Value::Null;
        r[0]["created"]["i"]["id"] = Value::Null;
        out.push(r);
    }
    assert_eq!(out[0], out[1]);
}

/// §3.1: a flag for a type the account does not list is
/// `invalidArguments`; here the backend keeps no history at all.
#[tokio::test]
async fn without_history_configured_flags_are_invalid_arguments() {
    let b = MemoryBackend::new().with_account(&Id::from(A));
    let mut d = Dispatcher::new();
    register_mail_handlers_with_history(&mut d, Arc::new(b.clone()));
    let s = Server { b, d };
    assert_eq!(
        error(
            &s,
            "Mailbox/get",
            json!({"ids": null, "includeDestroyed": true})
        )
        .await,
        "invalidArguments"
    );
    assert_eq!(
        error(&s, "Mailbox/query", json!({"onlyDestroyed": true})).await,
        "invalidArguments"
    );
    assert_eq!(
        error(&s, "Identity/set", json!({"restore": {"x": null}})).await,
        "invalidArguments"
    );
}
