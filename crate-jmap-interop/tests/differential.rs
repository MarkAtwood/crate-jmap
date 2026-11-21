//! Differential tests (bd:JMAP-cyub.4): the same JMAP requests go to
//! `jmap-testjig` (this workspace's reference server, started in-process)
//! and to the configured external server (Stalwart), and the responses
//! are compared on protocol facts from RFC 8620 and RFC 8621.
//!
//! Each scenario runs once per server and returns a map of named facts:
//! normalized projections of the responses. Values the RFCs leave to the
//! server are normalized out before comparing: ids become the aliases the
//! test gave the objects, states are only compared with each other on the
//! same server, keywords the server sets itself (`$hasnoattachment`) and
//! capability limits are dropped. A fact that differs fails the test
//! unless [`KNOWN`] classifies it: as an RFC deviation of the external
//! server or a known testjig bug (both reported with
//! [`known_divergence`]), or as a difference the RFCs allow (printed).
//! docs/interop/README.md lists them.
//!
//! Run with `-- --ignored`; see docs/interop/README.md.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jmap_interop::{InteropEnv, known_divergence, unique};
use jmap_testjig::{TestjigConfig, TestjigHandle, spawn_in_process};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const SUBMISSION: &str = "urn:ietf:params:jmap:submission";
const ERROR_PREFIX: &str = "urn:ietf:params:jmap:error:";

/// How a fact that differs between the two servers is classified.
enum Verdict {
    /// The external server contradicts the cited RFC text; the testjig
    /// answer is the RFC's.
    Deviation(&'static str),
    /// The RFCs leave the behaviour to the server; both answers conform.
    Allowed(&'static str),
    /// The testjig (this workspace's code) contradicts the cited RFC text
    /// and the external server does not; tracked until fixed.
    #[allow(dead_code, reason = "no testjig bug is known at present")]
    TestjigBug(&'static str),
}

/// Facts known to differ, with the RFC section that decides each.
const KNOWN: &[(&str, Verdict)] = &[
    (
        "request/unknownCapability",
        Verdict::Deviation(
            "RFC 8620 §3.6.1: a `using` capability the server does not support is the \
             unknownCapability problem",
        ),
    ),
    (
        "request/notJSON",
        Verdict::Deviation(
            "RFC 8620 §3.6.1: a request that does not parse as I-JSON is the notJSON problem",
        ),
    ),
    (
        "request/createdIds echoed",
        Verdict::Deviation(
            "RFC 8620 §3.4: createdIds given in the Request is returned and \"MUST include all \
             creation ids passed in the original createdIds parameter\"",
        ),
    ),
    (
        "method-errors/bogus sinceState",
        Verdict::Allowed(
            "RFC 8620 §5.2 defines cannotCalculateChanges for a state the server cannot \
             calculate from, §3.6.2 invalidArguments for an invalid argument; neither says \
             which a state token the server never issued is",
        ),
    ),
    (
        "method-errors/method without its capability in using",
        Verdict::Allowed(
            "RFC 8620 §3.3: `using` is \"the set of capabilities the client wishes to use\"; \
             no rule makes the server reject a method whose capability is not listed",
        ),
    ),
    (
        "mailbox/create.created[A] echoes sent properties",
        Verdict::Allowed(
            "RFC 8620 §5.3: created holds the properties \"not sent by the client\"; it does \
             not forbid echoing sent ones",
        ),
    ),
    (
        "mailbox/create under a parent destroyed in the same call",
        Verdict::Allowed(
            "RFC 8620 §5.3 lets the server \"sort out ordering\" within one /set; Stalwart \
             rejects the create (with willDestroy, which §5.3 lists for updates only), the \
             testjig creates the child and then refuses the destroy with mailboxHasChild",
        ),
    ),
    (
        "mailbox/get[P].isSubscribed",
        Verdict::Deviation(IS_SUBSCRIBED),
    ),
    (
        "mailbox/get[A].isSubscribed",
        Verdict::Deviation(IS_SUBSCRIBED),
    ),
    (
        "mailbox/get[B].isSubscribed",
        Verdict::Deviation(IS_SUBSCRIBED),
    ),
    (
        "mailbox/get[P].myRights extensions",
        Verdict::Allowed(MAY_SET_METADATA),
    ),
    (
        "mailbox/get[A].myRights extensions",
        Verdict::Allowed(MAY_SET_METADATA),
    ),
    (
        "mailbox/get[B].myRights extensions",
        Verdict::Allowed(MAY_SET_METADATA),
    ),
];

const IS_SUBSCRIBED: &str = "RFC 8621 §2: isSubscribed \"SHOULD default to ... true for any \
                             new Mailboxes created by the user themself\"";
const MAY_SET_METADATA: &str = "draft-ietf-jmap-metadata-05 §4.1: maySetMetadata is in myRights \
                                only when the account capability lists Mailbox or Email";

/// One server under test.
struct Server {
    label: &'static str,
    http: reqwest::Client,
    api_url: String,
    auth: Auth,
    account: String,
    session: Value,
}

enum Auth {
    Basic(String, String),
    Bearer(String),
}

/// The method responses of one Request, by call id.
struct Responses(Vec<Value>);

impl Responses {
    /// The `[name, arguments]` of the response to `call_id`.
    fn get(&self, call_id: &str) -> (&str, &Value) {
        let inv = self
            .0
            .iter()
            .find(|inv| inv[2] == call_id)
            .unwrap_or_else(|| panic!("no response for call {call_id}: {:?}", self.0));
        (inv[0].as_str().expect("method name"), &inv[1])
    }

    /// The arguments of the successful response to `call_id`.
    fn args(&self, call_id: &str) -> &Value {
        let (name, args) = self.get(call_id);
        assert_ne!(name, "error", "call {call_id} failed: {args}");
        args
    }

    /// `"<name>"` for a success, `"error:<type>"` for a method error.
    fn outcome(&self, call_id: &str) -> Value {
        let (name, args) = self.get(call_id);
        if name == "error" {
            json!(format!("error:{}", args["type"].as_str().unwrap_or("?")))
        } else {
            json!(name)
        }
    }
}

impl Server {
    async fn stalwart(env: &InteropEnv) -> Self {
        let auth = Auth::Basic(env.user.username.clone(), env.user.password().to_owned());
        Self::connect("stalwart", &env.base_url, auth).await
    }

    async fn testjig(jig: &TestjigHandle) -> Self {
        let base = format!("http://{}", jig.addr);
        Self::connect("testjig", &base, Auth::Bearer(jig.token.clone())).await
    }

    async fn connect(label: &'static str, base: &str, auth: Auth) -> Self {
        let http = reqwest::Client::new();
        let mut server = Self {
            label,
            http,
            api_url: String::new(),
            auth,
            account: String::new(),
            session: Value::Null,
        };
        let resp = server
            .authed(server.http.get(format!("{base}/.well-known/jmap")))
            .send()
            .await
            .expect("Session request");
        assert!(resp.status().is_success(), "{label}: Session {resp:?}");
        server.session = resp.json().await.expect("Session JSON");
        server.api_url = server.session["apiUrl"]
            .as_str()
            .expect("apiUrl")
            .to_owned();
        server.account = server.session["primaryAccounts"][MAIL]
            .as_str()
            .expect("primary mail account")
            .to_owned();
        server
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Auth::Basic(user, pass) => req.basic_auth(user, Some(pass)),
            Auth::Bearer(token) => req.bearer_auth(token),
        }
    }

    /// POST `body` to the API endpoint; the HTTP status and the JSON body.
    async fn post(&self, body: impl Into<reqwest::Body>) -> (u16, Value) {
        let resp = self
            .authed(self.http.post(&self.api_url))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .expect("API request");
        let status = resp.status().as_u16();
        let text = resp.text().await.expect("API response body");
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, value)
    }

    /// Send `calls` (method, arguments, call id) with `using`.
    async fn call(&self, using: &[&str], calls: &[(&str, Value, &str)]) -> Responses {
        let calls: Vec<Value> = calls.iter().map(|(m, a, id)| json!([m, a, id])).collect();
        let req = json!({"using": using, "methodCalls": calls});
        let (status, body) = self.post(req.to_string()).await;
        assert_eq!(status, 200, "{}: {body}", self.label);
        Responses(
            body["methodResponses"]
                .as_array()
                .unwrap_or_else(|| panic!("{}: no methodResponses: {body}", self.label))
                .clone(),
        )
    }

    async fn mail(&self, calls: &[(&str, Value, &str)]) -> Responses {
        self.call(&[CORE, MAIL], calls).await
    }

    /// The current `state` of `type_name` (an empty `/get`).
    async fn state(&self, type_name: &str) -> Value {
        let method = format!("{type_name}/get");
        let r = self
            .mail(&[(&method, json!({"accountId": self.account, "ids": []}), "s")])
            .await;
        r.args("s")["state"].clone()
    }
}

/// Server ids by the alias the test gave the object.
#[derive(Default)]
struct Ids(HashMap<String, String>);

impl Ids {
    fn add(&mut self, id: &Value, alias: &str) {
        let id = id
            .as_str()
            .unwrap_or_else(|| panic!("id for {alias}: {id}"));
        self.0.insert(id.to_owned(), alias.to_owned());
    }

