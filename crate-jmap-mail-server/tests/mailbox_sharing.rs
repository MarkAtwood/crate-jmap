//! The Mailbox side of JMAP Mail Sharing in the reference memory backend
//! (bd JMAP-irij), with Principal-id callers (`MemoryBackend<Id>`).
//!
//! Oracles (draft-ietf-jmap-mail-sharing-02, "mail-sharing" below; RFC
//! 8621; RFC 9670):
//! - mail-sharing §2: `shareWith` is `Id[MailboxRights]|null` (default
//!   null), "Each key in the map is the id of a Principal with whom the
//!   mailbox is shared. The value for each key is the set of access rights
//!   that Principal has for the mailbox." "The Principal to which this
//!   mailbox belongs MUST NOT be in the map. The property may only be
//!   modified if the user has the mayShare right."
//! - mail-sharing §2 `mayShare`: "The user may modify the shareWith
//!   property for this mailbox."
//! - mail-sharing §3.1: "servers MUST treat any data the user does not have
//!   permission to access as if it did not exist."
//! - mail-sharing §3.4: "the server SHOULD return the same error response
//!   as it would if the mailbox did not exist".
//! - mail-sharing §3.6: "a user who has been granted limited access to a
//!   mailbox MUST NOT be able to grant themselves additional permissions by
//!   modifying the shareWith property unless they have been explicitly
//!   granted the mayShare right." The owner "always has implicit full
//!   rights".
//! - RFC 8621 §2 isSubscribed: "This SHOULD default to false for Mailboxes
//!   in shared accounts the user has access to ... This MUST be stored
//!   separately per user where multiple users have access to a shared
//!   Mailbox."
//! - RFC 8621 §2 mayReadItems: "If a sub-Mailbox is shared but not the
//!   parent Mailbox, this may be false."
//! - RFC 8621 §1.3.1 mayCreateTopLevelMailbox; §2 mayCreateChild, mayRename,
//!   mayDelete.
//! - RFC 8620 §5.3: a write that "would violate an ACL or other permissions
//!   policy" is "forbidden".
//! - mail-sharing §1.2.1: with `urn:ietf:params:jmap:mail:share` "the
//!   Mailbox data type has been extended with the sharing properties".

use std::collections::BTreeMap;

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{
    handle_email_get, handle_email_set, handle_mailbox_changes, handle_mailbox_get,
    handle_mailbox_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

const SHARE: &str = "urn:ietf:params:jmap:mail:share";

fn using() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:mail".to_owned(),
        SHARE.to_owned(),
    ]
}

fn alice() -> Id {
    Id::from("alice")
}

fn bob() -> Id {
    Id::from("bob")
}

/// Alice's account `alice`, owned by Principal `alice`.
fn backend() -> MemoryBackend<Id> {
    MemoryBackend::<Id>::default().with_account(&alice())
}

/// RFC 8621 §2 MailboxRights with the given rights true and the rest
/// false, plus mail-sharing §2 `mayShare`.
fn rights(granted: &[&str]) -> Value {
    let mut r = serde_json::Map::new();
    for k in [
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
    ] {
        r.insert(k.to_owned(), json!(granted.contains(&k)));
    }
    Value::Object(r)
}

async fn set_as(b: &MemoryBackend<Id>, who: &Id, args: Value, using: &[String]) -> Value {
    let mut args = args;
    args["accountId"] = json!("alice");
    handle_mailbox_set(b, who, &BTreeMap::new(), args, using)
        .await
        .unwrap()
        .primary
}

async fn get_as(b: &MemoryBackend<Id>, who: &Id, ids: Value, using: &[String]) -> Value {
    handle_mailbox_get(b, who, json!({"accountId": "alice", "ids": ids}), using)
        .await
        .unwrap()
        .primary
}

async fn create_as(b: &MemoryBackend<Id>, who: &Id, props: Value) -> Value {
    set_as(b, who, json!({"create": {"c": props}}), &using()).await
}

