//! Chat message, attachments, reactions, and delivery state types.

use jmap_cid_types::Sha256;
use jmap_types::{Id, UTCDate, impl_string_enum};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Delivery state of a [`Message`] as defined by the spec.
///
/// `Other` preserves any future value for round-trip fidelity.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeliveryState {
    /// Enqueued but not yet acknowledged by any recipient.
    Pending,
    /// Acknowledged by the recipient's server.
    Delivered,
    /// Delivery failed permanently.
    Failed,
    /// Received on the recipient's device.
    Received,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(DeliveryState, "a delivery state string",
    "pending" => Pending,
    "delivered" => Delivered,
    "failed" => Failed,
    "received" => Received,
);

/// MIME type for a message body (draft-atwood-jmap-chat-00 §Message).
///
/// The spec defines three well-known values. `Other(String)` preserves any
/// unrecognized MIME type for lossless round-trip.
///
/// Wire strings: `"text/plain"`, `"text/markdown"`, `"application/jmap-chat-rich"`.
///
/// # Forging caveat
///
/// `Other(String)` is `pub`, so callers can construct
/// `BodyType::Other("text/plain".into())`. The custom serde impl emits the
/// wrapped string verbatim on serialize and normalises canonical wire strings
/// to their typed variant on deserialize. Consequences:
/// * `BodyType::Other("text/plain".into()) != BodyType::Plain` on PartialEq,
///   but both serialize to `"text/plain"`.
/// * `Other("text/plain")` -> `"text/plain"` -> `Plain` is a lossy round-trip
///   (the variant changes shape).
///
/// Reserve `Other(s)` for genuinely unrecognised MIME types. Comparing wire-
/// string equality across two values requires matching on `as_str()`, not on
/// `PartialEq`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BodyType {
    /// `"text/plain"` — unformatted UTF-8 text.
    Plain,
    /// `"text/markdown"` — CommonMark-formatted text.
    Markdown,
    /// `"application/jmap-chat-rich"` — structured rich text (spans array).
    Rich,
    /// Any unrecognized MIME type string, preserved as-is. See the "Forging
    /// caveat" section in the enum-level rustdoc.
    Other(String),
}

impl_string_enum!(BodyType, "a BodyType MIME-string",
    "text/plain"                 => Plain,
    "text/markdown"              => Markdown,
    "application/jmap-chat-rich" => Rich,
);

/// Why a recipient acknowledged a message (RFC-JMAP-Chat §ReadDisposition).
///
/// `Other` preserves any unrecognized value for round-trip fidelity.
/// Servers MUST NOT reject messages carrying unknown values.
///
/// # MAINTENANCE
/// When adding a new variant: (1) add it to the enum below, (2) add the
/// corresponding `"wire-name" => Variant` arm in `impl_string_enum!` below.
/// Both must stay in sync — a variant absent from the macro falls through to
/// `Other(String)` on deserialize and serializes incorrectly.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReadDisposition {
    /// Message content was presented to the user's attention (default).
    Displayed,
    /// Message was removed without being displayed.
    Deleted,
    /// Message was handled by an automated process.
    Processed,
    /// A value not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(ReadDisposition, "a read disposition string",
    "displayed" => Displayed,
    "deleted"   => Deleted,
    "processed" => Processed,
);

/// A file attached to a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// The `blobId` property (draft-atwood-jmap-chat-00 §4.1).
    pub blob_id: Id,
    /// The `filename` property (draft-atwood-jmap-chat-00 §4.1).
    pub filename: String,
    /// The `contentType` property (draft-atwood-jmap-chat-00 §4.1).
    pub content_type: String,
    /// The `size` property (draft-atwood-jmap-chat-00 §4.1).
    pub size: u64,
    /// The `sha256` property: SHA-256 digest of the attachment content
    /// as 64 lowercase hex characters (draft-atwood-jmap-chat-00 §4.1).
    /// Typed as [`Sha256`], the same wire shape FileNode uses
    /// (draft-atwood-jmap-cid-00 §2), so a malformed digest is rejected
    /// at deserialize.
    pub sha256: Sha256,
    /// The `id` of a FileNode in the Space's file tree that this
    /// attachment links to (draft-atwood-jmap-chat-filenode-00
    /// §Attachment Link). Only meaningful when the
    /// `urn:ietf:params:jmap:chat:filenode` capability is active.
    ///
    /// The draft makes `blobId`, `filename`, `contentType`, `size` and
    /// `sha256` OPTIONAL on a `Message/set` create when this field is
    /// present, and the server "MUST populate any absent fields ...
    /// before storage", so a stored Attachment always carries them. A
    /// create request uses [`AttachmentCreate`] instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filenode_id: Option<Id>,
    /// The `blobId` of a thumbnail or preview image derived from the
    /// FileNode's blob (draft-atwood-jmap-chat-filenode-00
    /// §thumbnailBlobId). SHOULD be set only when
    /// [`filenode_id`](Self::filenode_id) is also present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_blob_id: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An [`Attachment`] as sent in a `Message/set` create request.
///
/// draft-atwood-jmap-chat-filenode-00 §filenodeId: "When `filenodeId` is
/// present on an Attachment, the fields `blobId`, `filename`,
/// `contentType`, `size`, and `sha256` become OPTIONAL for that
/// Attachment entry; the server MUST populate any absent fields from the
/// referenced FileNode's properties before storage. If `filenodeId` is
/// absent, these fields retain their normal required status." This type
/// therefore makes those five fields optional; the server, not this
/// type, rejects an entry that lacks them without a `filenodeId`. A
/// stored or returned attachment is an [`Attachment`].
///
/// Absent fields are omitted on serialization. `sha256`, when present,
/// is validated at deserialize (see [`Attachment::sha256`]).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentCreate {
    /// The `blobId` property (draft-atwood-jmap-chat-00 §4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<Id>,
    /// The `filename` property (draft-atwood-jmap-chat-00 §4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// The `contentType` property (draft-atwood-jmap-chat-00 §4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// The `size` property (draft-atwood-jmap-chat-00 §4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The `sha256` property (draft-atwood-jmap-chat-00 §4.1). See
    /// [`Attachment::sha256`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Sha256>,
    /// The `filenodeId` property (draft-atwood-jmap-chat-filenode-00
    /// §filenodeId). See [`Attachment::filenode_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filenode_id: Option<Id>,
    /// The `thumbnailBlobId` property (draft-atwood-jmap-chat-filenode-00
    /// §thumbnailBlobId). See [`Attachment::thumbnail_blob_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_blob_id: Option<Id>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A user that a [`Message`] mentions (draft-atwood-jmap-chat-00 §Mention).
