//! `jmap_types::data_types` against the documents that register each type.
//!
//! Oracle: RFC 9404 §6.3 Table 1 and draft-ietf-jmap-object-history-03
//! §7.2 Table 1, pasted below as published (multi-line cells joined), and
//! the per-type registration blocks of RFC 9425, RFC 9661, RFC 9670,
//! RFC 9610, draft-ietf-jmap-calendars-34, draft-ietf-jmap-filenode-20 and
//! the draft-atwood-jmap drafts, hand-copied. Nothing here is generated
//! from the code under test.

use jmap_types::data_types::{DATA_TYPES, data_type};

fn yes(s: &str) -> bool {
    match s.trim().to_ascii_lowercase().as_str() {
        "yes" => true,
        "no" => false,
        other => panic!("not Yes/No: {other}"),
    }
}

/// RFC 9404 §6.3 Table 1: Type Name | Can Ref Blobs | Can Use for State
/// Change | Capability | Reference.
const RFC9404_TABLE_1: &str = "
|Core            |No   |No     |urn:ietf:params:jmap:core|[RFC8620]|
|PushSubscription|No   |No     |urn:ietf:params:jmap:core|[RFC8620]|
|Mailbox         |Yes  |Yes    |urn:ietf:params:jmap:mail|[RFC8621]|
|Thread          |Yes  |Yes    |urn:ietf:params:jmap:mail|[RFC8621]|
|Email           |Yes  |Yes    |urn:ietf:params:jmap:mail|[RFC8621]|
|EmailDelivery   |No   |Yes    |urn:ietf:params:jmap:mail|[RFC8621]|
|SearchSnippet   |No   |No     |urn:ietf:params:jmap:mail|[RFC8621]|
|Identity        |No   |Yes    |urn:ietf:params:jmap:submission|[RFC8621]|
|EmailSubmission |No   |Yes    |urn:ietf:params:jmap:submission|[RFC8621]|
|VacationResponse|No   |Yes    |urn:ietf:params:jmap:vacationresponse|[RFC8621]|
|MDN             |No   |No     |urn:ietf:params:jmap:mdn |[RFC9007]|
";

/// draft-ietf-jmap-object-history-03 §7.2 Table 1: Type Name | Supports
/// History.
const OBJECT_HISTORY_TABLE_1: &str = "
| Core                      | No               |
| PushSubscription          | No               |
| Mailbox                   | Yes              |
| Thread                    | No               |
| Email                     | Yes              |
| EmailDelivery             | No               |
| SearchSnippet             | No               |
| Identity                  | Yes              |
| EmailSubmission           | No               |
| VacationResponse          | Yes              |
| MDN                       | No               |
| Quota                     | No               |
| SieveScript               | Yes              |
| Principal                 | No               |
| ShareNotification         | No               |
| AddressBook               | Yes              |
| ContactCard               | Yes              |
| Calendar                  | Yes              |
| CalendarEvent             | Yes              |
| CalendarEventNotification | No               |
| ParticipantIdentity       | Yes              |
| FileNode                  | Yes              |
";

