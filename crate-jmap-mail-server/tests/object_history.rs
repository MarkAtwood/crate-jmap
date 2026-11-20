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
const A: &str = "abc";
/// 2026-03-01T00:00:00Z.
const T0: i64 = 1_772_323_200_000;

fn supported() -> Vec<&'static str> {
    let mut t = vec!["Mailbox", "Email", "Identity", "VacationResponse"];
    if cfg!(feature = "sieve") {
        t.push("SieveScript");
    }
    t
}

fn backend() -> MemoryBackend<()> {
    let b = MemoryBackend::<()>::default()
        .with_account(&Id::from(A))
        .with_object_history(
            serde_json::from_value(json!({
                "maxHistoryDuration": 2_592_000,
                "supportedTypes": supported()
            }))
            .unwrap(),
            HistoryRetention::new().max_duration(Some(Duration::from_secs(2_592_000))),
        );
    b.set_history_clock_for_test(T0);
    b
}

struct Server {
    b: MemoryBackend<()>,
    d: Dispatcher<()>,
}

impl Server {
    fn new() -> Self {
        let b = backend();
        let mut d = Dispatcher::new();
        register_mail_handlers_with_history(&mut d, Arc::new(b.clone()));
        #[cfg(feature = "sieve")]
        jmap_mail_server::register_sieve_handlers_with_history(&mut d, Arc::new(b.clone()));
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
    let mut using = using.to_vec();
    if cfg!(feature = "sieve") && using.contains(&HISTORY) {
        using.push("urn:ietf:params:jmap:sieve");
    }
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
        json!({"using": [CORE, MAIL, HISTORY, "urn:ietf:params:jmap:sieve"], "methodCalls": [[method, args, "0"]]}),
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
    let b = MemoryBackend::<()>::default().with_account(&Id::from(A));
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

// ---- Email (bd JMAP-tna1.3.5) -------------------------------------------

/// 2026-03-15T16:30:00Z, the §3.2.2 destruction time of email42.
const T_DESTROY: i64 = 1_773_592_200_000;

/// The §3.2.2 account: email42 in mbOld (Thread T42) and email57 in the
/// Inbox mb1 (Thread T57), both destroyed a minute apart. Returns the Email
/// state from before the destroys.
async fn emails_destroyed(s: &Server) -> String {
    let acc = Id::from(A);
    for (id, name) in [("mbOld", "Old"), ("mb1", "Inbox")] {
        s.b.seed_object(
            &acc,
            "Mailbox",
            &Id::from(id),
            json!({"id": id, "name": name, "sortOrder": 0, "totalEmails": 0,
                   "unreadEmails": 0, "totalThreads": 0, "unreadThreads": 0,
                   "isSubscribed": true, "myRights": {"mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true, "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true, "mayRename": true, "mayDelete": true, "maySubmit": true}}),
        );
    }
    for (id, thread, mailbox, subject, name, addr, received) in [
        (
            "email42",
            "T42",
            "mbOld",
            "Meeting notes",
            "Alice",
            "alice@example.com",
            "2026-03-10T09:00:00Z",
        ),
        (
            "email57",
            "T57",
            "mb1",
            "Project update",
            "Bob",
            "bob@example.com",
            "2026-03-12T14:00:00Z",
        ),
    ] {
        s.b.seed_object(
            &acc,
            "Thread",
            &Id::from(thread),
            json!({"id": thread, "emailIds": [id]}),
        );
        s.b.seed_object(
            &acc,
            "Email",
            &Id::from(id),
            json!({"id": id, "blobId": format!("blob-{id}"), "threadId": thread,
                   "mailboxIds": {mailbox: true}, "keywords": {}, "size": 1000,
                   "receivedAt": received, "subject": subject,
                   "from": [{"name": name, "email": addr}]}),
        );
    }
    let before = state(s, "Email").await;
    s.b.set_history_clock_for_test(T_DESTROY);
    one(s, "Email/set", json!({"destroy": ["email42"]})).await;
    s.b.set_history_clock_for_test(T_DESTROY + 60_000);
    one(s, "Email/set", json!({"destroy": ["email57"]})).await;
    before
}

/// The §3.2.2 Email/get response list, verbatim.
fn example_list() -> Value {
    json!([
      {
        "id": "email42",
        "subject": "Meeting notes",
        "from": [{"name": "Alice", "email": "alice@example.com"}],
        "receivedAt": "2026-03-10T09:00:00Z",
        "objectHistory": {"version": 1, "replaced": "2026-03-15T16:30:00Z",
                          "hasOlder": false, "expires": "2026-04-14T16:30:00Z"}
      },
      {
        "id": "email57",
        "subject": "Project update",
        "from": [{"name": "Bob", "email": "bob@example.com"}],
        "receivedAt": "2026-03-12T14:00:00Z",
        "objectHistory": {"version": 1, "replaced": "2026-03-15T16:31:00Z",
                          "hasOlder": false, "expires": "2026-04-14T16:31:00Z"}
      }
    ])
}

fn sorted_by_id(list: &Value) -> Value {
    let mut v = list.as_array().unwrap().clone();
    v.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Value::Array(v)
}

/// §3.2.2: Email/changes, then Email/get on its `destroyed` with
/// includeDestroyed returns each Email's final version.
#[tokio::test]
async fn example_3_2_2_recovering_destroyed_emails() {
    let s = Server::new();
    let since = emails_destroyed(&s).await;
    let r = s
        .run(json!([
            ["Email/changes", {"accountId": A, "sinceState": since}, "0"],
            ["Email/get", {"accountId": A,
                "#ids": {"resultOf": "0", "name": "Email/changes", "path": "/destroyed"},
                "properties": ["subject", "from", "receivedAt"],
                "includeDestroyed": true}, "1"]
        ]))
        .await;
    // Email/changes lists `destroyed` in no particular order.
    assert_eq!(sorted_by_id(&r[1]["list"]), example_list());
    assert_eq!(r[1]["notFound"], json!([]));
}

/// §4.1, then §5.3 and the queryChanges that follows (-04 §4): the
/// destroyed query, most recent first; the restore into the Inbox,
/// reported as `{"email42": null}`; the query then loses email42.
#[tokio::test]
async fn example_4_1_and_5_3_query_restore_query_changes() {
    let s = Server::new();
    emails_destroyed(&s).await;
    let r = s
        .run(json!([
            ["Email/query", {"accountId": A, "onlyDestroyed": true,
                "destroyedAfter": "2026-03-01T00:00:00Z", "limit": 50}, "0"],
            ["Email/get", {"accountId": A,
                "#ids": {"resultOf": "0", "name": "Email/query", "path": "/ids"},
                "properties": ["subject", "from", "receivedAt"],
                "includeDestroyed": true}, "1"]
        ]))
        .await;
    let q = &r[0];
    assert_eq!(q["ids"], json!(["email57", "email42"]));
    assert_eq!(
        (&q["position"], &q["canCalculateChanges"]),
        (&json!(0), &json!(true))
    );
    assert_eq!(sorted_by_id(&r[1]["list"]), example_list());
    let query_state = q["queryState"].clone();

    // The Mailbox email42 was in is destroyed since.
    one(&s, "Mailbox/set", json!({"destroy": ["mbOld"]})).await;
    // §5.2: restoring into it fails as an update naming it would.
    let out = one(&s, "Email/set", json!({"restore": {"email42": null}})).await;
    assert_eq!(
        out["notRestored"]["email42"]["properties"],
        json!(["mailboxIds"])
    );

    // §5.3 verbatim, but for the states.
    let mut out = one(
        &s,
        "Email/set",
        json!({"restore": {"email42": {"mailboxIds": {"mb1": true}}}}),
    )
    .await;
    let map = out.as_object_mut().unwrap();
    assert_ne!(map.remove("oldState"), map.remove("newState"));
    assert_eq!(
        out,
        json!({"accountId": A, "created": null, "updated": null, "destroyed": null,
               "notCreated": null, "notUpdated": null, "notDestroyed": null,
               "restored": {"email42": null}, "notRestored": null})
    );
    // -04 §5.1: the same threadId, and the Thread holds it again.
    let r = s
        .run(json!([
            ["Email/get", {"accountId": A, "ids": ["email42"], "properties": ["threadId", "blobId", "mailboxIds"]}, "0"],
            ["Thread/get", {"accountId": A, "ids": ["T42"]}, "1"]
        ]))
        .await;
    assert_eq!(
        r[0]["list"][0],
        json!({"id": "email42", "threadId": "T42", "blobId": "blob-email42", "mailboxIds": {"mb1": true}})
    );
    assert_eq!(r[1]["list"][0]["emailIds"], json!(["email42"]));

    // -04 §4: queryChanges with the Foo/query values.
    let qc = one(
        &s,
        "Email/queryChanges",
        json!({"sinceQueryState": query_state, "onlyDestroyed": true,
               "destroyedAfter": "2026-03-01T00:00:00Z"}),
    )
    .await;
    assert_eq!(qc["removed"], json!(["email42"]));
    assert_eq!(qc["added"], json!([]));
}

/// §3: an Email destroyed by `onDestroyRemoveEmails` counts as destroyed.
#[tokio::test]
async fn cascade_destroys_are_history() {
    let s = Server::new();
    let acc = Id::from(A);
    s.b.seed_object(
        &acc,
        "Mailbox",
        &Id::from("mbX"),
        json!({"id": "mbX", "name": "X", "sortOrder": 0, "totalEmails": 1,
               "unreadEmails": 1, "totalThreads": 1, "unreadThreads": 1,
               "isSubscribed": true, "myRights": {"mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true, "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true, "mayRename": true, "mayDelete": true, "maySubmit": true}}),
    );
    s.b.seed_object(
        &acc,
        "Thread",
        &Id::from("T1"),
        json!({"id": "T1", "emailIds": ["e1"]}),
    );
    s.b.seed_object(
        &acc,
        "Email",
        &Id::from("e1"),
        json!({"id": "e1", "blobId": "b1", "threadId": "T1", "mailboxIds": {"mbX": true},
               "keywords": {}, "size": 10, "receivedAt": "2026-03-01T00:00:00Z"}),
    );
    let out = one(
        &s,
        "Mailbox/set",
        json!({"destroy": ["mbX"], "onDestroyRemoveEmails": true}),
    )
    .await;
    assert_eq!(out["destroyed"], json!(["mbX"]), "{out}");
    let got = one(
        &s,
        "Email/get",
        json!({"ids": ["e1"], "includeDestroyed": true, "properties": ["blobId"]}),
    )
    .await;
    assert_eq!(got["list"][0]["blobId"], "b1", "{got}");
    let q = one(&s, "Email/query", json!({"onlyDestroyed": true})).await;
    assert_eq!(q["ids"], json!(["e1"]));
}

// ---- SieveScript (bd JMAP-tna1.3.8) -------------------------------------

#[cfg(feature = "sieve")]
async fn create_script(s: &Server, name: &str) -> String {
    s.b.store_blob(&Id::from("script"), b"keep;".to_vec());
    let out = one(
        s,
        "SieveScript/set",
        json!({"create": {"s": {"name": name, "blobId": "script"}}}),
    )
    .await;
    out["created"]["s"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{out}"))
        .to_owned()
}

/// §4 over SieveScript (handler-side filter), §5.1/§5.2: a restored script
/// is inactive and can be activated by `onSuccessActivateScript` in the
/// same call; a name now taken is `alreadyExists`.
#[cfg(feature = "sieve")]
#[tokio::test]
async fn sieve_scripts_destroyed_queried_and_restored() {
    let s = Server::new();
    let one_id = create_script(&s, "one").await;
    let two_id = create_script(&s, "two").await;
    s.at(1);
    one(&s, "SieveScript/set", json!({"destroy": [&one_id]})).await;
    s.at(2);
    one(&s, "SieveScript/set", json!({"destroy": [&two_id]})).await;

    let q = one(&s, "SieveScript/query", json!({"onlyDestroyed": true})).await;
    assert_eq!(q["ids"], json!([&two_id, &one_id]), "most recent first");
    let q = one(
        &s,
        "SieveScript/query",
        json!({"onlyDestroyed": true, "filter": {"name": "one"}}),
    )
    .await;
    assert_eq!(q["ids"], json!([&one_id]));
    let q = one(
        &s,
        "SieveScript/query",
        json!({"onlyDestroyed": true, "destroyedBefore": "2026-03-01T00:02:00Z"}),
    )
    .await;
    assert_eq!(q["ids"], json!([&one_id]), "strictly before");
    let live = one(&s, "SieveScript/query", json!({})).await;
    assert_eq!(live["ids"], json!([]));

    let out = one(
        &s,
        "SieveScript/set",
        json!({"restore": {&one_id: null}, "onSuccessActivateScript": &one_id}),
    )
    .await;
    assert!(out["restored"].get(&one_id).is_some(), "{out}");
    let got = one(&s, "SieveScript/get", json!({"ids": [&one_id]})).await;
    assert_eq!(got["list"][0]["isActive"], true, "{got}");

    create_script(&s, "two").await;
    let out = one(&s, "SieveScript/set", json!({"restore": {&two_id: null}})).await;
    assert_eq!(out["notRestored"][&two_id]["type"], "alreadyExists");
    let out = one(
        &s,
        "SieveScript/set",
        json!({"restore": {&two_id: {"name": "two again"}}}),
    )
    .await;
    assert!(out["restored"].get(&two_id).is_some(), "{out}");
    let got = one(&s, "SieveScript/get", json!({"ids": [&two_id]})).await;
    assert_eq!(got["list"][0]["isActive"], false);

    let got = one(
        &s,
        "SieveScript/get",
        json!({"ids": [&two_id], "includeReplaced": true, "properties": ["name"]}),
    )
    .await;
    let names: Vec<&str> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["two", "two again"]);
}
