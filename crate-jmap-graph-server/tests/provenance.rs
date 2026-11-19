//! draft-atwood-jmap-graph-provenance-00 through the dispatcher and the
//! memory backend (bd JMAP-98sl.2 to .7).
//!
//! Oracle: draft-atwood-jmap-graph-provenance-00.md at jmap-chat-spec
//! addb3fb; each test quotes the rule it checks, with its line. Expected
//! values are written out by hand from that text.

mod common;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use common::{ACCOUNT, ALICE, BOB, CAROL, backend};
use jmap_graph_server::memory::MemoryBackend;
use jmap_graph_server::provenance::sweep_retention;
use jmap_graph_server::{register_graph_conditional, register_graph_handlers_with_history};
use jmap_object_history_server::HistoryRetention;
use jmap_server::Dispatcher;
use jmap_types::{Id, JmapRequest, State};
use serde_json::{Value, json};

const EVIDENCE: &str = "urn:ietf:params:jmap:graph:provenance#evidence";
/// The §Examples credential digest (L285).
const DIGEST: &str = "htSyDucPJhKUVHKPpzpMNO8bl50vhWucS4VMDVuc_ds";

struct Server {
    b: MemoryBackend,
    d: Dispatcher<Id>,
}

impl Server {
    /// [`ACCOUNT`] owned by [`ALICE`], with provenance, GraphNode history
    /// and conditional `/set`.
    fn new() -> Self {
        Self::with(true)
    }

    fn with(provenance: bool) -> Self {
        let b = backend().with_object_history(
            serde_json::from_value(json!({
                "maxHistoryDuration": null,
                "supportedTypes": ["Graph", "GraphNode", "GraphEdge"]
            }))
            .unwrap(),
            HistoryRetention::new(),
        );
        if provenance {
            b.register_provenance(&Id::from(ACCOUNT));
        }
        let mut d = Dispatcher::new();
        register_graph_handlers_with_history(&mut d, Arc::new(b.clone()));
        register_graph_conditional(&mut d, Arc::new(b.clone()));
        Self { b, d }
    }

