//! draft-ietf-jmap-object-history-03 over HTTP (bd JMAP-tna1.3.14,
//! JMAP-tna1.3.22).
//!
//! Oracles: object-history-03 §2.1 (the Session capability is `{}` and
//! present when an account has the capability), §3.2.1 (ContactCard/get
//! with `includeReplaced`), §3.2.2 (`includeDestroyed`), §4.1 (Email/query
//! with `onlyDestroyed`), §5.3 (Email/set `restore`); the -04 editor's copy
//! (the newest retained version of a destroyed object is its final one);
//! draft-atwood-jmap-chat-00 l.782 and decision 22 (a deleted Message keeps
//! no prior or destroyed versions). The spec's ids and times differ from
//! the jig's, so each response is compared field by field, not whole.

use jmap_testjig::{TestjigConfig, TestjigHandle, spawn_in_process};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const HISTORY: &str = "urn:ietf:params:jmap:object-history";
const CONTACTS: &str = "urn:ietf:params:jmap:contacts";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const CHAT: &str = "urn:ietf:params:jmap:chat";

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

/// POST `calls` with `using`; the method responses.
async fn post(jig: &TestjigHandle, using: &[&str], calls: Value) -> Vec<Value> {
    let resp: Value = reqwest::Client::new()
        .post(format!("http://{}/jmap", jig.addr))
        .bearer_auth(&jig.token)
        .json(&json!({"using": using, "methodCalls": calls}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    resp["methodResponses"]
        .as_array()
        .expect("methodResponses")
        .clone()
}

/// One call's arguments, failing on a method error.
async fn one(jig: &TestjigHandle, using: &[&str], name: &str, args: Value) -> Value {
    let r = post(jig, using, json!([[name, args, "0"]])).await;
    assert_eq!(r[0][0], name, "{r:?}");
    r[0][1].clone()
}

fn account(s: &Value) -> String {
    s["primaryAccounts"][MAIL]
        .as_str()
        .expect("account")
        .to_owned()
}

#[tokio::test]
async fn session_advertises_object_history() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let s = session(&jig).await;
    assert_eq!(s["capabilities"][HISTORY], json!({}), "{s}");
    let cap = &s["accounts"][account(&s)]["accountCapabilities"][HISTORY];
    assert_eq!(cap["maxHistoryDuration"], 2_592_000);
    let types: Vec<&str> = cap["supportedTypes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    for t in [
        "Email",
        "Mailbox",
        "ContactCard",
        "CalendarEvent",
        "Task",
        "FileNode",
        "Message",
        "Space",
    ] {
        assert!(types.contains(&t), "{t}: {types:?}");
    }
    for t in [
        "Thread",
        "EmailSubmission",
        "ReadPosition",
        "PresenceStatus",
        "TaskNotification",
    ] {
        assert!(!types.contains(&t), "{t}");
    }
}

/// The id `key` was created under, failing with the response otherwise.
fn created(r: &Value, key: &str) -> String {
    r["created"][key]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{key} not created: {r}"))
        .to_owned()
}

/// §3.2.1: a renamed card's versions, oldest first, each with
/// `objectHistory`; only the replaced one has `replaced` and `expires`.
#[tokio::test]
async fn example_3_2_1_contact_versions() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let using = [CORE, CONTACTS, HISTORY];
    let book = one(
        &jig,
        &using,
        "AddressBook/set",
        json!({"accountId": acc, "create": {"b": {"name": "Personal"}}}),
    )
    .await;
    let book = created(&book, "b");
    let card = one(
        &jig,
        &using,
        "ContactCard/set",
        json!({"accountId": acc, "create": {"c": {
            "@type": "Card", "version": "1.0", "addressBookIds": {book: true},
            "name": {"@type": "Name", "full": "Robert Smith"}
        }}}),
    )
    .await;
    let id = created(&card, "c");
    one(
        &jig,
        &using,
        "ContactCard/set",
        json!({"accountId": acc, "update": {&id: {
            "name": {"@type": "Name", "full": "Bob Smith"}
        }}}),
    )
    .await;
    let got = one(
        &jig,
        &using,
        "ContactCard/get",
        json!({
            "accountId": acc, "ids": [&id], "properties": ["name"],
            "includeReplaced": true, "historyLimit": 10
        }),
    )
    .await;
    let list = got["list"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{got}");
    assert_eq!(list[0]["name"]["full"], "Robert Smith");
    assert_eq!(list[1]["name"]["full"], "Bob Smith");
    assert_eq!(list[0]["objectHistory"]["version"], 1);
    assert!(list[0]["objectHistory"]["replaced"].is_string());
    assert!(list[0]["objectHistory"]["expires"].is_string());
    assert_eq!(list[1]["objectHistory"]["version"], 2);
    assert_eq!(list[1]["objectHistory"]["replaced"], Value::Null);
    assert_eq!(list[1]["objectHistory"]["hasOlder"], true);
}

/// §3.2.2, §4.1 and §5.3 on one Email: destroyed, then listed by
/// `includeDestroyed` and `onlyDestroyed`, then restored.
#[tokio::test]
async fn examples_3_2_2_4_1_5_3_email_destroy_and_restore() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let using = [CORE, MAIL, HISTORY];
    let boxes = one(
        &jig,
        &using,
        "Mailbox/set",
        json!({"accountId": acc, "create": {"m": {"name": "Inbox", "role": "inbox"}}}),
    )
    .await;
    let mb = created(&boxes, "m");
    let email = one(
        &jig,
        &using,
        "Email/set",
        json!({"accountId": acc, "create": {"e": {
            "mailboxIds": {&mb: true}, "subject": "Quarterly report",
            "from": [{"email": "a@example.com"}], "bodyValues": {"1": {"value": "hi"}},
            "textBody": [{"partId": "1", "type": "text/plain"}]
        }}}),
    )
    .await;
    let id = created(&email, "e");
    let destroyed = one(
        &jig,
        &using,
        "Email/set",
        json!({"accountId": acc, "destroy": [&id]}),
    )
    .await;
    assert_eq!(destroyed["destroyed"], json!([&id]));

    // §3.2.2 with -04 §3.1: the final version, replaced = destruction time.
    let got = one(
        &jig,
        &using,
        "Email/get",
        json!({
            "accountId": acc, "ids": [&id], "properties": ["subject"], "includeDestroyed": true
        }),
    )
    .await;
    assert_eq!(got["list"][0]["subject"], "Quarterly report", "{got}");
    assert!(got["list"][0]["objectHistory"]["replaced"].is_string());
    let plain = one(
        &jig,
        &using,
        "Email/get",
        json!({"accountId": acc, "ids": [&id]}),
    )
    .await;
    assert_eq!(plain["notFound"], json!([&id]));

    // §4.1.
    let q = one(
        &jig,
        &using,
        "Email/query",
        json!({
            "accountId": acc, "onlyDestroyed": true,
            "destroyedAfter": "2000-01-01T00:00:00Z", "limit": 50
        }),
    )
    .await;
    assert_eq!(q["ids"], json!([&id]), "{q}");

    // §5.3.
    let r = one(
        &jig,
        &using,
        "Email/set",
        json!({
            "accountId": acc, "restore": {&id: {"mailboxIds": {&mb: true}}}
        }),
    )
    .await;
    assert!(r["restored"].get(&id).is_some(), "{r}");
    let back = one(
        &jig,
        &using,
        "Email/get",
        json!({"accountId": acc, "ids": [&id], "properties": ["subject"]}),
    )
    .await;
    assert_eq!(back["list"][0]["subject"], "Quarterly report");

    // Decision 18: without the URI the same argument is invalidArguments.
    let r = post(
        &jig,
        &[CORE, MAIL],
        json!([["Email/get",
        {"accountId": acc, "ids": [&id], "includeReplaced": true}, "0"]]),
    )
    .await;
    assert_eq!(
        (&r[0][0], &r[0][1]["type"]),
        (&json!("error"), &json!("invalidArguments"))
    );
}

