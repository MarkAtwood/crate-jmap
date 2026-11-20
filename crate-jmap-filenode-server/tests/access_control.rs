//! Per-user access control of the reference `MemoryBackend`, with two users:
//! the account owner and `bob`, a sharee with limited rights
//! (draft-ietf-jmap-filenode-20 §5 "Access Control"; decision 47).
//!
//! Each test quotes the rule it checks from
//! `draft-ietf-jmap-filenode-20.txt`, with the line the quote starts on.
//!
//! The fixture, in account `acc1` owned by `owner`:
//!
//! ```text
//! home/                 (role "documents", created 2020)
//!   shared/             shareWith bob: mayRead
//!     doc               File
//!     inner/
//!   private/
//!     secret            File
//!   drop/               shareWith bob: mayAddChildren, mayDelete
//!     dropped           File
//!   work/               shareWith bob: mayRead, mayAddChildren
//!     report            File
//! loose                 File, top level
//! ```

mod common;

use std::collections::BTreeMap;

use common::{MemoryBackend, owner};
use jmap_filenode_server::{
    handle_filenode_changes, handle_filenode_copy, handle_filenode_get, handle_filenode_query,
    handle_filenode_set,
};
use jmap_types::Id;
use serde_json::{Value, json};

const ACCOUNT: &str = "acc1";
const BOB: &str = "bob";

fn rights(granted: &[&str]) -> Value {
    let mut r = json!({ "mayRead": false, "mayAddChildren": false, "mayRename": false,
                        "mayDelete": false, "mayModifyContent": false, "mayShare": false });
    for g in granted {
        r[*g] = json!(true);
    }
    r
}

fn fixture() -> MemoryBackend {
    let backend = MemoryBackend::new().with_account_owner(&Id::from(ACCOUNT), &owner());
    let dir = |id: &str, parent: Option<&str>| {
        json!({ "id": id, "@type": "Directory", "name": id, "parentId": parent,
                "created": "2020-01-01T00:00:00Z" })
    };
    let file = |id: &str, parent: Option<&str>| {
        json!({ "id": id, "@type": "File", "name": id, "parentId": parent,
                "blobId": format!("B-{id}"), "size": 3, "mediaType": "text/plain",
                "created": "2020-01-01T00:00:00Z" })
    };
    let mut home = dir("home", None);
    home["role"] = json!("documents");
    let mut shared = dir("shared", Some("home"));
    shared["shareWith"] = json!({ BOB: rights(&["mayRead"]) });
    let mut drop = dir("drop", Some("home"));
    drop["shareWith"] = json!({ BOB: rights(&["mayAddChildren", "mayDelete"]) });
    let mut work = dir("work", Some("home"));
    work["shareWith"] = json!({ BOB: rights(&["mayRead", "mayAddChildren"]) });
    for node in [
        home,
        shared,
        file("doc", Some("shared")),
        dir("inner", Some("shared")),
        dir("private", Some("home")),
        file("secret", Some("private")),
        drop,
        file("dropped", Some("drop")),
        work,
        file("report", Some("work")),
        file("loose", None),
    ] {
        backend.seed_node(
            &Id::from(ACCOUNT),
            serde_json::from_value(node).expect("fixture"),
        );
    }
    backend
}

async fn get(backend: &MemoryBackend, caller: &str, ids: &[&str]) -> Value {
    handle_filenode_get(
        backend,
        &Id::from(caller),
        json!({ "accountId": ACCOUNT, "ids": ids }),
        &[],
    )
    .await
    .expect("FileNode/get")
    .primary
}

/// The listed node `id` of a FileNode/get response.
fn listed(resp: &Value, id: &str) -> Value {
    resp["list"]
        .as_array()
        .and_then(|l| l.iter().find(|n| n["id"] == id))
        .cloned()
        .unwrap_or_else(|| panic!("{id} not listed: {resp}"))
}

fn sorted(v: &Value) -> Vec<String> {
    let mut ids: Vec<String> = serde_json::from_value(v.clone()).expect("id list");
    ids.sort();
    ids
}

