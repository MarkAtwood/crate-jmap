//! Quota/* handlers over the memory backend (RFC 9425 §4, §5).
//!
//! Oracle: the RFC 9425 §5.1 and §5.2 examples and the §4.1, §4.3 and
//! §4.4 rules, quoted at each test. Expected responses are hand-written.
//! The §5.1 example's `types` ("Mail", "Calendar", "Contact") are not
//! names in the "JMAP Data Types" registry, which §4.1 says `types`
//! holds; the tests use the registered names (Email, Calendar,
//! ContactCard).

use jmap_quotas_server::memory::MemoryBackend;
use jmap_quotas_server::{
    handle_quota_changes, handle_quota_get, handle_quota_query, handle_quota_query_changes,
};
use jmap_types::Id;
use serde_json::{Value, json};

const ACC: &str = "u33084183";
const Q1: &str = "2a06df0d-9865-4e74-a92f-74dcc814270e";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const CAL: &str = "urn:ietf:params:jmap:calendars";
const CONTACTS: &str = "urn:ietf:params:jmap:contacts";
const SIEVE: &str = "urn:ietf:params:jmap:sieve";

fn using(caps: &[&str]) -> Vec<String> {
    let mut u = vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:quota".to_owned(),
    ];
    u.extend(caps.iter().map(|&c| c.to_owned()));
    u
}

/// §5.1, with registered type names and the description on one line.
fn q1() -> Value {
    json!({
        "id": Q1,
        "resourceType": "count",
        "used": 1056,
        "warnLimit": 1600,
        "softLimit": 1800,
        "hardLimit": 2000,
        "scope": "account",
        "name": "bob@example.com",
        "description": "Personal account usage. When the soft limit is reached, the user is not allowed to send mails or create contacts and calendar events anymore.",
        "types": ["Email", "Calendar", "ContactCard"]
    })
}

fn quota(id: &str, name: &str, scope: &str, rt: &str, used: u64, types: &[&str]) -> Value {
    json!({ "id": id, "resourceType": rt, "used": used, "hardLimit": 10000,
            "scope": scope, "name": name, "types": types })
}

fn backend() -> MemoryBackend {
    let b = MemoryBackend::new().with_account(&Id::from(ACC));
    b.seed_quota(&Id::from(ACC), q1());
    b
}

async fn get(b: &MemoryBackend, args: Value, caps: &[&str]) -> Value {
    let mut args = args;
    args["accountId"] = json!(ACC);
    handle_quota_get(b, &(), args, &using(caps))
        .await
        .unwrap()
        .primary
}

async fn query(b: &MemoryBackend, args: Value, caps: &[&str]) -> Result<Value, String> {
    let mut args = args;
    args["accountId"] = json!(ACC);
    handle_quota_query(b, &(), args, &using(caps))
        .await
        .map(|o| o.primary)
        .map_err(|e| e.error_type.to_string())
}

/// §5.1: "Request fetching all quotas related to an account" with ids
/// null returns the Quota as stored when every type is recognized.
#[tokio::test]
async fn section_5_1_fetch_all() {
    let b = backend();
    let resp = get(&b, json!({ "ids": null }), &[MAIL, CAL, CONTACTS]).await;
    assert_eq!(
        resp,
        json!({ "accountId": ACC, "state": "0", "list": [q1()], "notFound": [] })
    );
}

/// §4.1: "The server MUST filter out any types for which the client did
/// not request the associated capability in the "using" section".
#[tokio::test]
async fn types_are_cut_to_the_using_capabilities() {
    let b = backend();
    let resp = get(&b, json!({ "ids": null }), &[MAIL]).await;
    assert_eq!(resp["list"][0]["types"], json!(["Email"]));
    let resp = get(
        &b,
        json!({ "ids": [Q1], "properties": ["types"] }),
        &[CAL, CONTACTS],
    )
    .await;
    assert_eq!(
        resp["list"],
        json!([{ "id": Q1, "types": ["Calendar", "ContactCard"] }])
    );
}

