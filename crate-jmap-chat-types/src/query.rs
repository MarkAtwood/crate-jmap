//! `/query` filter and comparator types for the JMAP Chat objects
//! (draft-atwood-jmap-chat-00 §ChatContact/query, §Chat/query,
//! §Message/query, §Space/query, §CustomEmoji/query; RFC 8620 §5.5).
//!
//! The generic [`Filter`], [`FilterOperator`], and [`Operator`] types are
//! re-exported from `jmap-types`.
//!
//! # Excluded from extras preservation
//!
//! Every type in this module is **out of scope** for the workspace
//! extras-preservation policy: no condition carries a flatten-extras
//! `extra` field and no property enum carries an `Other(String)` variant.
//! A server that silently drops a filter clause or sort key it does not
//! understand returns the wrong result set with no error signal. Vendors
//! who need filterable custom fields should use `draft-ietf-jmap-metadata`
//! (`jmap-metadata-types`, bd JMAP-ua37).
//!
//! The condition structs carry `#[serde(deny_unknown_fields)]`, which
//! composes with the `#[serde(untagged)]` [`Filter`]: an unknown or
//! misspelt property at any depth fails the decode, and servers map that
//! failure to RFC 8620 §5.5 `unsupportedFilter`.
//!
//! Cross-reference: bd JMAP-lbdy "Decision: filter algebra excluded".
//!
//! # Construction from outside this crate
//!
//! The condition structs are `#[non_exhaustive]`; build one from
//! [`Default::default`] and set the fields you need:
//!
//! ```rust
//! use jmap_chat_types::{MessageFilter, MessageFilterCondition};
//! use jmap_types::Id;
//!
//! let mut cond = MessageFilterCondition::default();
//! cond.chat_id = Some(Id::from("c1"));
//! let filter = MessageFilter::Condition(cond);
//! ```

use jmap_types::{Id, impl_string_enum};
use serde::{Deserialize, Serialize};

use crate::{ChatKind, Presence};

pub use jmap_types::{Filter, FilterOperator, Operator};

/// Filter for `ChatContact/query`.
pub type ChatContactFilter = Filter<ChatContactFilterCondition>;
/// Filter for `Chat/query`.
pub type ChatFilter = Filter<ChatFilterCondition>;
/// Filter for `Message/query`.
pub type MessageFilter = Filter<MessageFilterCondition>;
/// Filter for `Space/query`.
pub type SpaceFilter = Filter<SpaceFilterCondition>;
/// Filter for `CustomEmoji/query`.
pub type CustomEmojiFilter = Filter<CustomEmojiFilterCondition>;

/// Sort comparator for `ChatContact/query`.
pub type ChatContactComparator = Comparator<ChatContactComparatorProperty>;
/// Sort comparator for `Chat/query`.
pub type ChatComparator = Comparator<ChatComparatorProperty>;
/// Sort comparator for `Message/query`.
pub type MessageComparator = Comparator<MessageComparatorProperty>;
/// Sort comparator for `Space/query`.
pub type SpaceComparator = Comparator<SpaceComparatorProperty>;
/// Sort comparator for `CustomEmoji/query`.
pub type CustomEmojiComparator = Comparator<CustomEmojiComparatorProperty>;

/// Filter condition for `ChatContact/query`
/// (draft-atwood-jmap-chat-00 §ChatContact/query).
///
/// When several fields are set, all must match.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatContactFilterCondition {
    /// The contact's `blocked` property must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
    /// The contact's `presence` property must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
    /// The contact's `identity` must equal this value octet for octet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
}

/// Filter condition for `Chat/query` (draft-atwood-jmap-chat-00 §Chat/query).
///
/// When several fields are set, all must match.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatFilterCondition {
    /// The Chat's `kind` must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ChatKind>,
    /// The Chat's `muted` property must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
}

/// Filter condition for `Message/query`
/// (draft-atwood-jmap-chat-00 §Message/query).
///
/// A request must set `chat_id` unless it sets `has_mention: Some(true)`;
/// servers return `unsupportedFilter` otherwise. When several fields are
/// set, all must match.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageFilterCondition {
    /// Restrict results to this Chat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<Id>,
    /// Full-text search over `body`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Return only messages in this thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_root_id: Option<Id>,
    /// Return only messages whose `topicId` equals this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
    /// Return messages with (`true`) or without (`false`) attachments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_attachment: Option<bool>,
    /// Return messages that mention the owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_mention: Option<bool>,
}

/// Filter condition for `Space/query` (draft-atwood-jmap-chat-00 §Space/query).
///
/// When several fields are set, all must match.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpaceFilterCondition {
    /// The Space's `name` must contain this substring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The Space's `isPublic` property must equal this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
}

/// Filter condition for `CustomEmoji/query`
/// (draft-atwood-jmap-chat-00 §CustomEmoji/query).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomEmojiFilterCondition {
    /// Restrict results to this Space's emoji. When absent, all emoji
    /// accessible to the account are returned, including server-global ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_id: Option<Id>,
}