async fn set(backend: &MemoryBackend, caller: &str, mut args: Value) -> Value {
    args["accountId"] = json!(ACCOUNT);
    handle_filenode_set(backend, &Id::from(caller), &BTreeMap::new(), args, &[])
        .await
        .expect("FileNode/set")
        .primary
}

async fn query(backend: &MemoryBackend, caller: &str, filter: Value) -> Vec<String> {
    let resp = handle_filenode_query(
        backend,
        &Id::from(caller),
        json!({ "accountId": ACCOUNT, "filter": filter }),
        &[],
    )
    .await
    .expect("FileNode/query")
    .primary;
    sorted(&resp["ids"])
}

async fn state(backend: &MemoryBackend) -> String {
    get(backend, "owner", &[]).await["state"]
        .as_str()
        .expect("state")
        .to_owned()
}

async fn changes(backend: &MemoryBackend, caller: &str, since: &str) -> Value {
    handle_filenode_changes(
        backend,
        &Id::from(caller),
        json!({ "accountId": ACCOUNT, "sinceState": since }),
        &[],
    )
    .await
    .expect("FileNode/changes")
    .primary
}

const ALL: [&str; 11] = [
    "home", "shared", "doc", "inner", "private", "secret", "drop", "dropped", "work", "report",
    "loose",
];

/// L1270: "Nodes which are not "discoverable" MUST be reported in notFound
/// if fetched with FileNode/get". L1274: discoverable "1. If the node or an
/// ancestor of the node has mayRead true. 2. If the node or a descendant of
/// the node has a shareWith entry that grants the user any right." L1266: a
/// top-level File "is discoverable only by the account owner".
#[tokio::test]
async fn get_hides_undiscoverable_nodes_from_the_sharee() {
    let backend = fixture();
    let resp = get(&backend, BOB, &ALL).await;
    assert_eq!(
        sorted(&resp["notFound"]),
        ["dropped", "loose", "private", "secret"],
        "{resp}"
    );
    let owner_resp = get(&backend, "owner", &ALL).await;
    assert_eq!(owner_resp["notFound"], json!([]), "{owner_resp}");
}

/// L1294: for a node discoverable but not readable, FileNode/get "returns
/// the real values of id, @type, name, parentId, myRights, isSubscribed,
/// and shareWith ... It returns null for every other property defined for
/// the node's @type, including created, changed, and role". L1279: "A node
/// that is discoverable only through the second rule has mayRead false."
#[tokio::test]
async fn get_withholds_properties_of_a_node_the_sharee_cannot_read() {
    let backend = fixture();
    let resp = get(&backend, BOB, &["home", "shared"]).await;
    let home = listed(&resp, "home");
    assert_eq!(home["name"], json!("home"), "{resp}");
    assert_eq!(home["myRights"]["mayRead"], json!(false), "{resp}");
    for property in ["created", "changed", "role"] {
        assert!(home[property].is_null(), "{property} withheld: {resp}");
    }
    // A readable node keeps its values.
    let shared = listed(&resp, "shared");
    assert_eq!(shared["created"], json!("2020-01-01T00:00:00Z"), "{resp}");
    // The owner reads everything.
    let resp = get(&backend, "owner", &["home"]).await;
    assert_eq!(listed(&resp, "home")["role"], json!("documents"), "{resp}");
}

/// L1257: "a node's myRights is the union of the rights granted to the user
/// by the shareWith entries on the node and on all its ancestors". L1264:
/// "A node whose @type does not permit children has no myRights."
#[tokio::test]
async fn get_reports_the_sharees_own_rights() {
    let backend = fixture();
    let resp = get(&backend, BOB, &["shared", "inner", "doc", "work"]).await;
    assert_eq!(listed(&resp, "shared")["myRights"], rights(&["mayRead"]));
    assert_eq!(listed(&resp, "inner")["myRights"], rights(&["mayRead"]));
    assert_eq!(
        listed(&resp, "work")["myRights"],
        rights(&["mayRead", "mayAddChildren"])
    );
    assert!(listed(&resp, "doc").get("myRights").is_none(), "{resp}");
    let resp = get(&backend, "owner", &["inner"]).await;
    let all = rights(&[
        "mayRead",
        "mayAddChildren",
        "mayRename",
        "mayDelete",
        "mayModifyContent",
        "mayShare",
    ]);
    assert_eq!(listed(&resp, "inner")["myRights"], all, "{resp}");
}