    fn id(&self, alias: &str) -> String {
        self.0
            .iter()
            .find(|(_, a)| *a == alias)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| panic!("no id for {alias}"))
    }

    /// `v` with every known id replaced by its alias, in values and in
    /// object keys.
    fn norm(&self, v: &Value) -> Value {
        match v {
            Value::String(s) => Value::String(self.0.get(s).cloned().unwrap_or_else(|| s.clone())),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.norm(x)).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, x)| {
                        (
                            self.0.get(k).cloned().unwrap_or_else(|| k.clone()),
                            self.norm(x),
                        )
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// The aliases among `ids` (a JSON array of ids), sorted; ids the test
    /// did not create (other tests' objects on a shared server) are left
    /// out.
    fn known(&self, ids: &Value) -> Value {
        let set: BTreeSet<String> = ids
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().and_then(|s| self.0.get(s)).cloned())
                    .collect()
            })
            .unwrap_or_default();
        json!(set)
    }

    /// The aliases of `ids` in order; an unknown id is `"?"`.
    fn ordered(&self, ids: &Value) -> Value {
        json!(
            ids.as_array()
                .expect("ids array")
                .iter()
                .map(|x| x
                    .as_str()
                    .and_then(|s| self.0.get(s))
                    .map_or("?", String::as_str))
                .collect::<Vec<_>>()
        )
    }
}

/// Named facts observed on one server.
#[derive(Default)]
struct Facts(BTreeMap<String, Value>);

impl Facts {
    fn put(&mut self, name: &str, v: Value) {
        assert!(
            self.0.insert(name.to_owned(), v).is_none(),
            "fact {name} recorded twice"
        );
    }
}

/// The sorted keys of a JSON object (or `null`).
fn keys(v: &Value) -> Value {
    match v.as_object() {
        Some(o) => json!(o.keys().collect::<BTreeSet<_>>()),
        None => Value::Null,
    }
}

/// The rights of RFC 8621 §2 `myRights`, plus RFC 9670 `mayShare`.
const MAILBOX_RIGHTS: &[&str] = &[
    "mayReadItems",
    "mayAddItems",
    "mayRemoveItems",
    "maySetSeen",
    "maySetKeywords",
    "mayCreateChild",
    "mayRename",
    "mayDelete",
    "maySubmit",
    "mayShare",
];

/// A JSON array, sorted (`null` stays `null`): for lists whose order the
/// RFC leaves open.
fn sorted(v: &Value) -> Value {
    match v.as_array() {
        Some(a) => {
            let mut a = a.clone();
            a.sort_by_key(Value::to_string);
            Value::Array(a)
        }
        None => v.clone(),
    }
}