/// bd JMAP-tna1.3.22: a Message's edits are versions; a soft delete
/// leaves only the live tombstone, and a hard delete leaves nothing, so a
/// restore is notFound (chat-00 l.782, decision 22).
#[tokio::test]
async fn message_history_end_to_end() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let using = [CORE, CHAT, HISTORY];
    let bob = one(
        &jig,
        &using,
        "ChatContact/set",
        json!({"accountId": acc, "create": {"b": {"identity": "bob@example.net"}}}),
    )
    .await;
    let bob = created(&bob, "b");
    let chat = one(
        &jig,
        &using,
        "Chat/set",
        json!({"accountId": acc, "create": {"c": {
            "kind": "group", "name": "G", "memberIds": [bob]
        }}}),
    )
    .await;
    let chat = created(&chat, "c");
    let m = one(
        &jig,
        &using,
        "Message/set",
        json!({"accountId": acc, "create": {"m": {
            "chatId": chat, "body": "first", "bodyType": "text/plain",
            "sentAt": "2024-01-01T00:00:00Z"
        }}}),
    )
    .await;
    let m = created(&m, "m");
    one(
        &jig,
        &using,
        "Message/set",
        json!({"accountId": acc, "update": {&m: {"body": "second"}}}),
    )
    .await;
    let versions =
        json!({"accountId": acc, "ids": [&m], "properties": ["body"], "includeReplaced": true});
    let got = one(&jig, &using, "Message/get", versions.clone()).await;
    let bodies: Vec<&Value> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["body"])
        .collect();
    assert_eq!(bodies, [&json!("first"), &json!("second")], "{got}");

    // Soft delete: only the live tombstone.
    one(
        &jig,
        &using,
        "Message/set",
        json!({"accountId": acc, "update": {&m: {
            "deletedAt": "2026-01-02T00:00:00Z"
        }}}),
    )
    .await;
    let got = one(&jig, &using, "Message/get", versions).await;
    assert_eq!(got["list"].as_array().unwrap().len(), 1, "{got}");

    // Hard delete: nothing retained.
    one(
        &jig,
        &using,
        "Message/set",
        json!({"accountId": acc, "destroy": [&m]}),
    )
    .await;
    let got = one(
        &jig,
        &using,
        "Message/get",
        json!({
            "accountId": acc, "ids": [&m], "includeReplaced": true, "includeDestroyed": true
        }),
    )
    .await;
    assert_eq!(got["notFound"], json!([&m]), "{got}");
    let r = one(
        &jig,
        &using,
        "Message/set",
        json!({"accountId": acc, "restore": {&m: null}}),
    )
    .await;
    assert_eq!(r["notRestored"][&m]["type"], "notFound", "{r}");
}