///
/// A Mention carries no position in `body`. The `mentions` array is
/// authoritative for every `bodyType`: servers take mentions only from it
/// and never derive them from `body` (including rich-body `"mention"`
/// spans, which are display-only).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mention {
    /// The mentioned [`ChatContact`]'s `id` (draft-atwood-jmap-chat-00 §4.4).
    ///
    /// A server-assigned Id local to this server, not the user's
    /// authentication-layer identity (that is
    /// [`ChatContact::identity`](crate::ChatContact::identity)). The
    /// textual representation in the message body (e.g. `@user@host`
    /// or `@display-name`) is composer-defined and is NOT recoverable
    /// from this field alone — locating and rendering the displayed
    /// text is presentation, left to the composer and renderer.
    ///
    /// [`ChatContact`]: crate::ChatContact
    pub id: Id,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The scope of a [`BroadcastMention`] (draft-atwood-jmap-chat-00
/// §BroadcastMention), also used by [`crate::Span::scope`] and
/// [`crate::ChatMessageEntry::mention_scopes`].
///
/// The spec defines exactly `"everyone"`, `"here"` and `"admins"`. Servers
/// reject any other value, but this type keeps it in
/// [`Other`](Self::Other) and serializes it back unchanged so a client can
/// pass it through.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BroadcastScope {
    /// `"everyone"`: all current members of the Space or Chat.
    Everyone,
    /// `"here"`: currently-active members (deployment-defined predicate).
    Here,
    /// `"admins"`: members holding administrative authority
    /// (deployment-defined predicate).
    Admins,
    /// Any scope this version does not recognize.
    ///
    /// **Forging caveat**: `Other("here".into())` is unequal to
    /// [`Here`](Self::Here) on `PartialEq` but serializes to the same wire
    /// string and deserializes back as `Here`. Reserve `Other(s)` for
    /// unrecognized strings; compare wire equality via
    /// [`as_str`](Self::as_str).
    Other(String),
}

impl_string_enum!(BroadcastScope, "a broadcast scope string",
    "everyone" => Everyone,
    "here" => Here,
    "admins" => Admins,
);

/// A broadcast-scope `@mention` in a [`Message`]
/// (draft-atwood-jmap-chat-00 §BroadcastMention).
///
/// Like [`Mention`], a `BroadcastMention` carries no position in `body`.
/// The `broadcastMentions` array is authoritative for every `bodyType`;
/// servers never derive broadcast mentions from `body`.
///
/// Unlike [`Mention`], a `BroadcastMention` does not name a specific
/// participant; it names a member set whose composition is resolved at
/// delivery time by each receiving server.
///
/// # Scope values
///
/// The spec defines exactly three scope values:
/// - `"everyone"` — all current members of the Space or Chat
/// - `"here"` — currently-active members (deployment-defined predicate)
/// - `"admins"` — members holding administrative authority (deployment-defined)
///
/// Servers MUST reject a `BroadcastMention` with an unrecognized `scope`
/// value with `invalidArguments`. The [`BROADCAST_MENTION_SCOPES`] constant
/// enumerates the accepted values for validation.
///
/// # Permission
///
/// Sending a `Message` with a non-empty `broadcastMentions` array requires
/// the `"mention_broadcast"` Space permission. The server MUST reject
/// `Message/set` create or update requests lacking this permission with
/// `forbidden`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastMention {
    /// The broadcast scope: one of `"everyone"`, `"here"`, or `"admins"`.
    ///
    /// Servers MUST reject unrecognized values with `invalidArguments`.
    /// Clients SHOULD treat unrecognized values as opaque pass-through
    /// for forward-compatibility (a future spec revision might add scopes);
    /// they deserialize as [`BroadcastScope::Other`].
    pub scope: BroadcastScope,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Mention {
    /// Construct a [`Mention`] of the user whose ChatContact `id` (or
    /// accountId, for the owner) is `id`.
    pub fn new(id: Id) -> Self {
        Self {
            id,
            extra: serde_json::Map::new(),
        }
    }
}

impl BroadcastMention {
    /// Construct a [`BroadcastMention`] of `scope`.
    pub fn new(scope: BroadcastScope) -> Self {
        Self {
            scope,
            extra: serde_json::Map::new(),
        }
    }
}

/// The set of spec-defined broadcast-mention scope values
/// (draft-atwood-jmap-chat-00 §4.4.1).
///
/// Servers use this for validation: any `BroadcastMention.scope` not in
/// this list MUST be rejected with `invalidArguments`.
///
/// Values: `"everyone"`, `"here"`, `"admins"`.
pub const BROADCAST_MENTION_SCOPES: &[&str] = &["everyone", "here", "admins"];