/// The JSON type name of `v`, for shape comparisons.
fn kind(v: Option<&Value>) -> &'static str {
    match v {
        None => "absent",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(n)) if n.is_u64() => "unsignedInt",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

/// `true` for `null`, an absent value or an empty object/array: the
/// "nothing" forms RFC 8620 §5.3 allows for `notCreated` and friends.
fn empty(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::Object(o)) => o.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

/// The SetError `type` (and `properties`, sorted, when present) of each
/// entry of a `notCreated` / `notUpdated` / `notDestroyed` map.
fn set_errors(ids: &Ids, v: &Value) -> Value {
    let Some(o) = v.as_object() else {
        return Value::Null;
    };
    let mut out = BTreeMap::new();
    for (k, e) in o {
        let mut desc = e["type"].as_str().unwrap_or("?").to_owned();
        if let Some(props) = e.get("properties").and_then(Value::as_array) {
            let mut p: Vec<&str> = props.iter().filter_map(Value::as_str).collect();
            p.sort_unstable();
            desc = format!("{desc} {p:?}");
        }
        out.insert(ids.norm(&json!(k)).as_str().unwrap_or(k).to_owned(), desc);
    }
    json!(out)
}

/// Compare the facts of the two servers and fail on every difference
/// that [`KNOWN`] does not classify.
fn compare(scenario: &str, jig: &Facts, other: &Facts) {
    let names: BTreeSet<&String> = jig.0.keys().chain(other.0.keys()).collect();
    let mut failures = Vec::new();
    for name in names {
        let a = jig.0.get(name).unwrap_or(&Value::Null);
        let b = other.0.get(name).unwrap_or(&Value::Null);
        if a == b {
            continue;
        }
        let shown = format!("{scenario}/{name}: testjig {a} vs server {b}");
        match KNOWN
            .iter()
            .find(|(n, _)| *n == format!("{scenario}/{name}"))
        {
            Some((_, Verdict::Deviation(rfc))) => known_divergence(&format!("{rfc}: {shown}")),
            Some((_, Verdict::TestjigBug(rfc))) => {
                known_divergence(&format!("testjig bug, {rfc}: {shown}"));
            }
            Some((_, Verdict::Allowed(why))) => {
                eprintln!("allowed difference ({why}): {shown}");
            }
            None => failures.push(shown),
        }
    }
    assert!(
        failures.is_empty(),
        "{} unclassified difference(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Both servers, with the testjig running in this process.
struct Pair {
    jig: Server,
    other: Server,
    _handle: TestjigHandle,
}

async fn pair() -> Pair {
    let env = InteropEnv::require();
    let handle = spawn_in_process(TestjigConfig::default())
        .await
        .expect("spawn testjig");
    Pair {
        jig: Server::testjig(&handle).await,
        other: Server::stalwart(&env).await,
        _handle: handle,
    }
}

// ---------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------

/// RFC 8620 §2 Session shape and RFC 8621 §1.3 mail and submission
/// capabilities (types only: limits are the server's).
fn session_facts(s: &Server) -> Facts {
    let mut f = Facts::default();
    let session = &s.session;
    let top: BTreeMap<&str, &str> = [
        "capabilities",
        "accounts",
        "primaryAccounts",
        "username",
        "apiUrl",
        "downloadUrl",
        "uploadUrl",
        "eventSourceUrl",
        "state",
    ]
    .into_iter()
    .map(|k| (k, kind(session.get(k))))
    .collect();
    f.put("top-level", json!(top));

    let core = &session["capabilities"][CORE];
    let core_kinds: BTreeMap<&str, &str> = [
        "maxSizeUpload",
        "maxConcurrentUpload",
        "maxSizeRequest",
        "maxConcurrentRequests",
        "maxCallsInRequest",
        "maxObjectsInGet",
        "maxObjectsInSet",
        "collationAlgorithms",
    ]
    .into_iter()
    .map(|k| (k, kind(core.get(k))))
    .collect();
    f.put("core-capability", json!(core_kinds));
    for cap in [MAIL, SUBMISSION] {
        f.put(
            &format!("capabilities[{cap}]"),
            json!(kind(session["capabilities"].get(cap))),
        );
        f.put(
            &format!("primaryAccounts[{cap}]"),
            json!(session["primaryAccounts"][cap] == json!(s.account)),
        );
    }

    let account = &session["accounts"][&s.account];
    let acct: BTreeMap<&str, &str> = ["name", "isPersonal", "isReadOnly", "accountCapabilities"]
        .into_iter()
        .map(|k| (k, kind(account.get(k))))
        .collect();
    f.put("account", json!(acct));
    let mail = &account["accountCapabilities"][MAIL];
    let mail_kinds: BTreeMap<&str, &str> = [
        "maxMailboxesPerEmail",
        "maxMailboxDepth",
        "maxSizeMailboxName",
        "maxSizeAttachmentsPerEmail",
        "emailQuerySortOptions",
        "mayCreateTopLevelMailbox",
    ]
    .into_iter()
    .map(|k| {
        // RFC 8621 §1.3.1: the first two are "UnsignedInt|null" (null: no
        // limit); either form is the property's type.
        let t = match (k, kind(mail.get(k))) {
            ("maxMailboxesPerEmail" | "maxMailboxDepth", "null" | "unsignedInt") => {
                "UnsignedInt|null"
            }
            (_, t) => t,
        };
        (k, t)
    })
    .collect();
    f.put("accountCapabilities[mail]", json!(mail_kinds));
    let sub = &account["accountCapabilities"][SUBMISSION];
    let sub_kinds: BTreeMap<&str, &str> = ["maxDelayedSend", "submissionExtensions"]
        .into_iter()
        .map(|k| (k, kind(sub.get(k))))
        .collect();
    f.put("accountCapabilities[submission]", json!(sub_kinds));
    // RFC 8621 §1.3.1: emailQuerySortOptions lists the sorts Email/query
    // supports; the RFC 8621 §4.4.2 ones every server must support.
    let sorts: BTreeSet<&str> = mail["emailQuerySortOptions"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    f.put(
        "emailQuerySortOptions has receivedAt",
        json!(sorts.contains("receivedAt")),
    );
    f
}

/// RFC 8620 §3.6.1 request-level errors and §4 Core/echo.
async fn request_facts(s: &Server) -> Facts {
    let mut f = Facts::default();
    let problem = |(status, body): (u16, Value)| {
        let ty = body["type"].as_str().map_or_else(
            || format!("not a problem: {body}"),
            |t| t.replace(ERROR_PREFIX, ""),
        );
        json!({"status": status, "type": ty, "body.status": body.get("status")})
    };
    f.put("notJSON", problem(s.post("{not json").await));
    f.put(
        "notRequest (no using)",
        problem(s.post(json!({"methodCalls": []}).to_string()).await),
    );
    f.put(
        "notRequest (methodCalls not an array)",
        problem(
            s.post(json!({"using": [CORE], "methodCalls": {}}).to_string())
                .await,
        ),
    );
    f.put(
        "unknownCapability",
        problem(
            s.post(
                json!({"using": [CORE, "urn:example:not-a-capability"],
                       "methodCalls": [["Core/echo", {}, "e"]]})
                .to_string(),
            )
            .await,
        ),
    );

    let args = json!({"hello": true, "n": 5, "list": ["a", {"b": null}]});
    let (status, body) = s
        .post(json!({"using": [CORE], "methodCalls": [["Core/echo", args, "e"]]}).to_string())
        .await;
    f.put("echo.status", json!(status));
    f.put("echo.methodResponses", body["methodResponses"].clone());
    f.put("echo.sessionState", json!(kind(body.get("sessionState"))));
    // RFC 8620 §3.4: sessionState is the Session's state.
    f.put(
        "echo.sessionState == Session.state",
        json!(body["sessionState"] == s.session["state"]),
    );
    // RFC 8620 §3.3: createdIds is echoed only when the request had it.
    let (_, body) = s
        .post(
            json!({"using": [CORE], "methodCalls": [["Core/echo", {}, "e"]],
                   "createdIds": {"k": "zz9-x"}})
            .to_string(),
        )
        .await;
    f.put("createdIds echoed", body["createdIds"].clone());
    f
}

/// RFC 8620 §3.6.2 and §5 method-level errors for the same invalid
/// calls.
async fn method_error_facts(s: &Server) -> Facts {
    let mut f = Facts::default();
    let acct = &s.account;
    let r = s
        .mail(&[
            ("Core/notAMethod", json!({}), "unknownMethod"),
            (
                "Mailbox/get",
                json!({"accountId": "zz9-no-account", "ids": []}),
                "accountNotFound",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": "not-a-list"}),
                "ids not a list",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "properties": ["zz9NotAProperty"]}),
                "unknown property",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "zz9Unknown": 1}),
                "unknown argument",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct,
                       "#ids": {"resultOf": "nope", "name": "Mailbox/query", "path": "/ids"}}),
                "dangling reference",
            ),
            ("Mailbox/query", json!({"accountId": acct}), "q"),
            (
                "Mailbox/get",
                json!({"accountId": acct,
                       "#ids": {"resultOf": "q", "name": "Mailbox/get", "path": "/ids"}}),
                "reference name mismatch",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct,
                       "#ids": {"resultOf": "q", "name": "Mailbox/query", "path": "/zz9"}}),
                "reference bad path",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": [],
                       "#ids": {"resultOf": "q", "name": "Mailbox/query", "path": "/ids"}}),
                "both ids and #ids",
            ),
            (
                "Mailbox/changes",
                json!({"accountId": acct, "sinceState": "zz9-bogus-state"}),
                "bogus sinceState",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "sort": [{"property": "zz9NoSuchSort"}]}),
                "unknown sort",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"zz9NoSuchFilter": "x"}}),
                "unknown filter",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": ["zz9-none"]}),
                "get unknown id",
            ),
        ])
        .await;
    for id in [
        "unknownMethod",
        "accountNotFound",
        "ids not a list",
        "unknown property",
        "unknown argument",
        "dangling reference",
        "reference name mismatch",
        "reference bad path",
        "both ids and #ids",
        "bogus sinceState",
        "unknown sort",
        "unknown filter",
        "get unknown id",
    ] {
        f.put(id, r.outcome(id));
    }
    f.put(
        "get unknown id.notFound",
        r.args("get unknown id")["notFound"].clone(),
    );

    // RFC 8620 §3.3 `using`: a method of a capability the request did not
    // name.
    let r = s
        .call(
            &[CORE],
            &[("Mailbox/get", json!({"accountId": acct, "ids": []}), "m")],
        )
        .await;
    f.put("method without its capability in using", r.outcome("m"));
    f
}

