//! Toy consumer: one `Dispatcher` serving several extension families.
//!
//! Mail (with the jmap-emailpush, jmap-sieve and jmap-mdn facades), Tasks,
//! FileNode (two users), Graph (with provenance), VTC, conditional-02 (via
//! jmap-conditional) and object history, all on the reference memory
//! backends, behind one `Dispatcher<Id>` whose caller is the Principal id.
//!
//! The reference backends do not share a `CallerCtx` (Mail and Tasks use
//! `()`, FileNode and Graph `Id`, VTC `String`), so a single
//! `Dispatcher<C>` cannot host them directly (finding b1 in
//! docs/audits/2026-11-20-toy-consumers). FileNode and Graph register on
//! the front dispatcher; Mail, Tasks and VTC register on inner dispatchers
//! of their own `CallerCtx`, and `Bridge` forwards each of their methods
//! there as a one-call request.

use std::sync::Arc;

use jmap_filenode::server::memory::MemoryBackend as FileNodeMemory;
use jmap_graph::server::memory::MemoryBackend as GraphMemory;
use jmap_object_history::server::HistoryRetention;
use jmap_object_history::types::ObjectHistoryAccountCapability;
use jmap_server::{
    CallContext, Dispatcher, HandlerFuture, HandlerOutput, JmapError, JmapHandler,
    register_push_subscription_handlers,
};
use jmap_types::{Id, Invocation, JmapRequest, State};
use serde_json::{Value, json};

const MAIL: &str = "M1";
const TASKS: &str = "T1";
const FILES: &str = "F1";
const GRAPH: &str = "G1";
const VTC: &str = "V1";

/// Forwards one method call to an inner dispatcher of another `CallerCtx`.
struct Bridge<C: Clone + Send + Sync + 'static> {
    inner: Arc<Dispatcher<C>>,
    caller: fn(&Id) -> C,
}

impl<C: Clone + Send + Sync + 'static> JmapHandler<Id> for Bridge<C> {
    fn call(&self, cx: CallContext<Id>) -> HandlerFuture {
        let inner = Arc::clone(&self.inner);
        let caller = (self.caller)(&cx.caller);
        Box::pin(async move {
            let request = JmapRequest::new(
                cx.using,
                vec![Invocation::new(cx.method, cx.args, cx.call_id)],
                Some(cx.created_ids),
            );
            let mut responses = inner
                .dispatch(request, caller, State::from("s0"))
                .await
                .method_responses
                .into_iter();
            let first = responses
                .next()
                .ok_or_else(|| JmapError::server_fail("no response"))?;
            if first.name == "error" {
                let e: JmapError = serde_json::from_value(first.arguments)
                    .map_err(|e| JmapError::server_fail(e.to_string()))?;
                return Err(e);
            }
            Ok(HandlerOutput::new(first.arguments).with_extra(responses.collect()))
        })
    }
}