const FILENODE: &str = "urn:ietf:params:jmap:filenode";
const CONDITIONAL: &str = "urn:ietf:params:jmap:conditional";

/// Create root files `names` and destroy them; their ids in order.
async fn destroyed_files(jig: &TestjigHandle, acc: &str, names: &[&str]) -> Vec<String> {
    let using = [CORE, FILENODE, HISTORY];
    let create: serde_json::Map<String, Value> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            (
                format!("f{i}"),
                json!({"@type": "File", "name": n, "parentId": null, "blobId": format!("G{i}")}),
            )
        })
        .collect();
    let r = one(
        jig,
        &using,
        "FileNode/set",
        json!({"accountId": acc, "create": create}),
    )
    .await;
    let ids: Vec<String> = (0..names.len())
        .map(|i| created(&r, &format!("f{i}")))
        .collect();
    let r = one(
        jig,
        &using,
        "FileNode/set",
        json!({"accountId": acc, "destroy": ids}),
    )
    .await;
    assert_eq!(
        r["destroyed"].as_array().map(Vec::len),
        Some(names.len()),
        "{r}"
    );
    ids
}

/// bd JMAP-tna1.3.15, conditional-02 §3.1 with object-history-03 §5.1:
/// `ifUnchangedBy {x: null}` ("the object does not exist") holds for a
/// destroyed x, and the same call restores it.
#[tokio::test]
async fn if_unchanged_by_null_then_restore() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let ids = destroyed_files(&jig, &acc, &["a.txt"]).await;
    let x = &ids[0];
    let r = one(
        &jig,
        &[CORE, FILENODE, HISTORY, CONDITIONAL],
        "FileNode/set",
        json!({
            "accountId": acc, "ifUnchangedBy": {x: null}, "restore": {x: null}
        }),
    )
    .await;
    assert!(r["restored"].get(x).is_some(), "{r}");
}