/// Alice creates a Mailbox with `props` and returns its id.
async fn alice_creates(b: &MemoryBackend<Id>, props: Value) -> String {
    let resp = create_as(b, &alice(), props).await;
    resp["created"]["c"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("create failed: {resp}"))
        .to_owned()
}

async fn state_as(b: &MemoryBackend<Id>, who: &Id) -> String {
    get_as(b, who, json!([]), &using()).await["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn changes_as(b: &MemoryBackend<Id>, who: &Id, since: &str) -> Value {
    handle_mailbox_changes(
        b,
        who,
        json!({"accountId": "alice", "sinceState": since}),
        &using(),
    )
    .await
    .unwrap()
    .primary
}

#[tokio::test]
async fn owner_and_sharee_rights_and_share_with() {
    let b = backend();
    let id = alice_creates(
        &b,
        json!({"name": "Team", "shareWith": {"bob": rights(&["mayReadItems"])}}),
    )
    .await;

    let mine = get_as(&b, &alice(), json!([&id]), &using()).await["list"][0].clone();
    let mut all = rights(&[
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
    ]);
    assert_eq!(mine["myRights"], all, "{mine}");
    assert_eq!(mine["shareWith"], json!({"bob": rights(&["mayReadItems"])}));
    assert_eq!(mine["isSubscribed"], true);

    let theirs = get_as(&b, &bob(), json!([&id]), &using()).await["list"][0].clone();
    assert_eq!(theirs["myRights"], rights(&["mayReadItems"]), "{theirs}");
    // Without mayShare Bob sees only his own entry.
    assert_eq!(
        theirs["shareWith"],
        json!({"bob": rights(&["mayReadItems"])})
    );
    // RFC 8621 §2: false by default for a shared Mailbox.
    assert_eq!(theirs["isSubscribed"], false);

    // A Mailbox shared with nobody: shareWith is null for the owner.
    let solo = alice_creates(&b, json!({"name": "Solo"})).await;
    let got = get_as(&b, &alice(), json!([&solo]), &using()).await;
    assert_eq!(got["list"][0]["shareWith"], Value::Null, "{got}");
    all["mayShare"] = json!(true);
    assert_eq!(got["list"][0]["myRights"], all);
}

#[tokio::test]
async fn unshared_mailbox_does_not_exist_for_a_sharee() {
    let b = backend();
    let hidden = alice_creates(&b, json!({"name": "Private"})).await;
    let shared = alice_creates(
        &b,
        json!({"name": "Team", "shareWith": {"bob": rights(&["mayReadItems"])}}),
    )
    .await;
    let got = get_as(&b, &bob(), Value::Null, &using()).await;
    let ids: Vec<&str> = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![shared.as_str()], "{got}");
    let got = get_as(&b, &bob(), json!([&hidden]), &using()).await;
    assert_eq!(got["notFound"], json!([&hidden]));

    // An update or destroy of it answers as for a missing Mailbox (§3.4).
    let resp = set_as(
        &b,
        &bob(),
        json!({"update": {&hidden: {"name": "x"}}, "destroy": [&hidden]}),
        &using(),
    )
    .await;
    assert_eq!(resp["notUpdated"][&hidden]["type"], "notFound", "{resp}");
    assert_eq!(resp["notDestroyed"][&hidden]["type"], "notFound", "{resp}");
}

