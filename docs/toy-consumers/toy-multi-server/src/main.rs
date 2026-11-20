//! Toy consumer: one `Dispatcher` serving several extension families.
//!
//! Mail (with the jmap-emailpush, jmap-sieve and jmap-mdn facades), Tasks,
//! FileNode (two users), Graph (with provenance), VTC, conditional-02 (via
//! jmap-conditional) and object history, all on the reference memory
//! backends, behind one `Dispatcher<Id>` whose caller is the Principal id.
//!
//! Every reference backend is `MemoryBackend<C = Id>` over
//! `jmap_server::MemoryCaller` (finding b1, fixed), so each family
//! registers its handlers, and its conditional-02 `SetSupport`, on the one
//! dispatcher directly: no bridge handler.

use std::sync::Arc;

use jmap_filenode::server::memory::MemoryBackend as FileNodeMemory;
use jmap_graph::server::memory::MemoryBackend as GraphMemory;
use jmap_object_history::server::HistoryRetention;
use jmap_object_history::types::ObjectHistoryAccountCapability;
use jmap_server::{
    Dispatcher, register_push_subscription_conditional, register_push_subscription_handlers,
};
use jmap_types::{Id, Invocation, JmapRequest, State};
use serde_json::{Value, json};

const MAIL: &str = "M1";
const TASKS: &str = "T1";
const FILES: &str = "F1";
const GRAPH: &str = "G1";
const VTC: &str = "V1";

fn history_capability(types: &[&str]) -> ObjectHistoryAccountCapability {
    // Finding b3, fixed: a constructor.
    ObjectHistoryAccountCapability::new(
        Some(86400),
        types.iter().map(|t| (*t).to_owned()).collect(),
    )
}

