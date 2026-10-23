//! draft-ietf-jmap-metadata-04 properties and filter conditions on
//! TaskList and Task (bd JMAP-s4kp.4).
//!
//! Oracle: the -04 text quoted on each test
//! (/home/mark/PROJECT/IETF/specs/draft-ietf-jmap-metadata-04.txt), with
//! hand-written JSON. The namespace and keys follow the -04 §6.6 example
//! ("metadata/acme.example.com/approvalStatus").

use jmap_metadata_types::MetadataTextMatch;
use jmap_tasks_types::{Task, TaskFilterCondition, TaskList};
use serde_json::json;

/// -04 §2 adds `metadata` and `privateMetadata` (objects keyed by
/// namespace) to each listed type; -04 §4.1 covers "every data type that
/// has a myRights property", which a draft-ietf-jmap-tasks-06 §3 TaskList
/// has. The values land in the typed fields, not `extra`. -04 §3.2: "null
/// is equivalent to {}", so `null` decodes as absent; a non-object is
/// rejected.
#[test]
fn task_list_metadata_properties() {
    let mut raw = json!({
        "id": "tl1", "name": "Work", "isSubscribed": true,
        "myRights": {
            "mayReadItems": true, "mayWriteAll": true, "mayWriteOwn": true,
            "mayUpdatePrivate": true, "mayRSVP": true, "mayAdmin": true,
            "mayDelete": true, "maySetMetadata": true
        },
        "metadata": {"acme.example.com": {"department": "sales"}},
        "privateMetadata": {"acme.example.com": {"pinned": true}}
    });
    let list: TaskList = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        list.metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["department"]),
        Some(&json!("sales"))
    );
    assert_eq!(
        list.private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["pinned"]),
        Some(&json!(true))
    );
    assert!(list.extra.is_empty());
    let back = serde_json::to_value(&list).expect("encode");
    assert_eq!(back["metadata"], raw["metadata"]);
    assert_eq!(back["privateMetadata"], raw["privateMetadata"]);

    raw["metadata"] = serde_json::Value::Null;
    let list: TaskList = serde_json::from_value(raw.clone()).expect("decode null");
    assert_eq!(list.metadata, None);
    assert!(
        serde_json::to_value(&list)
            .expect("encode")
            .get("metadata")
            .is_none()
    );
    raw["metadata"] = json!(["x"]);
    assert!(serde_json::from_value::<TaskList>(raw).is_err());
}

/// -04 §2 on Task, which -04 §4.1 lets take the right from its container
/// (the TaskList named by `taskListId`). A non-object `privateMetadata` is
/// rejected.
#[test]
fn task_metadata_properties() {
    let mut raw = json!({
        "id": "T1", "taskListId": "tl1", "title": "Quarterly Review",
        "metadata": {"acme.example.com": {"approvalStatus": "pending"}},
        "privateMetadata": {"acme.example.com": {"reminder": "bring slides"}}
    });
    let task: Task = serde_json::from_value(raw.clone()).expect("decode");
    assert_eq!(
        task.metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["approvalStatus"]),
        Some(&json!("pending"))
    );
    assert_eq!(
        task.private_metadata
            .as_ref()
            .map(|m| &m["acme.example.com"]["reminder"]),
        Some(&json!("bring slides"))
    );
    assert!(task.extra.is_empty());
    let back = serde_json::to_value(&task).expect("encode");
    assert_eq!(back["metadata"], raw["metadata"]);
    assert_eq!(back["privateMetadata"], raw["privateMetadata"]);

    raw["privateMetadata"] = json!(7);
    assert!(serde_json::from_value::<Task>(raw).is_err());
}

/// The six wire names of -04 §3.6 ("metadataExists",
/// "privateMetadataExists", "metadataTextContains",
/// "privateMetadataTextContains", "metadataTextEquals",
/// "privateMetadataTextEquals") extend "the FilterCondition object for
/// Foo/query". Each decodes into its typed field and re-encodes under the
/// same name; a misspelt one is still rejected by `deny_unknown_fields`.
#[test]
fn metadata_condition_names_decode_and_typos_reject() {
    let m = json!({"path": "acme.example.com/approvalStatus", "value": "pending"});
    for (name, value) in [
        ("metadataExists", json!("acme.example.com")),
        ("privateMetadataExists", json!("acme.example.com")),
        ("metadataTextContains", m.clone()),
        ("privateMetadataTextContains", m.clone()),
        ("metadataTextEquals", m.clone()),
        ("privateMetadataTextEquals", m.clone()),
    ] {
        let wire = json!({ name: value });
        let c: TaskFilterCondition = serde_json::from_value(wire.clone()).expect(name);
        assert!(c.has_metadata_condition(), "{name}");
        assert_eq!(serde_json::to_value(&c).expect("encode"), wire, "{name}");
    }
    assert!(!TaskFilterCondition::default().has_metadata_condition());
    assert!(serde_json::from_value::<TaskFilterCondition>(json!({"metadataExist": "ns"})).is_err());
}

/// -04 §3.6: "A MetadataTextMatch object has the following fields:"
/// `path` and `value`. A leaf combining the draft-ietf-jmap-tasks-06 §4 `taskListId` with
/// `metadataTextEquals` decodes both.
#[test]
fn metadata_text_condition_shape() {
    let c: TaskFilterCondition = serde_json::from_value(json!({
        "taskListId": "tl1",
        "metadataTextEquals": { "path": "acme.example.com/approvalStatus", "value": "pending" }
    }))
    .expect("decode");
    assert_eq!(c.task_list_id.as_ref().map(AsRef::as_ref), Some("tl1"));
    assert_eq!(
        c.metadata_text_equals,
        Some(MetadataTextMatch::new(
            "acme.example.com/approvalStatus",
            "pending"
        ))
    );
}