#[tokio::test]
async fn parent_of_a_shared_child_is_visible_without_rights() {
    let b = backend();
    let parent = alice_creates(&b, json!({"name": "Projects"})).await;
    let child = alice_creates(
        &b,
        json!({"name": "Apollo", "parentId": &parent,
               "shareWith": {"bob": rights(&["mayReadItems"])}}),
    )
    .await;
    let got = get_as(&b, &bob(), json!([&parent, &child]), &using()).await;
    assert_eq!(got["notFound"], json!([]), "{got}");
    let p = got["list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == json!(&parent))
        .unwrap();
    assert_eq!(p["myRights"], rights(&[]));
    assert_eq!(p["shareWith"], Value::Null);
}

#[tokio::test]
async fn share_with_needs_may_share() {
    let b = backend();
    let id = alice_creates(
        &b,
        json!({"name": "Team", "shareWith": {"bob": rights(&["mayReadItems", "mayRename"])}}),
    )
    .await;
    // §3.6: Bob cannot grant himself more.
    let resp = set_as(
        &b,
        &bob(),
        json!({"update": {&id: {"shareWith/bob/mayDelete": true}}}),
        &using(),
    )
    .await;
    assert_eq!(resp["notUpdated"][&id]["type"], "forbidden", "{resp}");

    // With mayShare he may change the map.
    set_as(
        &b,
        &alice(),
        json!({"update": {&id: {"shareWith/bob/mayShare": true}}}),
        &using(),
    )
    .await;
    let resp = set_as(
        &b,
        &bob(),
        json!({"update": {&id: {"shareWith/carol": rights(&["mayReadItems"])}}}),
        &using(),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    let got = get_as(&b, &Id::from("carol"), json!([&id]), &using()).await;
    assert_eq!(
        got["list"][0]["myRights"],
        rights(&["mayReadItems"]),
        "{got}"
    );
    // A sharee with mayShare sees the whole map.
    let got = get_as(&b, &bob(), json!([&id]), &using()).await;
    let share_with = got["list"][0]["shareWith"].as_object().unwrap();
    assert_eq!(
        share_with.keys().collect::<Vec<_>>(),
        vec!["bob", "carol"],
        "{got}"
    );
}

#[tokio::test]
async fn owner_must_not_be_in_share_with() {
    let b = backend();
    let resp = create_as(
        &b,
        &alice(),
        json!({"name": "Team", "shareWith": {"alice": rights(&["mayReadItems"])}}),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["c"],
        json!({"type": "invalidProperties", "properties": ["shareWith"]}),
        "{resp}"
    );
    let id = alice_creates(&b, json!({"name": "Team"})).await;
    let resp = set_as(
        &b,
        &alice(),
        json!({"update": {&id: {"shareWith": {"alice": rights(&[])}}}}),
        &using(),
    )
    .await;
    assert_eq!(
        resp["notUpdated"][&id],
        json!({"type": "invalidProperties", "properties": ["shareWith"]}),
        "{resp}"
    );
}

#[tokio::test]
async fn sharee_writes_follow_their_rights() {
    let b = backend();
    let ro = alice_creates(
        &b,
        json!({"name": "ReadOnly", "shareWith": {"bob": rights(&["mayReadItems"])}}),
    )
    .await;
    let rw = alice_creates(
        &b,
        json!({"name": "Shared", "shareWith": {"bob": rights(&[
            "mayReadItems", "mayCreateChild", "mayRename", "mayDelete"])}}),
    )
    .await;

    // mayRename gates name.
    let resp = set_as(
        &b,
        &bob(),
        json!({"update": {&ro: {"name": "Mine"}, &rw: {"name": "Ours"}}}),
        &using(),
    )
    .await;
    assert_eq!(resp["notUpdated"][&ro]["type"], "forbidden", "{resp}");
    assert!(resp["updated"].get(&rw).is_some(), "{resp}");

    // mayDelete gates destroy.
    let resp = set_as(&b, &bob(), json!({"destroy": [&ro]}), &using()).await;
    assert_eq!(resp["notDestroyed"][&ro]["type"], "forbidden", "{resp}");

    // RFC 8621 §1.3.1: only the owner creates at the top level here;
    // mayCreateChild gates a child.
    let resp = create_as(&b, &bob(), json!({"name": "Top"})).await;
    assert_eq!(resp["notCreated"]["c"]["type"], "forbidden", "{resp}");
    let resp = create_as(&b, &bob(), json!({"name": "Sub", "parentId": &ro})).await;
    assert_eq!(resp["notCreated"]["c"]["type"], "forbidden", "{resp}");
    // A role is the account's (RFC 8621 §2: one Mailbox per role).
    let resp = create_as(
        &b,
        &bob(),
        json!({"name": "Sub", "parentId": &rw, "role": "archive"}),
    )
    .await;
    assert_eq!(resp["notCreated"]["c"]["type"], "forbidden", "{resp}");
    let resp = create_as(&b, &bob(), json!({"name": "Sub", "parentId": &rw})).await;
    let sub = resp["created"]["c"]["id"].as_str().unwrap().to_owned();
    // Bob keeps access to what he created.
    let got = get_as(&b, &bob(), json!([&sub]), &using()).await;
    assert_eq!(got["notFound"], json!([]), "{got}");

    let resp = set_as(&b, &bob(), json!({"destroy": [&sub]}), &using()).await;
    assert_eq!(resp["destroyed"], json!([&sub]), "{resp}");
}

#[tokio::test]
async fn is_subscribed_is_per_user() {
    let b = backend();
    let id = alice_creates(
        &b,
        json!({"name": "Team", "shareWith": {"bob": rights(&["mayReadItems"])}}),
    )
    .await;
    let alice_state = state_as(&b, &alice()).await;
    // No right is needed to subscribe.
    let resp = set_as(
        &b,
        &bob(),
        json!({"update": {&id: {"isSubscribed": true}}}),
        &using(),
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
    let got = get_as(&b, &bob(), json!([&id]), &using()).await;
    assert_eq!(got["list"][0]["isSubscribed"], true);

    // Alice's value is her own, and Bob's subscription is no change for
    // her.
    let got = get_as(&b, &alice(), json!([&id]), &using()).await;
    assert_eq!(got["list"][0]["isSubscribed"], true);
    set_as(
        &b,
        &alice(),
        json!({"update": {&id: {"isSubscribed": false}}}),
        &using(),
    )
    .await;
    let got = get_as(&b, &bob(), json!([&id]), &using()).await;
    assert_eq!(got["list"][0]["isSubscribed"], true);
    let ch = changes_as(&b, &alice(), &alice_state).await;
    assert_eq!(ch["updated"], json!([&id]), "{ch}");
}

/// RFC 8620 §5.2 for each user: a Mailbox that becomes visible is created,
/// one that stops being visible is destroyed, and a Mailbox the user never
/// saw is never named.
#[tokio::test]
async fn changes_follow_what_the_sharee_can_see() {
    let b = backend();
    let id = alice_creates(&b, json!({"name": "Team"})).await;
    let since = state_as(&b, &bob()).await;
    let hidden = alice_creates(&b, json!({"name": "Private"})).await;
    set_as(
        &b,
        &alice(),
        json!({"update": {&id: {"shareWith": {"bob": rights(&["mayReadItems"])}}}}),
        &using(),
    )
    .await;
    let ch = changes_as(&b, &bob(), &since).await;
    assert_eq!(ch["created"], json!([&id]), "{ch}");
    assert_eq!(ch["updated"], json!([]), "{ch}");
    assert_eq!(ch["destroyed"], json!([]), "{ch}");

    let since = ch["newState"].as_str().unwrap().to_owned();
    set_as(
        &b,
        &alice(),
        json!({"update": {&hidden: {"name": "Secret"}, &id: {"shareWith": null}}}),
        &using(),
    )
    .await;
    let ch = changes_as(&b, &bob(), &since).await;
    assert_eq!(ch["created"], json!([]), "{ch}");
    assert_eq!(ch["updated"], json!([]), "{ch}");
    assert_eq!(ch["destroyed"], json!([&id]), "{ch}");
}

/// Without `urn:ietf:params:jmap:mail:share` in `using` the Mailbox has no
/// `shareWith`: it is not returned, and writing it is `invalidProperties`,
/// as for any unknown property (RFC 8620 §5.3).
#[tokio::test]
async fn sharing_properties_need_the_capability() {
    let b = backend();
    let plain = vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:mail".to_owned(),
    ];
    let id = alice_creates(
        &b,
        json!({"name": "Team", "shareWith": {"bob": rights(&["mayReadItems"])}}),
    )
    .await;
    let got = get_as(&b, &alice(), json!([&id]), &plain).await;
    let m = &got["list"][0];
    assert!(m.get("shareWith").is_none(), "{got}");

    let resp = set_as(
        &b,
        &alice(),
        json!({"create": {"c": {"name": "X", "shareWith": null}},
               "update": {&id: {"shareWith": null}}}),
        &plain,
    )
    .await;
    let invalid = json!({"type": "invalidProperties", "properties": ["shareWith"]});
    assert_eq!(resp["notCreated"]["c"], invalid, "{resp}");
    assert_eq!(resp["notUpdated"][&id], invalid, "{resp}");
}

/// mail-sharing §3.1: this backend shares Mailboxes only, so a sharee
/// reads no Email (it refuses rather than over-grants).
#[tokio::test]
async fn sharee_reads_no_email() {
    let b = backend();
    let id = alice_creates(
        &b,
        json!({"name": "Team", "shareWith": {"bob": rights(&["mayReadItems"])}}),
    )
    .await;
    let resp = handle_email_set(
        &b,
        &alice(),
        &BTreeMap::new(),
        json!({"accountId": "alice", "create": {"e": {"mailboxIds": {&id: true}}}}),
        &[],
    )
    .await
    .unwrap()
    .primary;
    let e = resp["created"]["e"]["id"].as_str().unwrap().to_owned();
    let got = handle_email_get(&b, &bob(), json!({"accountId": "alice", "ids": [&e]}), &[])
        .await
        .unwrap()
        .primary;
    assert_eq!(got["notFound"], json!([&e]), "{got}");
}

/// draft-ietf-jmap-metadata-05 §4.1: "A write to metadata without
/// maySetMetadata MUST be rejected with a "forbidden" SetError." A
/// sharee's `maySetMetadata` is its `shareWith` entry's; other properties
/// its rights allow stay writable.
#[tokio::test]
async fn sharee_metadata_write_needs_may_set_metadata() {
    let info: jmap_metadata_types::DataTypeMetadataInfo = serde_json::from_value(json!({
        "namespaces": [], "supportsVendorNamespaces": true,
        "supportsPrivate": false, "maxDepth": 4
    }))
    .unwrap();
    let b = MemoryBackend::<Id>::default()
        .with_metadata_info("Mailbox", info)
        .with_account(&alice());
    let mut grant = rights(&["mayReadItems", "mayRename"]);
    grant["maySetMetadata"] = json!(false);
    let id = alice_creates(&b, json!({"name": "Team", "shareWith": {"bob": grant}})).await;
    let mut using = using();
    using.push("urn:ietf:params:jmap:metadata".to_owned());

    let got = get_as(&b, &bob(), json!([&id]), &using).await;
    assert_eq!(got["list"][0]["myRights"]["maySetMetadata"], false, "{got}");
    let resp = set_as(
        &b,
        &bob(),
        json!({"update": {&id: {"metadata/acme.example.com": {"k": 1}}}}),
        &using,
    )
    .await;
    assert_eq!(resp["notUpdated"][&id]["type"], "forbidden", "{resp}");
    let resp = set_as(
        &b,
        &bob(),
        json!({"update": {&id: {"name": "Renamed"}}}),
        &using,
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");

    // The owner holds the right.
    let resp = set_as(
        &b,
        &alice(),
        json!({"update": {&id: {"metadata/acme.example.com": {"k": 1}}}}),
        &using,
    )
    .await;
    assert!(resp["updated"].get(&id).is_some(), "{resp}");
}