fn using() -> Vec<String> {
    [
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:mail",
        "urn:ietf:params:jmap:sieve",
        "urn:ietf:params:jmap:mdn",
        jmap_emailpush::CAPABILITY,
        "urn:ietf:params:jmap:tasks",
        "urn:ietf:params:jmap:filenode",
        "urn:ietf:params:jmap:graph",
        "urn:ietf:params:jmap:graph:provenance",
        "urn:ietf:params:jmap:vtc",
        jmap_conditional::CAPABILITY,
        jmap_object_history::CAPABILITY,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

async fn dispatch(d: &Dispatcher<Id>, user: &str, calls: Vec<(&str, Value)>) -> Vec<Invocation> {
    let calls = calls
        .into_iter()
        .enumerate()
        .map(|(i, (name, args))| Invocation::new(name, args, format!("c{i}")))
        .collect();
    d.dispatch(
        JmapRequest::new(using(), calls, None),
        Id::from(user),
        State::from("s0"),
    )
    .await
    .method_responses
}

#[tokio::main]
async fn main() {
    let retention =
        HistoryRetention::new().max_duration(Some(std::time::Duration::from_secs(86400)));

    // ---- One dispatcher for every family ----
    let mut d: Dispatcher<Id> = Dispatcher::new();

    // ---- Mail, with EmailPush, Sieve and MDN, owned by alice ----
    let mail = Arc::new(
        jmap_mail_server::memory::MemoryBackend::new()
            .with_account_owner(&Id::from(MAIL), &Id::from("alice"))
            .with_object_history(history_capability(&["Mailbox"]), retention),
    );
    jmap_mail_server::register_mail_handlers_with_history(&mut d, Arc::clone(&mail));
    jmap_sieve::server::register_sieve_handlers(&mut d, Arc::clone(&mail));
    jmap_mdn::server::register_mdn_handlers(&mut d, Arc::clone(&mail));
    register_push_subscription_handlers(&mut d, Arc::clone(&mail));
    jmap_mail_server::register_mail_conditional(&mut d, Arc::clone(&mail));
    register_push_subscription_conditional(&mut d, Arc::clone(&mail))
        .expect("PushSubscription conditional");

    // ---- Tasks ----
    let tasks =
        Arc::new(jmap_tasks_server::memory::MemoryBackend::new().with_account(&Id::from(TASKS)));
    jmap_tasks_server::register_tasks_handlers(&mut d, Arc::clone(&tasks));
    jmap_tasks_server::register_tasks_conditional(&mut d, Arc::clone(&tasks));

    // ---- VTC ----
    // Finding b2, fixed: `with_account` is public.
    let vtc_capability = serde_json::from_value(json!({
        "mayCreateCall": true, "supportsRingCalls": false, "supportsRoomCalls": true,
        "supportsScheduledCalls": false, "supportsRecording": false,
        "supportsLivestream": false, "livestreamModes": [], "livestreamPushSchemes": [], "layouts": [], "supportsLobby": false,
        "supportsBreakoutRooms": false, "supportedMediaTypes": ["audio", "video"]
    }))
    .expect("VtcCapability");
    let vtc = Arc::new(
        jmap_vtc::server::memory::MemoryBackend::new().with_account(&Id::from(VTC), vtc_capability),
    );
    jmap_vtc::server::register_vtc_handlers(&mut d, Arc::clone(&vtc));
    jmap_vtc::server::register_vtc_conditional(&mut d, Arc::clone(&vtc));

    // ---- FileNode, owned by alice ----
    let files = Arc::new(
        FileNodeMemory::new()
            .with_account_owner(&Id::from(FILES), &Id::from("alice"))
            .with_object_history(history_capability(&["FileNode"]), retention),
    );
    jmap_filenode::server::register_filenode_handlers_with_history(&mut d, Arc::clone(&files));
    jmap_filenode::server::register_filenode_conditional(&mut d, Arc::clone(&files))
        .expect("FileNode conditional");

    // ---- Graph, owned by alice, with provenance (finding b2, fixed) ----
    let graph_capability =
        serde_json::from_value(json!({ "maxAnnotationSize": null })).expect("GraphCapability");
    let graph = Arc::new(GraphMemory::new().with_account(
        &Id::from(GRAPH),
        &Id::from("alice"),
        graph_capability,
    ));
    graph.register_provenance(&Id::from(GRAPH));
    jmap_graph::server::register_graph_handlers(&mut d, Arc::clone(&graph));
    jmap_graph::server::register_graph_conditional(&mut d, Arc::clone(&graph));

    // Every Foo/set of every family has conditional-02 support, so the
    // server may advertise urn:ietf:params:jmap:conditional.
    assert!(
        d.conditional_ready(),
        "conditional gaps: {:?}",
        d.conditional_gaps()
    );
    let front = d;

    // ---- Setup request: create in each family ----
    let read_only = json!({ "mayRead": true, "mayAddChildren": false, "mayRename": false,
                            "mayDelete": false, "mayModifyContent": false, "mayShare": false });
    let rs = dispatch(
        &front,
        "alice",
        vec![
            (
                "Mailbox/set",
                json!({ "accountId": MAIL, "create": { "inbox": { "name": "Inbox" } } }),
            ),
            (
                "TaskList/set",
                json!({ "accountId": TASKS, "create": { "l": { "name": "Chores" } } }),
            ),
            (
                "Task/set",
                json!({ "accountId": TASKS, "create": { "t": {
                "taskListId": "#l", "title": "Water plants",
                "start": "2026-11-20T09:00:00", "timeZone": "Etc/UTC",
            } } }),
            ),
            ("Graph/set", json!({ "accountId": GRAPH, "create": { "g": { "name": "Notes" } } })),
            // conditional-02: an atomic FileNode/set.
            (
                "FileNode/set",
                json!({ "accountId": FILES, "atomic": true, "create": {
                "dir": { "@type": "Directory", "name": "shared", "parentId": null,
                         "shareWith": { "bob": read_only } },
                "priv": { "@type": "Directory", "name": "private", "parentId": null }
            } }),
            ),
        ],
    )
    .await;
    print(&rs, "setup");
    let created = |i: usize, key: &str| -> String {
        let r = &rs[i];
        r.arguments["created"][key]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{} did not create {key}: {}", r.name, r.arguments))
            .to_owned()
    };
    let inbox = created(0, "inbox");
    let list = created(1, "l");
    let task = created(2, "t");
    let graph_id = created(3, "g");
    let dir = created(4, "dir");
    let _private = created(4, "priv");

    // ---- One request touching each family ----
    let rs = dispatch(
        &front,
        "alice",
        vec![
            // Mail, with object history on Mailbox.
            ("Mailbox/set", json!({ "accountId": MAIL, "update": { &inbox: { "name": "In" } } })),
            ("Mailbox/get", json!({ "accountId": MAIL, "ids": [&inbox], "includeReplaced": true })),
            // The Sieve, MDN and EmailPush facades.
            ("SieveScript/get", json!({ "accountId": MAIL, "ids": null })),
            ("MDN/parse", json!({ "accountId": MAIL, "blobIds": ["nosuchblob"] })),
            ("PushSubscription/set", json!({ "create": { "p": {
                "deviceClientId": "dev1",
                "url": "https://push.example/abc",
                "types": ["Email"],
                "keys": {
                    "p256dh": "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
                    "auth": "BTBZMqHH6r4Tts7J_aSIgg"
                },
                "emailPush": { MAIL: { "properties": ["subject"] } }
            } } })),
            // Tasks: Task/query with inTaskLists, timeZone, expandRecurrences.
            ("Task/query", json!({ "accountId": TASKS,
                "filter": { "inTaskLists": [&list], "after": "2026-11-19T00:00:00",
                            "before": "2026-11-30T00:00:00" },
                "timeZone": "Etc/UTC", "expandRecurrences": true })),
            // FileNode, with object history.
            ("FileNode/set", json!({ "accountId": FILES, "update": { &dir: { "name": "Shared" } } })),
            ("FileNode/get", json!({ "accountId": FILES, "ids": [&dir], "includeReplaced": true })),
            // Graph and VTC.
            ("Graph/get", json!({ "accountId": GRAPH, "ids": [&graph_id] })),
            ("VTCCall/get", json!({ "accountId": VTC, "ids": null })),
        ],
    )
    .await;
    print(&rs, "alice");
    let ok = |i: usize, name: &str| -> Value {
        let r = rs
            .iter()
            .find(|r| r.call_id == format!("c{i}"))
            .unwrap_or_else(|| panic!("no c{i}"));
        assert_eq!(r.name, name, "c{i}: {}", r.arguments);
        r.arguments.clone()
    };
    assert!(ok(0, "Mailbox/set")["updated"].is_object());
    let mbox = ok(1, "Mailbox/get");
    assert!(
        mbox["list"].as_array().is_some_and(|l| l.len() >= 2),
        "history entries: {mbox}"
    );
    ok(2, "SieveScript/get");
    let mdn = ok(3, "MDN/parse");
    assert_eq!(mdn["notFound"], json!(["nosuchblob"]), "{mdn}");
    let push = ok(4, "PushSubscription/set");
    assert!(push["created"]["p"]["id"].is_string(), "{push}");
    let q = ok(5, "Task/query");
    // The reference backend has no recurrence engine (a recurring match is
    // `cannotCalculateOccurrences`) and no time-zone database, so the task
    // is non-recurring and in the query's time zone (finding b6).
    assert_eq!(q["ids"], json!([&task]), "{q}");
    assert!(ok(6, "FileNode/set")["updated"].is_object());
    let fh = ok(7, "FileNode/get");
    assert!(
        fh["list"].as_array().is_some_and(|l| l.len() >= 2),
        "history entries: {fh}"
    );
    // Graph with provenance: the owner's myRights carry mayCurate.
    let g = ok(8, "Graph/get");
    assert_eq!(g["list"][0]["id"], json!(&graph_id), "{g}");
    assert_eq!(g["list"][0]["myRights"]["mayCurate"], true, "{g}");
    // VTC: the account exists and has no calls yet.
    let v = ok(9, "VTCCall/get");
    assert_eq!(v["list"], json!([]), "{v}");

    // ---- Bob, the second user: sees the shared folder only, and may
    // not rename it ----
    let rs = dispatch(
        &front,
        "bob",
        vec![
            ("FileNode/query", json!({ "accountId": FILES })),
            (
                "FileNode/set",
                json!({ "accountId": FILES, "update": { &dir: { "name": "mine" } } }),
            ),
        ],
    )
    .await;
    print(&rs, "bob");
    assert_eq!(rs[0].arguments["ids"], json!([&dir]), "{}", rs[0].arguments);
    assert_eq!(
        rs[1].arguments["notUpdated"][&dir]["type"], "forbidden",
        "{}",
        rs[1].arguments
    );

    // ---- conditional-02 on every family: one atomic Foo/set each ----
    // Mailbox/set was refused here while Mail sat behind a bridge handler
    // (finding b1); it now runs atomically like the rest.
    let rs = dispatch(
        &front,
        "alice",
        vec![
            (
                "Mailbox/set",
                json!({ "accountId": MAIL, "atomic": true,
                        "create": { "x": { "name": "Archive" } } }),
            ),
            (
                "TaskList/set",
                json!({ "accountId": TASKS, "atomic": true,
                        "create": { "x": { "name": "Errands" } } }),
            ),
            (
                "Graph/set",
                json!({ "accountId": GRAPH, "atomic": true,
                        "create": { "x": { "name": "Ideas" } } }),
            ),
            (
                "VTCCall/set",
                json!({ "accountId": VTC, "atomic": true,
                        "create": { "x": { "callType": "room", "mediaTypes": ["audio"] } } }),
            ),
            (
                "FileNode/set",
                json!({ "accountId": FILES, "atomic": true,
                        "create": { "x": { "@type": "Directory", "name": "more", "parentId": null } } }),
            ),
        ],
    )
    .await;
    print(&rs, "conditional");
    for r in &rs {
        assert_ne!(r.name, "error", "{}: {}", r.call_id, r.arguments);
        assert!(
            r.arguments["created"]["x"]["id"].is_string(),
            "{} {}: {}",
            r.call_id,
            r.name,
            r.arguments
        );
    }
    // And atomicity holds on a family that was bridged: a failing create
    // (a server-set property, RFC 8621 §2) rolls back the good one,
    // answering atomicFailure.
    let rs = dispatch(
        &front,
        "alice",
        vec![(
            "Mailbox/set",
            json!({ "accountId": MAIL, "atomic": true, "create": {
                "ok": { "name": "Kept?" },
                "bad": { "name": "Counted", "totalEmails": 5 }
            } }),
        )],
    )
    .await;
    print(&rs, "atomic-failure");
    assert_eq!(rs[0].name, "error", "{:?}", rs[0]);
    assert_eq!(rs[0].arguments["type"], "atomicFailure", "{:?}", rs[0]);

    println!("toy-multi-server: ok");
}

fn print(rs: &[Invocation], who: &str) {
    for r in rs {
        println!("{who} {} {} {}", r.call_id, r.name, r.arguments);
    }
}
