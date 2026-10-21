//! draft-atwood-jmap-graph-projection-00 (jmap-chat-spec 6bcd9d8) rules,
//! checked against hand-written JSON. Line numbers cite that revision of
//! the draft, RFC 8621 and RFC 9610 (~/PROJECT/IETF/specs).

use jmap_contacts_types::{AddressBook, ContactCard};
use jmap_graph_projection_server::{
    node_matches, node_object_uri, object_uri, projected_edge, projected_graph_rights,
    projected_node, visible_ids, Projectable, Reference,
};
use jmap_graph_types::query::GraphNodeFilterCondition;
use jmap_graph_types::GraphNode;
use jmap_mail_server::memory::{seed::setup_seed_data, MemoryBackend};
use jmap_mail_types::{Email, EmailSubmission, Identity, Mailbox, Thread};
use jmap_types::{Id, UTCDate};
use serde_json::{json, Value};

fn id(s: &str) -> Id {
    Id::from(s)
}

/// `(edge type, target object URI in account A1)` for each reference.
fn edges<O: Projectable>(obj: &O) -> Vec<(String, String)> {
    obj.references()
        .iter()
        .map(|r| {
            (
                r.edge_type.clone(),
                r.target_uri(&id("A1")).unwrap().to_string(),
            )
        })
        .collect()
}

fn pair(t: &str, uri: &str) -> (String, String) {
    (t.to_owned(), uri.to_owned())
}

/// The draft's Example (L238, L272): Email `M42` in Mailbox `MB1`, in
/// Thread `T7`. "The edges returned have types `Email/mailboxIds` (to the
/// node for `MB1`) and `Email/threadId` (to the node for `T7`)."
fn example_email() -> Email {
    serde_json::from_value(json!({
        "id": "M42",
        "blobId": "B1",
        "threadId": "T7",
        "mailboxIds": { "MB1": true },
        "size": 1024,
        "receivedAt": "2026-10-20T12:00:00Z"
    }))
    .unwrap()
}

#[test]
fn example_email_object_uri() {
    // L206: "for example `jmap:email:A1:M42`"; L252 the same in the filter.
    let uri = object_uri::<Email>(&id("A1"), &id("M42")).unwrap();
    assert_eq!(uri.to_string(), "jmap:email:A1:M42");
}

#[test]
fn example_email_edges() {
    // L184: "the edge from an Email to one of its Mailboxes has type
    // `Email/mailboxIds`"; L272 adds `Email/threadId`. `blobId` (RFC 8621
    // L1321) gives none: L177 "References to blobs ... produce no edge".
    assert_eq!(
        edges(&example_email()),
        vec![
            pair("Email/threadId", "jmap:thread:A1:T7"),
            pair("Email/mailboxIds", "jmap:mailbox:A1:MB1"),
        ]
    );
}

#[test]
fn id_map_gives_one_edge_per_key() {
    // L175: "A property of type `Id[T]` ... produces one edge for each key."
    // RFC 8621 L1331 `mailboxIds: "Id[Boolean]"`.
    let mut email = serde_json::to_value(example_email()).unwrap();
    email["mailboxIds"] = json!({ "MB1": true, "MB2": true });
    let email: Email = serde_json::from_value(email).unwrap();
    assert_eq!(
        edges(&email),
        vec![
            pair("Email/threadId", "jmap:thread:A1:T7"),
            pair("Email/mailboxIds", "jmap:mailbox:A1:MB1"),
            pair("Email/mailboxIds", "jmap:mailbox:A1:MB2"),
        ]
    );
}

fn mailbox(parent_id: Value) -> Mailbox {
    serde_json::from_value(json!({
        "id": "MB2",
        "name": "Child",
        "parentId": parent_id,
        "role": null,
        "sortOrder": 0,
        "totalEmails": 0,
        "unreadEmails": 0,
        "totalThreads": 0,
        "unreadThreads": 0,
        "myRights": {
            "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
            "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
            "mayRename": true, "mayDelete": true, "maySubmit": true
        },
        "isSubscribed": true
    }))
    .unwrap()
}

#[test]
fn nullable_id_gives_an_edge_only_when_not_null() {
    // L173: "A property of type `Id` or `Id|null` produces one edge when
    // its value is not `null`." RFC 8621 L511 `parentId: "Id|null"`.
    assert_eq!(edges(&mailbox(Value::Null)), vec![]);
    assert_eq!(
        edges(&mailbox(json!("MB1"))),
        vec![pair("Mailbox/parentId", "jmap:mailbox:A1:MB1")]
    );
}

