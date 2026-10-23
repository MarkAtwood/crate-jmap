//! RSVP helper tests (draft-atwood-jmap-chat-calendars-00 §RSVP Handling).
//!
//! Oracles: quoted draft/RFC text and hand-written JSON copied from the
//! published examples in RFC 8984 and draft-ietf-jmap-calendars-26.

use jmap_chat_client::{RsvpStatus, own_participant_key, rsvp_patch};
use serde_json::{Value, json};

fn patch_json(key: &str, status: RsvpStatus) -> Value {
    serde_json::to_value(rsvp_patch(key, status)).expect("PatchObject serializes")
}

/// draft-atwood-jmap-chat-calendars-00 L192: "Compose a `CalendarEvent/set`
/// request patching the `participants/{participantKey}/participationStatus`
/// field". RFC 8984 §6 example (rfc8984.txt L2705-2707) uses that exact
/// patch shape:
/// `"participants/dG9tQGZvb2Jhci5xlLmNvbQ/participationStatus": "declined"`.
#[test]
fn patch_matches_rfc8984_example() {
    assert_eq!(
        patch_json("dG9tQGZvb2Jhci5xlLmNvbQ", RsvpStatus::Declined),
        json!({ "participants/dG9tQGZvb2Jhci5xlLmNvbQ/participationStatus": "declined" })
    );
}

/// draft-atwood-jmap-chat-calendars-00 L192: "to one of `\"accepted\"`,
/// `\"declined\"`, `\"tentative\"`, or `\"delegated\"`"; RFC 8984 §4.4.6
/// defines these `participationStatus` values.
#[test]
fn every_rsvp_status_uses_rfc8984_value() {
    for (status, wire) in [
        (RsvpStatus::Accepted, "accepted"),
        (RsvpStatus::Declined, "declined"),
        (RsvpStatus::Tentative, "tentative"),
        (RsvpStatus::Delegated, "delegated"),
    ] {
        assert_eq!(
            patch_json("p1", status),
            json!({ "participants/p1/participationStatus": wire })
        );
    }
}

/// RFC 8620 §5.3: "The keys are a path in JSON Pointer format"; RFC 6901
/// §3: "'~' needs to be encoded as '~0' and '/' needs to
/// be encoded as '~1'".
#[test]
fn patch_escapes_json_pointer_characters() {
    assert_eq!(
        patch_json("a/b~c", RsvpStatus::Accepted),
        json!({ "participants/a~1b~0c/participationStatus": "accepted" })
    );
}

/// Participants of the draft-ietf-jmap-calendars-26 CalendarEvent example
/// (L1750-1773).
fn draft_participants() -> Value {
    json!({
        "dG9tQGZvb2Jhci5xlLmNvbQ": {
            "@type": "Participant",
            "name": "Tom",
            "email": "tom@foobar.example.com",
            "calendarAddress": "mailto:6489-4f14-a57f-c1@calendar.example.com",
            "participationStatus": "accepted",
            "roles": { "attendee": true }
        },
        "em9lQGZvb2GFtcGxlLmNvbQ": {
            "@type": "Participant",
            "name": "Zoe",
            "email": "zoe@foobar.example.com",
            "calendarAddress": "mailto:zoe@foobar.example.com",
            "participationStatus": "accepted",
            "roles": { "owner": true, "attendee": true, "chair": true }
        }
    })
}

/// draft-atwood-jmap-chat-calendars-00 L192: "`{participantKey}` is the map
/// key of the current user's participant entry, which the client MUST look
/// up by matching its own identity within the participants map".
/// draft-ietf-jmap-calendars-26 §3: "A participant in an event corresponds
/// to a ParticipantIdentity if the \"calendarAddress\" property of the
/// participant is equivalent to the \"calendarAddress\" property of the
/// identity".
#[test]
fn finds_own_key_by_calendar_address() {
    let participants = draft_participants();
    assert_eq!(
        own_participant_key(&participants, ["mailto:zoe@foobar.example.com"]),
        Some("em9lQGZvb2GFtcGxlLmNvbQ")
    );
    assert_eq!(
        own_participant_key(
            &participants,
            [
                "mailto:other@example.com",
                "mailto:6489-4f14-a57f-c1@calendar.example.com",
            ]
        ),
        Some("dG9tQGZvb2Jhci5xlLmNvbQ")
    );
}

/// No identity matches, or `participants` is absent (`null`): no key, so the
/// client has nothing to patch.
#[test]
fn no_key_without_a_matching_identity() {
    let participants = draft_participants();
    assert_eq!(
        own_participant_key(&participants, ["mailto:bob@example.com"]),
        None
    );
    assert_eq!(
        own_participant_key(&participants, Vec::<String>::new()),
        None
    );
    assert_eq!(
        own_participant_key(&Value::Null, ["mailto:zoe@foobar.example.com"]),
        None
    );
}

/// draft-ietf-jmap-calendars-26 §3: equivalence is "after syntax-based
/// normalisation, as per Section 6.2.2 of [RFC3986]". RFC 3986 §6.2.2: "an
/// application using this approach could reasonably consider the following
/// two URIs equivalent: example://a/b/c/%7Bfoo%7D
/// eXAMPLE://a/./b/../b/%63/%7bfoo%7d".
#[test]
fn matches_rfc3986_syntax_based_equivalent() {
    let participants = json!({ "p1": { "calendarAddress": "example://a/b/c/%7Bfoo%7D" } });
    assert_eq!(
        own_participant_key(&participants, ["eXAMPLE://a/./b/../b/%63/%7bfoo%7d"]),
        Some("p1")
    );
}

/// RFC 3986 §6.2.2.1: "the URI <HTTP://www.EXAMPLE.com/> is equivalent to
/// <http://www.example.com/>".
#[test]
fn matches_rfc3986_case_equivalent() {
    let participants = json!({ "p1": { "calendarAddress": "http://www.example.com/" } });
    assert_eq!(
        own_participant_key(&participants, ["HTTP://www.EXAMPLE.com/"]),
        Some("p1")
    );
}

/// RFC 3986 §6.2.2.1: "The other generic syntax components are assumed to
/// be case-sensitive unless specifically defined otherwise by the scheme";
/// syntax-based normalisation leaves the mailto path's case alone.
#[test]
fn path_case_is_not_normalised() {
    let participants = draft_participants();
    assert_eq!(
        own_participant_key(&participants, ["mailto:Zoe@foobar.example.com"]),
        None
    );
}
