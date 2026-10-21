//! Spec-enumerated wire-vocabulary string constants.
//!
//! Several fields in this crate carry strings drawn from a
//! spec-defined enumeration (permission names, chat-member roles,
//! ephemeral data-type tags, channel-permission target types). The
//! fields themselves are typed as `String` / `Vec<String>` because
//! either (a) the spec mandates silent-ignore of unrecognized
//! values rather than rejection, or (b) the consumer surface is
//! small enough that the workspace canonical-template rule has not
//! yet been amended to retype them as typed enums.
//!
//! This module hosts the canonical const list of currently
//! spec-known values for each vocabulary, so consumers that need
//! caller-side validation, lint checks, IDE completion, or
//! lookup-table construction have a single source of truth instead
//! of duplicating literal &str arrays across crates.
//!
//! The lists are NOT exhaustive in the type-system sense. They
//! reflect the values defined by the current draft revision.
//! Future draft revisions MAY add values; servers MUST ignore
//! unrecognized values per the spec. Consumers that build a
//! HashSet/HashMap from these slices SHOULD plan for that.
//!
//! When a future revision retypes any of these fields as a typed
//! `impl_string_enum!` enum (per workspace AGENTS.md
//! extras-preservation policy for wire-format result enums), the
//! corresponding const slice here MAY be removed in favour of the
//! enum's own variant list. Track the propagation epic in `bd`.

/// Spec-enumerated permission names usable in `SpaceRole.permissions`,
/// `ChannelPermission.allow`, `ChannelPermission.deny`, and
/// `RolePatch.permissions`, per draft-atwood-jmap-chat-00 §4.12.
///
/// Servers MUST ignore unrecognized permission names. Consumers that
/// validate caller-supplied input SHOULD compare against this list
/// (e.g. as a `HashSet<&str>`) to surface typos at the boundary; the
/// server will reject unknown values with `forbidden` or silently
/// drop them, depending on the operation.
pub const SPEC_PERMISSION_NAMES: &[&str] = &[
    "view",
    "send",
    "pin",
    "manage_channels",
    "manage_members",
    "manage_roles",
    "manage_space",
    "ban",
    "mention_broadcast",
    "start_call",
];

/// Spec-enumerated values for `ChatMember.role`, per
/// draft-atwood-jmap-chat-00 §4.9.
///
/// The wire-observable role enum is fixed at two values:
/// `"admin"` and `"member"`. Servers MAY designate additional
/// internal principals as having admin-equivalent authority
/// (server admins, dedicated moderator roles, automated systems,
/// etc.); from a remote peer's perspective these still appear as
/// `"admin"` on the wire.
pub const SPEC_CHAT_MEMBER_ROLES: &[&str] = &["admin", "member"];

/// Spec-enumerated values for `ChannelPermission.target_type`, per
/// draft-atwood-jmap-chat-00 §4.15.
pub const SPEC_CHANNEL_PERMISSION_TARGET_TYPES: &[&str] = &["role", "member"];

/// Spec-enumerated values for `ChatStreamEnable.data_types`, per
/// draft-atwood-jmap-chat-wss-00 §7.1.
///
/// A `dataTypes` array containing ONLY unrecognized values MUST be
/// rejected by the server with a `RequestError`. Unrecognized
/// values appearing alongside recognized values MUST be silently
/// ignored; only recognized values take effect.
pub const SPEC_EPHEMERAL_DATA_TYPES: &[&str] = &["typing", "presence"];

/// Companion-registered values for `ChatStreamEnable.data_types`, per
/// draft-atwood-jmap-chat-wss-00 §Enabling Ephemeral Push: `"vtc"` for
/// in-call events (draft-atwood-jmap-vtc-wss-00) and `"scene"` for spatial
/// events (draft-atwood-jmap-scene-wss-00).
///
/// Kept apart from [`SPEC_EPHEMERAL_DATA_TYPES`]: each is recognized only
/// when the server advertises its companion WebSocket capability,
/// `urn:ietf:params:jmap:vtc:websocket` or
/// `urn:ietf:params:jmap:scene:websocket` (L175). When it does, the value
/// counts as recognized for the rejection rule ("none of `"typing"`,
/// `"presence"`, or a companion value whose capability the server
/// advertises", L194). When it does not, the value "is treated as
/// unrecognized and silently ignored" (L196).
pub const COMPANION_EPHEMERAL_DATA_TYPES: &[&str] = &["vtc", "scene"];

/// Endpoint / MessageAction `type` for a video/voice teleconference
/// (draft-atwood-jmap-chat-00 §Endpoint).
pub const CHAT_CAP_VTC: &str = "urn:ietf:params:jmap:chat:cap:vtc";

/// Endpoint / MessageAction `type` for a calendar event link or meeting
/// invite (draft-atwood-jmap-chat-00 §Endpoint;
/// draft-atwood-jmap-chat-calendars-00 §MessageAction format).
pub const CHAT_CAP_CALENDAR_EVENT: &str = "urn:ietf:params:jmap:chat:cap:calendar-event";

/// Endpoint / MessageAction `type` for a free/busy availability lookup
/// (draft-atwood-jmap-chat-00 §Endpoint;
/// draft-atwood-jmap-chat-calendars-00 §Availability Lookup).
pub const CHAT_CAP_AVAILABILITY: &str = "urn:ietf:params:jmap:chat:cap:availability";

/// Endpoint / MessageAction `type` for a task or to-do item link
/// (draft-atwood-jmap-chat-00 §Endpoint;
/// draft-atwood-jmap-chat-tasks-00 §MessageAction format).
pub const CHAT_CAP_TASK: &str = "urn:ietf:params:jmap:chat:cap:task";