    /// The `[name, arguments]` of the one response to `method`.
    async fn raw(&self, who: &str, method: &str, mut args: Value) -> Value {
        args["accountId"] = json!(ACCOUNT);
        let req: JmapRequest = serde_json::from_value(json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:graph",
                      "urn:ietf:params:jmap:graph:provenance",
                      "urn:ietf:params:jmap:object-history",
                      "urn:ietf:params:jmap:conditional"],
            "methodCalls": [[method, args, "0"]]
        }))
        .unwrap();
        let resp =
            serde_json::to_value(self.d.dispatch(req, Id::from(who), State::from("s")).await)
                .unwrap();
        resp["methodResponses"][0].clone()
    }

    async fn call(&self, who: &str, method: &str, args: Value) -> Value {
        let r = self.raw(who, method, args).await;
        assert_ne!(r[0], "error", "{r}");
        r[1].clone()
    }

    /// Create one object as `who`; the `created` entry, or the
    /// `notCreated` entry as `Err`.
    async fn create_as(&self, who: &str, method: &str, object: Value) -> Result<Value, Value> {
        let out = self
            .call(who, method, json!({ "create": { "x": object } }))
            .await;
        match out["notCreated"].get("x") {
            Some(e) => Err(e.clone()),
            None => Ok(out["created"]["x"].clone()),
        }
    }

    async fn create(&self, method: &str, object: Value) -> String {
        let c = self.create_as(ALICE, method, object).await.unwrap();
        c["id"].as_str().unwrap().to_owned()
    }

    /// Update `id` as `who`; the `updated` entry, or `notUpdated` as `Err`.
    async fn update_as(
        &self,
        who: &str,
        method: &str,
        id: &str,
        patch: Value,
    ) -> Result<Value, Value> {
        let out = self
            .call(who, method, json!({ "update": { id: patch } }))
            .await;
        match out["notUpdated"].get(id) {
            Some(e) => Err(e.clone()),
            None => Ok(out["updated"][id].clone()),
        }
    }

    async fn destroy_as(&self, who: &str, method: &str, id: &str) -> Result<(), Value> {
        let out = self.call(who, method, json!({ "destroy": [id] })).await;
        match out["notDestroyed"].get(id) {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    async fn node(&self, id: &str) -> Value {
        let out = self
            .call(ALICE, "GraphNode/get", json!({ "ids": [id] }))
            .await;
        out["list"][0].clone()
    }

    /// A Graph shared with [`BOB`] (mayWrite, not mayCurate) and [`CAROL`]
    /// (mayCurate, not mayWrite).
    async fn graph(&self) -> String {
        self.create(
            "Graph/set",
            json!({ "name": "G", "shareWith": {
                BOB: { "mayRead": true, "mayWrite": true, "mayAdmin": false, "mayCurate": false },
                CAROL: { "mayRead": true, "mayWrite": false, "mayAdmin": false, "mayCurate": true }
            }}),
        )
        .await
    }
}

fn invalid(e: &Value, property: &str) {
    assert_eq!(e["type"], "invalidProperties", "{e}");
    assert_eq!(e["properties"], json!([property]), "{e}");
}

fn forbidden(e: &Value) {
    assert_eq!(e["type"], "forbidden", "{e}");
}

// ---------------------------------------------------------------------------
// §Provenance Properties and §Curation Properties (98sl.2)
// ---------------------------------------------------------------------------

/// §Examples L271-286: the agent's record and the credential node are
/// accepted, and the omitted properties come back with their defaults
/// (L135-180: `null`, `status` `active`, `pinned` `false`).
#[tokio::test]
async fn the_example_records_are_accepted_with_defaults() {
    let s = Server::new();
    let g = s.graph().await;
    let r1 = s
        .create_as(
            ALICE,
            "GraphNode/set",
            json!({
                "graphId": g, "blobId": "Bk7q", "mediaType": "text/markdown",
                "basis": "inferred", "confidence": 0.6,
                "validUntil": "2027-01-31T00:00:00Z",
                "validFor": { "purposes": ["https://w3id.org/dpv#ServiceProvision"] }
            }),
        )
        .await
        .unwrap();
    assert_eq!(r1["status"], "active");
    assert_eq!(r1["pinned"], false);
    assert_eq!(r1["curationReason"], Value::Null);
    assert_eq!(r1["credentialDigest"], Value::Null);
    let c1 = s
        .create_as(
            ALICE,
            "GraphNode/set",
            json!({
                "graphId": g,
                "uri": "urn:uuid:4f1c9a3e-8d2b-4e7f-9a61-0c5d2b7e8f13",
                "credentialDigest": DIGEST
            }),
        )
        .await
        .unwrap();
    assert_eq!(c1["basis"], Value::Null);
}

/// Each value rule rejects with `invalidProperties` naming the property:
/// `basis` "rejects any other value" (L143); `confidence` "outside the
/// range" 0..1 (L146); `validFor` "with neither member, or with a
/// `purposes` array that is empty or has more than 8 entries" (L160);
/// `credentialDigest` "not 43 base64url characters" (L163); `status` "any
/// other value" (L174); `validUntil` is a UTCDate (L148).
#[tokio::test]
async fn invalid_values_name_the_property() {
    let s = Server::new();
    let g = s.graph().await;
    let nine: Vec<String> = (0..9)
        .map(|i| format!("https://example.com/p{i}"))
        .collect();
    for (property, value) in [
        ("basis", json!("guessed")),
        ("confidence", json!(1.5)),
        ("confidence", json!(-0.1)),
        ("confidence", json!("high")),
        ("validFor", json!({})),
        ("validFor", json!({ "purposes": [] })),
        ("validFor", json!({ "purposes": nine })),
        ("validFor", json!({ "purposes": ["not an iri"] })),
        // 43 base64url characters whose last one no SHA-256 digest ends in.
        ("credentialDigest", json!(format!("{}t", &DIGEST[..42]))),
        ("credentialDigest", json!(&DIGEST[..42])),
        ("credentialDigest", json!(format!("{}+", &DIGEST[..42]))),
        ("status", json!("archived")),
        ("status", Value::Null),
        ("pinned", json!("yes")),
        ("validUntil", json!("2027-01-31")),
    ] {
        let mut node = json!({ "graphId": g });
        node[property] = value.clone();
        let e = s.create_as(ALICE, "GraphNode/set", node).await.unwrap_err();
        invalid(&e, property);
    }
    // The bounds are inclusive, and a note alone is a PermittedUse.
    for node in [
        json!({ "graphId": g, "confidence": 0 }),
        json!({ "graphId": g, "confidence": 1 }),
        json!({ "graphId": g, "validFor": { "note": "inventory only" } }),
        json!({ "graphId": g, "validFor": { "purposes": nine[..8] } }),
    ] {
        s.create_as(ALICE, "GraphNode/set", node).await.unwrap();
    }
}

/// The same rules hold for an update, on the record as it would be stored.
#[tokio::test]
async fn invalid_values_on_update() {
    let s = Server::new();
    let g = s.graph().await;
    let n = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let e = s
        .update_as(ALICE, "GraphNode/set", &n, json!({ "basis": "rumour" }))
        .await
        .unwrap_err();
    invalid(&e, "basis");
}

/// Without the capability the properties are not this document's: they
/// are kept as given, unchecked and without defaults (opt-in, like
/// graph-edit and graph-search).
#[tokio::test]
async fn without_the_capability_nothing_is_checked() {
    let s = Server::with(false);
    let g = s.create("Graph/set", json!({ "name": "G" })).await;
    let c = s
        .create_as(
            ALICE,
            "GraphNode/set",
            json!({ "graphId": g, "basis": "rumour" }),
        )
        .await
        .unwrap();
    assert!(c.get("status").is_none(), "{c}");
    let id = c["id"].as_str().unwrap();
    assert_eq!(s.node(id).await["basis"], "rumour");
}

// ---------------------------------------------------------------------------
// §Sharing Rights (98sl.3)
// ---------------------------------------------------------------------------

/// "The owner of the account that holds the Graph implicitly has it, as
/// for every right" (L209).
#[tokio::test]
async fn the_owner_may_curate() {
    let s = Server::new();
    let g = s.graph().await;
    let out = s.call(ALICE, "Graph/get", json!({ "ids": [g] })).await;
    assert_eq!(out["list"][0]["myRights"]["mayCurate"], true, "{out}");
}

/// "The server rejects, with a `forbidden` SetError, a `GraphNode/set`
/// update by a user without `mayCurate` that changes `status`, `pinned`, or
/// `curationReason`. An update that changes only those properties requires
/// `mayCurate` and does not require `mayWrite`" (L211).
#[tokio::test]
async fn curation_needs_may_curate_and_not_may_write() {
    let s = Server::new();
    let g = s.graph().await;
    let n = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let demote = json!({ "status": "deprecated", "curationReason": "Superseded." });
    forbidden(
        &s.update_as(BOB, "GraphNode/set", &n, demote.clone())
            .await
            .unwrap_err(),
    );
    s.update_as(CAROL, "GraphNode/set", &n, demote)
        .await
        .unwrap();
    // Carol may not write anything else, Bob still may.
    forbidden(
        &s.update_as(CAROL, "GraphNode/set", &n, json!({ "basis": "observed" }))
            .await
            .unwrap_err(),
    );
    s.update_as(BOB, "GraphNode/set", &n, json!({ "basis": "observed" }))
        .await
        .unwrap();
    // "A curator may change `status` from `deprecated` to `active`" (L186).
    s.update_as(CAROL, "GraphNode/set", &n, json!({ "status": "active" }))
        .await
        .unwrap();
}

/// "Destroying a record requires `mayCurate`, in place of the `mayWrite`
/// that {{JMAP-GRAPH}} requires" (L213).
#[tokio::test]
async fn destroy_needs_may_curate() {
    let s = Server::new();
    let g = s.graph().await;
    let n = s.create("GraphNode/set", json!({ "graphId": g })).await;
    forbidden(&s.destroy_as(BOB, "GraphNode/set", &n).await.unwrap_err());
    s.destroy_as(CAROL, "GraphNode/set", &n).await.unwrap();
}

/// "On create, a user without `mayCurate` may set `status` only to
/// `active`, `pinned` only to `false`, and `curationReason` only to `null`;
/// the server rejects other values with a `forbidden` SetError" (L213).
#[tokio::test]
async fn create_limits_for_non_curators() {
    let s = Server::new();
    let g = s.graph().await;
    for extra in [
        json!({ "status": "deprecated" }),
        json!({ "pinned": true }),
        json!({ "curationReason": "why" }),
    ] {
        let mut node = json!({ "graphId": g });
        node.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        forbidden(
            &s.create_as(BOB, "GraphNode/set", node.clone())
                .await
                .unwrap_err(),
        );
        s.create_as(ALICE, "GraphNode/set", node).await.unwrap();
    }
    s.create_as(
        BOB,
        "GraphNode/set",
        json!({ "graphId": g, "status": "active", "pinned": false, "curationReason": null }),
    )
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// §Retraction (98sl.4)
// ---------------------------------------------------------------------------

async fn record_with_content(s: &Server, g: &str) -> String {
    s.create(
        "GraphNode/set",
        json!({
            "graphId": g, "blobId": "Bk7q", "mediaType": "text/markdown",
            "uri": "https://example.com/r", "basis": "observed", "confidence": 0.9,
            "validUntil": "2027-01-31T00:00:00Z", "validFor": { "note": "n" },
            "credentialDigest": DIGEST, "annotations": { "com.example.k": 1 }
        }),
    )
    .await
}

/// "A record is created with a `status` other than `retracted`; the server
/// rejects a create with `status` `retracted` with an `invalidProperties`
/// SetError naming `status`" (L184).
#[tokio::test]
async fn no_create_as_retracted() {
    let s = Server::new();
    let g = s.graph().await;
    let e = s
        .create_as(
            ALICE,
            "GraphNode/set",
            json!({ "graphId": g, "status": "retracted" }),
        )
        .await
        .unwrap_err();
    invalid(&e, "status");
}

/// "When an update makes a record's `status` `retracted`, the server sets
/// its `blobId`, `mediaType`, `uri`, `basis`, `confidence`, `validUntil`,
/// `validFor`, and `credentialDigest` to `null`, and its `annotations` to
/// `{}`, in the same update" (L184); "The node and its edges remain".
#[tokio::test]
async fn retraction_clears_the_content() {
    let s = Server::new();
    let g = s.graph().await;
    let n = record_with_content(&s, &g).await;
    let other = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let e = s
        .create(
            "GraphEdge/set",
            json!({ "graphId": g, "fromId": n, "toId": other, "type": EVIDENCE }),
        )
        .await;
    let echo = s
        .update_as(
            CAROL,
            "GraphNode/set",
            &n,
            json!({ "status": "retracted", "curationReason": "Wrong." }),
        )
        .await
        .unwrap();
    assert_eq!(
        echo["blobId"],
        Value::Null,
        "the clearing is echoed: {echo}"
    );
    let node = s.node(&n).await;
    for k in [
        "blobId",
        "mediaType",
        "uri",
        "basis",
        "confidence",
        "validUntil",
        "validFor",
        "credentialDigest",
    ] {
        assert_eq!(node[k], Value::Null, "{k}: {node}");
    }
    assert_eq!(node["annotations"], json!({}));
    assert_eq!(node["status"], "retracted");
    let edges = s.call(ALICE, "GraphEdge/get", json!({ "ids": [e] })).await;
    assert_eq!(edges["list"].as_array().unwrap().len(), 1, "{edges}");
}

/// "An update that makes `status` `retracted` and also sets any of those
/// properties to a value other than the one listed is rejected with an
/// `invalidProperties` SetError naming that property" (L184).
#[tokio::test]
async fn retraction_cannot_set_a_cleared_property() {
    let s = Server::new();
    let g = s.graph().await;
    let n = record_with_content(&s, &g).await;
    let e = s
        .update_as(
            ALICE,
            "GraphNode/set",
            &n,
            json!({ "status": "retracted", "blobId": "Other" }),
        )
        .await
        .unwrap_err();
    invalid(&e, "blobId");
    // Setting one to the cleared value is not a conflict.
    s.update_as(
        ALICE,
        "GraphNode/set",
        &n,
        json!({ "status": "retracted", "blobId": null, "annotations": {} }),
    )
    .await
    .unwrap();
}

/// "Retraction cannot be undone. The server rejects any update to a
/// retracted record other than one to `pinned` or `curationReason`, or a
/// destroy, with an `invalidProperties` SetError naming the property"
/// (L186).
#[tokio::test]
async fn retraction_cannot_be_undone() {
    let s = Server::new();
    let g = s.graph().await;
    let n = s.create("GraphNode/set", json!({ "graphId": g })).await;
    s.update_as(ALICE, "GraphNode/set", &n, json!({ "status": "retracted" }))
        .await
        .unwrap();
    invalid(
        &s.update_as(ALICE, "GraphNode/set", &n, json!({ "status": "active" }))
            .await
            .unwrap_err(),
        "status",
    );
    invalid(
        &s.update_as(ALICE, "GraphNode/set", &n, json!({ "blobId": "B2" }))
            .await
            .unwrap_err(),
        "blobId",
    );
    s.update_as(
        ALICE,
        "GraphNode/set",
        &n,
        json!({ "pinned": true, "curationReason": "Keep the tombstone." }),
    )
    .await
    .unwrap();
    s.destroy_as(ALICE, "GraphNode/set", &n).await.unwrap();
}

/// "The server rejects, with a `forbidden` SetError, a `GraphEdge/set` that
/// would create, update, or destroy an edge whose `fromId` is, or would
/// become, a retracted record" (L186). An edge to a retracted record is
/// unaffected.
#[tokio::test]
async fn edges_from_a_retracted_record_are_frozen() {
    let s = Server::new();
    let g = s.graph().await;
    let r = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let a = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let b = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let from_r = s
        .create(
            "GraphEdge/set",
            json!({ "graphId": g, "fromId": r, "toId": a, "type": EVIDENCE }),
        )
        .await;
    let from_a = s
        .create(
            "GraphEdge/set",
            json!({ "graphId": g, "fromId": a, "toId": b, "type": "com.example.link" }),
        )
        .await;
    s.update_as(ALICE, "GraphNode/set", &r, json!({ "status": "retracted" }))
        .await
        .unwrap();
    let edge = |from: &str, to: &str| json!({ "graphId": g, "fromId": from, "toId": to, "type": EVIDENCE });
    forbidden(
        &s.create_as(ALICE, "GraphEdge/set", edge(&r, &b))
            .await
            .unwrap_err(),
    );
    forbidden(
        &s.update_as(ALICE, "GraphEdge/set", &from_r, json!({ "toId": b }))
            .await
            .unwrap_err(),
    );
    forbidden(
        &s.update_as(ALICE, "GraphEdge/set", &from_a, json!({ "fromId": r }))
            .await
            .unwrap_err(),
    );
    forbidden(
        &s.destroy_as(ALICE, "GraphEdge/set", &from_r)
            .await
            .unwrap_err(),
    );
    s.create_as(ALICE, "GraphEdge/set", edge(&a, &r))
        .await
        .unwrap();
}

/// "A server that also implements {{JMAP-OBJ-HISTORY}} and lists
/// `GraphNode` in its `supportedTypes` SHOULD NOT return any prior version
/// of a retracted record, and SHOULD permanently remove those versions"
/// (L188).
#[tokio::test]
async fn retraction_removes_prior_versions() {
    let s = Server::new();
    s.b.set_history_clock_for_test(1_000);
    let g = s.graph().await;
    let n = record_with_content(&s, &g).await;
    s.b.set_history_clock_for_test(2_000);
    s.update_as(ALICE, "GraphNode/set", &n, json!({ "blobId": "Bk8" }))
        .await
        .unwrap();
    let versions = |out: Value| out["list"].as_array().unwrap().len();
    let before = s
        .call(
            ALICE,
            "GraphNode/get",
            json!({ "ids": [&n], "includeReplaced": true }),
        )
        .await;
    assert_eq!(versions(before), 2);
    s.b.set_history_clock_for_test(3_000);
    s.update_as(ALICE, "GraphNode/set", &n, json!({ "status": "retracted" }))
        .await
        .unwrap();
    s.b.set_history_clock_for_test(4_000);
    s.update_as(ALICE, "GraphNode/set", &n, json!({ "pinned": true }))
        .await
        .unwrap();
    let after = s
        .call(
            ALICE,
            "GraphNode/get",
            json!({ "ids": [&n], "includeReplaced": true }),
        )
        .await;
    assert_eq!(
        versions(after.clone()),
        1,
        "only the current version: {after}"
    );
}

// ---------------------------------------------------------------------------
// §Evidence and §Limits (98sl.5)
// ---------------------------------------------------------------------------

/// "A record has at most 32 evidence edges. The server rejects, with an
/// `overQuota` SetError, a `GraphEdge/set` create or update that would give
/// a record more than 32" (L260). Edges of other types do not count.
#[tokio::test]
async fn at_most_32_evidence_edges() {
    let s = Server::new();
    let g = s.graph().await;
    let r = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let t = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let edge = |ty: &str| json!({ "graphId": g, "fromId": r, "toId": t, "type": ty });
    for _ in 0..32 {
        s.create_as(ALICE, "GraphEdge/set", edge(EVIDENCE))
            .await
            .unwrap();
    }
    let e = s
        .create_as(ALICE, "GraphEdge/set", edge(EVIDENCE))
        .await
        .unwrap_err();
    assert_eq!(e["type"], "overQuota", "{e}");
    let other = s.create("GraphEdge/set", edge("com.example.link")).await;
    let e = s
        .update_as(ALICE, "GraphEdge/set", &other, json!({ "type": EVIDENCE }))
        .await
        .unwrap_err();
    assert_eq!(e["type"], "overQuota", "{e}");
}

/// conditional-02 §4.2: an atomic call is judged on its final state, so it
/// can replace one evidence edge of a record at the limit (the create runs
/// before the destroy, RFC 8620 §5.3), and one that ends above it fails.
#[tokio::test]
async fn an_atomic_replacement_at_the_limit_commits() {
    let s = Server::new();
    let g = s.graph().await;
    let r = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let t = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let edge = json!({ "graphId": g, "fromId": r, "toId": t, "type": EVIDENCE });
    let mut ids = Vec::new();
    for _ in 0..32 {
        ids.push(s.create("GraphEdge/set", edge.clone()).await);
    }
    let out = s
        .call(
            ALICE,
            "GraphEdge/set",
            json!({ "atomic": true, "create": { "x": edge }, "destroy": [ids[0]] }),
        )
        .await;
    assert!(out["created"].get("x").is_some(), "{out}");
    let r = s
        .raw(
            ALICE,
            "GraphEdge/set",
            json!({ "atomic": true, "create": { "y": edge } }),
        )
        .await;
    assert_eq!(r[1]["type"], "atomicFailure", "{r}");
}

// ---------------------------------------------------------------------------
// §Graph Extensions and §Retention (98sl.6)
// ---------------------------------------------------------------------------

/// "Only a user with `mayAdmin` may change it; the server rejects a change
/// by any other user with a `forbidden` SetError"; "Each value, when not
/// `null`, is between 1 and 36500. The server rejects others with an
/// `invalidProperties` SetError naming `retention`" (L220-228).
#[tokio::test]
async fn retention_rules() {
    let s = Server::new();
    let g = s.graph().await;
    forbidden(
        &s.update_as(
            BOB,
            "Graph/set",
            &g,
            json!({ "retention": { "recordDays": 30 } }),
        )
        .await
        .unwrap_err(),
    );
    for bad in [
        json!({ "recordDays": 0 }),
        json!({ "tombstoneDays": 36501 }),
        json!(30),
    ] {
        invalid(
            &s.update_as(ALICE, "Graph/set", &g, json!({ "retention": bad }))
                .await
                .unwrap_err(),
            "retention",
        );
    }
    s.update_as(
        ALICE,
        "Graph/set",
        &g,
        json!({ "retention": { "recordDays": 1, "tombstoneDays": 36500 } }),
    )
    .await
    .unwrap();
    let created = s
        .create_as(ALICE, "Graph/set", json!({ "name": "H" }))
        .await
        .unwrap();
    assert_eq!(created["retention"], Value::Null, "default null: {created}");
}

/// "The server destroys each record that retention makes eligible, as if
/// by a `GraphNode/set` destroy, which also destroys its edges"; "Pinned
/// records are never eligible"; `recordDays` counts for records not
/// retracted and `tombstoneDays` for retracted ones, from `updated`
/// (L222-232). "Clients learn of destroyed records through
/// `GraphNode/changes`" (L234).
#[tokio::test]
async fn the_retention_sweep() {
    let s = Server::new();
    let g = s.graph().await;
    s.update_as(
        ALICE,
        "Graph/set",
        &g,
        json!({ "retention": { "recordDays": 30, "tombstoneDays": 60 } }),
    )
    .await
    .unwrap();
    let plain = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let pinned = s
        .create("GraphNode/set", json!({ "graphId": g, "pinned": true }))
        .await;
    let tomb = s.create("GraphNode/set", json!({ "graphId": g })).await;
    s.update_as(
        ALICE,
        "GraphNode/set",
        &tomb,
        json!({ "status": "retracted" }),
    )
    .await
    .unwrap();
    let e = s
        .create(
            "GraphEdge/set",
            json!({ "graphId": g, "fromId": pinned, "toId": plain, "type": "com.example.link" }),
        )
        .await;
    let state = s.call(ALICE, "GraphNode/get", json!({ "ids": [] })).await["state"].clone();

    let (caller, account) = (Id::from(ALICE), Id::from(ACCOUNT));
    let day = Duration::from_secs(86_400);
    let at = |days: u32| SystemTime::now() + day * days;
    assert_eq!(
        sweep_retention(&s.b, &caller, &account, at(29))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sweep_retention(&s.b, &caller, &account, at(31))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sweep_retention(&s.b, &caller, &account, at(61))
            .await
            .unwrap(),
        1
    );

    let changes = s
        .call(ALICE, "GraphNode/changes", json!({ "sinceState": state }))
        .await;
    let mut gone: Vec<String> = serde_json::from_value(changes["destroyed"].clone()).unwrap();
    gone.sort();
    let mut want = vec![plain.clone(), tomb.clone()];
    want.sort();
    assert_eq!(gone, want, "{changes}");
    let edges = s.call(ALICE, "GraphEdge/get", json!({ "ids": [e] })).await;
    assert_eq!(edges["notFound"], json!([e]), "the edge went with its node");
    assert_eq!(s.node(&pinned).await["id"], json!(pinned));
}

// ---------------------------------------------------------------------------
// §Query Filters (98sl.7)
// ---------------------------------------------------------------------------

async fn query(s: &Server, g: &str, filter: Value) -> Vec<String> {
    let mut f = filter;
    f["graphId"] = json!(g);
    let out = s
        .call(ALICE, "GraphNode/query", json!({ "filter": f }))
        .await;
    let mut ids: Vec<String> = serde_json::from_value(out["ids"].clone()).unwrap();
    ids.sort();
    ids
}

/// `basis` ("To match records with no basis, include `null` in the
/// array"), `status`, `pinned`, and `validAt`: "The record's `validUntil` is
/// `null` or later than the given time" (L244-254).
#[tokio::test]
async fn the_four_filters() {
    let s = Server::new();
    let g = s.graph().await;
    let none = s.create("GraphNode/set", json!({ "graphId": g })).await;
    let inferred = s
        .create(
            "GraphNode/set",
            json!({ "graphId": g, "basis": "inferred", "validUntil": "2027-01-31T00:00:00Z" }),
        )
        .await;
    let pinned = s
        .create(
            "GraphNode/set",
            json!({ "graphId": g, "basis": "declared", "pinned": true, "status": "deprecated" }),
        )
        .await;
    let sorted = |mut v: Vec<&String>| {
        v.sort();
        v.into_iter().cloned().collect::<Vec<_>>()
    };
    assert_eq!(
        query(&s, &g, json!({ "basis": [null, "inferred"] })).await,
        sorted(vec![&none, &inferred])
    );
    assert_eq!(
        query(&s, &g, json!({ "status": ["deprecated"] })).await,
        vec![pinned.clone()]
    );
    assert_eq!(
        query(&s, &g, json!({ "status": ["active"] })).await,
        sorted(vec![&none, &inferred])
    );
    assert_eq!(
        query(&s, &g, json!({ "pinned": true })).await,
        vec![pinned.clone()]
    );
    assert_eq!(
        query(&s, &g, json!({ "validAt": "2027-02-01T00:00:00Z" })).await,
        sorted(vec![&none, &pinned])
    );
    assert_eq!(
        query(&s, &g, json!({ "validAt": "2027-01-30T00:00:00Z" })).await,
        sorted(vec![&none, &inferred, &pinned])
    );
    // "later than": a record valid until exactly the given time is out.
    assert_eq!(
        query(&s, &g, json!({ "validAt": "2027-01-31T00:00:00Z" })).await,
        sorted(vec![&none, &pinned])
    );
}

/// Without the capability the four conditions are `unsupportedFilter`.
#[tokio::test]
async fn filters_need_the_capability() {
    let s = Server::with(false);
    let g = s.create("Graph/set", json!({ "name": "G" })).await;
    let r = s
        .raw(
            ALICE,
            "GraphNode/query",
            json!({ "filter": { "graphId": g, "pinned": true } }),
        )
        .await;
    assert_eq!(r[1]["type"], "unsupportedFilter", "{r}");
}

/// "Nor is a node that has an edit log ({{JMAP-GRAPH-EDIT}}), because edit
/// operations do not change `updated`" (L232): a node whose `editFormat`
/// is set is not swept.
#[tokio::test]
async fn a_node_with_an_edit_log_is_not_swept() {
    let s = Server::new();
    let cap: jmap_graph_types::GraphEditCapability = serde_json::from_value(json!({
        "maxOpSize": 8, "maxCursorSize": 4, "opRetention": null
    }))
    .unwrap();
    s.b.register_edit(&Id::from(ACCOUNT), cap);
    let g = s.graph().await;
    s.update_as(
        ALICE,
        "Graph/set",
        &g,
        json!({ "retention": { "recordDays": 1 } }),
    )
    .await
    .unwrap();
    let doc = s
        .create(
            "GraphNode/set",
            json!({ "graphId": g, "editFormat": "application/x-example-ops" }),
        )
        .await;
    let later = SystemTime::now() + Duration::from_secs(10 * 86_400);
    let n = sweep_retention(&s.b, &Id::from(ALICE), &Id::from(ACCOUNT), later)
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(s.node(&doc).await["id"], json!(doc));
}