/// Register every method `inner` serves on `front` as a bridge to it,
/// listed with `Dispatcher::methods` (finding b4, fixed). Which of them
/// are history-aware is still spelled out: a `Dispatcher` does not say.
fn bridge<C: Clone + Send + Sync + 'static>(
    front: &mut Dispatcher<Id>,
    inner: &Arc<Dispatcher<C>>,
    caller: fn(&Id) -> C,
    history_aware: &[&str],
) -> usize {
    let mut n = 0;
    for m in inner.methods() {
        let h: Arc<dyn JmapHandler<Id>> = Arc::new(Bridge {
            inner: Arc::clone(inner),
            caller,
        });
        if history_aware.contains(&m) {
            front.register_history_aware(m, h);
        } else {
            front.register(m, h);
        }
        n += 1;
    }
    n
}

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

    // ---- Mail, with EmailPush, Sieve and MDN (CallerCtx = ()) ----
    let mail = Arc::new(
        jmap_mail_server::memory::MemoryBackend::new()
            .with_account(&Id::from(MAIL))
            .with_object_history(history_capability(&["Mailbox"]), retention),
    );
    let mut mail_d: Dispatcher<()> = Dispatcher::new();
    jmap_mail_server::register_mail_handlers_with_history(&mut mail_d, Arc::clone(&mail));
    jmap_sieve::server::register_sieve_handlers(&mut mail_d, Arc::clone(&mail));
    jmap_mdn::server::register_mdn_handlers(&mut mail_d, Arc::clone(&mail));
    register_push_subscription_handlers(&mut mail_d, Arc::clone(&mail));
    jmap_mail_server::register_mail_conditional(&mut mail_d, Arc::clone(&mail));
    let mail_d = Arc::new(mail_d);

    // ---- Tasks (CallerCtx = ()) ----
    let tasks =
        Arc::new(jmap_tasks_server::memory::MemoryBackend::new().with_account(&Id::from(TASKS)));
    let mut tasks_d: Dispatcher<()> = Dispatcher::new();
    jmap_tasks_server::register_tasks_handlers(&mut tasks_d, Arc::clone(&tasks));
    let tasks_d = Arc::new(tasks_d);

    // ---- VTC (CallerCtx = String) ----
    // Finding b2, fixed: `with_account` is public.
    let vtc_capability = serde_json::from_value(json!({
        "mayCreateCall": true, "supportsRingCalls": false, "supportsRoomCalls": true,
        "supportsScheduledCalls": false, "supportsRecording": false,
        "supportsLivestream": false, "livestreamModes": [], "supportsLobby": false,
        "supportsBreakoutRooms": false, "supportedMediaTypes": ["audio", "video"]
    }))
    .expect("VtcCapability");
    let vtc = Arc::new(
        jmap_vtc::server::memory::MemoryBackend::new().with_account(&Id::from(VTC), vtc_capability),
    );
    let mut vtc_d: Dispatcher<String> = Dispatcher::new();
    jmap_vtc::server::register_vtc_handlers(&mut vtc_d, Arc::clone(&vtc));
    let vtc_d = Arc::new(vtc_d);

    // ---- The front dispatcher: FileNode and Graph directly ----
    let mut front: Dispatcher<Id> = Dispatcher::new();

    let files = Arc::new(
        FileNodeMemory::new()
            .with_account_owner(&Id::from(FILES), &Id::from("alice"))
            .with_object_history(history_capability(&["FileNode"]), retention),
    );
    jmap_filenode::server::register_filenode_handlers_with_history(&mut front, Arc::clone(&files));
    jmap_filenode::server::register_filenode_conditional(&mut front, Arc::clone(&files))
        .expect("FileNode conditional");

    // Graph, owned by alice, with provenance (finding b2, fixed).
    let graph_capability =
        serde_json::from_value(json!({ "maxAnnotationSize": null })).expect("GraphCapability");
    let graph = Arc::new(GraphMemory::new().with_account(
        &Id::from(GRAPH),
        &Id::from("alice"),
        graph_capability,
    ));
    graph.register_provenance(&Id::from(GRAPH));
    jmap_graph::server::register_graph_handlers(&mut front, Arc::clone(&graph));

    let bridged = bridge(&mut front, &mail_d, |_| (), &["Mailbox/get", "Mailbox/set"])
        + bridge(&mut front, &tasks_d, |_| (), &[])
        + bridge(&mut front, &vtc_d, |id| id.to_string(), &[]);
    assert!(
        front.methods().any(|m| m == "VTCCall/set") && front.methods().any(|m| m == "Task/query"),
        "every inner method is bridged"
    );
    assert!(front.methods().count() > bridged, "plus FileNode and Graph");

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

    // A bridged family loses conditional-02: the front dispatcher has no
    // `SetSupport` for Mailbox/set (it takes a `ConditionalBackend` of the
    // front's `CallerCtx`), so it refuses `atomic` before the bridge runs,
    // although the inner dispatcher has `register_mail_conditional`
    // (finding b1).
    let rs = dispatch(
        &front,
        "alice",
        vec![(
            "Mailbox/set",
            json!({ "accountId": MAIL, "atomic": true,
                    "create": { "x": { "name": "Archive" } } }),
        )],
    )
    .await;
    print(&rs, "conditional");
    assert_eq!(rs[0].name, "error", "{:?}", rs[0]);
    assert_eq!(rs[0].arguments["type"], "invalidArguments", "{:?}", rs[0]);

    println!("toy-multi-server: ok");
}

fn print(rs: &[Invocation], who: &str) {
    for r in rs {
        println!("{who} {} {} {}", r.call_id, r.name, r.arguments);
    }
}
