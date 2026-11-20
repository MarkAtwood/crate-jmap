//! Toy consumer: a client app.
//!
//! - jmap-chat-client `Message/query` with a JMAP Search `where` filter
//!   and jmap-tasks-client `Task/query` with `TaskQueryParams`, both over
//!   HTTP to a loopback JMAP server (the reference Chat and Tasks memory
//!   backends behind a `Dispatcher`).
//! - jmap-graph-client `supports_provenance` on a Session.
//! - jmap-scene-client `send_scene_stream_enable` with a Session, over a
//!   loopback `ws://` connection.
//! - jmap-platform-push: an `FcmPusher` and an `ApnsPusher` built from
//!   dummy credentials, and a `Message`, without sending.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use futures::StreamExt as _;
use jmap_base_client::{ClientConfig, JmapClient, NoneAuth, QueryParams, Session};
use jmap_chat_client::JmapChatExt as _;
use jmap_graph_client::JmapGraphExt as _;
use jmap_platform_push::{Message, PlatformPusher, PushKind, Urgency, apns, fcm};
use jmap_scene_client::SceneWsExt as _;
use jmap_search::{DataTypeSearchInfo, SearchCapability};
use jmap_server::Dispatcher;
use jmap_tasks_client::{JmapTasksExt as _, TaskQueryParams};
use jmap_types::{Filter, Id, Invocation, JmapRequest, State};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

const ACCOUNT: &str = "A1";

/// The loopback server's dispatcher: Chat (with search) and Tasks.
async fn server_dispatcher() -> (Dispatcher<Id>, String, String) {
    let chat = Arc::new(
        jmap_chat_server::memory::MemoryBackend::new()
            .with_search_capability(
                SearchCapability::new(4).with_data_type("Message", DataTypeSearchInfo::new(true)),
            )
            .with_account(&Id::from(ACCOUNT)),
    );
    let tasks =
        Arc::new(jmap_tasks_server::memory::MemoryBackend::new().with_account(&Id::from(ACCOUNT)));
    let mut d: Dispatcher<Id> = Dispatcher::new();
    jmap_chat_server::register_chat_handlers(&mut d, chat);
    jmap_tasks_server::register_tasks_handlers(&mut d, tasks);

    // Seed through the dispatcher: a Chat with two Messages, a TaskList
    // with one Task.
    let calls = vec![
        Invocation::new(
            "Chat/set",
            json!({ "accountId": ACCOUNT, "create": { "g": { "kind": "group", "name": "general" } } }),
            "0",
        ),
        Invocation::new(
            "Message/set",
            json!({ "accountId": ACCOUNT, "create": {
                "m1": { "chatId": "#g", "body": "Release notes", "bodyType": "text/markdown",
                        "sentAt": "2026-11-20T10:00:00Z" },
                "m2": { "chatId": "#g", "body": "Lunch?", "bodyType": "text/plain",
                        "sentAt": "2026-11-20T10:01:00Z" } } }),
            "1",
        ),
        Invocation::new(
            "TaskList/set",
            json!({ "accountId": ACCOUNT, "create": { "l": { "name": "Chores" } } }),
            "2",
        ),
        Invocation::new(
            "Task/set",
            json!({ "accountId": ACCOUNT, "create": { "t": {
                "taskListId": "#l", "title": "Water plants",
                "start": "2026-11-20T09:00:00", "timeZone": "Etc/UTC" } } }),
            "3",
        ),
    ];
    let using = [
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:chat",
        "urn:ietf:params:jmap:tasks",
    ]
    .map(str::to_owned)
    .to_vec();
    let r = d
        .dispatch(JmapRequest::new(using, calls, None), Id::from(ACCOUNT), State::from("s0"))
        .await;
    let m1 = r.method_responses[1].arguments["created"]["m1"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("seed: {:?}", r.method_responses))
        .to_owned();
    assert!(
        r.method_responses[3].arguments["created"]["t"]["id"].is_string(),
        "seed: {:?}",
        r.method_responses
    );
    let chat_id = r.method_responses[0].arguments["created"]["g"]["id"]
        .as_str()
        .expect("chat id")
        .to_owned();
    (d, chat_id, m1)
}

