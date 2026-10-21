//! Rich body format: the parsed `body` of a Message whose `bodyType` is
//! `"application/jmap-chat-rich"` (draft-atwood-jmap-chat-00 §Rich Body
//! Format).
//!
//! Spans are presentation only. Servers must not map, validate, or rewrite
//! any part of a rich body, and must not base any authorization,
//! notification, push, query, or delivery decision on span content. The
//! Message's `mentions` and `broadcastMentions` arrays are what take effect.
//!
//! [`RichBody`] is the object that the JSON string in `body` encodes.
//! Parse it with [`serde_json::from_str`]:
//!
//! ```rust
//! use jmap_chat_types::{RichBody, SpanType};
//!
//! let body = r#"{"spans":[{"type":"bold","text":"hi"}]}"#;
//! let rich: RichBody = serde_json::from_str(body).expect("valid rich body");
//! assert_eq!(rich.spans[0].kind, SpanType::Bold);
//! assert_eq!(rich.spans[0].text, "hi");
//! ```

use crate::message::BroadcastScope;
use jmap_types::impl_string_enum;
use serde::{Deserialize, Serialize};

/// The `type` of a [`Span`] (draft-atwood-jmap-chat-00 §Span Object).
///
/// `Other` keeps an unrecognized span type so it round-trips: servers must
/// not reject a message only because it has unrecognized span types, and
/// clients render such a span's `text` as plaintext.
///
/// # MAINTENANCE
/// When adding a new variant: (1) add it to the enum below, (2) add the
/// corresponding `"wire-name" => Variant` arm in `impl_string_enum!` below.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpanType {
    /// `"text"`: plain text.
    Text,
    /// `"bold"`: bold text.
    Bold,
    /// `"italic"`: italic text.
    Italic,
    /// `"bold-italic"`: bold and italic text.
    BoldItalic,
    /// `"code"`: inline code.
    Code,
    /// `"codeblock"`: fenced code block; see [`Span::lang`].
    Codeblock,
    /// `"blockquote"`: quoted text.
    Blockquote,
    /// `"mention"`: per-user @mention, for display; see [`Span::user_id`].
    Mention,
    /// `"broadcast"`: broadcast-scope @mention, for display; see
    /// [`Span::scope`].
    Broadcast,
    /// `"link"`: hyperlink; see [`Span::uri`].
    Link,
    /// A span type not recognized by this version of the library.
    Other(String),
}

impl_string_enum!(SpanType, "a span type string",
    "text"        => Text,
    "bold"        => Bold,
    "italic"      => Italic,
    "bold-italic" => BoldItalic,
    "code"        => Code,
    "codeblock"   => Codeblock,
    "blockquote"  => Blockquote,
    "mention"     => Mention,
    "broadcast"   => Broadcast,
    "link"        => Link,
);

/// The parsed body object of an `"application/jmap-chat-rich"` Message
/// (draft-atwood-jmap-chat-00 §Span Object).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichBody {
    /// The message content as ordered spans, from left to right.
    pub spans: Vec<Span>,
    /// Catch-all for fields not covered above. Preserves unknown fields
    /// across deserialize/serialize round-trip per workspace
    /// extras-preservation policy (see workspace AGENTS.md).
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// One span of a [`RichBody`] (draft-atwood-jmap-chat-00 §Span Object).
///
/// The type-specific fields are typed as optional on every span. This type
/// does not check that a field matches its span type, because servers must
/// not validate a rich body.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    /// The span type (wire field `type`).
    #[serde(rename = "type")]
    pub kind: SpanType,
    /// The plaintext content of this span; present on every span type.
    pub text: String,
    /// `"codeblock"`: language hint for syntax highlighting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// `"mention"`: display data written by the composing client. It has no
    /// server meaning and need not be an id on the receiving server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// `"broadcast"`: one of `"everyone"`, `"here"`, or `"admins"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<BroadcastScope>,
    /// `"link"`: the hyperlink target. It SHOULD be treated as untrusted, and
    /// MUST NOT be a JMAP object URI (`jmap:`, draft-atwood-jmap-object-uri-00),
    /// because `body` is relayed between servers unchanged
    /// (draft-atwood-jmap-chat-00 §Span Object). Inside encrypted content a
    /// client MUST NOT place one either, and treats one found after
    /// decryption as unresolvable (§Fields Inside the Ciphertext). This is a client
    /// rule: servers do not parse bodies to enforce it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Catch-all for fields not covered above, such as the fields of an
    /// unrecognized span type. Preserves them across deserialize/serialize
    /// round-trip per workspace extras-preservation policy.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