/// Registration blocks after RFC 9404: Type Name, Can Reference Blobs,
/// Can Use for State Change, Capability, document.
const LATER_REGISTRATIONS: &[(&str, &str, &str, &str, &str)] = &[
    // docs/specs/jmap-tasks-local.md §IANA "JMAP Data Types" table:
    // "| TaskList | No | Yes | Yes |", "| Task | Yes | Yes | Yes |",
    // "| TaskNotification | No | Yes | No |".
    (
        "Task",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:tasks",
        "crate-jmap local Tasks profile",
    ),
    (
        "TaskList",
        "No",
        "Yes",
        "urn:ietf:params:jmap:tasks",
        "crate-jmap local Tasks profile",
    ),
    (
        "TaskNotification",
        "No",
        "Yes",
        "urn:ietf:params:jmap:tasks",
        "crate-jmap local Tasks profile",
    ),
    (
        "Quota",
        "No",
        "Yes",
        "urn:ietf:params:jmap:quota",
        "RFC 9425",
    ),
    (
        "SieveScript",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:sieve",
        "RFC 9661",
    ),
    (
        "Principal",
        "No",
        "Yes",
        "urn:ietf:params:jmap:principals",
        "RFC 9670",
    ),
    (
        "ShareNotification",
        "No",
        "Yes",
        "urn:ietf:params:jmap:principals",
        "RFC 9670",
    ),
    (
        "AddressBook",
        "No",
        "Yes",
        "urn:ietf:params:jmap:contacts",
        "RFC 9610",
    ),
    (
        "ContactCard",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:contacts",
        "RFC 9610",
    ),
    (
        "Calendar",
        "no",
        "yes",
        "urn:ietf:params:jmap:calendars",
        "draft-ietf-jmap-calendars-34",
    ),
    (
        "CalendarEvent",
        "yes",
        "yes",
        "urn:ietf:params:jmap:calendars",
        "draft-ietf-jmap-calendars-34",
    ),
    (
        "CalendarEventNotification",
        "no",
        "yes",
        "urn:ietf:params:jmap:calendars",
        "draft-ietf-jmap-calendars-34",
    ),
    (
        "ParticipantIdentity",
        "no",
        "yes",
        "urn:ietf:params:jmap:calendars",
        "draft-ietf-jmap-calendars-34",
    ),
    (
        "FileNode",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:filenode",
        "draft-ietf-jmap-filenode-20",
    ),
    (
        "ChatContact",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "Chat",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "Message",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "Space",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "SpaceInvite",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "CustomEmoji",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "SpaceBan",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "ChatBan",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "ReadPosition",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "PresenceStatus",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat",
        "draft-atwood-jmap-chat-00",
    ),
    (
        "KeyPackage",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat:mimi",
        "draft-atwood-jmap-chat-mimi-00",
    ),
    (
        "Consent",
        "No",
        "Yes",
        "urn:ietf:params:jmap:chat:mimi",
        "draft-atwood-jmap-chat-mimi-00",
    ),
    (
        "Graph",
        "No",
        "Yes",
        "urn:ietf:params:jmap:graph",
        "draft-atwood-jmap-graph-00",
    ),
    (
        "GraphNode",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:graph",
        "draft-atwood-jmap-graph-00",
    ),
    (
        "GraphEdge",
        "No",
        "Yes",
        "urn:ietf:params:jmap:graph",
        "draft-atwood-jmap-graph-00",
    ),
    (
        "GraphNodeOp",
        "No",
        "Yes",
        "urn:ietf:params:jmap:graph:edit",
        "draft-atwood-jmap-graph-edit-00",
    ),
    (
        "VTCCall",
        "No",
        "Yes",
        "urn:ietf:params:jmap:vtc",
        "draft-atwood-jmap-vtc-00",
    ),
    (
        "VTCParticipant",
        "No",
        "Yes",
        "urn:ietf:params:jmap:vtc",
        "draft-atwood-jmap-vtc-00",
    ),
    (
        "VTCRecording",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:vtc",
        "draft-atwood-jmap-vtc-00",
    ),
    (
        "VTCLivestream",
        "No",
        "Yes",
        "urn:ietf:params:jmap:vtc",
        "draft-atwood-jmap-vtc-00",
    ),
    (
        "SceneRegion",
        "No",
        "Yes",
        "urn:ietf:params:jmap:scene",
        "draft-atwood-jmap-scene-00",
    ),
    (
        "SceneObject",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:scene",
        "draft-atwood-jmap-scene-00",
    ),
    (
        "SceneAvatar",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:scene",
        "draft-atwood-jmap-scene-00",
    ),
    (
        "SceneAsset",
        "Yes",
        "Yes",
        "urn:ietf:params:jmap:scene",
        "draft-atwood-jmap-scene-00",
    ),
];

