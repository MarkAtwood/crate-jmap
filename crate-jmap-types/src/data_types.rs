//! The IANA "JMAP Data Types" registry (RFC 9404 §6.3, its fields
//! restated by draft-ietf-jmap-blobext-03 §11.5), with the "Supports
//! History" column draft-ietf-jmap-object-history-03 §7.2 adds.
//!
//! One shared table read by Blob/lookup (which type names may be looked up,
//! and which capability each needs) and by object history (which types
//! support it), so the two never disagree.
//!
//! Rows are the registrations in RFC 9404 Table 1, RFC 9425, RFC 9661,
//! RFC 9670, RFC 9610, draft-ietf-jmap-calendars-34 and
//! draft-ietf-jmap-filenode-20, plus the registrations requested by the
//! draft-atwood-jmap drafts (chat, chat-mimi, graph, graph-edit, scene and
//! vtc), each citing its document in [`DataTypeEntry::reference`].
//!
//! "Supports History" is object-history-03 §7.2 Table 1 for the types that
//! table lists. For the draft-atwood types it is the value those drafts are
//! to register (decision 22 in `docs/design/DECISIONS.md`); a type with no
//! value takes the §7.2 default, "No".
//!
//! # Example
//!
//! ```
//! use jmap_types::data_types::data_type;
//!
//! let email = data_type("Email").unwrap();
//! assert_eq!(email.capability, "urn:ietf:params:jmap:mail");
//! assert!(email.can_reference_blobs);
//! assert!(data_type("NoSuchType").is_none());
//! ```

/// One row of the "JMAP Data Types" registry.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataTypeEntry {
    /// The data type name, as used in `typeNames` in Blob/lookup and as a
    /// key in a StateChange object (RFC 8620 §7.1), such as `"Email"`.
    pub type_name: &'static str,
    /// The capability URI a client puts in `using` to use the type.
    pub capability: &'static str,
    /// "Can Reference Blobs": whether objects of the type can reference
    /// blobs, which makes the type usable in Blob/lookup.
    pub can_reference_blobs: bool,
    /// "Can Use for State Change": whether the type name can appear in a
    /// StateChange object.
    pub can_use_for_state_change: bool,
    /// "Supports History" (object-history-03 §7.2): whether the history
    /// extensions apply to the type.
    pub supports_history: bool,
    /// The document that registers the type, such as `"RFC 8621"` or
    /// `"draft-atwood-jmap-chat-00"`.
    pub reference: &'static str,
}

const fn row(
    type_name: &'static str,
    capability: &'static str,
    can_reference_blobs: bool,
    can_use_for_state_change: bool,
    supports_history: bool,
    reference: &'static str,
) -> DataTypeEntry {
    DataTypeEntry {
        type_name,
        capability,
        can_reference_blobs,
        can_use_for_state_change,
        supports_history,
        reference,
    }
}