/// L496: `shareWith` "is null if the user requesting the object does not
/// have myRights.mayShare".
#[tokio::test]
async fn share_with_is_null_without_may_share() {
    let backend = fixture();
    let resp = get(&backend, BOB, &["shared"]).await;
    assert!(listed(&resp, "shared")["shareWith"].is_null(), "{resp}");
    let resp = get(&backend, "owner", &["shared"]).await;
    assert_eq!(
        listed(&resp, "shared")["shareWith"],
        json!({ BOB: rights(&["mayRead"]) }),
        "{resp}"
    );
}

/// L1270: undiscoverable nodes "MUST NOT be returned in response to
/// FileNode/query". L1069: "For a node the user can discover but not read,
/// a condition on a property that FileNode/get returns as null for that
/// node ... does not match the node, whatever its value."
#[tokio::test]
async fn query_returns_only_discoverable_nodes_and_ignores_hidden_values() {
    let backend = fixture();
    assert_eq!(
        query(&backend, BOB, json!({})).await,
        ["doc", "drop", "home", "inner", "report", "shared", "work"]
    );
    assert_eq!(
        query(&backend, BOB, json!({ "hasAnyRole": true })).await,
        Vec::<String>::new()
    );
    assert_eq!(
        query(&backend, "owner", json!({ "hasAnyRole": true })).await,
        ["home"]
    );
}