/// bd JMAP-tna1.3.15, -04 §5.1 (decision 6(b)): under `atomic`, one
/// failing restore fails the call with `atomicFailure` carrying
/// `notRestored`; the state does not move, the other restore is undone,
/// and it leaves no version behind.
#[tokio::test]
async fn atomic_failed_restore_rolls_back() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let ids = destroyed_files(&jig, &acc, &["b.txt"]).await;
    let good = &ids[0];
    let using = [CORE, FILENODE, HISTORY];
    let versions = json!({"accountId": acc, "ids": [good], "includeDestroyed": true,
                          "includeReplaced": true, "properties": ["name"]});
    let before = one(&jig, &using, "FileNode/get", versions.clone()).await;

    let r = post(
        &jig,
        &[CORE, FILENODE, HISTORY, CONDITIONAL],
        json!([["FileNode/set", {
        "accountId": acc, "atomic": true, "restore": {good: null, "never-existed": null}
    }, "0"]]),
    )
    .await;
    assert_eq!(r[0][0], "error", "{r:?}");
    assert_eq!(r[0][1]["type"], "atomicFailure", "{r:?}");
    assert_eq!(
        r[0][1]["notRestored"]["never-existed"]["type"], "notFound",
        "{r:?}"
    );

    let after = one(&jig, &using, "FileNode/get", versions).await;
    assert_eq!(after["state"], before["state"]);
    assert_eq!(after["list"], before["list"], "no version left behind");
    let live = one(
        &jig,
        &using,
        "FileNode/get",
        json!({"accountId": acc, "ids": [good]}),
    )
    .await;
    assert_eq!(
        live["notFound"],
        json!([good]),
        "the good restore was undone"
    );
}

const BLOB2: &str = "urn:ietf:params:jmap:blob2";
const BLOB: &str = "urn:ietf:params:jmap:blob";

/// `Blob/lookup` of `blob` for Emails, through `uri` (blob2 or RFC 9404
/// blob); the matched Email ids.
async fn emails_using(jig: &TestjigHandle, acc: &str, uri: &str, blob: &str) -> Value {
    let r = one(
        jig,
        &[CORE, uri, MAIL],
        "Blob/lookup",
        json!({
            "accountId": acc, "typeNames": ["Email", "Mailbox", "Thread"], "ids": [blob]
        }),
    )
    .await;
    assert_eq!(r["list"][0]["id"], blob, "{r}");
    r["list"][0]["matchedIds"].clone()
}