/// RFC 8621 §2 Mailbox: /set create, update, destroy and their
/// SetErrors, /get shapes, /changes after /set, /query order.
async fn mailbox_facts(s: &Server) -> Facts {
    let mut f = Facts::default();
    let mut ids = Ids::default();
    let acct = s.account.as_str();
    let root = unique("diff-mb");

    let state0 = s.state("Mailbox").await;
    let r = s
        .mail(&[(
            "Mailbox/set",
            json!({"accountId": acct, "create": {
                "P": {"name": root, "parentId": null},
                "A": {"name": "alpha", "parentId": "#P"},
                "B": {"name": "bravo", "parentId": "#P"},
            }}),
            "c",
        )])
        .await;
    let set = r.args("c");
    f.put(
        "create.notCreated empty",
        json!(empty(set.get("notCreated"))),
    );
    for k in ["P", "A", "B"] {
        ids.add(&set["created"][k]["id"], k);
    }
    // RFC 8620 §5.3: created holds the properties "that were not sent by
    // the client": the server-set ones and the defaulted ones. Echoing a
    // sent property too is compared separately.
    let created_a = set["created"]["A"].as_object().expect("created[A]");
    let sent = ["name", "parentId"];
    f.put(
        "create.created[A] keys not sent",
        json!(
            created_a
                .keys()
                .filter(|k| !sent.contains(&k.as_str()))
                .collect::<BTreeSet<_>>()
        ),
    );
    f.put(
        "create.created[A] echoes sent properties",
        json!(
            created_a
                .keys()
                .filter(|k| sent.contains(&k.as_str()))
                .collect::<BTreeSet<_>>()
        ),
    );
    f.put(
        "create.oldState == state0",
        json!(set["oldState"] == state0),
    );
    f.put("create.newState kind", json!(kind(set.get("newState"))));
    f.put(
        "create.newState != oldState",
        json!(set["newState"] != set["oldState"]),
    );

    let r = s
        .mail(&[
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": [ids.id("P"), ids.id("A"), ids.id("B")]}),
                "g",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": [ids.id("A")], "properties": ["name"]}),
                "name only",
            ),
        ])
        .await;
    let got = r.args("g");
    f.put("get keys", keys(got));
    f.put(
        "get.state == create.newState",
        json!(got["state"] == set["newState"]),
    );
    for m in got["list"].as_array().expect("list") {
        let alias = ids.norm(&m["id"]);
        let alias = alias.as_str().expect("alias");
        let mut m = ids.norm(m);
        if m["name"] == json!(root) {
            m["name"] = json!("<unique root name>");
        }
        // RFC 8621 §2 names nine rights and RFC 9670 adds mayShare;
        // extensions add more (draft-ietf-jmap-metadata-05 §4.1
        // maySetMetadata, only for a type the server lists). Compare those
        // values and, separately, the extension rights' names.
        let rights = m["myRights"].as_object().cloned().unwrap_or_default();
        let (core, extra): (BTreeMap<_, _>, BTreeMap<_, _>) = rights
            .into_iter()
            .partition(|(k, _)| MAILBOX_RIGHTS.contains(&k.as_str()));
        m["myRights"] = json!(core);
        f.put(
            &format!("get[{alias}].myRights extensions"),
            keys(&json!(extra)),
        );
        // RFC 8621 §2 isSubscribed default, compared on its own.
        f.put(
            &format!("get[{alias}].isSubscribed"),
            m["isSubscribed"].take(),
        );
        f.put(&format!("get[{alias}]"), m);
    }
    f.put("get name-only keys", keys(&r.args("name only")["list"][0]));

    let r = s
        .mail(&[
            (
                "Mailbox/changes",
                json!({"accountId": acct, "sinceState": state0}),
                "ch",
            ),
            (
                "Mailbox/query",
                json!({"accountId": acct, "filter": {"parentId": ids.id("P")},
                       "sort": [{"property": "name", "isAscending": true}],
                       "calculateTotal": true}),
                "asc",
            ),
            (
                "Mailbox/query",
                json!({"accountId": acct, "filter": {"parentId": ids.id("P")},
                       "sort": [{"property": "name", "isAscending": false}]}),
                "desc",
            ),
            (
                "Mailbox/query",
                json!({"accountId": acct, "filter": {"parentId": ids.id("P"), "name": "brav"}}),
                "name filter",
            ),
        ])
        .await;
    let ch = r.args("ch");
    f.put("changes keys", keys(ch));
    f.put("changes.created", ids.known(&ch["created"]));
    f.put("changes.updated", ids.known(&ch["updated"]));
    f.put("changes.destroyed", ids.known(&ch["destroyed"]));
    f.put("changes.hasMoreChanges", ch["hasMoreChanges"].clone());
    f.put(
        "changes.oldState == state0",
        json!(ch["oldState"] == state0),
    );
    f.put(
        "changes.newState == get.state",
        json!(ch["newState"] == got["state"]),
    );
    let asc = r.args("asc");
    f.put("query keys", keys(asc));
    f.put("query asc", ids.ordered(&asc["ids"]));
    f.put("query asc.total", asc["total"].clone());
    f.put("query asc.position", asc["position"].clone());
    f.put("query desc", ids.ordered(&r.args("desc")["ids"]));
    f.put(
        "query name filter",
        ids.ordered(&r.args("name filter")["ids"]),
    );

    // Update, then /changes reports it as updated.
    let state1 = got["state"].clone();
    let r = s
        .mail(&[
            (
                "Mailbox/set",
                json!({"accountId": acct, "update": {ids.id("A"): {"name": "alpha2"}}}),
                "u",
            ),
            (
                "Mailbox/changes",
                json!({"accountId": acct, "sinceState": state1}),
                "ch",
            ),
        ])
        .await;
    let u = r.args("u");
    f.put("update.updated", ids.norm(&u["updated"]));
    let ch = r.args("ch");
    f.put("update changes.created", ids.known(&ch["created"]));
    f.put("update changes.updated", ids.known(&ch["updated"]));

    // SetErrors for invalid input.
    let r = s
        .mail(&[(
            "Mailbox/set",
            json!({"accountId": acct,
                "create": {
                    "noParent": {"name": "x", "parentId": "zz9-no-such-mailbox"},
                    "sibling": {"name": "bravo", "parentId": ids.id("P")},
                    "serverSet": {"name": "y", "parentId": ids.id("P"), "totalEmails": 5},
                    "noName": {"parentId": ids.id("P")},
                },
                "update": {
                    "zz9-none": {"name": "z"},
                    ids.id("B"): {"unreadEmails": 9},
                },
                "destroy": ["zz9-none"],
            }),
            "e",
        )])
        .await;
    let e = r.args("e");
    f.put("errors.notCreated", set_errors(&ids, &e["notCreated"]));
    f.put("errors.notUpdated", set_errors(&ids, &e["notUpdated"]));
    f.put("errors.notDestroyed", set_errors(&ids, &e["notDestroyed"]));
    f.put("errors.created", keys(&e["created"]));
    // Clean up whatever the server accepted above.
    let mut extra: Vec<Value> = e["created"]
        .as_object()
        .map(|o| o.values().map(|c| c["id"].clone()).collect())
        .unwrap_or_default();

    // RFC 8621 §2.5: a Mailbox with a child is not destroyed
    // (mailboxHasChild). Then the same with a child created in the same
    // call: RFC 8620 §5.3 lets the server "sort out ordering".
    let r = s
        .mail(&[
            (
                "Mailbox/set",
                json!({"accountId": acct, "destroy": [ids.id("P")]}),
                "has child",
            ),
            (
                "Mailbox/set",
                json!({"accountId": acct,
                       "create": {"late": {"name": "late", "parentId": ids.id("P")}},
                       "destroy": [ids.id("P")]}),
                "create under destroyed",
            ),
        ])
        .await;
    f.put(
        "destroy parent with children",
        set_errors(&ids, &r.args("has child")["notDestroyed"]),
    );
    let c = r.args("create under destroyed");
    f.put(
        "create under a parent destroyed in the same call",
        json!({"notCreated": set_errors(&ids, &c["notCreated"]),
               "notDestroyed": set_errors(&ids, &c["notDestroyed"]),
               "created": keys(&c["created"])}),
    );
    if let Some(id) = c["created"]["late"].get("id") {
        extra.push(id.clone());
    }

    // Destroy children then the parent, in one call.
    let state2 = s.state("Mailbox").await;
    extra.extend([json!(ids.id("A")), json!(ids.id("B")), json!(ids.id("P"))]);
    let r = s
        .mail(&[
            (
                "Mailbox/set",
                json!({"accountId": acct, "destroy": extra}),
                "d",
            ),
            (
                "Mailbox/changes",
                json!({"accountId": acct, "sinceState": state2}),
                "ch",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": [ids.id("A")]}),
                "gone",
            ),
        ])
        .await;
    let d = r.args("d");
    f.put("destroy.destroyed", ids.known(&d["destroyed"]));
    f.put("destroy.notDestroyed", set_errors(&ids, &d["notDestroyed"]));
    f.put(
        "destroy changes.destroyed",
        ids.known(&r.args("ch")["destroyed"]),
    );
    f.put(
        "destroyed get.notFound",
        ids.norm(&r.args("gone")["notFound"]),
    );
    f
}