/// An interactive action button attached to a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageAction {
    /// The `type` property (draft-atwood-jmap-chat-00 §4.3).
    ///
    /// Wire name is `"type"` — Rust keyword, so renamed explicitly.
    #[serde(rename = "type")]
    pub action_type: String,
    /// The `uri` property (draft-atwood-jmap-chat-00 §4.3).
    pub uri: String,
    /// The `label` property (draft-atwood-jmap-chat-00 §4.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The `expiresAt` property (draft-atwood-jmap-chat-00 §4.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UTCDate>,
    /// The `metadata` property (draft-atwood-jmap-chat-00 §4.3).
    ///
    /// Type-specific key-value pairs whose shape is keyed by the
    /// [`action_type`](Self::action_type) discriminant. The draft
    /// (§4.3) deliberately leaves the per-type value shape open
    /// and requires that "Clients MUST ignore unknown keys".
    /// Consumers that need typed access for a known
    /// `action_type` value MUST cast via
    /// `serde_json::from_value::<MyTypedShape>(metadata.clone())`
    /// at their boundary; this crate does not enforce a schema and
    /// will not in future revisions, because the draft is the
    /// schema authority and explicitly keeps the value shape
    /// extensible per type.
    ///
    /// The value is a JSON Object on the wire per the draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single emoji reaction placed on a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    /// The `emoji` property (draft-atwood-jmap-chat-00 §4.6).
    pub emoji: String,
    /// The `customEmojiId` property (draft-atwood-jmap-chat-00 §4.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_emoji_id: Option<Id>,
    /// The `senderId` property (draft-atwood-jmap-chat-00 §4.6).
    ///
    /// The accountId for the owner's reaction, otherwise the reacting
    /// user's `ChatContact.id`.
    pub sender_id: Id,
    /// The `sentAt` property (draft-atwood-jmap-chat-00 §4.6).
    pub sent_at: UTCDate,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A prior revision of a [`Message`] body, stored in edit history.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRevision {
    /// The `body` property (draft-atwood-jmap-chat-00 §4.5).
    pub body: String,
    /// The `bodyType` property (draft-atwood-jmap-chat-00 §4.5).
    pub body_type: BodyType,
    /// The `editedAt` property (draft-atwood-jmap-chat-00 §4.5).
    pub edited_at: UTCDate,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Per-recipient delivery receipt for a [`Message`].
///
/// The natural zero state — "nothing acknowledged yet" — is
/// `DeliveryReceipt::default()` (all four optional fields `None`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    /// The `deliveredAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<UTCDate>,
    /// The `deviceDeliveredAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_delivered_at: Option<UTCDate>,
    /// The `readAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<UTCDate>,
    /// The `readDisposition` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_disposition: Option<ReadDisposition>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single chat message as defined by the JMAP Chat extension.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The `id` property (draft-atwood-jmap-chat-00 §4.11).
    pub id: Id,
    /// The `senderMsgId` property (draft-atwood-jmap-chat-00 §4.11).
    pub sender_msg_id: Id,
    /// The `senderId` property (draft-atwood-jmap-chat-00 §4.11).
    ///
    /// The accountId for owner-composed messages, otherwise the
    /// sender's `ChatContact.id`.
    pub sender_id: Id,
    /// The `chatId` property (draft-atwood-jmap-chat-00 §4.11).
    pub chat_id: Id,
    /// The `body` property (draft-atwood-jmap-chat-00 §4.11).
    pub body: String,
    /// The `bodyType` property (draft-atwood-jmap-chat-00 §4.11).
    pub body_type: BodyType,
    /// The `attachments` property (draft-atwood-jmap-chat-00 §4.11).
    pub attachments: Vec<Attachment>,
    /// The `mentions` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(default)]
    pub mentions: Vec<Mention>,
    /// The `broadcastMentions` property (draft-atwood-jmap-chat-00 §4.11).
    ///
    /// Structured broadcast-scope mention annotations (`@everyone`,
    /// `@here`, `@admins`). Empty by default. Authoritative for every
    /// `bodyType`, including rich bodies, whose `"broadcast"` spans are
    /// display-only.
    ///
    /// Sending a message with a non-empty array requires the
    /// `"mention_broadcast"` Space permission.
    #[serde(default)]
    pub broadcast_mentions: Vec<BroadcastMention>,
    /// The `actions` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(default)]
    pub actions: Vec<MessageAction>,
    /// The `reactions` property (draft-atwood-jmap-chat-00 §4.11).
    ///
    /// Keyed by `senderReactionId` — a client-assigned ULID for
    /// owner-placed reactions, or a peer-supplied ULID for
    /// inbound reactions. See the draft (§4.11 "reactions") for
    /// the assignment contract: this id is what `Message/set`
    /// patches use as the key for individual reaction add/remove
    /// operations (`reactions/<senderReactionId>: <Reaction>` to
    /// add, `reactions/<senderReactionId>: null` to remove).
    #[serde(default)]
    pub reactions: BTreeMap<String, Reaction>,
    /// The `sentAt` property (draft-atwood-jmap-chat-00 §4.11).
    pub sent_at: UTCDate,
    /// The `receivedAt` property (draft-atwood-jmap-chat-00 §4.11).
    pub received_at: UTCDate,
    /// The `deliveryState` property (draft-atwood-jmap-chat-00 §4.11).
    ///
    /// # Relationship to `delivered_at` and `delivery_receipts`
    ///
    /// Three fields on `Message` encode aspects of "did this
    /// message get delivered?":
    ///
    /// - `delivery_state` (this field, always present) — coarse
    ///   aggregate state (`Pending` / `Delivered` / `Failed` /
    ///   `Received`). The server-authoritative summary value;
    ///   downstream code that needs a single "is this delivered"
    ///   boolean SHOULD branch on this field.
    /// - [`delivered_at`](Self::delivered_at) (optional) — the
    ///   aggregate first-acknowledgment timestamp on the owner side.
    ///   Set when the first outbound delivery is acknowledged;
    ///   absent before that. Per the draft, `delivered_at` is the
    ///   timeline counterpart of `delivery_state == Delivered` but
    ///   `delivery_state` is the dispatch primacy.
    /// - [`delivery_receipts`](Self::delivery_receipts) (optional,
    ///   present only when `sender_id` is the accountId) — the per-recipient
    ///   breakdown keyed by `ChatContact.id`. The aggregate fields
    ///   above MAY be derived from this map (e.g. `delivered_at` =
    ///   min over receipts), but the server is the source of truth
    ///   for the aggregates — consumers SHOULD NOT recompute them
    ///   client-side.
    ///
    /// Primacy summary: `delivery_state` is the dispatch field;
    /// `delivered_at` is the dispatch field's timestamp;
    /// `delivery_receipts` is the per-recipient detail. The three
    /// MUST be mutually consistent on a well-formed wire frame.
    pub delivery_state: DeliveryState,

    /// The `replyTo` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Id>,
    /// The `threadRootId` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_root_id: Option<Id>,
    /// The `replyCount` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_count: Option<u64>,
    /// The `unreadReplyCount` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread_reply_count: Option<u64>,
    /// The `topicId` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    /// The `subject` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// The `senderExpiresAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_expires_at: Option<UTCDate>,
    /// The `burnOnRead` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burn_on_read: Option<bool>,
    /// The `expiresAfterReadSeconds` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_after_read_seconds: Option<u64>,
    /// The `deliveryReceipts` property (draft-atwood-jmap-chat-00 §4.11).
    ///
    /// Keyed by recipient `ChatContact.id` (one entry per non-owner
    /// participant). Present only when this message's
    /// [`sender_id`](Self::sender_id) is the accountId — the
    /// owner-side per-recipient delivery state. See the draft
    /// (§4.11 "deliveryReceipts") for the contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_receipts: Option<BTreeMap<String, DeliveryReceipt>>,
    /// The `deliveredAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<UTCDate>,
    /// The `readAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<UTCDate>,
    /// The `readDisposition` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_disposition: Option<ReadDisposition>,
    /// The `editedAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<UTCDate>,
    /// The `editHistory` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_history: Option<Vec<MessageRevision>>,
    /// The `deletedAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<UTCDate>,
    /// The `deletedForAll` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_for_all: Option<bool>,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Message {
    /// Construct a [`Message`] from its required wire fields.
    ///
    /// Empty-by-default collection fields (`attachments`, `mentions`,
    /// `broadcast_mentions`, `actions`, `reactions`) and all optional
    /// metadata fields default
    /// to their zero values. Callers assign them via struct-update after
    /// construction.
    ///
    /// Shape mirrors the canonical `jmap_mail_types::Email::new`:
    /// the signature carries only the spec-required wire fields; "empty
    /// by default" collections and optional metadata are filled in here.
    ///
    /// The argument count exceeds clippy's default `too_many_arguments`
    /// threshold of 7 because every parameter is a spec-required Message
    /// field with no meaningful default (identity, content, timestamps,
    /// delivery state). Compressing further would push spec-required
    /// fields into post-construction assignment, which the workspace
    /// constructor convention specifically prohibits.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        sender_msg_id: Id,
        sender_id: Id,
        chat_id: Id,
        body: impl Into<String>,
        body_type: impl Into<BodyType>,
        sent_at: UTCDate,
        received_at: UTCDate,
        delivery_state: DeliveryState,
    ) -> Self {
        Self {
            id,
            sender_msg_id,
            sender_id,
            chat_id,
            body: body.into(),
            body_type: body_type.into(),
            attachments: Vec::new(),
            mentions: Vec::new(),
            broadcast_mentions: Vec::new(),
            actions: Vec::new(),
            reactions: BTreeMap::new(),
            sent_at,
            received_at,
            delivery_state,
            reply_to: None,
            thread_root_id: None,
            reply_count: None,
            unread_reply_count: None,
            topic_id: None,
            subject: None,
            sender_expires_at: None,
            burn_on_read: None,
            expires_after_read_seconds: None,
            delivery_receipts: None,
            delivered_at: None,
            read_at: None,
            read_disposition: None,
            edited_at: None,
            edit_history: None,
            deleted_at: None,
            deleted_for_all: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: hand-crafted minimal JSON matching the spec's required field set.
    // Vec fields deserialize from `[]` — they must be empty, not None.
    // reactions deserializes from `{}` — must be an empty map.
    #[test]
    fn message_deser_minimal() {
        let json = r#"{
            "id": "m1",
            "senderMsgId": "smid1",
            "senderId": "self",
            "chatId": "c1",
            "body": "hi",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize minimal Message");
        assert_eq!(msg.id.as_ref(), "m1");
        assert_eq!(msg.sender_id.as_ref(), "self");
        assert!(msg.attachments.is_empty());
        assert!(msg.mentions.is_empty());
        assert!(msg.broadcast_mentions.is_empty());
        assert!(msg.actions.is_empty());
        assert!(msg.reactions.is_empty());
        assert!(msg.reply_to.is_none());
        assert!(msg.edit_history.is_none());
        assert!(msg.deleted_at.is_none());
    }

    // Oracle: hand-crafted JSON with one reaction entry; verify key and emoji field.
    #[test]
    fn message_deser_with_reactions() {
        let json = r#"{
            "id": "m2",
            "senderMsgId": "smid2",
            "senderId": "u42",
            "chatId": "c1",
            "body": "hello",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [],
            "actions": [],
            "reactions": {
                "r1": {
                    "emoji": "👍",
                    "senderId": "u99",
                    "sentAt": "2026-01-02T10:00:00Z"
                }
            },
            "sentAt": "2026-01-02T09:00:00Z",
            "receivedAt": "2026-01-02T09:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize Message with reactions");
        assert_eq!(msg.reactions.len(), 1);
        let reaction = msg.reactions.get("r1").expect("reaction key r1");
        assert_eq!(reaction.emoji, "👍");
        assert_eq!(reaction.sender_id.as_ref(), "u99");
    }

    // Oracle: serde rename contract — the wire key for action_type must be "type".
    // Verified by serializing and checking the JSON string directly.
    #[test]
    fn message_action_type_wire_name() {
        let action = MessageAction {
            action_type: "button".to_string(),
            uri: "https://example.com".to_string(),
            label: None,
            expires_at: None,
            metadata: None,
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&action).expect("serialize MessageAction");
        assert!(
            json.contains(r#""type":"button""#),
            "expected wire key \"type\", got: {json}"
        );
        assert!(
            !json.contains("actionType"),
            "wire key must not be actionType, got: {json}"
        );
    }

    // Oracle: skip_serializing_if = "Option::is_none" contract — absent optional
    // fields must not appear in serialized output.
    #[test]
    fn message_ser_omits_none() {
        let json_in = r#"{
            "id": "m3",
            "senderMsgId": "smid3",
            "senderId": "self",
            "chatId": "c1",
            "body": "test",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-03T00:00:00Z",
            "receivedAt": "2026-01-03T00:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json_in).expect("deserialize");
        let json_out = serde_json::to_string(&msg).expect("serialize");
        assert!(
            !json_out.contains("replyTo"),
            "replyTo must be absent when None, got: {json_out}"
        );
        assert!(
            !json_out.contains("editHistory"),
            "editHistory must be absent when None, got: {json_out}"
        );
        assert!(
            !json_out.contains("deletedAt"),
            "deletedAt must be absent when None, got: {json_out}"
        );
    }

    // Oracle: hand-crafted DeliveryReceipt JSON; roundtrip must preserve all fields.
    #[test]
    fn delivery_receipt_roundtrip() {
        let json = r#"{
            "u1": {
                "deliveredAt": "2026-01-04T08:00:00Z",
                "readAt": "2026-01-04T08:05:00Z",
                "readDisposition": "displayed"
            },
            "u2": {}
        }"#;
        let map: BTreeMap<String, DeliveryReceipt> =
            serde_json::from_str(json).expect("deserialize DeliveryReceipt map");
        assert_eq!(map.len(), 2);
        let u1 = map.get("u1").expect("u1");
        assert_eq!(
            u1.delivered_at.as_ref().map(|d| d.as_ref()),
            Some("2026-01-04T08:00:00Z")
        );
        assert_eq!(
            u1.read_at.as_ref().map(|d| d.as_ref()),
            Some("2026-01-04T08:05:00Z")
        );
        assert_eq!(u1.read_disposition, Some(ReadDisposition::Displayed));
        assert!(u1.device_delivered_at.is_none());
        let u2 = map.get("u2").expect("u2");
        assert!(u2.delivered_at.is_none());
        assert!(u2.read_disposition.is_none());

        let roundtrip = serde_json::to_string(&map).expect("serialize");
        let map2: BTreeMap<String, DeliveryReceipt> =
            serde_json::from_str(&roundtrip).expect("re-deserialize");
        assert_eq!(map, map2);
    }

    // Oracle: spec §ReadDisposition wire values (hand-crafted; verified against spec text).
    #[test]
    fn read_disposition_roundtrip() {
        let cases = [
            ("\"displayed\"", ReadDisposition::Displayed),
            ("\"deleted\"", ReadDisposition::Deleted),
            ("\"processed\"", ReadDisposition::Processed),
            (
                "\"voice-listened\"",
                ReadDisposition::Other("voice-listened".to_owned()),
            ),
        ];
        for (json_str, expected) in cases {
            let got: ReadDisposition =
                serde_json::from_str(json_str).expect("deserialize ReadDisposition");
            assert_eq!(got, expected, "deser {json_str}");
            let back = serde_json::to_string(&got).expect("serialize");
            assert_eq!(back, json_str, "reser {json_str}");
        }
    }

    /// BodyType canonical variants round-trip through their registered wire
    /// strings. Oracle: draft-atwood-jmap-chat-00 §Message bodyType MIME
    /// set `{text/plain, text/markdown, application/jmap-chat-rich}`.
    #[test]
    fn body_type_canonical_variants_round_trip() {
        let cases: &[(&str, BodyType)] = &[
            (r#""text/plain""#, BodyType::Plain),
            (r#""text/markdown""#, BodyType::Markdown),
            (r#""application/jmap-chat-rich""#, BodyType::Rich),
        ];
        for (raw, expected) in cases {
            let parsed: BodyType = serde_json::from_str(raw).expect("must deserialize");
            assert_eq!(&parsed, expected);
            let back = serde_json::to_string(&parsed).expect("serialize");
            assert_eq!(back.as_str(), *raw);
        }
    }

    /// BodyType: unknown wire string round-trips via Other(s).
    /// Oracle: `"application/x-acme"` is not in draft-atwood-jmap-chat-00
    /// §Message body-type set and uses an `x-` prefix to make the
    /// vendor-extension intent explicit.
    #[test]
    fn body_type_unknown_round_trips_via_other() {
        let raw = r#""application/x-acme""#;
        let parsed: BodyType = serde_json::from_str(raw).expect("must deserialize");
        assert_eq!(parsed, BodyType::Other("application/x-acme".to_owned()));
        assert_eq!(serde_json::to_string(&parsed).unwrap(), raw);
    }

    /// `BodyType::as_str` returns the wire MIME string for every variant,
    /// including the stored string of `Other`. Oracle: the MIME strings
    /// listed in draft-atwood-jmap-chat-00 §supportedBodyTypes (line 326 at
    /// spec 6c38e2f); `"application/x-acme"` is a vendor `x-` type outside
    /// that set.
    #[test]
    fn body_type_as_str_returns_wire_string() {
        assert_eq!(BodyType::Plain.as_str(), "text/plain");
        assert_eq!(BodyType::Markdown.as_str(), "text/markdown");
        assert_eq!(BodyType::Rich.as_str(), "application/jmap-chat-rich");
        assert_eq!(
            BodyType::Other("application/x-acme".to_owned()).as_str(),
            "application/x-acme"
        );
    }

    /// `Mention::new` serializes to exactly `{"id": <id>}`, the sole
    /// property draft-atwood-jmap-chat-00 §Mention defines.
    #[test]
    fn mention_new_serializes_to_spec_shape() {
        let m = Mention::new(Id::from("c42"));
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            serde_json::json!({"id": "c42"})
        );
    }

    // ── Extras-preservation policy tests (JMAP-lbdy.3) ───────────────────

    /// `Attachment.extra` captures vendor fields and preserves them.
    #[test]
    fn attachment_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "blobId": "b1",
            "filename": "a.png",
            "contentType": "image/png",
            "size": 100,
            "sha256": "0".repeat(64),
            "acmeCorpScanResult": "clean"
        });
        let a: Attachment = serde_json::from_value(raw).unwrap();
        assert_eq!(
            a.extra.get("acmeCorpScanResult").and_then(|v| v.as_str()),
            Some("clean")
        );
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["acmeCorpScanResult"], "clean");
    }

    /// Oracle: draft-atwood-jmap-chat-filenode-00 §Attachment Link — "The
    /// `Attachment` type defined in {{JMAP-CHAT}} is extended with the
    /// following optional fields": `filenodeId` (String, optional) and
    /// `thumbnailBlobId` (String, optional). Both land in typed fields
    /// (not `extra`), round-trip under their wire names, and are omitted
    /// when absent; unknown fields still round-trip via `extra`.
    #[test]
    fn attachment_filenode_fields_typed_and_round_trip() {
        let raw = serde_json::json!({
            "blobId": "b1",
            "filename": "a.png",
            "contentType": "image/png",
            "size": 100,
            "sha256": "0".repeat(64),
            "filenodeId": "fn1",
            "thumbnailBlobId": "thumb1",
            "acmeCorpScanResult": "clean"
        });
        let a: Attachment = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(a.filenode_id.as_ref().map(|i| i.as_ref()), Some("fn1"));
        assert_eq!(
            a.thumbnail_blob_id.as_ref().map(|i| i.as_ref()),
            Some("thumb1")
        );
        assert!(a.extra.get("filenodeId").is_none());
        assert!(a.extra.get("thumbnailBlobId").is_none());
        assert_eq!(serde_json::to_value(&a).unwrap(), raw);

        let plain = serde_json::json!({
            "blobId": "b1",
            "filename": "a.png",
            "contentType": "image/png",
            "size": 100,
            "sha256": "0".repeat(64)
        });
        let p: Attachment = serde_json::from_value(plain.clone()).unwrap();
        assert!(p.filenode_id.is_none() && p.thumbnail_blob_id.is_none());
        assert_eq!(serde_json::to_value(&p).unwrap(), plain);
    }

    /// Oracle: draft-atwood-jmap-chat-00 §Attachment — `sha256` is
    /// "Lowercase hex SHA-256 of blob content". The SHA-256 of "abc"
    /// (FIPS 180-2 Appendix B.1) parses on both Attachment and
    /// AttachmentCreate; its uppercase form and a 63-character prefix do
    /// not.
    #[test]
    fn attachment_sha256_must_be_lowercase_hex() {
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let with = |sha256: &str| {
            serde_json::json!({
                "blobId": "b1",
                "filename": "a.txt",
                "contentType": "text/plain",
                "size": 3,
                "sha256": sha256
            })
        };
        let a: Attachment = serde_json::from_value(with(abc)).unwrap();
        assert_eq!(a.sha256.as_str(), abc);
        let c: AttachmentCreate = serde_json::from_value(with(abc)).unwrap();
        assert_eq!(c.sha256.as_ref().map(|d| d.as_str()), Some(abc));
        for bad in [abc.to_uppercase(), abc[..63].to_owned()] {
            assert!(serde_json::from_value::<Attachment>(with(&bad)).is_err());
            assert!(serde_json::from_value::<AttachmentCreate>(with(&bad)).is_err());
        }
    }

    /// Oracle: draft-atwood-jmap-chat-filenode-00 §filenodeId — "When
    /// `filenodeId` is present on an Attachment, the fields `blobId`,
    /// `filename`, `contentType`, `size`, and `sha256` become OPTIONAL for
    /// that Attachment entry". A filenodeId-only create entry parses and
    /// serializes back without the absent fields.
    #[test]
    fn attachment_create_accepts_filenode_only_entry() {
        let raw = serde_json::json!({ "filenodeId": "fn1", "thumbnailBlobId": "thumb1" });
        let a: AttachmentCreate = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(a.filenode_id.as_ref().map(|i| i.as_ref()), Some("fn1"));
        assert_eq!(
            a.thumbnail_blob_id.as_ref().map(|i| i.as_ref()),
            Some("thumb1")
        );
        assert!(a.blob_id.is_none() && a.filename.is_none() && a.content_type.is_none());
        assert!(a.size.is_none() && a.sha256.is_none() && a.extra.is_empty());
        assert_eq!(serde_json::to_value(&a).unwrap(), raw);
    }

    /// Oracle: draft-atwood-jmap-chat-00 §Attachment property list
    /// (`blobId`, `filename`, `contentType`, `size`, `sha256`). A full
    /// create entry lands in the typed fields and round-trips unchanged,
    /// vendor fields included.
    #[test]
    fn attachment_create_full_entry_round_trips() {
        let raw = serde_json::json!({
            "blobId": "b1",
            "filename": "a.png",
            "contentType": "image/png",
            "size": 100,
            "sha256": "0".repeat(64),
            "acmeCorpScanResult": "clean"
        });
        let a: AttachmentCreate = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(a.blob_id.as_ref().map(|i| i.as_ref()), Some("b1"));
        assert_eq!(a.filename.as_deref(), Some("a.png"));
        assert_eq!(a.content_type.as_deref(), Some("image/png"));
        assert_eq!(a.size, Some(100));
        assert_eq!(a.extra["acmeCorpScanResult"], "clean");
        assert_eq!(serde_json::to_value(&a).unwrap(), raw);
    }

    /// `Mention.extra` captures vendor fields and preserves them.
    #[test]
    fn mention_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "u1",
            "offset": 0,
            "length": 5,
            "acmeCorpHighlight": "soft"
        });
        let m: Mention = serde_json::from_value(raw).unwrap();
        assert_eq!(
            m.extra.get("acmeCorpHighlight").and_then(|v| v.as_str()),
            Some("soft")
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["acmeCorpHighlight"], "soft");
    }

    /// `MessageAction.extra` captures vendor fields and preserves them.
    #[test]
    fn message_action_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "type": "button",
            "uri": "https://example.com",
            "acmeCorpDisplayPriority": 5
        });
        let a: MessageAction = serde_json::from_value(raw).unwrap();
        assert_eq!(
            a.extra
                .get("acmeCorpDisplayPriority")
                .and_then(|v| v.as_u64()),
            Some(5)
        );
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["acmeCorpDisplayPriority"], 5);
    }

    /// `Reaction.extra` captures vendor fields and preserves them.
    #[test]
    fn reaction_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "emoji": "👍",
            "senderId": "self",
            "sentAt": "2026-01-02T10:00:00Z",
            "acmeCorpClientUuid": "device-7"
        });
        let r: Reaction = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpClientUuid").and_then(|v| v.as_str()),
            Some("device-7")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpClientUuid"], "device-7");
    }

    /// `MessageRevision.extra` captures vendor fields and preserves them.
    #[test]
    fn message_revision_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "body": "v1",
            "bodyType": "text/plain",
            "editedAt": "2026-01-05T12:00:00Z",
            "acmeCorpEditReason": "typo"
        });
        let r: MessageRevision = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpEditReason").and_then(|v| v.as_str()),
            Some("typo")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpEditReason"], "typo");
    }

    /// `DeliveryReceipt.extra` captures vendor fields and preserves them.
    #[test]
    fn delivery_receipt_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "deliveredAt": "2026-01-04T08:00:00Z",
            "acmeCorpReceiptId": "rcpt-9"
        });
        let r: DeliveryReceipt = serde_json::from_value(raw).unwrap();
        assert_eq!(
            r.extra.get("acmeCorpReceiptId").and_then(|v| v.as_str()),
            Some("rcpt-9")
        );
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back["acmeCorpReceiptId"], "rcpt-9");
    }

    /// `Message.extra` captures vendor fields and preserves them across
    /// deserialize/serialize round-trip.
    #[test]
    fn message_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "id": "m1",
            "senderMsgId": "smid1",
            "senderId": "self",
            "chatId": "c1",
            "body": "hi",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered",
            "acmeCorpRoutedVia": "edge-3"
        });
        let m: Message = serde_json::from_value(raw).unwrap();
        assert_eq!(
            m.extra.get("acmeCorpRoutedVia").and_then(|v| v.as_str()),
            Some("edge-3")
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["acmeCorpRoutedVia"], "edge-3");
    }

    // Oracle: hand-crafted MessageRevision JSON; roundtrip must preserve all fields.
    #[test]
    fn message_revision_roundtrip() {
        let json = r#"{
            "body": "original text",
            "bodyType": "text/plain",
            "editedAt": "2026-01-05T12:00:00Z"
        }"#;
        let rev: MessageRevision = serde_json::from_str(json).expect("deserialize MessageRevision");
        assert_eq!(rev.body, "original text");
        assert_eq!(rev.body_type, BodyType::Plain);
        assert_eq!(rev.edited_at.as_ref(), "2026-01-05T12:00:00Z");

        let roundtrip = serde_json::to_string(&rev).expect("serialize");
        let rev2: MessageRevision =
            serde_json::from_str(&roundtrip).expect("re-deserialize MessageRevision");
        assert_eq!(rev, rev2);
    }

    // ── BroadcastMention tests ────────────────────────────────────────────

    /// Oracle: hand-crafted JSON matching the §BroadcastMention shape.
    /// All three spec-defined scope values must round-trip. §BroadcastMention:
    /// "Like Mention, a BroadcastMention carries no position in `body`."
    #[test]
    fn broadcast_mention_roundtrip_all_scopes() {
        for scope in BROADCAST_MENTION_SCOPES {
            let raw = serde_json::json!({ "scope": scope });
            let bm: BroadcastMention =
                serde_json::from_value(raw.clone()).expect("deserialize BroadcastMention");
            assert_eq!(bm.scope.as_str(), *scope, "scope must match for {scope}");
            assert!(bm.extra.is_empty(), "no extras expected");

            let back = serde_json::to_value(&bm).expect("serialize");
            assert_eq!(back, raw, "round-trip must be lossless for scope={scope}");
        }
    }

    /// Oracle: spec §BroadcastMention — wire key names must be camelCase,
    /// and "Like Mention, a BroadcastMention carries no position in `body`."
    #[test]
    fn broadcast_mention_wire_key_names() {
        let bm = BroadcastMention::new(BroadcastScope::Everyone);
        let json = serde_json::to_string(&bm).expect("serialize");
        assert!(json.contains(r#""scope""#), "must have scope key: {json}");
        assert!(
            !json.contains(r#""offset""#),
            "must not have offset key: {json}"
        );
        assert!(
            !json.contains(r#""length""#),
            "must not have length key: {json}"
        );
        // Negative: no snake_case keys leaked.
        assert!(
            !json.contains("broadcast_mention"),
            "snake_case must not appear: {json}"
        );
    }

    /// `BroadcastMention.extra` captures vendor fields and preserves them.
    #[test]
    fn broadcast_mention_preserves_vendor_extras() {
        let raw = serde_json::json!({
            "scope": "here",
            "offset": 10,
            "length": 5,
            "acmeCorpResolvedCount": 42
        });
        let bm: BroadcastMention = serde_json::from_value(raw).unwrap();
        assert_eq!(
            bm.extra
                .get("acmeCorpResolvedCount")
                .and_then(|v| v.as_u64()),
            Some(42)
        );
        let back = serde_json::to_value(&bm).unwrap();
        assert_eq!(back["acmeCorpResolvedCount"], 42);
    }

    /// Each spec scope maps to its typed variant and back. Oracle:
    /// draft-atwood-jmap-chat-00 §BroadcastMention lists `"everyone"`,
    /// `"here"` and `"admins"`; the JSON strings are hand-written.
    #[test]
    fn broadcast_scope_known_values() {
        for (wire, scope) in [
            (r#""everyone""#, BroadcastScope::Everyone),
            (r#""here""#, BroadcastScope::Here),
            (r#""admins""#, BroadcastScope::Admins),
        ] {
            let parsed: BroadcastScope = serde_json::from_str(wire).expect("deserialize");
            assert_eq!(parsed, scope);
            assert_eq!(serde_json::to_string(&scope).expect("serialize"), wire);
        }
    }

    /// An unknown scope deserializes as `Other` and serializes back
    /// unchanged, so clients can pass it through (§BroadcastMention:
    /// servers reject it; this type does not). Oracle: hand-written JSON.
    #[test]
    fn broadcast_scope_unknown_round_trips() {
        let raw = serde_json::json!({ "scope": "moderators" });
        let bm: BroadcastMention = serde_json::from_value(raw.clone()).expect("deserialize");
        assert_eq!(bm.scope, BroadcastScope::Other("moderators".into()));
        assert_eq!(serde_json::to_value(&bm).expect("serialize"), raw);
    }

    /// Verify `BROADCAST_MENTION_SCOPES` matches the spec enumeration exactly.
    /// Oracle: draft-atwood-jmap-chat-00 §4.4.1 scope list.
    #[test]
    fn broadcast_mention_scopes_matches_spec() {
        assert_eq!(
            BROADCAST_MENTION_SCOPES,
            &["everyone", "here", "admins"],
            "scope list must match spec §4.4.1 exactly"
        );
    }

    /// Verify a Message with broadcastMentions deserializes and round-trips.
    /// Oracle: hand-crafted JSON with one @everyone broadcast mention, which
    /// per §BroadcastMention "carries no position in `body`."
    #[test]
    fn message_with_broadcast_mentions_roundtrip() {
        let json = r#"{
            "id": "m10",
            "senderMsgId": "smid10",
            "senderId": "self",
            "chatId": "c1",
            "body": "@everyone hello",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "broadcastMentions": [
                {"scope": "everyone"}
            ],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-06-01T00:00:00Z",
            "receivedAt": "2026-06-01T00:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize");
        assert_eq!(msg.broadcast_mentions.len(), 1);
        assert_eq!(msg.broadcast_mentions[0].scope, BroadcastScope::Everyone);

        let back = serde_json::to_string(&msg).expect("serialize");
        let msg2: Message = serde_json::from_str(&back).expect("re-deserialize");
        assert_eq!(
            msg.broadcast_mentions, msg2.broadcast_mentions,
            "broadcast_mentions must round-trip"
        );
    }

    /// Constructor sets broadcast_mentions to empty vec (workspace convention:
    /// empty-by-default collections in constructor body, not signature).
    #[test]
    fn message_new_has_empty_broadcast_mentions() {
        let msg = Message::new(
            Id::from("m1"),
            Id::from("smid1"),
            Id::from("a1"),
            Id::from("c1"),
            "hi",
            "text/plain",
            UTCDate::from("2026-01-01T00:00:00Z"),
            UTCDate::from("2026-01-01T00:00:01Z"),
            DeliveryState::Delivered,
        );
        assert!(
            msg.broadcast_mentions.is_empty(),
            "constructor must default broadcast_mentions to empty"
        );
    }

    /// Oracle: draft-atwood-jmap-chat-00 Message `broadcastMentions`:
    /// "Empty by default." A Message JSON without it deserializes with an
    /// empty array.
    #[test]
    fn message_missing_broadcast_mentions_defaults_empty() {
        let json = r#"{
            "id": "m1",
            "senderMsgId": "smid1",
            "senderId": "self",
            "chatId": "c1",
            "body": "hi",
            "bodyType": "text/plain",
            "attachments": [],
            "mentions": [],
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize");
        assert!(msg.broadcast_mentions.is_empty());
    }

    /// Oracle: draft-atwood-jmap-chat-00 §Message — "`topicId` (String,
    /// optional, immutable)", "`subject` (String, optional, immutable): The
    /// human-readable name of the topic identified by `topicId`", and
    /// "`expiresAfterReadSeconds` (UnsignedInt, optional, immutable)".
    #[test]
    fn message_topic_subject_expires_after_read_are_typed() {
        let json = r#"{
            "id": "m1",
            "senderMsgId": "smid1",
            "senderId": "self",
            "chatId": "c1",
            "body": "hi",
            "bodyType": "text/plain",
            "attachments": [],
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered",
            "topicId": "t-1",
            "subject": "Q3 planning",
            "expiresAfterReadSeconds": 30
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize");
        assert_eq!(msg.topic_id.as_deref(), Some("t-1"));
        assert_eq!(msg.subject.as_deref(), Some("Q3 planning"));
        assert_eq!(msg.expires_after_read_seconds, Some(30));
        assert!(msg.extra.is_empty(), "typed fields leaked into extra");
        let back = serde_json::to_value(&msg).expect("serialize");
        assert_eq!(back["topicId"], "t-1");
        assert_eq!(back["subject"], "Q3 planning");
        assert_eq!(back["expiresAfterReadSeconds"], 30);
    }

    // Oracle: workspace map-ordering policy (bd:JMAP-pdux) — wire-format
    // maps serialize in sorted key order. Expected bytes are hand-written.
    #[test]
    fn delivery_receipts_serialize_in_sorted_key_order() {
        let mut msg = Message::new(
            Id::from("m1"),
            Id::from("smid1"),
            Id::from("a1"),
            Id::from("c1"),
            "hi",
            "text/plain",
            UTCDate::from("2026-01-01T00:00:00Z"),
            UTCDate::from("2026-01-01T00:00:01Z"),
            DeliveryState::Delivered,
        );
        let empty: DeliveryReceipt = serde_json::from_str("{}").expect("receipt");
        let mut receipts = BTreeMap::new();
        receipts.insert("u2".to_owned(), empty.clone());
        receipts.insert("u1".to_owned(), empty);
        msg.delivery_receipts = Some(receipts);
        let j = serde_json::to_string(&msg).expect("serialize");
        assert!(
            j.contains(r#""deliveryReceipts":{"u1":{},"u2":{}}"#),
            "deliveryReceipts keys must be sorted: {j}"
        );
    }
}