/// bd JMAP-tna1.3.15, object-history-03 §3.1 with blobext-03 §4, §6 and
/// RFC 9404 (decision 11): mail shares the jig's blob store. A blob that
/// only a destroyed Email's history uses is no reference: Blob/lookup
/// finds nothing through blob2 or RFC 9404 blob, and Blob/set destroy
/// succeeds; the Email's versions go with it, so it is notFound for
/// includeDestroyed, absent from onlyDestroyed, and its restore is
/// notFound (-04 §3.1).
#[tokio::test]
async fn blob_of_a_destroyed_email() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let mail = [CORE, MAIL, HISTORY];
    let boxes = one(
        &jig,
        &mail,
        "Mailbox/set",
        json!({"accountId": acc, "create": {"m": {"name": "Inbox", "role": "inbox"}}}),
    )
    .await;
    let mb = created(&boxes, "m");

    // An uploaded blob, imported as an Email.
    let raw = "From: a@example.com\r\nTo: b@example.com\r\nSubject: Shared\r\n\
               Message-ID: <shared@example.com>\r\n\r\nhello\r\n";
    let up = one(
        &jig,
        &[CORE, BLOB2],
        "Blob/set",
        json!({"accountId": acc, "create": {"b": {
            "data": [{"data:asText": raw}], "type": "message/rfc822"
        }}}),
    )
    .await;
    let blob = created(&up, "b");
    let imp = one(
        &jig,
        &mail,
        "Email/import",
        json!({"accountId": acc, "emails": {"i": {
            "blobId": blob, "mailboxIds": {&mb: true}
        }}}),
    )
    .await;
    let email = created(&imp, "i");
    assert_eq!(imp["created"]["i"]["blobId"], blob, "{imp}");

    // Referenced by a current Email: found, and not destroyable.
    let m = emails_using(&jig, &acc, BLOB2, &blob).await;
    assert_eq!(m["Email"], json!([&email]), "{m}");
    assert_eq!(m["Mailbox"], json!([&mb]), "{m}");
    let d = one(
        &jig,
        &[CORE, BLOB2],
        "Blob/set",
        json!({"accountId": acc, "destroy": [&blob]}),
    )
    .await;
    assert_eq!(d["notDestroyed"][&blob]["type"], "blobHasReference", "{d}");

    // Destroyed: its history is no reference (§3.1).
    one(
        &jig,
        &mail,
        "Email/set",
        json!({"accountId": acc, "destroy": [&email]}),
    )
    .await;
    for uri in [BLOB2, BLOB] {
        let m = emails_using(&jig, &acc, uri, &blob).await;
        assert_eq!(
            m,
            json!({"Email": [], "Mailbox": [], "Thread": []}),
            "{uri}: {m}"
        );
    }
    let kept = one(&jig, &mail, "Email/get",
        json!({"accountId": acc, "ids": [&email], "includeDestroyed": true, "properties": ["blobId"]})).await;
    assert_eq!(kept["list"][0]["blobId"], blob, "{kept}");

    // The blob can go, and the versions using it go too.
    let d = one(
        &jig,
        &[CORE, BLOB2],
        "Blob/set",
        json!({"accountId": acc, "destroy": [&blob]}),
    )
    .await;
    assert_eq!(d["destroyed"], json!([&blob]), "{d}");
    let gone = one(
        &jig,
        &mail,
        "Email/get",
        json!({"accountId": acc, "ids": [&email], "includeDestroyed": true}),
    )
    .await;
    assert_eq!(gone["notFound"], json!([&email]), "{gone}");
    let q = one(
        &jig,
        &mail,
        "Email/query",
        json!({"accountId": acc, "onlyDestroyed": true}),
    )
    .await;
    assert_eq!(q["ids"], json!([]), "{q}");
    let r = one(
        &jig,
        &mail,
        "Email/set",
        json!({"accountId": acc, "restore": {&email: null}}),
    )
    .await;
    assert_eq!(r["notRestored"][&email]["type"], "notFound", "{r}");
}

/// A created Email's message is a blob of the shared store.
#[tokio::test]
async fn created_email_message_is_a_blob() {
    let jig = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let acc = account(&session(&jig).await);
    let mail = [CORE, MAIL];
    let boxes = one(
        &jig,
        &mail,
        "Mailbox/set",
        json!({"accountId": acc, "create": {"m": {"name": "Inbox", "role": "inbox"}}}),
    )
    .await;
    let mb = created(&boxes, "m");
    let e = one(
        &jig,
        &mail,
        "Email/set",
        json!({"accountId": acc, "create": {"e": {
            "mailboxIds": {&mb: true}, "subject": "Mine",
            "from": [{"email": "a@example.com"}], "bodyValues": {"1": {"value": "hi"}},
            "textBody": [{"partId": "1", "type": "text/plain"}]
        }}}),
    )
    .await;
    let blob = e["created"]["e"]["blobId"]
        .as_str()
        .expect("blobId")
        .to_owned();
    let g = one(
        &jig,
        &[CORE, BLOB2],
        "Blob/get",
        json!({"accountId": acc, "ids": [&blob], "properties": ["size"]}),
    )
    .await;
    assert!(g["list"][0]["size"].as_u64().is_some_and(|n| n > 0), "{g}");
    assert_eq!(
        g["list"][0]["expires"],
        Value::Null,
        "referenced: no expiry: {g}"
    );
}