/// RFC 8621 §4 Email: /set create, /get, /query ordering and filters,
/// result references to /get and Thread/get, /changes after /set, and
/// SetErrors.
async fn email_facts(s: &Server) -> Facts {
    let mut f = Facts::default();
    let mut mb = Ids::default();
    let mut em = Ids::default();
    let mut th = Ids::default();
    let acct = s.account.as_str();

    let r = s
        .mail(&[(
            "Mailbox/set",
            json!({"accountId": acct, "create": {"X": {"name": unique("diff-email"), "parentId": null}}}),
            "c",
        )])
        .await;
    mb.add(&r.args("c")["created"]["X"]["id"], "X");
    let x = mb.id("X");
    let email_state0 = s.state("Email").await;
    let mailbox_state0 = s.state("Mailbox").await;

    let email = |subject: &str, at: &str, keywords: Value| {
        json!({
            "mailboxIds": {&x: true},
            "keywords": keywords,
            "subject": subject,
            "from": [{"name": "Alice", "email": "alice@example.org"}],
            "to": [{"name": "Bob", "email": "bob@example.org"}],
            "receivedAt": at,
            "bodyValues": {"t": {"value": format!("Hello {subject}.")}},
            "textBody": [{"partId": "t", "type": "text/plain"}],
        })
    };
    let r = s
        .mail(&[(
            "Email/set",
            json!({"accountId": acct, "create": {
                "e1": email("alpha", "2026-01-01T00:00:01Z", json!({"$seen": true})),
                "e2": email("bravo", "2026-01-01T00:00:02Z", json!({"$flagged": true})),
                "e3": email("charlie", "2026-01-01T00:00:03Z", json!({})),
            }}),
            "c",
        )])
        .await;
    let set = r.args("c");
    f.put(
        "create.notCreated empty",
        json!(empty(set.get("notCreated"))),
    );
    for k in ["e1", "e2", "e3"] {
        em.add(&set["created"][k]["id"], k);
    }
    // RFC 8621 §4.6: created has id, blobId, threadId and size.
    let created = &set["created"]["e1"];
    f.put(
        "create.created[e1] required",
        json!(
            ["id", "blobId", "threadId", "size"].map(|k| format!("{k}:{}", kind(created.get(k))))
        ),
    );

    let props = json!([
        "id",
        "mailboxIds",
        "keywords",
        "subject",
        "from",
        "to",
        "receivedAt",
        "hasAttachment",
        "threadId",
        "blobId",
        "size",
        "preview",
        "sentAt"
    ]);
    let r = s
        .mail(&[
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x},
                       "sort": [{"property": "receivedAt", "isAscending": false}],
                       "calculateTotal": true}),
                "q",
            ),
            (
                "Email/get",
                json!({"accountId": acct, "properties": props,
                       "#ids": {"resultOf": "q", "name": "Email/query", "path": "/ids"}}),
                "g",
            ),
            (
                "Thread/get",
                json!({"accountId": acct,
                       "#ids": {"resultOf": "g", "name": "Email/get", "path": "/list/*/threadId"}}),
                "t",
            ),
        ])
        .await;
    let q = r.args("q");
    f.put("query keys", keys(q));
    f.put("query receivedAt desc", em.ordered(&q["ids"]));
    f.put("query.total", q["total"].clone());
    f.put(
        "query.canCalculateChanges kind",
        json!(kind(q.get("canCalculateChanges"))),
    );
    let g = r.args("g");
    f.put(
        "get (via reference) order",
        em.ordered(&json!(
            g["list"]
                .as_array()
                .expect("list")
                .iter()
                .map(|e| e["id"].clone())
                .collect::<Vec<_>>()
        )),
    );
    for e in g["list"].as_array().expect("list") {
        let alias = em.norm(&e["id"]);
        let alias = alias.as_str().expect("alias").to_owned();
        th.add(&e["threadId"], &format!("thread-of-{alias}"));
        let mut e = e.clone();
        e["id"] = em.norm(&e["id"]);
        e["threadId"] = th.norm(&e["threadId"]);
        e["mailboxIds"] = mb.norm(&e["mailboxIds"]);
        // RFC 8621 §4.1.1: a server may set keywords itself (Stalwart's
        // $hasnoattachment); compare only the ones the test set.
        if let Some(kw) = e["keywords"].as_object_mut() {
            kw.retain(|k, _| ["$seen", "$flagged", "$draft", "$answered"].contains(&k.as_str()));
        }
        for k in ["blobId", "size", "sentAt", "preview"] {
            e[k] = json!(kind(e.get(k)));
        }
        f.put(&format!("get[{alias}]"), e);
    }
    let t = r.args("t");
    let mut threads: Vec<Value> = t["list"]
        .as_array()
        .expect("list")
        .iter()
        .map(|t| json!({"id": th.norm(&t["id"]), "emailIds": em.norm(&t["emailIds"])}))
        .collect();
    threads.sort_by_key(|th| th["id"].to_string());
    f.put("thread/get via /list/*/threadId", json!(threads));

    let r = s
        .mail(&[
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x},
                       "sort": [{"property": "receivedAt", "isAscending": true}]}),
                "asc",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x},
                       "sort": [{"property": "subject", "isAscending": false}]}),
                "subject desc",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x, "hasKeyword": "$flagged"}}),
                "hasKeyword",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x, "notKeyword": "$seen"},
                       "sort": [{"property": "receivedAt", "isAscending": true}]}),
                "notKeyword",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"operator": "AND", "conditions": [
                           {"inMailbox": &x}, {"subject": "bravo"}]}}),
                "subject",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"operator": "AND", "conditions": [
                           {"inMailbox": &x},
                           {"operator": "NOT", "conditions": [{"hasKeyword": "$seen"}]}]},
                       "sort": [{"property": "receivedAt", "isAscending": true}]}),
                "NOT",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x},
                       "sort": [{"property": "receivedAt", "isAscending": true}],
                       "position": 1, "limit": 1}),
                "position",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x},
                       "sort": [{"property": "receivedAt", "isAscending": true}],
                       "position": -1}),
                "negative position",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x},
                       "sort": [{"property": "receivedAt", "isAscending": true}],
                       "anchor": em.id("e2"), "anchorOffset": 1, "limit": 1}),
                "anchor",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x},
                       "anchor": "zz9-not-an-email"}),
                "anchor not found",
            ),
            (
                "Email/query",
                json!({"accountId": acct, "filter": {"inMailbox": &x}, "limit": -1}),
                "negative limit",
            ),
        ])
        .await;
    for id in [
        "asc",
        "subject desc",
        "hasKeyword",
        "notKeyword",
        "subject",
        "NOT",
    ] {
        f.put(&format!("query {id}"), em.ordered(&r.args(id)["ids"]));
    }
    let p = r.args("position");
    f.put(
        "query position=1 limit=1",
        json!([em.ordered(&p["ids"]), p["position"]]),
    );
    let p = r.args("negative position");
    f.put(
        "query position=-1",
        json!([em.ordered(&p["ids"]), p["position"]]),
    );
    let p = r.args("anchor");
    f.put(
        "query anchor e2 offset 1",
        json!([em.ordered(&p["ids"]), p["position"]]),
    );
    f.put("query anchor not found", r.outcome("anchor not found"));
    f.put("query negative limit", r.outcome("negative limit"));

    // Counts and /changes after creating the Emails.
    let r = s
        .mail(&[
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": [&x],
                       "properties": ["totalEmails", "unreadEmails", "totalThreads", "unreadThreads"]}),
                "m",
            ),
            (
                "Email/changes",
                json!({"accountId": acct, "sinceState": email_state0}),
                "ec",
            ),
            (
                "Mailbox/changes",
                json!({"accountId": acct, "sinceState": mailbox_state0}),
                "mc",
            ),
        ])
        .await;
    f.put("counts after create", mb.norm(&r.args("m")["list"][0]));
    let ec = r.args("ec");
    f.put("Email/changes.created", em.known(&ec["created"]));
    f.put("Email/changes.updated", em.known(&ec["updated"]));
    let mc = r.args("mc");
    f.put(
        "Mailbox/changes after Email create .updated",
        mb.known(&mc["updated"]),
    );
    f.put(
        "Mailbox/changes after Email create .updatedProperties",
        sorted(&mc["updatedProperties"]),
    );
    let email_state1 = ec["newState"].clone();
    let mailbox_state1 = mc["newState"].clone();

    // Patch a keyword; /changes reports e3 updated and the Mailbox counts.
    let r = s
        .mail(&[
            (
                "Email/set",
                json!({"accountId": acct, "update": {em.id("e3"): {"keywords/$seen": true}}}),
                "u",
            ),
            (
                "Email/changes",
                json!({"accountId": acct, "sinceState": email_state1}),
                "ec",
            ),
            (
                "Mailbox/changes",
                json!({"accountId": acct, "sinceState": mailbox_state1}),
                "mc",
            ),
            (
                "Email/get",
                json!({"accountId": acct, "ids": [em.id("e3")], "properties": ["keywords"]}),
                "g",
            ),
            (
                "Mailbox/get",
                json!({"accountId": acct, "ids": [&x], "properties": ["unreadEmails"]}),
                "m",
            ),
        ])
        .await;
    f.put("update.updated", em.norm(&r.args("u")["updated"]));
    let ec = r.args("ec");
    f.put("update Email/changes.created", em.known(&ec["created"]));
    f.put("update Email/changes.updated", em.known(&ec["updated"]));
    let mc = r.args("mc");
    f.put("update Mailbox/changes.updated", mb.known(&mc["updated"]));
    f.put(
        "update Mailbox/changes.updatedProperties",
        sorted(&mc["updatedProperties"]),
    );
    f.put(
        "update keyword visible",
        r.args("g")["list"][0]["keywords"]["$seen"].clone(),
    );
    f.put(
        "update unreadEmails",
        r.args("m")["list"][0]["unreadEmails"].clone(),
    );

    // Creation ids across calls (RFC 8620 §5.3, §3.3 createdIds): a
    // Mailbox created in one call is the Email's mailbox in the next.
    let r = s
        .mail(&[
            (
                "Mailbox/set",
                json!({"accountId": acct, "create": {"Y": {"name": "why", "parentId": &x}}}),
                "y",
            ),
            (
                "Email/set",
                json!({"accountId": acct, "create": {
                    "e4": {"mailboxIds": {"#Y": true}, "subject": "delta",
                           "bodyValues": {"t": {"value": "d"}},
                           "textBody": [{"partId": "t", "type": "text/plain"}]}}}),
                "e",
            ),
        ])
        .await;
    mb.add(&r.args("y")["created"]["Y"]["id"], "Y");
    let e4 = &r.args("e")["created"]["e4"]["id"];
    f.put("creation-id reference created e4", json!(e4.is_string()));
    if e4.is_string() {
        em.add(e4, "e4");
        let r = s
            .mail(&[(
                "Email/get",
                json!({"accountId": acct, "ids": [em.id("e4")], "properties": ["mailboxIds"]}),
                "g",
            )])
            .await;
        f.put(
            "e4.mailboxIds",
            mb.norm(&r.args("g")["list"][0]["mailboxIds"]),
        );
    }

    // SetErrors.
    let r = s
        .mail(&[
            (
                "Email/set",
                json!({"accountId": acct,
                    "create": {
                        "noMailbox": {"subject": "x", "mailboxIds": {}},
                        "badMailbox": {"subject": "x", "mailboxIds": {"zz9-no-such": true}},
                    },
                    "update": {
                        "zz9-none": {"keywords/$seen": true},
                        em.id("e1"): {"mailboxIds": {"zz9-no-such": true}},
                    },
                    "destroy": ["zz9-none"]}),
                "e",
            ),
            (
                "Mailbox/set",
                json!({"accountId": acct, "destroy": [&x]}),
                "has email",
            ),
            (
                "Email/get",
                json!({"accountId": acct, "ids": ["zz9-none"]}),
                "get unknown",
            ),
        ])
        .await;
    let e = r.args("e");
    f.put("errors.notCreated", set_errors(&em, &e["notCreated"]));
    f.put("errors.notUpdated", set_errors(&em, &e["notUpdated"]));
    f.put("errors.notDestroyed", set_errors(&em, &e["notDestroyed"]));
    f.put(
        "errors mailbox with child and Emails",
        set_errors(&mb, &r.args("has email")["notDestroyed"]),
    );
    f.put(
        "get unknown.notFound",
        r.args("get unknown")["notFound"].clone(),
    );

    // Clean up: e1 (which a server that accepted the move above no longer
    // has in X) and any Email created above, then Y and X with the rest of
    // their Emails.
    let mut stray: Vec<Value> = e["created"]
        .as_object()
        .map(|o| o.values().map(|c| c["id"].clone()).collect())
        .unwrap_or_default();
    stray.push(json!(em.id("e1")));
    let destroy = vec![json!(mb.id("Y")), json!(x)];
    let r = s
        .mail(&[
            (
                "Email/set",
                json!({"accountId": acct, "destroy": stray}),
                "stray",
            ),
            (
                "Mailbox/set",
                json!({"accountId": acct, "destroy": destroy,
                       "onDestroyRemoveEmails": true}),
                "d",
            ),
            (
                "Email/get",
                json!({"accountId": acct, "ids": [em.id("e2"), em.id("e3")],
                       "properties": ["id"]}),
                "g",
            ),
        ])
        .await;
    f.put("cleanup.destroyed", mb.known(&r.args("d")["destroyed"]));
    f.put("cleanup Emails gone", em.known(&r.args("g")["notFound"]));
    f
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