/// Sort property for `ChatContact/query`
/// (draft-atwood-jmap-chat-00 §ChatContact/query "Sort properties").
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatContactComparatorProperty {
    /// Sort by `lastSeenAt`.
    LastSeenAt,
    /// Sort by `login`.
    Login,
    /// Sort by `lastActiveAt`; contacts without it sort last.
    LastActiveAt,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(ChatContactComparatorProperty, "a sort property string",
    "lastSeenAt" => LastSeenAt,
    "login" => Login,
    "lastActiveAt" => LastActiveAt,
);

/// Sort property for `Chat/query`.
///
/// draft-atwood-jmap-chat-00 §Chat/query names only the default sort,
/// `lastMessageAt`; chats without messages sort last.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatComparatorProperty {
    /// Sort by `lastMessageAt`.
    LastMessageAt,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(ChatComparatorProperty, "a sort property string",
    "lastMessageAt" => LastMessageAt,
);

/// Sort property for `Message/query`.
///
/// draft-atwood-jmap-chat-00 §Message/query names only the default sort,
/// `receivedAt`; §Input Validation forbids ordering by `sentAt`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageComparatorProperty {
    /// Sort by `receivedAt`.
    ReceivedAt,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(MessageComparatorProperty, "a sort property string",
    "receivedAt" => ReceivedAt,
);

/// Sort property for `Space/query`.
///
/// draft-atwood-jmap-chat-00 §Space/query names only the default sort, `name`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpaceComparatorProperty {
    /// Sort by `name`.
    Name,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(SpaceComparatorProperty, "a sort property string",
    "name" => Name,
);

/// Sort property for `CustomEmoji/query`.
///
/// draft-atwood-jmap-chat-00 §CustomEmoji/query names only the default
/// sort, `name`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CustomEmojiComparatorProperty {
    /// Sort by `name`.
    Name,
    /// A property not listed above, kept so it round-trips; a server
    /// answers it with `unsupportedSort` (RFC 8620 §5.5).
    Other(String),
}

impl_string_enum!(CustomEmojiComparatorProperty, "a sort property string",
    "name" => Name,
);

/// RFC 8620 §5.5 `Comparator` over the sort property type `P`.
///
/// `is_ascending` defaults to `true` (RFC 8620 §5.5).
///
/// ```rust
/// use jmap_chat_types::{MessageComparator, MessageComparatorProperty};
///
/// let mut cmp = MessageComparator::new(MessageComparatorProperty::ReceivedAt);
/// cmp.is_ascending = false;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparator<P> {
    /// The property to sort by.
    pub property: P,
    /// Sort ascending when `true`, descending when `false`.
    #[serde(
        default = "crate::default_true",
        skip_serializing_if = "crate::is_true"
    )]
    pub is_ascending: bool,
    /// Collation algorithm for string comparison (RFC 8620 §5.5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collation: Option<String>,
}

