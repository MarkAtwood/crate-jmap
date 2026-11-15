//! End-to-end draft-atwood-jmap-updates-00 over HTTP and WebSocket
//! against an in-process jig (bd JMAP-cayv.16).
//!
//! Oracle: the draft at jmap-chat-spec 2b85363 plus its working-copy
//! change (account capability `{}`): §3 (capability, gating), §4.1
//! (pruning), §4.2 (`language` on /get and /query; "the server MUST NOT
//! let the Accept-Language header of the request change the result of
//! /get or /query"), §4.3 (the WebSocket handshake's Accept-Language
//! applies to every Request on the connection) and §7.2 (problem
//! details, localized `title` with Content-Language). Expected values are
//! hand-written.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use jmap_testjig::{TestjigConfig, TestjigHandle, spawn_in_process};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

const I18N: &str = "urn:ietf:params:jmap:i18n";

fn using(i18n: bool) -> Vec<&'static str> {
    let mut u = vec![
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:mail",
        "urn:ietf:params:jmap:contacts",
        "urn:ietf:params:jmap:calendars",
    ];
    if i18n {
        u.push(I18N);
    }
    u
}

struct Jig {
    handle: TestjigHandle,
    account: String,
}

async fn jig() -> Jig {
    let handle = spawn_in_process(TestjigConfig::default()).await.unwrap();
    let session: Value = reqwest::Client::new()
        .get(format!("http://{}/.well-known/jmap", handle.addr))
        .bearer_auth(&handle.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let account = session["primaryAccounts"]["urn:ietf:params:jmap:mail"]
        .as_str()
        .unwrap()
        .to_owned();
    Jig { handle, account }
}

impl Jig {
    /// POST one method call; return the response's args for it.
    async fn call(
        &self,
        method: &str,
        args: Value,
        i18n: bool,
        accept_language: Option<&str>,
    ) -> Value {
        let mut args = args;
        args["accountId"] = json!(self.account);
        let mut req = reqwest::Client::new()
            .post(format!("http://{}/jmap", self.handle.addr))
            .bearer_auth(&self.handle.token)
            .json(&json!({ "using": using(i18n), "methodCalls": [[method, args, "c"]] }));
        if let Some(l) = accept_language {
            req = req.header("Accept-Language", l);
        }
        let body: Value = req.send().await.unwrap().json().await.unwrap();
        let r = &body["methodResponses"][0];
        assert_eq!(r[0], method, "method-level error: {body}");
        r[1].clone()
    }

    /// Create three Mailboxes whose base names sort A, B, C and whose
    /// German names sort Z, Y, X; return their ids.
    async fn mailboxes(&self) -> Vec<String> {
        let resp = self
            .call(
                "Mailbox/set",
                json!({ "create": {
                    "a": { "name": "A-i18n", "locale": "en", "localizations": { "de": { "name": "Z-i18n" } } },
                    "b": { "name": "B-i18n", "locale": "en", "localizations": { "de": { "name": "Y-i18n" } } },
                    "c": { "name": "C-i18n", "locale": "en", "localizations": { "de": { "name": "X-i18n" } } }
                } }),
                true,
                None,
            )
            .await;
        ["a", "b", "c"]
            .iter()
            .map(|k| {
                resp["created"][k]["id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{resp}"))
                    .to_owned()
            })
            .collect()
    }
}

