//! Conversion rules between MIMI content and JMAP Chat Messages
//! (draft-atwood-jmap-chat-mimi-00 §Message Identifiers, §Edits, §Topics,
//! §Reactions, §Message Status, §Security Considerations).
//!
//! These are pure functions for a converting party, a gateway or client
//! that decrypts MIMI messages. A MIMI provider never converts: it "is not
//! a member of any MLS group and never holds message keys".
//!
//! The caller parses MIMI content and stores Messages and aliases. This
//! module derives the identifiers that the store keys on:
//!
//! - [`sender_msg_id`] derives a Message's `senderMsgId` from the hub's
//!   acceptance time and the MIMI message ID, through [`ulid`].
//! - [`RefKey`] is the 80-bit value that references resolve on. It is
//!   both the `senderMsgId` random component and the form an alias is
//!   recorded in. A MIMI `replaces` or `inReplyTo` resolves to the Message
//!   in the same Chat whose [`RefKey::from_sender_msg_id`], or one of
//!   whose recorded aliases, equals [`RefKey::of_message_id`] of the
//!   reference. An unresolved `inReplyTo` leaves `replyTo` absent.
//! - [`check_ref_key`] enforces the collision rule before a new Message or
//!   alias is stored, and [`may_record_alias`] the SHOULD that an edit
//!   comes from the original sender.
//!
//! A JMAP `topicId` maps to MIMI as its UTF-8 octets (`str::as_bytes`).
//! [`topic_id`] is the other direction.

use std::fmt;

/// A MIMI message ID: `MessageId = bstr .size 32` (draft-ietf-mimi-content
/// §Message ID), a hash algorithm octet then 31 hash octets.
pub type MimiMessageId = [u8; 32];

/// Crockford's Base32 alphabet, as the ULID specification uses it.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The low `chars * 5` bits of `value` as `chars` Crockford characters,
/// most significant first.
fn crockford(value: u128, chars: u32) -> String {
    (0..chars)
        .rev()
        .map(|i| char::from(CROCKFORD[((value >> (5 * i)) & 31) as usize]))
        .collect()
}

/// Octets 1 through 10 of a MIMI message ID as a big-endian integer.
fn random_component(id: &MimiMessageId) -> u128 {
    id[1..11]
        .iter()
        .fold(0, |acc, &octet| (acc << 8) | u128::from(octet))
}

/// The ULID whose 48-bit time component is `time_ms` and whose 80-bit
/// random component is `random`, as 26 upper-case Crockford Base32
/// characters.
///
/// Returns `None` when `time_ms` does not fit in 48 bits, which the ULID
/// specification rejects.
pub fn ulid(time_ms: u64, random: &[u8; 10]) -> Option<String> {
    if time_ms >> 48 != 0 {
        return None;
    }
    let random = random
        .iter()
        .fold(0, |acc, &octet| (acc << 8) | u128::from(octet));
    Some(crockford((u128::from(time_ms) << 80) | random, 26))
}

/// The `senderMsgId` of a MIMI message (draft §Message Identifiers).
///
/// The ULID's 48-bit time component is `accepted_ms`, the hub's acceptance
/// time in milliseconds: the FanoutMessage `timestamp`, or the
/// SubmitMessageResponse `accepted_timestamp` for the party's own message.
/// Its 80-bit random component is octets 1 through 10 of
/// `mimi_message_id`. Returns `None` when `accepted_ms` does not fit in 48
/// bits, which the ULID specification rejects.
pub fn sender_msg_id(accepted_ms: u64, mimi_message_id: &MimiMessageId) -> Option<String> {
    ulid(
        accepted_ms,
        &std::array::from_fn(|i| mimi_message_id[i + 1]),
    )
}

/// The 80 bits that MIMI references resolve on (draft §Message
/// Identifiers, §Edits), as 16 upper-case Crockford Base32 characters.
///
/// A ULID's random component is exactly its last 16 characters, so the
/// `RefKey` of a derived `senderMsgId` is that suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefKey(String);