/// A minimal HTTP/1.1 server: every POST body is a JMAP Request for `d`.
async fn serve_http(listener: TcpListener, d: Arc<Dispatcher<Id>>) {
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            return;
        };
        let d = Arc::clone(&d);
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head_end, len) = loop {
                let n = sock.read(&mut chunk).await.unwrap();
                assert!(n > 0, "connection closed before the request");
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map_or(0, |v| v.trim().parse::<usize>().unwrap());
                    break (i + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let n = sock.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
            }
            let req: JmapRequest = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap();
            let resp = d.dispatch(req, Id::from(ACCOUNT), State::from("s0")).await;
            let body = serde_json::to_vec(&resp).unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).await.unwrap();
            sock.write_all(&body).await.unwrap();
        });
    }
}

fn session(api: &str, ws: &str) -> Session {
    serde_json::from_value(json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {},
            "urn:ietf:params:jmap:chat": {},
            "urn:ietf:params:jmap:tasks": {},
            "urn:ietf:params:jmap:graph": {},
            "urn:ietf:params:jmap:scene": {},
            "urn:ietf:params:jmap:scene:websocket": { "url": ws },
            "urn:ietf:params:jmap:search": {}
        },
        "accounts": { ACCOUNT: {
            "name": "alice@example.com", "isPersonal": true, "isReadOnly": false,
            "accountCapabilities": {
                "urn:ietf:params:jmap:chat": {},
                "urn:ietf:params:jmap:tasks": {},
                "urn:ietf:params:jmap:graph": {},
                "urn:ietf:params:jmap:graph:provenance": {},
                "urn:ietf:params:jmap:scene": {},
                "urn:ietf:params:jmap:search": { "dataTypes": { "Message": { "supportsText": true } }, "maxDepth": 4 }
            }
        } },
        "primaryAccounts": {
            "urn:ietf:params:jmap:chat": ACCOUNT,
            "urn:ietf:params:jmap:tasks": ACCOUNT,
            "urn:ietf:params:jmap:graph": ACCOUNT,
            "urn:ietf:params:jmap:scene": ACCOUNT
        },
        "username": "alice@example.com",
        "apiUrl": api,
        "downloadUrl": format!("{api}/dl/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
        "uploadUrl": format!("{api}/ul/{{accountId}}/"),
        "eventSourceUrl": format!("{api}/sse/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
        "state": "s0"
    }))
    .expect("Session")
}

fn pem(der: &[u8]) -> String {
    let b64 = STANDARD.encode(der);
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        lines.join("\n")
    )
}