#[test]
fn id_list_gives_one_edge_per_distinct_element() {
    // L174: "A property of type `Id[]` produces one edge for each element";
    // L189: "There is one edge for each combination of source object,
    // property, and referenced id." RFC 8621 L1149 `emailIds: "Id[]"`.
    let thread: Thread =
        serde_json::from_value(json!({ "id": "T7", "emailIds": ["M1", "M2", "M1"] })).unwrap();
    assert_eq!(
        edges(&thread),
        vec![
            pair("Thread/emailIds", "jmap:email:A1:M1"),
            pair("Thread/emailIds", "jmap:email:A1:M2"),
        ]
    );
}

#[test]
fn email_submission_edges_skip_blob_ids() {
    // RFC 8621 L4138 identityId, L4151 emailId, L4157 threadId are `Id`;
    // L4410 dsnBlobIds and L4417 mdnBlobIds are blob ids (draft L177).
    let sub: EmailSubmission = serde_json::from_value(json!({
        "id": "S1",
        "identityId": "I1",
        "emailId": "M42",
        "threadId": "T7",
        "sendAt": "2026-10-20T12:00:00Z",
        "undoStatus": "final",
        "dsnBlobIds": ["B2"],
        "mdnBlobIds": ["B3"]
    }))
    .unwrap();
    assert_eq!(
        edges(&sub),
        vec![
            pair("EmailSubmission/identityId", "jmap:identity:A1:I1"),
            pair("EmailSubmission/emailId", "jmap:email:A1:M42"),
            pair("EmailSubmission/threadId", "jmap:thread:A1:T7"),
        ]
    );
}

#[test]
fn contact_card_edges_from_address_book_ids() {
    // RFC 9610 L311 `addressBookIds: Id[Boolean]`; draft L175 "The value
    // associated with each key is not carried on the edge." The Media
    // `blobId` (RFC 9610 L321) is nested: L193 "Properties nested inside
    // other values ... do not produce edges".
    let card: ContactCard = serde_json::from_value(json!({
        "@type": "Card",
        "version": "1.0",
        "id": "C1",
        "uid": "urn:uuid:0b6b9d8e-2e3a-4a3f-9d43-2c1f5e3c7a10",
        "addressBookIds": { "AB1": true, "AB2": true },
        "media": { "p1": { "@type": "Media", "kind": "photo", "blobId": "B9" } }
    }))
    .unwrap();
    assert_eq!(
        edges(&card),
        vec![
            pair("ContactCard/addressBookIds", "jmap:addressbook:A1:AB1"),
            pair("ContactCard/addressBookIds", "jmap:addressbook:A1:AB2"),
        ]
    );
}

#[test]
fn types_without_projected_references_give_no_edges() {
    // RFC 8621 Identity has no `Id` property besides its own `id`; RFC 9610
    // L211 AddressBook `shareWith` names Principals, not projected here
    // (draft L177: "objects of data types that are not projected").
    let identity: Identity = serde_json::from_value(json!({
        "id": "I1", "email": "a@example.com", "replyTo": null, "bcc": null,
        "mayDelete": true
    }))
    .unwrap();
    assert_eq!(edges(&identity), vec![]);
    let rights = json!({ "mayRead": true, "mayWrite": true, "mayShare": true, "mayDelete": true });
    let book: AddressBook = serde_json::from_value(json!({
        "id": "AB1", "name": "Personal", "isDefault": true, "isSubscribed": true,
        "shareWith": { "P2": rights }, "myRights": rights
    }))
    .unwrap();
    assert_eq!(edges(&book), vec![]);
}

#[test]
fn registered_type_names() {
    // L317-L325: the IANA "JMAP Object URI Types" registrations.
    let names = [
        Mailbox::URI_TYPE,
        Thread::URI_TYPE,
        Email::URI_TYPE,
        Identity::URI_TYPE,
        EmailSubmission::URI_TYPE,
        AddressBook::URI_TYPE,
        ContactCard::URI_TYPE,
    ];
    assert_eq!(
        names,
        [
            "mailbox",
            "thread",
            "email",
            "identity",
            "emailsubmission",
            "addressbook",
            "contactcard"
        ]
    );
}