impl RefKey {
    /// The key of a MIMI message ID: octets 1 through 10, "the ten octets
    /// that follow the hash algorithm octet". Use it for a `replaces` or
    /// `inReplyTo` reference, and for the alias an edit records.
    pub fn of_message_id(mimi_message_id: &MimiMessageId) -> Self {
        Self(crockford(random_component(mimi_message_id), 16))
    }

    /// The random component of a stored Message's `senderMsgId`.
    ///
    /// Returns `None` unless `sender_msg_id` is a ULID: 26 Crockford
    /// Base32 characters, in either case, whose first character is at
    /// most `7`.
    pub fn from_sender_msg_id(sender_msg_id: &str) -> Option<Self> {
        let bytes = sender_msg_id.as_bytes();
        let valid = bytes.len() == 26
            && bytes[0] <= b'7'
            && bytes
                .iter()
                .all(|b| CROCKFORD.contains(&b.to_ascii_uppercase()));
        valid.then(|| Self(sender_msg_id[10..].to_ascii_uppercase()))
    }

    /// The 16-character key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for RefKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RefKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A [`RefKey`] already belongs to a different Message in the Chat
/// (draft §Security Considerations).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RefKeyCollision;

impl fmt::Display for RefKeyCollision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MIMI message ID collides with a different message in the Chat")
    }
}

impl std::error::Error for RefKeyCollision {}

/// The collision rule (draft §Security Considerations): a converting
/// party "MUST NOT accept a MIMI message whose octets 1 through 10 equal
/// the `senderMsgId` random component or an alias of a different message
/// already stored in the same Chat".
///
/// `owner` is the Message in the Chat whose `senderMsgId` random
/// component or alias already equals the key, if any. `message` is the
/// Message the key would belong to: the id a new Message will get, or for
/// an edit's alias the Message that `replaces` resolved to. On `Err` the
/// caller rejects the MIMI message and keeps the stored one unchanged.
pub fn check_ref_key<I: PartialEq + ?Sized>(
    owner: Option<&I>,
    message: &I,
) -> Result<(), RefKeyCollision> {
    match owner {
        Some(owner) if owner != message => Err(RefKeyCollision),
        _ => Ok(()),
    }
}

/// Whether to record an edit's alias (draft §Security Considerations): a
/// converting party "SHOULD verify that the sender of an edit is the
/// sender of the original message before recording its alias".
pub fn may_record_alias<S: PartialEq + ?Sized>(edit_sender: &S, original_sender: &S) -> bool {
    edit_sender == original_sender
}

/// The JMAP `topicId` of a MIMI `topicId` (draft §Topics): the octets
/// decoded as UTF-8, or `None` (no `topicId` and no `subject`) when they
/// are empty, not valid UTF-8, or longer than 255 octets. The message is
/// still converted.
pub fn topic_id(mimi_topic_id: &[u8]) -> Option<&str> {
    if mimi_topic_id.is_empty() || mimi_topic_id.len() > 255 {
        return None;
    }
    std::str::from_utf8(mimi_topic_id).ok()
}

/// Whether a MIMI reaction part is a media reaction (draft §Reactions),
/// carried in JMAP Chat as a reply containing the media rather than as a
/// Reaction.
///
/// `content_type` is the reaction part's `contentType`. A text reaction
/// (top-level media type `text`, compared case-insensitively per RFC 6838
/// §4.2) is an emoji; any other type, such as an image, sound or video, is
/// media.
pub fn is_media_reaction(content_type: &str) -> bool {
    let media_type = content_type.split(['/', ';']).next().unwrap_or_default();
    !media_type.trim().eq_ignore_ascii_case("text")
}

