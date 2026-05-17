//! Chat message, attachments, reactions, and delivery state types.

use jmap_types::{impl_string_enum, Id, UTCDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

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
#[derive(Debug, Clone, PartialEq)]
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

impl BodyType {
    /// The canonical MIME type string for this body type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Plain => "text/plain",
            Self::Markdown => "text/markdown",
            Self::Rich => "application/jmap-chat-rich",
            Self::Other(s) => s.as_str(),
        }
    }
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

/// Identifies who sent a [`Message`] or placed a [`Reaction`].
///
/// The account owner is represented as the wire sentinel `"self"`
/// (draft-atwood-jmap-chat-00 §4.5, §4.7). All other participants
/// carry their `ChatContact.id` string verbatim.
///
/// # Sentinel collision with `ChatContact.id == "self"`
///
/// `ChatContact.id` is the userId string provided by the authentication
/// layer and the draft places no constraints on its form
/// (draft-atwood-jmap-chat-00 §4.5: "the specific form of the identifier
/// … is not constrained by this specification; servers MUST treat it
/// as opaque regardless of form"). Consequently, nothing in the wire
/// format prevents a `ChatContact.id` whose value is the literal
/// 4-character string `"self"`.
///
/// **When such a contact exists, this type cannot distinguish a message
/// authored by that contact from a message authored by the account
/// owner**: both deserialize as [`SenderId::Owner`] and both serialize
/// back to the wire string `"self"`. The collision is lossy in only
/// one direction — wire `"self"` always decodes as `Owner` — and is
/// preserved by serialization, so round-trip fidelity is maintained
/// for the wire bytes but lost for the semantic distinction.
///
/// ## Consequences for downstream consumers
///
/// Authorization, audit, mention-resolution, and edit-permission code
/// that branches on `SenderId::Owner` vs `SenderId::Contact(_)` MUST
/// NOT use this enum alone to attribute a peer-originated message to
/// the account owner. Instead, cross-check against the authentication
/// layer's verified principal id (the account's own userId): a
/// `SenderId::Owner` value on an inbound peer-delivered message is
/// only trustworthy when the transport layer has independently
/// verified the message originated from the local account.
///
/// On owner-composed (locally-originated) traffic the collision is
/// not exploitable, since the local server is the source of truth
/// for who composed the message.
///
/// ## Guidance for deployments
///
/// Authentication layers SHOULD avoid issuing userIds whose string
/// representation is `"self"` (or any reserved sentinel a future
/// revision of this draft might define). This is deployment policy,
/// not a draft constraint, and is the cheapest mitigation for the
/// collision risk above.
///
/// ## Future spec revision
///
/// Eliminating the collision at the wire level would require the
/// draft to either (a) reserve a sentinel form that cannot occur in
/// the userId namespace, or (b) encode the owner relationship out of
/// band rather than overloading the `senderId` field. Both are
/// breaking changes to the wire format; neither is in scope for this
/// crate, which is canonical for the draft as currently written.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SenderId {
    /// The message or reaction was sent by the account owner.
    ///
    /// On the wire this is `"self"`. See the type-level rustdoc for the
    /// sentinel-collision caveat: a peer-originated message whose
    /// `senderId` wire string is literally `"self"` (because the
    /// authoring contact's userId happens to be `"self"`) also
    /// decodes as `Owner`.
    Owner,
    /// Another participant, identified by their `ChatContact.id`.
    ///
    /// Any wire string other than `"self"` decodes here. Construction
    /// of `SenderId::Contact("self".to_owned())` is permitted by the
    /// type system but collapses to [`SenderId::Owner`] on
    /// serialize-then-deserialize round-trip — see the type-level
    /// rustdoc.
    Contact(String),
}

impl SenderId {
    /// Borrow the wire-format string representation of this
    /// `SenderId`. `Owner` borrows the static `"self"` sentinel;
    /// `Contact(id)` borrows the inner string.
    ///
    /// Used as the single source of truth by both [`Serialize`] and
    /// [`std::fmt::Display`] so the two impls cannot drift.
    fn wire_str(&self) -> &str {
        match self {
            SenderId::Owner => "self",
            SenderId::Contact(id) => id.as_str(),
        }
    }
}