/// §3: the capability is `{}` in the Session and the account.
#[tokio::test]
async fn session_advertises_i18n() {
    let j = jig().await;
    let session: Value = reqwest::Client::new()
        .get(format!("http://{}/.well-known/jmap", j.handle.addr))
        .bearer_auth(&j.handle.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(session["capabilities"][I18N], json!({}));
    assert_eq!(
        session["accounts"][&j.account]["accountCapabilities"][I18N],
        json!({})
    );
    j.handle.shutdown().await;
}

/// §4.2.1 and §4.2.2 over HTTP: the language view, a sort on the localized
/// name, and Accept-Language leaving data unchanged.
#[tokio::test]
async fn language_view_sort_and_accept_language() {
    let j = jig().await;
    let ids = j.mailboxes().await;

    let de = j
        .call(
            "Mailbox/get",
            json!({ "ids": [ids[0]], "properties": ["name", "locale"], "language": ["de-CH", "de"] }),
            true,
            None,
        )
        .await;
    assert_eq!(
        de["list"][0],
        json!({ "id": ids[0], "name": "Z-i18n", "locale": "de" })
    );

    // Accept-Language: de without `language`: stored values.
    let plain = j
        .call(
            "Mailbox/get",
            json!({ "ids": [ids[0]], "properties": ["name"] }),
            true,
            Some("de"),
        )
        .await;
    assert_eq!(plain["list"][0]["name"], "A-i18n");
    assert_eq!(plain["state"], de["state"]);

    let sort = json!([{ "property": "name", "isAscending": true }]);
    let filter = json!({ "name": "-i18n" });
    let base = j
        .call(
            "Mailbox/query",
            json!({ "filter": filter, "sort": sort }),
            true,
            None,
        )
        .await;
    assert_eq!(base["ids"], json!(ids));
    let localized = j
        .call(
            "Mailbox/query",
            json!({ "filter": filter, "sort": sort, "language": ["de"] }),
            true,
            None,
        )
        .await;
    let mut reversed = ids.clone();
    reversed.reverse();
    assert_eq!(localized["ids"], json!(reversed));
    assert_ne!(localized["queryState"], base["queryState"]);
    j.handle.shutdown().await;
}

/// §4.1: a client without the capability renames a Mailbox; its stale
/// German name is pruned.
#[tokio::test]
async fn rename_without_capability_prunes() {
    let j = jig().await;
    let ids = j.mailboxes().await;
    let resp = j
        .call(
            "Mailbox/set",
            json!({ "update": { ids[1].clone(): { "name": "B2-i18n" } } }),
            false,
            None,
        )
        .await;
    assert!(resp["updated"].get(&ids[1]).is_some(), "{resp}");
    assert!(
        resp["updated"][&ids[1]].get("localizations").is_none(),
        "{resp}"
    );
    let got = j
        .call("Mailbox/get", json!({ "ids": [ids[1]] }), true, None)
        .await;
    assert_eq!(got["list"][0]["localizations"], Value::Null);
    j.handle.shutdown().await;
}

/// §5.3 on AddressBook, the draft's own Appendix A types.
#[tokio::test]
async fn address_book_view() {
    let j = jig().await;
    let resp = j
        .call(
            "AddressBook/set",
            json!({ "create": { "ab": {
                "name": "Team contacts", "locale": "en",
                "localizations": { "fr": { "name": "Contacts du groupe" }, "de": { "name": "Team-Kontakte" } }
            } } }),
            true,
            None,
        )
        .await;
    let id = resp["created"]["ab"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{resp}"))
        .to_owned();
    let got = j
        .call(
            "AddressBook/get",
            json!({ "ids": [id], "properties": ["name", "locale"], "language": ["fr-CH", "de"] }),
            true,
            None,
        )
        .await;
    assert_eq!(
        got["list"][0],
        json!({ "id": id, "name": "Contacts du groupe", "locale": "fr" })
    );
    j.handle.shutdown().await;
}

/// §7.2: problem details from the Session resource, the API and the event
/// source, with a `title` in the Accept-Language and Content-Language.
#[tokio::test]
async fn localized_problem_details() {
    let j = jig().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://{}/.well-known/jmap", j.handle.addr))
        .header("Accept-Language", "de-AT, en;q=0.5")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    assert_eq!(resp.headers()["content-type"], "application/problem+json");
    assert_eq!(resp.headers()["content-language"], "de");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Nicht autorisiert");

    let resp = client
        .post(format!("http://{}/jmap", j.handle.addr))
        .bearer_auth(&j.handle.token)
        .header("Accept-Language", "de")
        .body("not json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(resp.headers()["content-language"], "de");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["type"], "urn:ietf:params:jmap:error:notJSON");
    assert_eq!(body["title"], "Ungültige Anfrage");

    let resp = client
        .get(format!("http://{}/events?ping=x", j.handle.addr))
        .bearer_auth(&j.handle.token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(resp.headers()["content-language"], "en");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["title"], "Bad Request");
    j.handle.shutdown().await;
}

/// The next text frame on `socket`, parsed as JSON.
async fn next_json<S, E>(socket: &mut S) -> Value
where
    S: futures_util::Stream<Item = Result<Message, E>> + Unpin,
    E: std::fmt::Debug,
{
    let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let Message::Text(t) = msg else {
        panic!("{msg:?}")
    };
    serde_json::from_str(&t).unwrap()
}

/// §4.3: the handshake's Accept-Language applies to every Request on the
/// connection (RequestError titles), and `language` works over WebSocket.
#[tokio::test]
async fn websocket_handshake_language() {
    let j = jig().await;
    let ids = j.mailboxes().await;
    let url = format!("ws://{}/ws?token={}", j.handle.addr, j.handle.token);
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Sec-WebSocket-Protocol", "jmap".parse().unwrap());
    request
        .headers_mut()
        .insert("Accept-Language", "de".parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();

    // Two requests on the same connection: a bad one and a good one.
    socket.send(Message::Text("not json".into())).await.unwrap();
    let err = next_json(&mut socket).await;
    assert_eq!(err["@type"], "RequestError");
    assert_eq!(err["title"], "Ungültige Anfrage");

    let req = json!({
        "@type": "Request", "id": "r2", "using": using(true),
        "methodCalls": [["Mailbox/get", {
            "accountId": j.account, "ids": [ids[2]], "properties": ["name"], "language": ["de"]
        }, "c"]]
    });
    socket
        .send(Message::Text(req.to_string().into()))
        .await
        .unwrap();
    let resp = next_json(&mut socket).await;
    assert_eq!(
        resp["methodResponses"][0][1]["list"][0]["name"], "X-i18n",
        "{resp}"
    );
    drop(socket);
    j.handle.shutdown().await;
}

/// §5.5 on Calendar end to end (the case moved here from JMAP-cayv.16).
#[tokio::test]
async fn calendar_view() {
    let j = jig().await;
    let resp = j
        .call(
            "Calendar/set",
            json!({ "create": { "cal": {
                "name": "Work", "locale": "en",
                "localizations": { "de": { "name": "Arbeit" }, "fr": { "name": "Travail" } }
            } } }),
            true,
            None,
        )
        .await;
    let id = resp["created"]["cal"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{resp}"))
        .to_owned();
    let got = j
        .call(
            "Calendar/get",
            json!({ "ids": [id], "properties": ["name", "locale"], "language": ["fr-BE", "de"] }),
            true,
            None,
        )
        .await;
    assert_eq!(
        got["list"][0],
        json!({ "id": id, "name": "Travail", "locale": "fr" })
    );
    j.handle.shutdown().await;
}