#[tokio::main]
async fn main() {
    // ---- Loopback HTTP JMAP server and WebSocket server ----
    let (d, chat_id, m1) = server_dispatcher().await;
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", http.local_addr().unwrap());
    tokio::spawn(serve_http(http, Arc::new(d)));

    let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_url = format!("ws://{}/ws", ws_listener.local_addr().unwrap());
    let ws_server = tokio::spawn(async move {
        let (sock, _) = ws_listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        let frame = ws.next().await.unwrap().unwrap();
        serde_json::from_str::<Value>(frame.to_text().unwrap()).unwrap()
    });

    let session = session(&format!("{base}/api"), &ws_url);
    let client = JmapClient::new_plain(NoneAuth, &base, ClientConfig::default()).unwrap();

    // ---- Chat: Message/query with `where` ----
    let chat = client.with_chat_session(session.clone());
    // Message/query needs `chatId` unless `hasMention` is true.
    let mut cond = jmap_chat_types::MessageFilterCondition::default();
    cond.chat_id = Some(Id::from(chat_id.as_str()));
    cond.where_ =
        Some(json!({ "$and": [ { "/bodyType": "text/markdown" }, { "$text": "release" } ] }));
    let r = chat
        .message_query(
            Some(&Filter::Condition(cond)),
            None,
            &QueryParams::default(),
        )
        .await
        .expect("Message/query");
    assert_eq!(r.ids, vec![Id::from(m1.as_str())], "{r:?}");

    // ---- Tasks: Task/query with TaskQueryParams ----
    let tasks = client.with_tasks_session(session.clone());
    let mut tcond = jmap_tasks_types::TaskFilterCondition::default();
    tcond.after = Some(serde_json::from_value(json!("2026-11-19T00:00:00")).unwrap());
    tcond.before = Some(serde_json::from_value(json!("2026-11-30T00:00:00")).unwrap());
    let tparams = TaskQueryParams {
        expand_recurrences: Some(true),
        time_zone: Some("Etc/UTC".into()),
        ..TaskQueryParams::default()
    };
    let r = tasks
        .task_query(
            Some(&Filter::Condition(tcond)),
            None,
            &QueryParams::default(),
            Some(tparams),
        )
        .await
        .expect("Task/query");
    assert_eq!(r.ids.len(), 1, "{r:?}");

    // ---- Graph: provenance from the Session, no I/O ----
    let graph = client.with_graph_session(session.clone());
    assert!(graph.supports_provenance());

    // ---- Scene: SceneStreamEnable over the loopback WebSocket ----
    let mut ws = jmap_base_client::connect_ws_allow_plaintext(
        &ws_url,
        None,
        jmap_base_client::DEFAULT_WS_MAX_MESSAGE_BYTES,
    )
    .await
    .expect("ws connect");
    let mut enable = jmap_scene_types::SceneStreamEnable::default();
    enable.region_ids = Some(vec![Id::from("R1")]);
    ws.send_scene_stream_enable(&session, &enable)
        .await
        .expect("SceneStreamEnable");
    let sent = ws_server.await.unwrap();
    assert_eq!(sent["@type"], "SceneStreamEnable", "{sent}");
    assert_eq!(sent["regionIds"], json!(["R1"]), "{sent}");

    // ---- Platform push: pushers from dummy credentials, no sending ----
    use rsa::pkcs8::{EncodePrivateKey as _, LineEnding};
    use rsa::{Generate as _, PrivateKey as RsaPrivateKey};
    let rsa_pem = RsaPrivateKey::generate()
        .to_pkcs8_pem(LineEnding::LF)
        .unwrap()
        .to_string();
    let account = fcm::ServiceAccount::from_json(
        &json!({
            "project_id": "toy-project",
            "client_email": "toy@toy-project.iam.gserviceaccount.com",
            "private_key": rsa_pem,
            "private_key_id": "k1",
            "token_uri": "https://oauth2.googleapis.com/token"
        })
        .to_string(),
    )
    .expect("ServiceAccount");
    assert_eq!(account.project_id(), "toy-project");
    let fcm_pusher = fcm::FcmPusher::new(account).expect("FcmPusher");

    let rng = ring::rand::SystemRandom::new();
    let p8 = ring::signature::EcdsaKeyPair::generate_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &rng,
    )
    .unwrap();
    let key =
        apns::ApnsKey::from_p8(&pem(p8.as_ref()), "ABCDE12345", "TEAM123456").expect("ApnsKey");
    let apns_pusher = apns::ApnsPusher::new(key, "com.example.toy", apns::Environment::Sandbox)
        .expect("ApnsPusher");

    let pushers: Vec<Box<dyn PlatformPusher>> = vec![Box::new(fcm_pusher), Box::new(apns_pusher)];
    let msg = Message::new(
        &json!({ "@type": "StateChange", "changed": { ACCOUNT: { "Message": "s9" } } }),
        Duration::from_secs(3600),
    )
    .unwrap()
    .with_urgency(Urgency::High)
    .with_kind(PushKind::Data);
    assert_eq!(msg.urgency(), Urgency::High);
    assert!(msg.json().contains("StateChange"));
    assert_eq!(pushers.len(), 2);

    println!("toy-client-app: ok");
}