/// Whether a MIMI message status never sets `readAt` or a ReadDisposition
/// (draft §Message Status): `expired` (3), `hidden` (5) and `error` (6)
/// of draft-mahy-mimi-message-status-01 `baseStatus` "are not read
/// acknowledgements".
pub fn never_sets_read(status: u8) -> bool {
    matches!(status, 3 | 5 | 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn id(hex: &[u8; 64]) -> MimiMessageId {
        let mut out = [0u8; 32];
        let mut i = 0;
        while i < 32 {
            out[i] = (nibble(hex[2 * i]) << 4) | nibble(hex[2 * i + 1]);
            i += 1;
        }
        out
    }

    const fn nibble(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("not lower-case hex"),
        }
    }

    // draft-ietf-mimi-content §Original Message, §Reply, §Edit
    // and §Reaction examples: "message ID = h'...'" header lines.
    const ORIGINAL: MimiMessageId =
        id(b"017ce54837404c3696e0c747b985cb172716d0ed0a3d249ca63ace7d82a096f4");
    const REPLY: MimiMessageId =
        id(b"015354973c2b65ca937bf1e035ae53a5ab80e947afa43d46920d4202e5cc0b27");
    const EDIT: MimiMessageId =
        id(b"014028c0deddbdea56bec26172f6ede953d11024cb82b8192b5e2aea62d7fb47");
    const REACTION: MimiMessageId =
        id(b"0158c4288911e50a8f6be3f47746b6682f10fd91bc8c05557aa589a3157aff68");

    #[test]
    fn sender_msg_id_from_mimi_content_examples() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L118: "the ULID whose
        // 48-bit time component is the hub's acceptance time of the message
        // in milliseconds ... and whose 80-bit random component is octets 1
        // through 10 of its MIMI message ID, counting from 0". Inputs are
        // the message IDs and "timestamp = ..." lines of
        // draft-ietf-mimi-content §Reaction (1644387237728) and
        // §Original Message (1644387225019). Expected ULIDs were computed by an
        // independent Python Crockford Base32 encoder per the ULID spec.
        assert_eq!(
            sender_msg_id(1_644_387_237_728, &REACTION).as_deref(),
            Some("01FVEHVZV0B322H28HWM58YTZ3")
        );
        assert_eq!(
            sender_msg_id(1_644_387_225_019, &ORIGINAL).as_deref(),
            Some("01FVEHVKDVFKJMGDT09GV9DR67")
        );
    }

    #[test]
    fn ulid_bounds_from_ulid_spec() {
        // Oracle: ULID spec (github.com/ulid/spec) §Overflow Errors: "The
        // largest valid ULID encoded in Base32 is
        // 7ZZZZZZZZZZZZZZZZZZZZZZZZZ, which corresponds to an epoch time of
        // 281474976710655 or 2 ^ 48 - 1"; larger values "should be
        // rejected".
        assert_eq!(
            ulid((1 << 48) - 1, &[0xff; 10]).as_deref(),
            Some("7ZZZZZZZZZZZZZZZZZZZZZZZZZ")
        );
        assert_eq!(ulid(1 << 48, &[0; 10]), None);
    }

    #[test]
    fn sender_msg_id_bounds_from_ulid_spec() {
        // Oracle: ULID spec (github.com/ulid/spec) §Overflow Errors: "The
        // largest valid ULID encoded in Base32 is
        // 7ZZZZZZZZZZZZZZZZZZZZZZZZZ, which corresponds to an epoch time of
        // 281474976710655 or 2 ^ 48 - 1"; larger values "should be
        // rejected". Octet 0 (the hash algorithm) does not enter the ULID.
        let mut all_ones = [0xff; 32];
        all_ones[0] = 0;
        assert_eq!(
            sender_msg_id((1 << 48) - 1, &all_ones).as_deref(),
            Some("7ZZZZZZZZZZZZZZZZZZZZZZZZZ")
        );
        assert_eq!(sender_msg_id(1 << 48, &all_ones), None);
        assert_eq!(
            sender_msg_id(0, &[0; 32]).as_deref(),
            Some("00000000000000000000000000")
        );
    }

    #[test]
    fn reaction_in_reply_to_resolves_to_original() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L120: "A converting
        // party resolves it to the Message in the same Chat whose
        // `senderMsgId` random component ... equals octets 1 through 10 of
        // that ID." draft-ietf-mimi-content §Reaction example carries
        // "inReplyTo = h'017ce548...96f4' # Original", the ID of
        // §Original Message. "FKJMGDT09GV9DR67" is octets 1-10 of that ID in
        // Crockford Base32 (independent Python encoder).
        let in_reply_to = id(b"017ce54837404c3696e0c747b985cb172716d0ed0a3d249ca63ace7d82a096f4");
        let key = RefKey::of_message_id(&in_reply_to);
        assert_eq!(key.as_str(), "FKJMGDT09GV9DR67");
        assert_eq!(
            RefKey::from_sender_msg_id("01FVEHVKDVFKJMGDT09GV9DR67"),
            Some(key.clone())
        );
        // L120: "A reference to a message the party has not stored does
        // not resolve": another message's key differs.
        assert_ne!(RefKey::of_message_id(&REPLY), key);
    }

    #[test]
    fn ref_key_ignores_hash_algorithm_octet() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L118: "octets 1
        // through 10 of its MIMI message ID, counting from 0: the ten
        // octets that follow the hash algorithm octet". Only octets 1-10
        // count; octet 0 and octets 11-31 do not.
        let mut other = ORIGINAL;
        other[0] = 0x02;
        other[11] ^= 0xff;
        other[31] ^= 0xff;
        assert_eq!(
            RefKey::of_message_id(&other),
            RefKey::of_message_id(&ORIGINAL)
        );
        other[10] ^= 0x01;
        assert_ne!(
            RefKey::of_message_id(&other),
            RefKey::of_message_id(&ORIGINAL)
        );
    }

    #[test]
    fn from_sender_msg_id_accepts_only_ulids() {
        // Oracle: ULID spec §Encoding: "Crockford's Base32 is used ... This
        // alphabet excludes the letters I, L, O, and U"; §Canonical String
        // Representation: 26 characters; the spec's "case insensitive"
        // property; §Overflow Errors: the first character is at most 7.
        assert_eq!(
            RefKey::from_sender_msg_id("01fvehvkdvfkjmgdt09gv9dr67")
                .unwrap()
                .as_str(),
            "FKJMGDT09GV9DR67"
        );
        for bad in [
            "01FVEHVKDVFKJMGDT09GV9DR6",
            "01FVEHVKDVFKJMGDT09GV9DR677",
            "81FVEHVKDVFKJMGDT09GV9DR67",
            "01FVEHVKDVFKJMGDT09GV9DR6U",
            "01FVEHVKDVFKJMGDT09GV9DR6I",
            "01FVEHVKDVFKJMGDT09GV9DR6é",
        ] {
            assert_eq!(RefKey::from_sender_msg_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn edit_alias_reaches_original_message() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L128: "The converting
        // party records octets 1 through 10 of each edit's own MIMI message
        // ID as an alias of that message, so a MIMI reply or reaction that
        // names a later version refers to the message as a whole."
        // draft-ietf-mimi-content §Edit example: own ID
        // h'014028c0...fb47', "replaces = h'01535497...0b27' # Reply".
        // Expected keys are octets 1-10 in Crockford Base32 (independent
        // Python encoder).
        let replaces = id(b"015354973c2b65ca937bf1e035ae53a5ab80e947afa43d46920d4202e5cc0b27");
        assert_eq!(
            RefKey::of_message_id(&replaces).as_str(),
            "ADA9EF1BCQ596YZH"
        );
        let alias = RefKey::of_message_id(&EDIT);
        assert_eq!(alias.as_str(), "80MC1QPXQQN5DFP2");
        // The alias names the edited Message ("reply" below); recording it
        // when it already names that Message is not a collision.
        assert_eq!(check_ref_key(None, "reply"), Ok(()));
        assert_eq!(check_ref_key(Some("reply"), "reply"), Ok(()));
    }

    #[test]
    fn collision_with_different_message_rejected() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L590: "A converting
        // party MUST NOT accept a MIMI message whose octets 1 through 10
        // equal the `senderMsgId` random component or an alias of a
        // different message already stored in the same Chat; it rejects
        // the message rather than overwriting or merging the stored one".
        assert_eq!(check_ref_key(Some("original"), "new"), Err(RefKeyCollision));
        assert_eq!(
            check_ref_key(Some("original"), "reply"),
            Err(RefKeyCollision)
        );
        assert_eq!(check_ref_key::<str>(None, "new"), Ok(()));
    }

    #[test]
    fn alias_only_from_original_sender() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L592: "A converting
        // party SHOULD verify that the sender of an edit is the sender of
        // the original message before recording its alias, so that a third
        // party cannot attach an alias to ... someone else's message."
        // Sender URIs from draft-ietf-mimi-content examples (extension 1).
        let alice = "mimi://example.com/u/alice-smith";
        let cathy = "mimi://example.com/u/cathy-washington";
        assert!(may_record_alias(alice, alice));
        assert!(!may_record_alias(cathy, alice));
    }

    #[test]
    fn topic_id_utf8_mapping() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L132: "the JMAP
        // `topicId` is the MIMI `topicId` octets decoded as UTF-8 ... A
        // MIMI `topicId` that is empty, is not valid UTF-8, or exceeds 255
        // octets maps to no `topicId` and no `subject`". Inputs:
        // draft-ietf-mimi-content §Conferencing example "'Foo 118', #
        // topicId" and the empty h'' of the other examples.
        assert_eq!(topic_id(b"Foo 118"), Some("Foo 118"));
        assert_eq!("Foo 118".as_bytes(), b"Foo 118");
        assert_eq!(topic_id(b""), None);
        assert_eq!(topic_id(&[0xff, 0xfe]), None);
        assert_eq!(topic_id(&[b'a'; 255]), Some("a".repeat(255).as_str()));
        assert_eq!(topic_id(&[b'a'; 256]), None);
        // U+2665 is three octets (0xe299a5, draft-ietf-mimi-content
        // §Reaction): 85 of them are 255 octets, 86 are 258.
        assert!(topic_id("\u{2665}".repeat(85).as_bytes()).is_some());
        assert!(topic_id("\u{2665}".repeat(86).as_bytes()).is_none());
    }

    #[test]
    fn media_reaction_is_not_text() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L136: "A MIMI media
        // reaction (a reaction that is not an emoji) is carried in JMAP
        // Chat as a reply containing the media". draft-ietf-mimi-content
        // §Reaction example: "text/plain;charset=utf-8" with U+2665;
        // §Reaction: "a reaction could be another media type (ex: image,
        // audio, or video)". RFC 6838 §4.2: type names are
        // case-insensitive.
        assert!(!is_media_reaction("text/plain;charset=utf-8"));
        assert!(!is_media_reaction("TEXT/plain"));
        for media in ["image/png", "audio/ogg", "video/mp4"] {
            assert!(is_media_reaction(media), "{media}");
        }
    }

    #[test]
    fn non_read_statuses() {
        // Oracle: draft-atwood-jmap-chat-mimi-00.md L140: "MIMI message
        // statuses `expired`, `hidden`, and `error` {{MIMI-MESSAGE-STATUS}}
        // are not read acknowledgements". draft-mahy-mimi-message-status-01
        // §4 CDDL: "unread: 0, delivered: 1, read: 2, expired: 3, deleted:
        // 4, hidden: 5, error: 6".
        for status in [3, 5, 6] {
            assert!(never_sets_read(status), "{status}");
        }
        for status in [0, 1, 2, 4, 7, 255] {
            assert!(!never_sets_read(status), "{status}");
        }
    }
}