impl<P> Comparator<P> {
    /// An ascending comparator on `property` with no collation.
    pub fn new(property: P) -> Self {
        Self {
            property,
            is_ascending: true,
            collation: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};

    /// `wire` uses every filter property the draft lists for the object.
    /// It must decode and re-encode to the same JSON.
    fn assert_spec_condition<
        T: Serialize + DeserializeOwned + Default + PartialEq + std::fmt::Debug,
    >(
        wire: Value,
    ) {
        let cond: T = serde_json::from_value(wire.clone()).expect("decode");
        assert_ne!(cond, T::default(), "every field must decode: {wire}");
        assert_eq!(serde_json::to_value(&cond).expect("encode"), wire);
    }

    /// §ChatContact/query: "Filter properties: `blocked` (Boolean),
    /// `presence` (String), `identity` (String; ...)".
    #[test]
    fn chat_contact_condition_matches_spec_properties() {
        assert_spec_condition::<ChatContactFilterCondition>(
            json!({"blocked": true, "presence": "away", "identity": "alice@example.com"}),
        );
    }

    /// §Chat/query: "Filter properties: `kind` (String), `muted` (Boolean)."
    #[test]
    fn chat_condition_matches_spec_properties() {
        assert_spec_condition::<ChatFilterCondition>(json!({"kind": "group", "muted": false}));
    }

    /// §Message/query filter properties: `chatId`, `text`, `threadRootId`,
    /// `topicId`, `hasAttachment`, `hasMention`.
    #[test]
    fn message_condition_matches_spec_properties() {
        assert_spec_condition::<MessageFilterCondition>(json!({
            "chatId": "c1",
            "text": "hello",
            "threadRootId": "m1",
            "topicId": "t1",
            "hasAttachment": true,
            "hasMention": true,
        }));
    }

    /// §Space/query: "Filter properties: `name` (String, substring match),
    /// `isPublic` (Boolean)."
    #[test]
    fn space_condition_matches_spec_properties() {
        assert_spec_condition::<SpaceFilterCondition>(json!({"name": "rust", "isPublic": true}));
    }

    /// §CustomEmoji/query: "Filter properties: `spaceId` (String, optional
    /// ...)".
    #[test]
    fn custom_emoji_condition_matches_spec_properties() {
        assert_spec_condition::<CustomEmojiFilterCondition>(json!({"spaceId": "s1"}));
    }

    /// RFC 8620 §5.5: "unsupportedFilter: The "filter" is syntactically
    /// valid, but the server cannot process it." A key outside the
    /// draft's filter properties, at any depth, or an extra key on an
    /// operator node must fail the decode rather than match every record.
    #[test]
    fn filters_reject_unknown_keys() {
        fn rejects<T: DeserializeOwned>(raw: Value) {
            assert!(
                serde_json::from_value::<T>(raw.clone()).is_err(),
                "{raw} must be rejected"
            );
        }
        rejects::<ChatContactFilter>(json!({"is_blocked": true}));
        rejects::<ChatFilter>(json!({"kind": "group", "bogus": 1}));
        rejects::<MessageFilter>(json!({"chat_id": "c1"}));
        rejects::<MessageFilter>(json!({
            "operator": "AND",
            "conditions": [{"operator": "NOT", "conditions": [{"chat_id": "c1"}]}],
        }));
        rejects::<MessageFilter>(json!({
            "operator": "AND",
            "conditions": [{"chatId": "c1"}],
            "chatId": "c2",
        }));
        rejects::<SpaceFilter>(json!({"is_public": true}));
        rejects::<CustomEmojiFilter>(json!({"space_id": "s1"}));
    }

    /// RFC 8620 §5.5: a FilterOperator has `operator` ("AND" | "OR" |
    /// "NOT") and `conditions` (FilterOperator|FilterCondition[]).
    #[test]
    fn message_filter_operator_tree_decodes() {
        let wire = json!({
            "operator": "AND",
            "conditions": [{"chatId": "c1"}, {"hasMention": true}],
        });
        let filter: MessageFilter = serde_json::from_value(wire).expect("decode");
        let Filter::Operator(op) = filter else {
            panic!("expected an operator node");
        };
        assert_eq!(op.operator, Operator::And);
        let [Filter::Condition(a), Filter::Condition(b)] = op.conditions.as_slice() else {
            panic!("expected two conditions");
        };
        assert_eq!(a.chat_id, Some(Id::from("c1")));
        assert_eq!(b.has_mention, Some(true));
    }

    /// §ChatContact/query: "Sort properties: `lastSeenAt`, `login`,
    /// `lastActiveAt`." RFC 8620 §5.5 Comparator: "isAscending: Boolean
    /// (optional; default: true)".
    #[test]
    fn chat_contact_comparator_decodes_spec_properties_with_default_ascending() {
        for (wire, property) in [
            ("lastSeenAt", ChatContactComparatorProperty::LastSeenAt),
            ("login", ChatContactComparatorProperty::Login),
            ("lastActiveAt", ChatContactComparatorProperty::LastActiveAt),
        ] {
            let cmp: ChatContactComparator =
                serde_json::from_value(json!({"property": wire})).expect("decode");
            assert_eq!(cmp.property, property);
            assert!(cmp.is_ascending);
            assert_eq!(cmp.collation, None);
        }
    }

    /// Default sorts named by the draft: §Chat/query `lastMessageAt`,
    /// §Message/query `receivedAt`, §Space/query and §CustomEmoji/query
    /// `name`. RFC 8620 §5.5 `isAscending` "(default: true)": an ascending
    /// comparator omits it, as `EmailComparator` does.
    #[test]
    fn default_sort_comparators_encode_to_spec_wire() {
        let mut chat = ChatComparator::new(ChatComparatorProperty::LastMessageAt);
        chat.is_ascending = false;
        assert_eq!(
            serde_json::to_value(chat).expect("encode"),
            json!({"property": "lastMessageAt", "isAscending": false})
        );
        assert_eq!(
            serde_json::to_value(MessageComparator::new(
                MessageComparatorProperty::ReceivedAt
            ))
            .expect("encode"),
            json!({"property": "receivedAt"})
        );
        assert_eq!(
            serde_json::to_value(SpaceComparator::new(SpaceComparatorProperty::Name))
                .expect("encode"),
            json!({"property": "name"})
        );
        assert_eq!(
            serde_json::to_value(CustomEmojiComparator::new(
                CustomEmojiComparatorProperty::Name
            ))
            .expect("encode"),
            json!({"property": "name"})
        );
    }

    /// Draft §Input Validation: "`sentAt`, `editedAt`: store as-is; never
    /// use for ordering or expiry." It is not a listed sort property, so it
    /// decodes to `Other`, which a server answers with `unsupportedSort`.
    #[test]
    fn message_comparator_sent_at_is_other() {
        assert_eq!(
            serde_json::from_value::<MessageComparator>(json!({"property": "sentAt"}))
                .unwrap()
                .property,
            MessageComparatorProperty::Other("sentAt".to_owned())
        );
    }
}
