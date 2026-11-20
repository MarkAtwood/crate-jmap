//! Object history for TaskList and Task through the dispatcher and the
//! memory backend (bd JMAP-tna1.3.13).
//!
//! Oracles: draft-ietf-jmap-object-history-03 §3 (an
//! `onDestroyRemoveTasks` cascade counts as destroyed), §4 (onlyDestroyed),
//! §5.2 (a missing container fails as an update would; restore it in an
//! earlier call, -04), §7.4 (`objectHistory` is reserved in JSCalendar
//! Task), the -04 editor's copy §5.1 (Foo/changes compares sinceState with
//! now), and docs/design/DECISIONS.md decisions 5, 22, 23 (Task and
//! TaskList have history ahead of their registration; TaskNotification
//! has none).

use std::sync::Arc;

use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_tasks_server::memory::MemoryBackend;
use jmap_tasks_server::{register_tasks_handlers, register_tasks_handlers_with_history};
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const A: &str = "abc";
const USING: [&str; 3] = [
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:tasks",
    "urn:ietf:params:jmap:object-history",
];

fn backend() -> MemoryBackend<()> {
    let b = MemoryBackend::<()>::default()
        .with_account(&Id::from(A))
        .with_object_history(
            serde_json::from_value(
                json!({"maxHistoryDuration": null, "supportedTypes": ["TaskList", "Task"]}),
            )
            .unwrap(),
            HistoryRetention::new(),
        );
    b.set_history_clock_for_test(1_000);
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
        register_tasks_handlers_with_history(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    async fn run(&self, calls: Value) -> Vec<Value> {
        let req: JmapRequest =
            serde_json::from_value(json!({"using": USING, "methodCalls": calls})).unwrap();
        let resp = serde_json::to_value(self.d.dispatch(req, (), State::from("s")).await).unwrap();
        resp["methodResponses"].as_array().unwrap().clone()
    }

    async fn one(&self, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(A);
        let r = self.run(json!([[method, args, "0"]])).await.remove(0);
        assert_ne!(r[0], "error", "{r}");
        r[1].clone()
    }

    async fn create(&self, method: &str, object: Value) -> String {
        let out = self.one(method, json!({"create": {"x": object}})).await;
        out["created"]["x"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{out}"))
            .to_owned()
    }

    async fn state(&self, t: &str) -> Value {
        self.one(&format!("{t}/get"), json!({"ids": []})).await["state"].clone()
    }
}

fn task(list: &str, title: &str) -> Value {
    json!({"@type": "Task", "uid": title, "title": title, "taskListId": list})
}

#[tokio::test]
async fn task_versions_cascade_and_restore() {
    let s = Server::new();
    let list = s.create("TaskList/set", json!({"name": "Chores"})).await;
    let t = s.create("Task/set", task(&list, "Dishes")).await;
    s.b.set_history_clock_for_test(61_000);
    s.one(
        "Task/set",
        json!({"update": {&t: {"title": "Dishes and pans"}}}),
    )
    .await;
    let got = s
        .one(
            "Task/get",
            json!({"ids": [&t], "includeReplaced": true, "properties": ["title"]}),
        )
        .await;
    let titles: Vec<&str> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Dishes", "Dishes and pans"]);

    let since_list = s.state("TaskList").await;
    let since_task = s.state("Task").await;
    s.b.set_history_clock_for_test(121_000);
    let out = s
        .one(
            "TaskList/set",
            json!({"destroy": [&list], "onDestroyRemoveTasks": true}),
        )
        .await;
    assert_eq!(out["destroyed"], json!([&list]), "{out}");

    let q = s
        .one(
            "Task/query",
            json!({"onlyDestroyed": true, "filter": {"inTaskLists": [&list]}}),
        )
        .await;
    assert_eq!(q["ids"], json!([&t]), "{q}");
    let r = s
        .run(
            json!([["Task/query", {"accountId": A, "onlyDestroyed": true,
            "filter": {"title": "x"}}, "0"]]),
        )
        .await;
    assert_eq!(r[0][1]["type"], "unsupportedFilter", "{}", r[0]);

    let out = s.one("Task/set", json!({"restore": {&t: null}})).await;
    assert_eq!(
        out["notRestored"][&t]["properties"],
        json!(["taskListId"]),
        "{out}"
    );
    let r = s
        .run(json!([
            ["TaskList/set", {"accountId": A, "restore": {&list: null}}, "0"],
            ["Task/set", {"accountId": A, "restore": {&t: null}}, "1"]
        ]))
        .await;
    assert_eq!(r[0][1]["restored"], json!({&list: null}), "{}", r[0]);
    assert_eq!(r[1][1]["restored"], json!({&t: null}), "{}", r[1]);

    let ch = s
        .one("TaskList/changes", json!({"sinceState": since_list}))
        .await;
    assert_eq!(
        (&ch["updated"], &ch["destroyed"]),
        (&json!([&list]), &json!([])),
        "{ch}"
    );
    let ch = s
        .one("Task/changes", json!({"sinceState": since_task}))
        .await;
    assert_eq!(
        (&ch["updated"], &ch["destroyed"]),
        (&json!([&t]), &json!([])),
        "{ch}"
    );

    // The restored task counts toward its list again: destroying the list
    // without onDestroyRemoveTasks is refused.
    let out = s.one("TaskList/set", json!({"destroy": [&list]})).await;
    assert_eq!(out["destroyed"], Value::Null, "{out}");
}

/// §7.4: `objectHistory` is not a stored Task property.
#[tokio::test]
async fn object_history_in_a_task_create_is_invalid_properties() {
    let s = Server::new();
    let list = s.create("TaskList/set", json!({"name": "L"})).await;
    let mut t = task(&list, "T");
    t["objectHistory"] = json!({});
    let out = s.one("Task/set", json!({"create": {"t": t}})).await;
    assert_eq!(
        out["notCreated"]["t"]["properties"],
        json!(["objectHistory"]),
        "{out}"
    );
}

/// TaskNotification keeps no history (decision 22): a history flag for
/// it is `invalidArguments` (§3.1), from the dispatcher, since its methods
/// are not history-aware.
#[tokio::test]
async fn task_notification_has_no_history() {
    let s = Server::new();
    let r = s
        .run(
            json!([["TaskNotification/get", {"accountId": A, "ids": null,
            "includeDestroyed": true}, "0"]]),
        )
        .await;
    assert_eq!(
        (&r[0][0], &r[0][1]["type"]),
        (&json!("error"), &json!("invalidArguments")),
        "{}",
        r[0]
    );
}

#[test]
fn registration_has_no_gaps() {
    let mut d = Dispatcher::new();
    register_tasks_handlers_with_history(&mut d, Arc::new(backend()));
    assert!(d.object_history_gaps(&["TaskList", "Task"]).is_empty());
    let mut d = Dispatcher::new();
    register_tasks_handlers(&mut d, Arc::new(backend()));
    assert_eq!(
        d.object_history_gaps(&["TaskList"]),
        ["TaskList/get", "TaskList/set"]
    );
}