#[test]
fn projected_node_properties() {
    // L157-L165 §Nodes.
    let uri = object_uri::<Email>(&id("A1"), &id("M42")).unwrap();
    let node = projected_node(
        &id("N9"),
        &id("PG1"),
        &uri,
        &UTCDate::from("2026-10-20T12:00:00Z"),
    );
    assert_eq!(
        serde_json::to_value(&node).unwrap(),
        json!({
            "id": "N9",
            "graphId": "PG1",
            "objectUri": "jmap:email:A1:M42",
            "annotations": {},
            "blobId": null,
            "mediaType": null,
            "uri": null,
            "created": "2026-10-20T12:00:00Z",
            "updated": "2026-10-20T12:00:00Z",
            "createdBy": null,
            "updatedBy": null
        })
    );
    assert_eq!(node_object_uri(&node), Some("jmap:email:A1:M42"));
}

#[test]
fn projected_edge_properties() {
    // L179-L187 §Edges.
    let reference: Reference = example_email().references().remove(1);
    let edge = projected_edge(
        &id("E1"),
        &id("PG1"),
        &id("N9"),
        &id("N3"),
        &reference,
        &UTCDate::from("2026-10-20T12:00:00Z"),
    );
    assert_eq!(
        serde_json::to_value(&edge).unwrap(),
        json!({
            "id": "E1",
            "graphId": "PG1",
            "fromId": "N9",
            "toId": "N3",
            "type": "Email/mailboxIds",
            "annotations": {},
            "created": "2026-10-20T12:00:00Z",
            "updated": "2026-10-20T12:00:00Z",
            "createdBy": null,
            "updatedBy": null
        })
    );
}

fn cond(v: Value) -> GraphNodeFilterCondition {
    serde_json::from_value(v).unwrap()
}

fn node(v: Value) -> GraphNode {
    let mut base = json!({
        "id": "N9", "graphId": "PG1",
        "created": "2026-10-20T12:00:00Z", "updated": "2026-10-20T12:00:00Z",
        "createdBy": null, "updatedBy": null
    });
    base.as_object_mut()
        .unwrap()
        .extend(v.as_object().unwrap().clone());
    serde_json::from_value(base).unwrap()
}

#[test]
fn object_uri_filter() {
    // L212-L213: "`objectUri` (String[]): The node's `objectUri` is one of
    // these values." L250-L252 the Example's filter.
    let projected = node(json!({ "objectUri": "jmap:email:A1:M42" }));
    let plain = node(json!({}));
    let by_uri = cond(json!({ "graphId": "PG1", "objectUri": ["jmap:email:A1:M42"] }));
    let other = cond(json!({ "graphId": "PG1", "objectUri": ["jmap:email:A1:M43"] }));
    let elsewhere = cond(json!({ "graphId": "G2", "objectUri": ["jmap:email:A1:M42"] }));
    let graph_only = cond(json!({ "graphId": "PG1" }));
    assert!(node_matches(&by_uri, &projected));
    assert!(!node_matches(&other, &projected));
    assert!(!node_matches(&elsewhere, &projected));
    assert!(node_matches(&graph_only, &projected));
    // L206: `objectUri` is "Always `null` for nodes that are not in a
    // projected Graph", so such a node is never "one of these values".
    assert!(!node_matches(&by_uri, &plain));
    assert_eq!(node_object_uri(&plain), None);
}

#[test]
fn projected_graph_rights_accept_no_writes() {
    // L221: "`mayRead` is `true` for every user who can see the projected
    // Graph. `mayWrite` is `true` if the server accepts at least some
    // writes from the user ... `mayAdmin` is `false`". This crate accepts
    // none (L219 "or accept no writes at all").
    assert_eq!(
        serde_json::to_value(projected_graph_rights()).unwrap(),
        json!({ "mayRead": true, "mayWrite": false, "mayAdmin": false })
    );
}

#[tokio::test]
async fn visibility_follows_native_get() {
    // L229: "A node is visible to a user if and only if the user could
    // fetch its source object with that data type's native `/get`
    // method." The seeded `inbox` exists; `nosuch` does not.
    let backend = MemoryBackend::new();
    let account = id("A1");
    let seed = setup_seed_data(&backend, &account).await;
    let inbox = seed.mailbox["inbox"].clone();
    let visible =
        visible_ids::<Mailbox, _>(&backend, &(), &account, &[inbox.clone(), id("nosuch")])
            .await
            .unwrap();
    assert_eq!(visible, vec![inbox]);
}