/// RFC 8620 §2 and RFC 8621 §1.3: the Session's shape.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn session_shape() {
    let _server = jmap_interop::exclusive().await;
    let p = pair().await;
    compare("session", &session_facts(&p.jig), &session_facts(&p.other));
}

/// RFC 8620 §3.6.1 request-level errors, §3.3 and §4 Core/echo.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn request_level() {
    let _server = jmap_interop::exclusive().await;
    let p = pair().await;
    compare(
        "request",
        &request_facts(&p.jig).await,
        &request_facts(&p.other).await,
    );
}

/// RFC 8620 §3.6.2, §3.7 and §5 method-level errors.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn method_errors() {
    let _server = jmap_interop::exclusive().await;
    let p = pair().await;
    compare(
        "method-errors",
        &method_error_facts(&p.jig).await,
        &method_error_facts(&p.other).await,
    );
}

/// RFC 8621 §2 Mailbox methods.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn mailbox() {
    let _server = jmap_interop::exclusive().await;
    let p = pair().await;
    compare(
        "mailbox",
        &mailbox_facts(&p.jig).await,
        &mailbox_facts(&p.other).await,
    );
}

/// RFC 8621 §3 and §4 Thread and Email methods.
#[tokio::test]
#[ignore = "needs a JMAP server: scripts/stalwart-up.sh, see docs/interop/README.md"]
async fn email() {
    let _server = jmap_interop::exclusive().await;
    let p = pair().await;
    compare(
        "email",
        &email_facts(&p.jig).await,
        &email_facts(&p.other).await,
    );
}