impl Serialize for SenderId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.wire_str())
    }
}

impl<'de> Deserialize<'de> for SenderId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(if s == "self" {
            SenderId::Owner
        } else {
            SenderId::Contact(s)
        })
    }
}

impl std::fmt::Display for SenderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.wire_str())
    }
}

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
    /// SHA-256 digest of the attachment content, hex-encoded: exactly 64 lowercase hex characters.
    ///
    /// Kept as `String` rather than `jmap_cid_types::Sha256` for
    /// canonical-template parity with `jmap-mail-types`, the
    /// workspace-canonical extension-types crate, whose
    /// dependency set is the same three crates (jmap-types, serde,
    /// serde_json) as this one. Adopting `jmap_cid_types::Sha256`
    /// here without first propagating the dep into the canonical
    /// would diverge from the canonical-template rule documented
    /// in the per-crate AGENTS.md ("Do not introduce a dependency
    /// that `jmap-mail-types` does not also have, without explicit
    /// user approval.").
    ///
    /// The workspace-architectural question of whether to take a
    /// hard dep on `jmap-cid-types` from every extension-types crate
    /// that carries a content hash on the wire is tracked
    /// separately. Until that lands, consumers MUST validate this
    /// field's shape themselves: exactly 64 lowercase hex
    /// characters (`[0-9a-f]{64}`) per the spec. A `String`
    /// without validation is a known trust-the-wire posture; the
    /// failure mode is a silent-acceptance bug if a consumer
    /// forgets the validation step.
    pub sha256: String,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// An `@mention` within a [`Message`] body.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mention {
    /// The mentioned [`ChatContact`]'s `id` (draft-atwood-jmap-chat-00 §4.4).
    ///
    /// Any opaque URI form: `user@host` (federated), a W3C DID-Core
    /// URI (`did:web:...`, `did:key:...`), a local-account id, or
    /// any future form. The textual representation in the message
    /// body (e.g. `@user@host`, `@did:web:...`, or `@display-name`)
    /// is composer-defined and is NOT recoverable from this field
    /// alone — the composer is responsible for both rendering the
    /// in-body span (driven by `offset` + `length`) and resolving
    /// the displayed text out of band. Servers and clients MUST
    /// treat the value as opaque.
    ///
    /// See [`ChatContact`] (`id` field) for the full URI-latitude
    /// note, including the list of accepted forms AND the
    /// Id-layer constraints (max 255 bytes, SAFE-CHAR character
    /// set) that apply when constructing via `Id::new_validated`.
    ///
    /// [`ChatContact`]: crate::ChatContact
    pub id: Id,
    /// The `offset` property (draft-atwood-jmap-chat-00 §4.4).
    pub offset: u64,
    /// The `length` property (draft-atwood-jmap-chat-00 §4.4).
    pub length: u64,
    /// Catch-all for vendor / site / private extension fields not covered
    /// by the typed fields above. Preserves unknown fields across
    /// deserialize/serialize round-trip per workspace extras-preservation
    /// policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

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
    pub sender_id: SenderId,
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
    pub body_type: String,
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
    pub sender_id: SenderId,
    /// The `chatId` property (draft-atwood-jmap-chat-00 §4.11).
    pub chat_id: Id,
    /// The `body` property (draft-atwood-jmap-chat-00 §4.11).
    pub body: String,
    /// The `bodyType` property (draft-atwood-jmap-chat-00 §4.11).
    pub body_type: String,
    /// The `attachments` property (draft-atwood-jmap-chat-00 §4.11).
    pub attachments: Vec<Attachment>,
    /// The `mentions` property (draft-atwood-jmap-chat-00 §4.11).
    pub mentions: Vec<Mention>,
    /// The `actions` property (draft-atwood-jmap-chat-00 §4.11).
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
    pub reactions: HashMap<String, Reaction>,
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
    ///   present only when `sender_id == Owner`) — the per-recipient
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
    /// The `senderExpiresAt` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_expires_at: Option<UTCDate>,
    /// The `burnOnRead` property (draft-atwood-jmap-chat-00 §4.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burn_on_read: Option<bool>,
    /// The `deliveryReceipts` property (draft-atwood-jmap-chat-00 §4.11).
    ///
    /// Keyed by recipient `ChatContact.id` (one entry per non-owner
    /// participant). Present only when this message's
    /// [`sender_id`](Self::sender_id) is `SenderId::Owner` — the
    /// owner-side per-recipient delivery state. See the draft
    /// (§4.11 "deliveryReceipts") for the contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_receipts: Option<HashMap<String, DeliveryReceipt>>,
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
    /// `actions`, `reactions`) and all optional metadata fields default
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
        sender_id: SenderId,
        chat_id: Id,
        body: impl Into<String>,
        body_type: impl Into<String>,
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
            actions: Vec::new(),
            reactions: HashMap::new(),
            sent_at,
            received_at,
            delivery_state,
            reply_to: None,
            thread_root_id: None,
            reply_count: None,
            unread_reply_count: None,
            sender_expires_at: None,
            burn_on_read: None,
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
            "actions": [],
            "reactions": {},
            "sentAt": "2026-01-01T00:00:00Z",
            "receivedAt": "2026-01-01T00:00:01Z",
            "deliveryState": "delivered"
        }"#;
        let msg: Message = serde_json::from_str(json).expect("deserialize minimal Message");
        assert_eq!(msg.id.as_ref(), "m1");
        assert_eq!(msg.sender_id, SenderId::Owner);
        assert!(msg.attachments.is_empty());
        assert!(msg.mentions.is_empty());
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
        assert_eq!(reaction.sender_id, SenderId::Contact("u99".to_owned()));
    }

    // Oracle: hand-built two-line JSON tokens chosen for the collision case;
    // expected post-deserialize variants come from the documented `"self"` →
    // `Owner` rule in `SenderId::deserialize`. This test asserts the
    // collision behavior is preserved as documented on `SenderId`'s
    // rustdoc — a wire-form `senderId` of `"self"` always decodes as
    // `Owner`, regardless of whether the authoring party is the account
    // owner or a peer contact whose userId happens to be the string
    // `"self"`. A future revision that disambiguates the wire format
    // MUST update this test alongside the wire-format change so that
    // downstream consumers see the breaking change in their test
    // failures.
    #[test]
    fn sender_id_self_sentinel_collision_documented_on_wire() {
        // Wire "self" → SenderId::Owner, unambiguously.
        let from_self: SenderId = serde_json::from_str(r#""self""#).expect("deserialize \"self\"");
        assert_eq!(from_self, SenderId::Owner);

        // Wire "alice@example.com" → SenderId::Contact, unambiguously.
        let from_alice: SenderId = serde_json::from_str(r#""alice@example.com""#)
            .expect("deserialize \"alice@example.com\"");
        assert_eq!(
            from_alice,
            SenderId::Contact("alice@example.com".to_owned())
        );

        // The collision: a hypothetical ChatContact whose userId is the
        // literal string "self" cannot be represented as
        // SenderId::Contact("self") on the wire — that wire form
        // decodes as SenderId::Owner instead. Downstream consumers
        // doing authorization-grade attribution MUST NOT rely on this
        // enum alone (see the SenderId type-level rustdoc).
        let constructed = SenderId::Contact("self".to_owned());
        let wire = serde_json::to_string(&constructed).expect("serialize Contact(\"self\")");
        assert_eq!(wire, r#""self""#);
        let round_tripped: SenderId =
            serde_json::from_str(&wire).expect("deserialize round-tripped Contact(\"self\")");
        assert_eq!(round_tripped, SenderId::Owner);
        assert_ne!(round_tripped, constructed);
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
        let map: HashMap<String, DeliveryReceipt> =
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
        let map2: HashMap<String, DeliveryReceipt> =
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
        assert_eq!(rev.body_type, "text/plain");
        assert_eq!(rev.edited_at.as_ref(), "2026-01-05T12:00:00Z");

        let roundtrip = serde_json::to_string(&rev).expect("serialize");
        let rev2: MessageRevision =
            serde_json::from_str(&roundtrip).expect("re-deserialize MessageRevision");
        assert_eq!(rev, rev2);
    }
}