/// L244 `mayCreateTopLevelFileNode`: "If false, creating a node with a null
/// parentId, or moving one there, fails with "forbidden"". L250:
/// "Permission for creating a child of an existing FileNode is given by the
/// "myRights" property on that FileNode" (`mayAddChildren`, L476).
#[tokio::test]
async fn create_needs_may_add_children_or_the_top_level_right() {
    let backend = fixture();
    let dir = |parent: Value| json!({ "@type": "Directory", "name": "new", "parentId": parent });
    let resp = set(
        &backend,
        BOB,
        json!({ "create": {
            "top": dir(Value::Null),
            "ro": dir(json!("shared")),
            "rw": dir(json!("work"))
        } }),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["top"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    assert_eq!(
        resp["notCreated"]["ro"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    assert_eq!(
        resp["created"]["rw"]["myRights"],
        rights(&["mayRead", "mayAddChildren"]),
        "{resp}"
    );
    let resp = set(
        &backend,
        "owner",
        json!({ "create": { "top": dir(Value::Null) } }),
    )
    .await;
    assert!(resp["created"].get("top").is_some(), "{resp}");
}

/// L1331: "To rename or move a node, or change its role, the server checks
/// mayRename, and to destroy it mayDelete, in the rights of the node
/// itself; for a node with no myRights, these are its parent's rights ...
/// Without these rights, the operation fails with "forbidden"." A node the
/// user cannot discover reads as missing (L1270).
#[tokio::test]
async fn update_and_destroy_need_the_rights_of_the_node() {
    let backend = fixture();
    let resp = set(
        &backend,
        BOB,
        json!({ "update": {
            "doc": { "name": "renamed" },
            "inner": { "name": "renamed" },
            "secret": { "name": "renamed" }
        }, "destroy": ["report", "loose"] }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["doc"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["inner"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["secret"]["type"],
        json!("notFound"),
        "{resp}"
    );
    assert_eq!(
        resp["notDestroyed"]["report"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    assert_eq!(
        resp["notDestroyed"]["loose"]["type"],
        json!("notFound"),
        "{resp}"
    );

    // L486 mayModifyContent covers "blobId, mediaType, target, created,
    // modified, accessed, executable".
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { "report": { "mediaType": "text/csv" } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["report"]["type"],
        json!("forbidden"),
        "{resp}"
    );

    let resp = set(
        &backend,
        "owner",
        json!({ "update": { "doc": { "name": "renamed" } }, "destroy": ["report"] }),
    )
    .await;
    assert!(resp["updated"].get("doc").is_some(), "{resp}");
    assert_eq!(resp["destroyed"], json!(["report"]), "{resp}");
}

/// L1335: "Moving a node also requires mayAddChildren on the new parent, or
/// mayCreateTopLevelFileNode to move it to the top level." Bob holds
/// `mayRename` on a node of his own under `work`, but not `mayAddChildren`
/// on `shared`.
#[tokio::test]
async fn move_needs_may_add_children_on_the_new_parent() {
    let backend = fixture();
    set(
        &backend,
        "owner",
        json!({ "update": { "work": { "shareWith": { BOB: rights(&[
            "mayRead", "mayAddChildren", "mayRename"
        ]) } } } }),
    )
    .await;
    let resp = set(
        &backend,
        BOB,
        json!({ "update": {
            "report": { "parentId": "shared" }
        } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["report"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { "report": { "parentId": null } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["report"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { "report": { "name": "r2" } } }),
    )
    .await;
    assert!(resp["updated"].get("report").is_some(), "{resp}");
}

/// L491 `shareWith`: "The owner of the node MUST NOT be in this set."
/// L488 `mayShare`: "The user may change the sharing of this node".
#[tokio::test]
async fn share_with_needs_may_share_and_excludes_the_owner() {
    let backend = fixture();
    let resp = set(
        &backend,
        "owner",
        json!({ "update": { "inner": { "shareWith": { "owner": rights(&["mayRead"]) } } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["inner"]["type"],
        json!("invalidProperties"),
        "{resp}"
    );
    assert_eq!(
        resp["notUpdated"]["inner"]["properties"],
        json!(["shareWith"]),
        "{resp}"
    );
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { "inner": { "shareWith": { "carol": rights(&["mayRead"]) } } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["inner"]["type"],
        json!("forbidden"),
        "{resp}"
    );
}

/// L460 `isSubscribed`: "This property is stored per user, and if true, the
/// node should be displayed to the current user. Setting it changes only
/// the current user's value, and requires no rights beyond being able to
/// discover the node."
#[tokio::test]
async fn is_subscribed_is_per_user() {
    let backend = fixture();
    let resp = set(
        &backend,
        BOB,
        json!({ "update": { "home": { "isSubscribed": false } } }),
    )
    .await;
    assert!(resp["updated"].get("home").is_some(), "{resp}");
    let bob_view = get(&backend, BOB, &["home"]).await;
    assert_eq!(listed(&bob_view, "home")["isSubscribed"], json!(false));
    let owner_view = get(&backend, "owner", &["home"]).await;
    assert_eq!(listed(&owner_view, "home")["isSubscribed"], json!(true));
}

/// L1366: "When a node stops being discoverable by the user, for example
/// because a shareWith entry was removed ..., the server reports it as
/// destroyed in FileNode/changes." L1372: when Alice "first shared that
/// directory, Bob would have been told of a large number of new nodes",
/// which "The server might report ... as "created"". The owner, whose
/// view did not change, sees only the updated `shareWith` (L1357: rights
/// changes are reported "for that user").
#[tokio::test]
async fn changes_report_discoverability_per_user() {
    let backend = fixture();
    let since = state(&backend).await;
    set(
        &backend,
        "owner",
        json!({ "update": { "shared": { "shareWith": null } } }),
    )
    .await;
    let bob = changes(&backend, BOB, &since).await;
    assert_eq!(
        sorted(&bob["destroyed"]),
        ["doc", "inner", "shared"],
        "{bob}"
    );
    assert_eq!(bob["created"], json!([]), "{bob}");
    let owner_view = changes(&backend, "owner", &since).await;
    assert_eq!(owner_view["updated"], json!(["shared"]), "{owner_view}");
    assert_eq!(owner_view["destroyed"], json!([]), "{owner_view}");

    let since = state(&backend).await;
    set(
        &backend,
        "owner",
        json!({ "update": { "private": { "shareWith": { BOB: rights(&["mayRead"]) } } } }),
    )
    .await;
    let bob = changes(&backend, BOB, &since).await;
    assert_eq!(sorted(&bob["created"]), ["private", "secret"], "{bob}");
    assert_eq!(bob["updated"], json!([]), "{bob}");
}

/// Changes to a node the user cannot discover are not the user's: L1270
/// "MUST be reported in notFound", so its id is not revealed through
/// FileNode/changes either.
#[tokio::test]
async fn changes_leave_out_undiscoverable_nodes() {
    let backend = fixture();
    let since = state(&backend).await;
    set(
        &backend,
        "owner",
        json!({ "update": { "secret": { "name": "s2" }, "doc": { "name": "d2" } } }),
    )
    .await;
    let bob = changes(&backend, BOB, &since).await;
    assert_eq!(bob["updated"], json!(["doc"]), "{bob}");
}

/// L760 (onExists "replace"): "2. If the user does not have mayDelete on
/// the existing node, the operation fails with "forbidden"." Bob may add
/// children in `work`, where `report` exists, but not delete it.
#[tokio::test]
async fn replace_needs_may_delete_on_the_existing_node() {
    let backend = fixture();
    let resp = set(
        &backend,
        BOB,
        json!({ "onExists": "replace", "create": { "r": {
            "@type": "File", "name": "report", "parentId": "work", "blobId": "B9"
        } } }),
    )
    .await;
    assert_eq!(
        resp["notCreated"]["r"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    let resp = get(&backend, "owner", &["report"]).await;
    assert_eq!(resp["notFound"], json!([]), "{resp}");
}

/// L1304: a discoverable node "can have descendants the user cannot
/// discover. A destroy that would remove any of them, either through
/// onDestroyRemoveChildren or as part of a node replaced through onExists,
/// MUST be rejected with "forbidden", and the server MUST NOT destroy any
/// node in that subtree." Bob may delete `drop`, but cannot discover
/// `dropped` inside it.
#[tokio::test]
async fn destroy_refuses_to_remove_undiscoverable_descendants() {
    let backend = fixture();
    let resp = set(
        &backend,
        BOB,
        json!({ "destroy": ["drop"], "onDestroyRemoveChildren": true }),
    )
    .await;
    assert_eq!(
        resp["notDestroyed"]["drop"]["type"],
        json!("forbidden"),
        "{resp}"
    );
    let resp = get(&backend, "owner", &["drop", "dropped"]).await;
    assert_eq!(resp["notFound"], json!([]), "{resp}");
}

/// The backend re-checks the rights with the write, whatever the handler
/// checked (backend canonical): a direct `destroy_object` of an
/// undiscoverable node is `notFound`, and of a node without `mayDelete`
/// `forbidden` (L1331).
#[tokio::test]
async fn backend_rechecks_rights_atomically() {
    use jmap_filenode_server::FileNodeBackend;
    use jmap_filenode_types::FileNode;
    use jmap_server::{BackendSetError, SetErrorType};

    let backend = fixture();
    let bob = Id::from(BOB);
    let account = Id::from(ACCOUNT);
    let kind = |r: Result<(), BackendSetError<_>>| match r {
        Err(BackendSetError::SetError(e)) => e.error_type.clone(),
        other => panic!("expected a SetError, got {:?}", other.is_ok()),
    };
    assert_eq!(
        kind(
            backend
                .destroy_object::<FileNode>(&bob, &account, &Id::from("secret"))
                .await
        ),
        SetErrorType::NotFound
    );
    assert_eq!(
        kind(
            backend
                .destroy_object::<FileNode>(&bob, &account, &Id::from("doc"))
                .await
        ),
        SetErrorType::Forbidden
    );
}

/// A sharee with `mayAddChildren` but not `mayRead` (L1279: discoverable
/// "only through the second rule has mayRead false") may still create in
/// the Directory; the new node takes the parent's rights (L1257), so the
/// sharee cannot discover it afterwards.
#[tokio::test]
async fn sharee_can_drop_into_a_directory_it_cannot_read() {
    let backend = fixture();
    let resp = set(
        &backend,
        BOB,
        json!({ "create": { "f": {
            "@type": "File", "name": "gift", "parentId": "drop", "blobId": "B7"
        } } }),
    )
    .await;
    let id = resp["created"]["f"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("created: {resp}"))
        .to_owned();
    let resp = get(&backend, BOB, &[&id]).await;
    assert_eq!(resp["notFound"], json!([id]), "{resp}");
    let resp = get(&backend, "owner", &[&id]).await;
    assert_eq!(resp["notFound"], json!([]), "{resp}");
}

// ─── Withheld properties are null (bd:JMAP-vcp4) ───────────────────────────
//
// L1293: "For a node that is discoverable but does not have mayRead,
// FileNode/get returns the real values of id, @type, name, parentId,
// myRights, isSubscribed, and shareWith (which follows its own rule for
// mayShare).  It returns null for every other property defined for the
// node's @type, including created, changed, and role, even where that
// property's type does not otherwise allow null." The properties defined
// for each @type are those of L340-L620 (§3.1.1 common: id, @type,
// parentId, name, created, modified, accessed, changed; §3.1.2 container:
// isSubscribed, myRights, shareWith; §3.1.3 File: blobId, size, mediaType,
// executable; §3.1.4 Directory: role; §3.1.5 SymLink: target).

/// Assert `node` holds `property`, and that its value is null.
fn null_present(node: &Value, property: &str) {
    assert!(
        node.get(property).is_some_and(Value::is_null),
        "{property} must be present as null: {node}"
    );
}

/// `drop` is discoverable by bob only through its own shareWith entry
/// (L1279: "A node that is discoverable only through the second rule has
/// mayRead false"): every withheld Directory property is present as null.
#[tokio::test]
async fn withheld_directory_properties_are_present_as_null() {
    let backend = fixture();
    let resp = get(&backend, BOB, &["drop", "home"]).await;
    for id in ["drop", "home"] {
        let node = listed(&resp, id);
        for property in ["created", "modified", "accessed", "changed", "role"] {
            null_present(&node, property);
        }
        assert_eq!(node["name"], json!(id), "{node}");
        assert_eq!(node["@type"], json!("Directory"), "{node}");
        assert_eq!(node["myRights"]["mayRead"], json!(false), "{node}");
        assert_eq!(node["isSubscribed"], json!(true), "{node}");
        assert!(node.get("shareWith").is_some(), "{node}");
    }
    assert_eq!(listed(&resp, "drop")["parentId"], json!("home"), "{resp}");
    // A readable Directory keeps its values; the owner reads everything.
    let resp = get(&backend, BOB, &["work"]).await;
    assert_eq!(
        listed(&resp, "work")["created"],
        json!("2020-01-01T00:00:00Z"),
        "{resp}"
    );
    let resp = get(&backend, "owner", &["drop"]).await;
    assert_eq!(
        listed(&resp, "drop")["created"],
        json!("2020-01-01T00:00:00Z"),
        "{resp}"
    );
}

/// With `properties`, only the requested withheld properties appear (RFC
/// 8620 §5.1: "If supplied, only the properties listed in the array are
/// returned for each Foo object"), still as null.
#[tokio::test]
async fn withheld_properties_follow_the_properties_argument() {
    let backend = fixture();
    let resp = handle_filenode_get(
        &backend,
        &Id::from(BOB),
        json!({ "accountId": ACCOUNT, "ids": ["drop"],
                "properties": ["name", "role", "modified"] }),
        &[],
    )
    .await
    .expect("FileNode/get")
    .primary;
    let drop = listed(&resp, "drop");
    null_present(&drop, "role");
    null_present(&drop, "modified");
    assert_eq!(drop["name"], json!("drop"), "{drop}");
    assert!(drop.get("created").is_none(), "not requested: {drop}");
}

/// A File the caller can discover without mayRead (L1264: "The user's
/// rights on it are those in its parent's myRights"; here bob's parent
/// `private` gives none, and the memory backend counts the File by a
/// shareWith entry seeded on it, by the second rule) gets null for every
/// withheld File property, not the defaults of mediaType ("default:
/// "application/octet-stream"") and executable ("default: false").
#[tokio::test]
async fn withheld_file_properties_are_null_not_defaults() {
    let backend = fixture();
    backend.seed_node(
        &Id::from(ACCOUNT),
        serde_json::from_value(json!({
            "id": "note", "@type": "File", "name": "note", "parentId": "private",
            "blobId": "B-note", "size": 3, "mediaType": "text/plain", "executable": true,
            "created": "2020-01-01T00:00:00Z",
            "shareWith": { BOB: rights(&["mayDelete"]) }
        }))
        .expect("fixture"),
    );
    let resp = get(&backend, BOB, &["note"]).await;
    let note = listed(&resp, "note");
    for property in [
        "blobId",
        "size",
        "mediaType",
        "executable",
        "created",
        "modified",
        "accessed",
        "changed",
    ] {
        null_present(&note, property);
    }
    assert_eq!(note["name"], json!("note"), "{note}");
    assert_eq!(note["parentId"], json!("private"), "{note}");
    assert_eq!(note["@type"], json!("File"), "{note}");
    let resp = get(&backend, "owner", &["note"]).await;
    assert_eq!(listed(&resp, "note")["mediaType"], json!("text/plain"));
}

/// The withheld properties are marked unreadable for the
/// draft-ietf-jmap-conditional-02 §9.1 evaluator (a precondition on a
/// property the caller cannot read is `forbidden`).
#[tokio::test]
async fn withheld_properties_are_marked_unreadable() {
    let backend = fixture();
    let out = handle_filenode_get(
        &backend,
        &Id::from(BOB),
        json!({ "accountId": ACCOUNT, "ids": ["drop", "work"] }),
        &[],
    )
    .await
    .expect("FileNode/get");
    let drop = out
        .unreadable
        .get(&Id::from("drop"))
        .expect("drop withheld");
    assert!(
        drop.contains("role") && drop.contains("created"),
        "{drop:?}"
    );
    assert!(!drop.contains("name"), "{drop:?}");
    assert!(!out.unreadable.contains_key(&Id::from("work")));
}

/// L1293: "A change to a property the user sees as null is not visible to
/// the user, so the server SHOULD NOT report the node as updated in
/// FileNode/changes because of it". A rename is visible (name keeps its
/// value).
#[tokio::test]
async fn changes_hide_updates_of_withheld_properties() {
    let backend = fixture();
    let since = state(&backend).await;
    set(
        &backend,
        "owner",
        json!({ "update": { "drop": { "modified": "2024-05-05T00:00:00Z" } } }),
    )
    .await;
    let bob = changes(&backend, BOB, &since).await;
    assert_eq!(bob["updated"], json!([]), "{bob}");
    let owner_view = changes(&backend, "owner", &since).await;
    assert_eq!(owner_view["updated"], json!(["drop"]), "{owner_view}");

    let since = state(&backend).await;
    set(
        &backend,
        "owner",
        json!({ "update": { "drop": { "name": "inbox" } } }),
    )
    .await;
    let bob = changes(&backend, BOB, &since).await;
    assert_eq!(bob["updated"], json!(["drop"]), "{bob}");
}

/// L852: "Copying a node requires mayRead on it". Bob cannot copy `drop`,
/// which he can discover but not read, into an account he owns.
#[tokio::test]
async fn copy_of_an_unreadable_node_is_forbidden() {
    let backend = fixture().with_account_owner(&Id::from("bobs"), &Id::from(BOB));
    let resp = handle_filenode_copy(
        &backend,
        &Id::from(BOB),
        json!({ "fromAccountId": ACCOUNT, "accountId": "bobs",
                "create": { "c": { "id": "drop", "parentId": null } } }),
        "c0",
        &[],
    )
    .await
    .expect("FileNode/copy")
    .primary;
    assert_eq!(resp["notCreated"]["c"]["type"], "forbidden", "{resp}");
    assert!(resp["created"].is_null(), "{resp}");
}
