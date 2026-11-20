//! The "JMAP Object URI Types" registry
//! (draft-atwood-jmap-object-uri-00 §JMAP Object URI Types Registry).
//!
//! The draft creates the registry with no initial entries; each document
//! that uses JMAP object URIs for a data type registers its type name
//! (§Use). The rows here are the registrations requested by those
//! documents, each citing it in [`UriTypeEntry::reference`].

/// One row of the "JMAP Object URI Types" registry.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UriTypeEntry {
    /// The type name used in the URI, such as `"calendarevent"`: the
    /// lowercased data type name, matching the `type-name` rule.
    pub type_name: &'static str,
    /// The data type it stands for, exactly as registered in the "JMAP
    /// Data Types" registry (RFC 9404 §6.3), such as `"CalendarEvent"`.
    /// Look it up with [`jmap_types::data_types::data_type`].
    pub data_type: &'static str,
    /// The document that registers the type name, such as
    /// `"draft-atwood-jmap-chat-calendars-00"`.
    pub reference: &'static str,
}

const fn row(
    type_name: &'static str,
    data_type: &'static str,
    reference: &'static str,
) -> UriTypeEntry {
    UriTypeEntry {
        type_name,
        data_type,
        reference,
    }
}

const CHAT: &str = "draft-atwood-jmap-chat-00";
const CALENDARS: &str = "draft-atwood-jmap-chat-calendars-00";
const TASKS: &str = "draft-atwood-jmap-chat-tasks-00";
const FILENODE: &str = "draft-atwood-jmap-chat-filenode-00";
const GRAPH: &str = "draft-atwood-jmap-graph-00";
const PROJECTION: &str = "draft-atwood-jmap-graph-projection-00";
const SCENE: &str = "draft-atwood-jmap-scene-00";
const VTC: &str = "draft-atwood-jmap-vtc-00";

/// Every registered type name, sorted by [`UriTypeEntry::type_name`]
/// (byte order).
///
/// Sources: the "JMAP Object URI Types" IANA sections of
/// draft-atwood-jmap-chat-00, -chat-calendars-00, -chat-tasks-00,
/// -chat-filenode-00, -graph-00, -graph-projection-00, -scene-00 and
/// -vtc-00.
pub const URI_TYPES: &[UriTypeEntry] = &[
    row("addressbook", "AddressBook", PROJECTION),
    row("calendar", "Calendar", CALENDARS),
    row("calendarevent", "CalendarEvent", CALENDARS),
    row("chat", "Chat", CHAT),
    row("chatban", "ChatBan", CHAT),
    row("chatcontact", "ChatContact", CHAT),
    row("contactcard", "ContactCard", PROJECTION),
    row("customemoji", "CustomEmoji", CHAT),
    row("email", "Email", PROJECTION),
    row("emailsubmission", "EmailSubmission", PROJECTION),
    row("filenode", "FileNode", FILENODE),
    row("graph", "Graph", GRAPH),
    row("graphedge", "GraphEdge", GRAPH),
    row("graphnode", "GraphNode", GRAPH),
    row("identity", "Identity", PROJECTION),
    row("mailbox", "Mailbox", PROJECTION),
    row("message", "Message", CHAT),
    row("presencestatus", "PresenceStatus", CHAT),
    row("principal", "Principal", CALENDARS),
    row("readposition", "ReadPosition", CHAT),
    row("sceneasset", "SceneAsset", SCENE),
    row("sceneavatar", "SceneAvatar", SCENE),
    row("sceneobject", "SceneObject", SCENE),
    row("sceneregion", "SceneRegion", SCENE),
    row("space", "Space", CHAT),
    row("spaceban", "SpaceBan", CHAT),
    row("spaceinvite", "SpaceInvite", CHAT),
    row("task", "Task", TASKS),
    row("tasklist", "TaskList", TASKS),
    row("thread", "Thread", PROJECTION),
    row("vtccall", "VTCCall", VTC),
    row("vtclivestream", "VTCLivestream", VTC),
    row("vtcparticipant", "VTCParticipant", VTC),
    row("vtcrecording", "VTCRecording", VTC),
];

/// The registry row for `type_name`, compared case-sensitively (the
/// `type-name` rule allows only lowercase letters and digits); `None` when
/// the type name is not registered.
///
/// ```
/// use jmap_object_uri::uri_type;
///
/// assert_eq!(uri_type("calendarevent").unwrap().data_type, "CalendarEvent");
/// assert!(uri_type("CalendarEvent").is_none());
/// ```
pub fn uri_type(type_name: &str) -> Option<&'static UriTypeEntry> {
    URI_TYPES
        .binary_search_by(|e| e.type_name.cmp(type_name))
        .ok()
        .map(|i| &URI_TYPES[i])
}

/// The registry row whose [`UriTypeEntry::data_type`] is `data_type`,
/// compared case-sensitively as data type names are; `None` when no type
/// name is registered for it. A server building a URI for an object uses
/// the row's `type_name`.
///
/// ```
/// use jmap_object_uri::{JmapUri, uri_type_for_data_type};
/// use jmap_types::Id;
///
/// let entry = uri_type_for_data_type("Email").unwrap();
/// let uri = JmapUri::new(entry.type_name, Id::from("A1"), Id::from("M42")).unwrap();
/// assert_eq!(uri.to_string(), "jmap:email:A1:M42");
/// assert!(uri_type_for_data_type("SearchSnippet").is_none());
/// ```
pub fn uri_type_for_data_type(data_type: &str) -> Option<&'static UriTypeEntry> {
    URI_TYPES.iter().find(|e| e.data_type == data_type)
}