/// Every registered data type, sorted by [`DataTypeEntry::type_name`]
/// (byte order).
///
/// Task, TaskList and TaskNotification are not listed: no published
/// document registers them yet.
pub const DATA_TYPES: &[DataTypeEntry] = &[
    row(
        "AddressBook",
        "urn:ietf:params:jmap:contacts",
        false,
        true,
        true,
        "RFC 9610",
    ),
    row(
        "Calendar",
        "urn:ietf:params:jmap:calendars",
        false,
        true,
        true,
        "draft-ietf-jmap-calendars-34",
    ),
    row(
        "CalendarEvent",
        "urn:ietf:params:jmap:calendars",
        true,
        true,
        true,
        "draft-ietf-jmap-calendars-34",
    ),
    row(
        "CalendarEventNotification",
        "urn:ietf:params:jmap:calendars",
        false,
        true,
        false,
        "draft-ietf-jmap-calendars-34",
    ),
    row(
        "Chat",
        "urn:ietf:params:jmap:chat",
        true,
        true,
        true,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "ChatContact",
        "urn:ietf:params:jmap:chat",
        false,
        true,
        true,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "Consent",
        "urn:ietf:params:jmap:chat:mimi",
        false,
        true,
        false,
        "draft-atwood-jmap-chat-mimi-00",
    ),
    row(
        "ContactCard",
        "urn:ietf:params:jmap:contacts",
        true,
        true,
        true,
        "RFC 9610",
    ),
    row(
        "Core",
        "urn:ietf:params:jmap:core",
        false,
        false,
        false,
        "RFC 8620",
    ),
    row(
        "CustomEmoji",
        "urn:ietf:params:jmap:chat",
        true,
        true,
        true,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "Email",
        "urn:ietf:params:jmap:mail",
        true,
        true,
        true,
        "RFC 8621",
    ),
    row(
        "EmailDelivery",
        "urn:ietf:params:jmap:mail",
        false,
        true,
        false,
        "RFC 8621",
    ),
    row(
        "EmailSubmission",
        "urn:ietf:params:jmap:submission",
        false,
        true,
        false,
        "RFC 8621",
    ),
    row(
        "FileNode",
        "urn:ietf:params:jmap:filenode",
        true,
        true,
        true,
        "draft-ietf-jmap-filenode-20",
    ),
    row(
        "Graph",
        "urn:ietf:params:jmap:graph",
        false,
        true,
        true,
        "draft-atwood-jmap-graph-00",
    ),
    row(
        "GraphEdge",
        "urn:ietf:params:jmap:graph",
        false,
        true,
        true,
        "draft-atwood-jmap-graph-00",
    ),
    row(
        "GraphNode",
        "urn:ietf:params:jmap:graph",
        true,
        true,
        true,
        "draft-atwood-jmap-graph-00",
    ),
    row(
        "GraphNodeOp",
        "urn:ietf:params:jmap:graph:edit",
        false,
        true,
        false,
        "draft-atwood-jmap-graph-edit-00",
    ),
    row(
        "Identity",
        "urn:ietf:params:jmap:submission",
        false,
        true,
        true,
        "RFC 8621",
    ),
    row(
        "KeyPackage",
        "urn:ietf:params:jmap:chat:mimi",
        false,
        true,
        false,
        "draft-atwood-jmap-chat-mimi-00",
    ),
    row(
        "MDN",
        "urn:ietf:params:jmap:mdn",
        false,
        false,
        false,
        "RFC 9007",
    ),
    row(
        "Mailbox",
        "urn:ietf:params:jmap:mail",
        true,
        true,
        true,
        "RFC 8621",
    ),
    row(
        "Message",
        "urn:ietf:params:jmap:chat",
        true,
        true,
        true,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "ParticipantIdentity",
        "urn:ietf:params:jmap:calendars",
        false,
        true,
        true,
        "draft-ietf-jmap-calendars-34",
    ),
    row(
        "PresenceStatus",
        "urn:ietf:params:jmap:chat",
        false,
        true,
        false,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "Principal",
        "urn:ietf:params:jmap:principals",
        false,
        true,
        false,
        "RFC 9670",
    ),
    row(
        "PushSubscription",
        "urn:ietf:params:jmap:core",
        false,
        false,
        false,
        "RFC 8620",
    ),
    row(
        "Quota",
        "urn:ietf:params:jmap:quota",
        false,
        true,
        false,
        "RFC 9425",
    ),
    row(
        "ReadPosition",
        "urn:ietf:params:jmap:chat",
        false,
        true,
        false,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "SceneAsset",
        "urn:ietf:params:jmap:scene",
        true,
        true,
        true,
        "draft-atwood-jmap-scene-00",
    ),
    row(
        "SceneAvatar",
        "urn:ietf:params:jmap:scene",
        true,
        true,
        false,
        "draft-atwood-jmap-scene-00",
    ),
    row(
        "SceneObject",
        "urn:ietf:params:jmap:scene",
        true,
        true,
        true,
        "draft-atwood-jmap-scene-00",
    ),
    row(
        "SceneRegion",
        "urn:ietf:params:jmap:scene",
        false,
        true,
        true,
        "draft-atwood-jmap-scene-00",
    ),
    row(
        "SearchSnippet",
        "urn:ietf:params:jmap:mail",
        false,
        false,
        false,
        "RFC 8621",
    ),
    row(
        "ShareNotification",
        "urn:ietf:params:jmap:principals",
        false,
        true,
        false,
        "RFC 9670",
    ),
    row(
        "SieveScript",
        "urn:ietf:params:jmap:sieve",
        true,
        true,
        true,
        "RFC 9661",
    ),
    row(
        "Space",
        "urn:ietf:params:jmap:chat",
        true,
        true,
        true,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "SpaceBan",
        "urn:ietf:params:jmap:chat",
        false,
        true,
        true,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "SpaceInvite",
        "urn:ietf:params:jmap:chat",
        false,
        true,
        true,
        "draft-atwood-jmap-chat-00",
    ),
    row(
        "Thread",
        "urn:ietf:params:jmap:mail",
        true,
        true,
        false,
        "RFC 8621",
    ),
    row(
        "VTCCall",
        "urn:ietf:params:jmap:vtc",
        false,
        true,
        true,
        "draft-atwood-jmap-vtc-00",
    ),
    row(
        "VTCLivestream",
        "urn:ietf:params:jmap:vtc",
        false,
        true,
        true,
        "draft-atwood-jmap-vtc-00",
    ),
    row(
        "VTCParticipant",
        "urn:ietf:params:jmap:vtc",
        false,
        true,
        false,
        "draft-atwood-jmap-vtc-00",
    ),
    row(
        "VTCRecording",
        "urn:ietf:params:jmap:vtc",
        true,
        true,
        true,
        "draft-atwood-jmap-vtc-00",
    ),
    row(
        "VacationResponse",
        "urn:ietf:params:jmap:vacationresponse",
        false,
        true,
        true,
        "RFC 8621",
    ),
];

/// The registry row for `type_name`, compared case-sensitively as type
/// names are; `None` when the type is not registered.
pub fn data_type(type_name: &str) -> Option<&'static DataTypeEntry> {
    DATA_TYPES
        .binary_search_by(|e| e.type_name.cmp(type_name))
        .ok()
        .map(|i| &DATA_TYPES[i])
}