/// Decision 22 (docs/design/DECISIONS.md): the draft-atwood types that
/// support history. Every other draft-atwood type takes the §7.2 default.
const ATWOOD_HISTORY_YES: &[&str] = &[
    "Chat",
    "Message",
    "Space",
    "ChatContact",
    "CustomEmoji",
    "SpaceBan",
    "ChatBan",
    "SpaceInvite",
    "Graph",
    "GraphNode",
    "GraphEdge",
    "SceneAsset",
    "SceneRegion",
    "SceneObject",
    "VTCCall",
    "VTCRecording",
    "VTCLivestream",
];

fn cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

#[test]
fn rfc9404_table_1_rows() {
    for line in RFC9404_TABLE_1.lines().filter(|l| !l.trim().is_empty()) {
        let c = cells(line);
        let e = data_type(c[0]).unwrap_or_else(|| panic!("{} missing", c[0]));
        assert_eq!(e.can_reference_blobs, yes(c[1]), "{}", c[0]);
        assert_eq!(e.can_use_for_state_change, yes(c[2]), "{}", c[0]);
        assert_eq!(e.capability, c[3], "{}", c[0]);
        let reference = c[4].trim_matches(['[', ']']).replace("RFC", "RFC ");
        assert_eq!(e.reference, reference, "{}", c[0]);
    }
}

#[test]
fn object_history_table_1_values() {
    for line in OBJECT_HISTORY_TABLE_1
        .lines()
        .filter(|l| !l.trim().is_empty())
    {
        let c = cells(line);
        let e = data_type(c[0]).unwrap_or_else(|| panic!("{} missing", c[0]));
        assert_eq!(e.supports_history, yes(c[1]), "{}", c[0]);
    }
}

#[test]
fn later_registrations() {
    for &(name, blobs, state, cap, doc) in LATER_REGISTRATIONS {
        let e = data_type(name).unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(
            (
                e.can_reference_blobs,
                e.can_use_for_state_change,
                e.capability,
                e.reference
            ),
            (yes(blobs), yes(state), cap, doc),
            "{name}"
        );
    }
}

/// The local Tasks profile's "Supports History" column: TaskList and Task
/// Yes, TaskNotification No (object-history-03 §7.2, decision 23).
#[test]
fn tasks_history_values() {
    assert!(data_type("Task").unwrap().supports_history);
    assert!(data_type("TaskList").unwrap().supports_history);
    assert!(!data_type("TaskNotification").unwrap().supports_history);
}

#[test]
fn atwood_history_values() {
    for &(name, .., doc) in LATER_REGISTRATIONS {
        if doc.starts_with("draft-atwood") {
            let want = ATWOOD_HISTORY_YES.contains(&name);
            assert_eq!(data_type(name).unwrap().supports_history, want, "{name}");
        }
    }
}

/// Decision 14: exactly these draft-atwood types reference blobs.
#[test]
fn atwood_blob_rows() {
    let mut got: Vec<&str> = DATA_TYPES
        .iter()
        .filter(|e| e.reference.starts_with("draft-atwood") && e.can_reference_blobs)
        .map(|e| e.type_name)
        .collect();
    got.sort_unstable();
    let mut want = vec![
        "Chat",
        "Message",
        "Space",
        "CustomEmoji",
        "SceneAsset",
        "SceneObject",
        "SceneAvatar",
        "VTCRecording",
        "GraphNode",
    ];
    want.sort_unstable();
    assert_eq!(got, want);
}

/// The table holds exactly the oracle's rows, sorted and unique.
#[test]
fn sorted_unique_and_complete() {
    assert!(
        DATA_TYPES
            .windows(2)
            .all(|w| w[0].type_name < w[1].type_name),
        "DATA_TYPES must be strictly sorted by type_name"
    );
    let rfc9404 = RFC9404_TABLE_1
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    assert_eq!(DATA_TYPES.len(), rfc9404 + LATER_REGISTRATIONS.len());
}

#[test]
fn lookup() {
    assert!(!data_type("Identity").unwrap().can_reference_blobs);
    assert!(data_type("Tasks").is_none());
    assert!(
        data_type("email").is_none(),
        "type names are case-sensitive"
    );
    assert!(data_type("").is_none());
    for e in DATA_TYPES {
        assert_eq!(data_type(e.type_name), Some(e));
    }
}