/// §4.1: "the server MUST NOT return Quota objects for which there are no
/// types recognized by the client." Asked for by id, it is notFound.
#[tokio::test]
async fn quota_with_no_recognized_type_is_not_returned() {
    let b = backend();
    b.seed_quota(
        &Id::from(ACC),
        quota("s1", "sieve", "account", "count", 1, &["SieveScript"]),
    );
    let all = get(&b, json!({ "ids": null }), &[MAIL]).await;
    let ids: Vec<&Value> = all["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| &q["id"])
        .collect();
    assert_eq!(ids, [Q1]);
    assert_eq!(all["notFound"], json!([]));

    let by_id = get(
        &b,
        json!({ "ids": ["s1", Q1], "properties": ["used"] }),
        &[MAIL],
    )
    .await;
    assert_eq!(by_id["list"], json!([{ "id": Q1, "used": 1056 }]));
    assert_eq!(by_id["notFound"], json!(["s1"]));

    let sieve = get(&b, json!({ "ids": ["s1"] }), &[SIEVE]).await;
    assert_eq!(sieve["list"][0]["types"], json!(["SieveScript"]));
}

/// §5.2: a usage-only change gives updatedProperties ["used"], and the
/// back-referenced Quota/get returns only id and used. The example's
/// "newState": "78542" is two changes past "78540"; here state "0" to "2".
#[tokio::test]
async fn section_5_2_used_only_changes() {
    let b = backend();
    let acc = Id::from(ACC);
    b.set_used(&acc, &Id::from(Q1), 1100);
    b.set_used(&acc, &Id::from(Q1), 1246);
    let changes = handle_quota_changes(
        &b,
        &(),
        json!({ "accountId": ACC, "sinceState": "0", "maxChanges": 20 }),
        &using(&[MAIL]),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(
        changes,
        json!({ "accountId": ACC, "oldState": "0", "newState": "2", "hasMoreChanges": false,
                "updatedProperties": ["used"], "created": [], "updated": [Q1], "destroyed": [] })
    );
    let resp = get(
        &b,
        json!({ "ids": changes["updated"], "properties": changes["updatedProperties"] }),
        &[MAIL],
    )
    .await;
    assert_eq!(resp["list"], json!([{ "id": Q1, "used": 1246 }]));
    assert_eq!(resp["state"], "2");
}

/// §4.3: "If the server is unable to tell if only "used" has changed, it
/// MUST be null." A replacement may change anything.
#[tokio::test]
async fn other_changes_give_null_updated_properties() {
    let b = backend();
    let acc = Id::from(ACC);
    b.set_used(&acc, &Id::from(Q1), 1100);
    let mut renamed = q1();
    renamed["name"] = json!("robert@example.com");
    b.put_quota(&acc, renamed);
    b.put_quota(&acc, quota("n1", "new", "domain", "octets", 5, &["Email"]));
    let changes = handle_quota_changes(
        &b,
        &(),
        json!({ "accountId": ACC, "sinceState": "0" }),
        &using(&[MAIL]),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(changes["updatedProperties"], Value::Null);
    assert_eq!(changes["created"], json!(["n1"]));
    assert_eq!(changes["updated"], json!([Q1]));
}

fn query_backend() -> MemoryBackend {
    let b = MemoryBackend::new().with_account(&Id::from(ACC));
    for q in [
        quota("a", "Mail storage", "account", "octets", 300, &["Email"]),
        quota(
            "b",
            "mail count",
            "account",
            "count",
            100,
            &["Email", "SieveScript"],
        ),
        quota(
            "c",
            "Domain calendars",
            "domain",
            "count",
            200,
            &["CalendarEvent"],
        ),
        quota("d", "Sieve", "global", "count", 50, &["SieveScript"]),
    ] {
        b.seed_quota(&Id::from(ACC), q);
    }
    b
}

/// §4.4 conditions; only Quotas with a recognized type count (§4.1), so
/// `d` (SieveScript only) never appears without the sieve capability.
#[tokio::test]
async fn query_filters() {
    let b = query_backend();
    let ids = |r: Value| r["ids"].clone();
    let all = query(&b, json!({ "calculateTotal": true }), &[MAIL, CAL])
        .await
        .unwrap();
    assert_eq!(all["ids"], json!(["a", "b", "c"]));
    assert_eq!(all["total"], 3);
    assert_eq!(
        ids(
            query(&b, json!({ "filter": { "name": "MAIL" } }), &[MAIL, CAL])
                .await
                .unwrap()
        ),
        json!(["a", "b"])
    );
    assert_eq!(
        ids(
            query(&b, json!({ "filter": { "scope": "domain" } }), &[MAIL, CAL])
                .await
                .unwrap()
        ),
        json!(["c"])
    );
    assert_eq!(
        ids(query(
            &b,
            json!({ "filter": { "resourceType": "count" } }),
            &[MAIL, CAL, SIEVE]
        )
        .await
        .unwrap()),
        json!(["b", "c", "d"])
    );
    // `type` is matched against the cut-down `types`: without sieve, b's
    // SieveScript is invisible.
    assert_eq!(
        ids(
            query(&b, json!({ "filter": { "type": "SieveScript" } }), &[MAIL])
                .await
                .unwrap()
        ),
        json!([])
    );
    assert_eq!(
        ids(
            query(&b, json!({ "filter": { "type": "SieveScript" } }), &[SIEVE])
                .await
                .unwrap()
        ),
        json!(["b", "d"])
    );
    assert_eq!(
        ids(query(
            &b,
            json!({ "filter": { "operator": "NOT", "conditions": [{ "type": "SieveScript" }] } }),
            &[MAIL]
        )
        .await
        .unwrap()),
        json!(["a", "b"])
    );
    assert_eq!(
        query(&b, json!({ "filter": { "used": 3 } }), &[MAIL]).await,
        Err("unsupportedFilter".to_owned())
    );
}

/// §4.4: "The following Quota properties MUST be supported for sorting:
/// name, used". Others are unsupportedSort (RFC 8620 §5.5).
#[tokio::test]
async fn query_sorts() {
    let b = query_backend();
    let sorted = |sort: Value| {
        let b = b.clone();
        async move {
            query(&b, json!({ "sort": sort }), &[MAIL, CAL, SIEVE])
                .await
                .map(|r| r["ids"].clone())
        }
    };
    assert_eq!(
        sorted(json!([{ "property": "used" }])).await.unwrap(),
        json!(["d", "b", "c", "a"])
    );
    assert_eq!(
        sorted(json!([{ "property": "used", "isAscending": false }]))
            .await
            .unwrap(),
        json!(["a", "c", "b", "d"])
    );
    assert_eq!(
        sorted(json!([{ "property": "name", "collation": "i;unicode-casemap" }]))
            .await
            .unwrap(),
        json!(["c", "b", "a", "d"])
    );
    assert_eq!(
        sorted(json!([{ "property": "name" }])).await.unwrap(),
        json!(["c", "a", "d", "b"])
    );
    assert_eq!(
        sorted(json!([{ "property": "hardLimit" }])).await,
        Err("unsupportedSort".to_owned())
    );
}

/// RFC 8620 §5.6 over the restricted query: a usage change moves `a`.
#[tokio::test]
async fn query_changes_follow_used() {
    let b = query_backend();
    let first = query(&b, json!({ "sort": [{ "property": "used" }] }), &[MAIL])
        .await
        .unwrap();
    assert_eq!(first["ids"], json!(["b", "a"]));
    b.set_used(&Id::from(ACC), &Id::from("a"), 10);
    b.set_used(&Id::from(ACC), &Id::from("d"), 1);
    let qc = handle_quota_query_changes(
        &b,
        &(),
        json!({ "accountId": ACC, "sinceQueryState": first["queryState"],
                "sort": [{ "property": "used" }], "calculateTotal": true }),
        &using(&[MAIL]),
    )
    .await
    .unwrap()
    .primary;
    assert_eq!(qc["total"], 2);
    assert_eq!(qc["added"], json!([{ "id": "a", "index": 0 }]));
    // `d` is invisible here; removing an id the client never had is
    // harmless (RFC 8620 §5.6 allows extra ids in `removed`).
    assert_eq!(qc["removed"], json!(["a", "d"]));
}
