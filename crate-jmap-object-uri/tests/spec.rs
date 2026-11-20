//! Tests against draft-atwood-jmap-object-uri-00 and the drafts that use
//! the `jmap` scheme. Expected values come from the drafts' text, examples
//! and IANA tables, copied by hand; nothing is derived from the code under
//! test.

use jmap_object_uri::{
    JmapUri, JmapUriError, ResolveError, SCHEME, URI_TYPES, is_jmap_uri, resolve, resolve_type,
    uri_type, uri_type_for_data_type,
};
use jmap_types::Id;
use jmap_types::data_types::data_type;
use serde_json::{Value, json};

fn parse(s: &str) -> Result<JmapUri, JmapUriError> {
    s.parse()
}

// ---------------------------------------------------------------------------
// Examples from the drafts
// ---------------------------------------------------------------------------

/// Every concrete example URI in the drafts, with its parts as the
/// surrounding text describes them.
const DRAFT_EXAMPLES: &[(&str, &str, &str, &str)] = &[
    // object-uri-00 §JMAP Object URIs: "refers to the CalendarEvent with id
    // `E6B2` in account `A13824`".
    (
        "jmap:calendarevent:A13824:E6B2",
        "calendarevent",
        "A13824",
        "E6B2",
    ),
    // graph-projection-00 §objectUri and §Example.
    ("jmap:email:A1:M42", "email", "A1", "M42"),
    // graph-00 §Links Outside the Graph.
    ("jmap:graphnode:A2:N9", "graphnode", "A2", "N9"),
];

#[test]
fn draft_examples_parse_into_their_parts() {
    for &(s, ty, account, object) in DRAFT_EXAMPLES {
        let uri = parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(uri.data_type(), ty, "{s}");
        assert_eq!(uri.account_id().as_ref(), account, "{s}");
        assert_eq!(uri.object_id().as_ref(), object, "{s}");
        assert_eq!(uri.to_string(), s, "{s} formats back unchanged");
    }
}

/// The template forms `jmap:calendarevent:<accountId>:<eventId>`
/// (chat-calendars-00 §MessageAction format), `jmap:principal:…`
/// (same section) and `jmap:task:<accountId>:<taskId>`
/// (chat-tasks-00 §MessageAction format), filled with ids, and the data type each
/// text says the URI references.
#[test]
fn draft_template_forms_resolve_to_the_named_data_type() {
    let cases = [
        (
            "jmap:calendarevent:A1:E1",
            "CalendarEvent",
            "urn:ietf:params:jmap:calendars",
        ),
        (
            "jmap:principal:A1:P1",
            "Principal",
            "urn:ietf:params:jmap:principals",
        ),
        ("jmap:task:A1:T9", "Task", "urn:ietf:params:jmap:tasks"),
        ("jmap:email:A1:M42", "Email", "urn:ietf:params:jmap:mail"),
        (
            "jmap:graphnode:A2:N9",
            "GraphNode",
            "urn:ietf:params:jmap:graph",
        ),
    ];
    for (s, dt, cap) in cases {
        let found = resolve_type(&parse(s).unwrap()).unwrap();
        assert_eq!(found.data_type.type_name, dt, "{s}");
        assert_eq!(found.get_method(), format!("{dt}/get"), "{s}");
        assert_eq!(found.capability(), cap, "{s}");
    }
}

// ---------------------------------------------------------------------------
// Syntax (§JMAP Object URIs ABNF)
// ---------------------------------------------------------------------------

/// Expects the error a malformed input fails with.
type Check = fn(&JmapUriError) -> bool;

#[test]
fn malformed_inputs_are_rejected_with_the_failing_component() {
    use JmapUriError::*;
    let long = "a".repeat(256);
    let cases: Vec<(String, Check)> = vec![
        ("".into(), |e| matches!(e, NotJmapScheme)),
        ("https://example.com/x".into(), |e| {
            matches!(e, NotJmapScheme)
        }),
        ("urn:ietf:params:jmap:mail".into(), |e| {
            matches!(e, NotJmapScheme)
        }),
        ("jmapx:email:A1:M1".into(), |e| matches!(e, NotJmapScheme)),
        ("jmap:".into(), |e| matches!(e, InvalidDataType)),
        // type-name = 1*( %x61-7A / DIGIT ): lowercase only, no '-', '_'.
        ("jmap::A1:M1".into(), |e| matches!(e, InvalidDataType)),
        ("jmap:Email:A1:M1".into(), |e| matches!(e, InvalidDataType)),
        ("jmap:EMAIL:A1:M1".into(), |e| matches!(e, InvalidDataType)),
        ("jmap:e-mail:A1:M1".into(), |e| matches!(e, InvalidDataType)),
        ("jmap:e_mail:A1:M1".into(), |e| matches!(e, InvalidDataType)),
        ("jmap:%65mail:A1:M1".into(), |e| {
            matches!(e, InvalidDataType)
        }),
        // §Other Forms: `//` authority forms never match jmap-object-uri.
        ("jmap://example.com/A1/M1".into(), |e| {
            matches!(e, InvalidDataType)
        }),
        ("jmap:email".into(), |e| matches!(e, InvalidAccountId(None))),
        ("jmap:email::M1".into(), |e| {
            matches!(e, InvalidAccountId(Some(_)))
        }),
        // §Equivalence: no component is ever percent-encoded.
        ("jmap:email:A%31:M1".into(), |e| {
            matches!(e, InvalidAccountId(Some(_)))
        }),
        ("jmap:email:A.1:M1".into(), |e| {
            matches!(e, InvalidAccountId(Some(_)))
        }),
        (format!("jmap:email:{long}:M1"), |e| {
            matches!(e, InvalidAccountId(Some(_)))
        }),
        ("jmap:email:A1".into(), |e| matches!(e, MissingObjectId)),
        ("jmap:email:A1:".into(), |e| matches!(e, InvalidObjectId(_))),
        ("jmap:email:A1:M%34%32".into(), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        ("jmap:email:A1:M 42".into(), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        ("jmap:email:A1:M42:extra".into(), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        ("jmap:email:A1:M42#frag".into(), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        ("jmap:email:A1:M42?q=1".into(), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        ("jmap:email:A1:M42/".into(), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        ("jmap:email:A1:M\u{e9}".into(), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        (format!("jmap:email:A1:{long}"), |e| {
            matches!(e, InvalidObjectId(_))
        }),
        (" jmap:email:A1:M1".into(), |e| matches!(e, NotJmapScheme)),
    ];
    for (s, expected) in cases {
        match parse(&s) {
            Ok(u) => panic!("{s:?} parsed as {u}"),
            Err(e) => assert!(expected(&e), "{s:?}: unexpected error {e:?}"),
        }
    }
}

#[test]
fn ids_of_255_octets_and_every_id_char_are_accepted() {
    // account-id = 1*255id-char; id-char = ALPHA / DIGIT / "-" / "_".
    let max = "Z".repeat(255);
    let s = format!("jmap:email:{max}:{max}");
    assert_eq!(parse(&s).unwrap().to_string(), s);
    let all = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let s = format!("jmap:email:{all}:{all}");
    assert_eq!(parse(&s).unwrap().to_string(), s);
    // type-name allows digits anywhere, and needs no registration to parse.
    assert_eq!(parse("jmap:0x9:-:_").unwrap().data_type(), "0x9");
}

// ---------------------------------------------------------------------------
// Equivalence (§Equivalence)
// ---------------------------------------------------------------------------

#[test]
fn scheme_is_case_insensitive_and_formats_lowercase() {
    let canonical = parse("jmap:email:A1:M42").unwrap();
    for s in [
        "JMAP:email:A1:M42",
        "Jmap:email:A1:M42",
        "jMaP:email:A1:M42",
    ] {
        let uri = parse(s).unwrap();
        assert_eq!(uri, canonical, "{s}");
        assert_eq!(uri.to_string(), "jmap:email:A1:M42");
    }
}

#[test]
fn ids_compare_case_sensitively() {
    let a = parse("jmap:email:A1:M42").unwrap();
    assert_ne!(a, parse("jmap:email:a1:M42").unwrap());
    assert_ne!(a, parse("jmap:email:A1:m42").unwrap());
}

#[test]
fn new_rejects_parts_that_would_not_round_trip() {
    // An unvalidated `Id::from` could smuggle a `:` into the URI.
    assert!(matches!(
        JmapUri::new("email", Id::from("A:1"), Id::from("M1")),
        Err(JmapUriError::InvalidAccountId(Some(_)))
    ));
    assert!(matches!(
        JmapUri::new("email", Id::from("A1"), Id::from("M:1")),
        Err(JmapUriError::InvalidObjectId(_))
    ));
    assert!(matches!(
        JmapUri::new("Email", Id::from("A1"), Id::from("M1")),
        Err(JmapUriError::InvalidDataType)
    ));
}

// ---------------------------------------------------------------------------
// Scheme (§Other Forms; chat-federation-00 §User References in Peer Methods)
// ---------------------------------------------------------------------------

#[test]
fn scheme_test_matches_every_jmap_form() {
    assert_eq!(SCHEME, "jmap");
    for s in [
        "jmap:task:A1:T9",
        "JMAP:task:A1:T9",
        "jmap://peer.example/reserved",
        "jmap:",
        "jmap:not a valid object uri",
    ] {
        assert!(is_jmap_uri(s), "{s}");
    }
    for s in [
        "",
        "jmap",
        "https://tracker.example/T-1",
        "urn:ietf:params:jmap:chat:cap:task",
        "xjmap:task:A1:T9",
        "lightning:alice@alice.example",
    ] {
        assert!(!is_jmap_uri(s), "{s}");
    }
}

// ---------------------------------------------------------------------------
// The "JMAP Object URI Types" registry
// ---------------------------------------------------------------------------

/// The IANA tables of the drafts that register type names, copied by hand.
const DRAFT_REGISTRATIONS: &[(&str, &str, &str)] = &[
    // draft-atwood-jmap-chat-00 §JMAP Object URI Type Registrations
    ("chatcontact", "ChatContact", "draft-atwood-jmap-chat-00"),
    ("chat", "Chat", "draft-atwood-jmap-chat-00"),
    ("message", "Message", "draft-atwood-jmap-chat-00"),
    ("space", "Space", "draft-atwood-jmap-chat-00"),
    ("spaceinvite", "SpaceInvite", "draft-atwood-jmap-chat-00"),
    ("customemoji", "CustomEmoji", "draft-atwood-jmap-chat-00"),
    ("spaceban", "SpaceBan", "draft-atwood-jmap-chat-00"),
    ("chatban", "ChatBan", "draft-atwood-jmap-chat-00"),
    ("readposition", "ReadPosition", "draft-atwood-jmap-chat-00"),
    (
        "presencestatus",
        "PresenceStatus",
        "draft-atwood-jmap-chat-00",
    ),
    // draft-atwood-jmap-chat-calendars-00
    (
        "calendar",
        "Calendar",
        "draft-atwood-jmap-chat-calendars-00",
    ),
    (
        "calendarevent",
        "CalendarEvent",
        "draft-atwood-jmap-chat-calendars-00",
    ),
    (
        "principal",
        "Principal",
        "draft-atwood-jmap-chat-calendars-00",
    ),
    // draft-atwood-jmap-chat-tasks-00
    ("tasklist", "TaskList", "draft-atwood-jmap-chat-tasks-00"),
    ("task", "Task", "draft-atwood-jmap-chat-tasks-00"),
    // draft-atwood-jmap-chat-filenode-00
    ("filenode", "FileNode", "draft-atwood-jmap-chat-filenode-00"),
    // draft-atwood-jmap-graph-00
    ("graph", "Graph", "draft-atwood-jmap-graph-00"),
    ("graphnode", "GraphNode", "draft-atwood-jmap-graph-00"),
    ("graphedge", "GraphEdge", "draft-atwood-jmap-graph-00"),
    // draft-atwood-jmap-graph-projection-00
    (
        "mailbox",
        "Mailbox",
        "draft-atwood-jmap-graph-projection-00",
    ),
    ("thread", "Thread", "draft-atwood-jmap-graph-projection-00"),
    ("email", "Email", "draft-atwood-jmap-graph-projection-00"),
    (
        "identity",
        "Identity",
        "draft-atwood-jmap-graph-projection-00",
    ),
    (
        "emailsubmission",
        "EmailSubmission",
        "draft-atwood-jmap-graph-projection-00",
    ),
    (
        "addressbook",
        "AddressBook",
        "draft-atwood-jmap-graph-projection-00",
    ),
    (
        "contactcard",
        "ContactCard",
        "draft-atwood-jmap-graph-projection-00",
    ),
    // draft-atwood-jmap-scene-00
    ("sceneregion", "SceneRegion", "draft-atwood-jmap-scene-00"),
    ("sceneobject", "SceneObject", "draft-atwood-jmap-scene-00"),
    ("sceneavatar", "SceneAvatar", "draft-atwood-jmap-scene-00"),
    ("sceneasset", "SceneAsset", "draft-atwood-jmap-scene-00"),
    // draft-atwood-jmap-vtc-00
    ("vtccall", "VTCCall", "draft-atwood-jmap-vtc-00"),
    (
        "vtcparticipant",
        "VTCParticipant",
        "draft-atwood-jmap-vtc-00",
    ),
    ("vtcrecording", "VTCRecording", "draft-atwood-jmap-vtc-00"),
    ("vtclivestream", "VTCLivestream", "draft-atwood-jmap-vtc-00"),
];

#[test]
fn registry_is_exactly_the_drafts_registrations() {
    assert_eq!(URI_TYPES.len(), DRAFT_REGISTRATIONS.len());
    for &(name, dt, reference) in DRAFT_REGISTRATIONS {
        let e = uri_type(name).unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(
            (e.type_name, e.data_type, e.reference),
            (name, dt, reference)
        );
        assert_eq!(uri_type_for_data_type(dt), Some(e), "{dt}");
    }
}

#[test]
fn registry_rows_satisfy_the_expert_review_rules() {
    // §JMAP Object URI Types Registry: the type name matches `type-name`,
    // is the lowercased data type name, and is unique; the data type is in
    // the "JMAP Data Types" registry.
    for e in URI_TYPES {
        assert!(
            e.type_name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
            "{}",
            e.type_name
        );
        assert_eq!(e.type_name, e.data_type.to_ascii_lowercase());
        assert!(
            data_type(e.data_type).is_some(),
            "{} not in DATA_TYPES",
            e.data_type
        );
    }
    // Sorted and unique, which `uri_type`'s binary search relies on.
    assert!(
        URI_TYPES
            .windows(2)
            .all(|w| w[0].type_name < w[1].type_name)
    );
}

#[test]
fn unregistered_type_names_do_not_resolve() {
    for s in [
        "jmap:widget:A1:W1",
        "jmap:searchsnippet:A1:S1",
        "jmap:core:A1:C1",
    ] {
        assert!(
            matches!(resolve_type(&parse(s).unwrap()), Err(ResolveError::UnknownType { ref type_name }) if s.contains(type_name.as_str())),
            "{s}"
        );
    }
}

// ---------------------------------------------------------------------------
// Resolution (§Interpretation)
// ---------------------------------------------------------------------------

/// A Session in the shape of the RFC 8620 §2.1 example, trimmed to the
/// members resolution reads, with a Calendars account.
fn session() -> Value {
    json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": { "maxSizeUpload": 50000000 },
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:calendars": {}
        },
        "accounts": {
            "A13824": {
                "name": "john@example.com",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:mail": {},
                    "urn:ietf:params:jmap:calendars": {}
                }
            },
            "A97813": {
                "name": "jane@example.com",
                "isPersonal": false,
                "isReadOnly": true,
                "accountCapabilities": { "urn:ietf:params:jmap:mail": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:mail": "A13824" },
        "username": "john@example.com",
        "apiUrl": "https://jmap.example.com/api/",
        "downloadUrl": "https://jmap.example.com/download/{accountId}/{blobId}/{name}?accept={type}",
        "uploadUrl": "https://jmap.example.com/upload/{accountId}/",
        "eventSourceUrl": "https://jmap.example.com/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
        "state": "75128aab4b1b"
    })
}

#[test]
fn resolves_the_draft_example_against_its_session() {
    let uri = parse("jmap:calendarevent:A13824:E6B2").unwrap();
    let found = resolve(&uri, &session()).unwrap();
    assert_eq!(found.uri_type.type_name, "calendarevent");
    assert_eq!(found.data_type.type_name, "CalendarEvent");
    assert_eq!(found.get_method(), "CalendarEvent/get");
    assert_eq!(found.capability(), "urn:ietf:params:jmap:calendars");
    assert_eq!(found.account_id.as_ref(), "A13824");
    assert_eq!(found.object_id.as_ref(), "E6B2");
    assert_eq!(
        found.get_arguments(),
        json!({ "accountId": "A13824", "ids": ["E6B2"] })
    );
}

#[test]
fn resolution_failures_name_what_the_session_lacks() {
    let s = session();
    let r = |u: &str| resolve(&parse(u).unwrap(), &s);
    assert_eq!(
        r("jmap:task:A13824:T1"),
        Err(ResolveError::CapabilityNotSupported {
            capability: "urn:ietf:params:jmap:tasks"
        })
    );
    assert_eq!(
        r("jmap:email:A00000:M1"),
        Err(ResolveError::UnknownAccount {
            account_id: Id::from("A00000")
        })
    );
    assert_eq!(
        r("jmap:calendarevent:A97813:E1"),
        Err(ResolveError::AccountLacksCapability {
            account_id: Id::from("A97813"),
            capability: "urn:ietf:params:jmap:calendars"
        })
    );
    assert_eq!(
        r("jmap:widget:A13824:W1"),
        Err(ResolveError::UnknownType {
            type_name: "widget".into()
        })
    );
    // Account ids are case-sensitive: "a13824" is not "A13824".
    assert!(matches!(
        r("jmap:email:a13824:M1"),
        Err(ResolveError::UnknownAccount { .. })
    ));
    // Not a Session object at all.
    assert!(matches!(
        resolve(&parse("jmap:email:A13824:M1").unwrap(), &json!([])),
        Err(ResolveError::CapabilityNotSupported { .. })
    ));
}

#[test]
fn resolve_error_messages() {
    assert_eq!(
        ResolveError::UnknownAccount {
            account_id: Id::from("A1")
        }
        .to_string(),
        "session has no account A1"
    );
    assert_eq!(
        ResolveError::UnknownType {
            type_name: "widget".into()
        }
        .to_string(),
        "jmap: URI type name \"widget\" is not registered"
    );
}

// ---------------------------------------------------------------------------
// Property tests (no proptest in the workspace: a seeded xorshift drives
// them, so runs are reproducible)
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn string(&mut self, alphabet: &[u8], min: usize, max: usize) -> String {
        let len = min + self.below(max - min + 1);
        (0..len)
            .map(|_| alphabet[self.below(alphabet.len())] as char)
            .collect()
    }
}

const TYPE_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
const ID_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The ABNF of §JMAP Object URIs, written out directly as an independent
/// recognizer.
fn abnf_matches(s: &str) -> bool {
    let Some(rest) = s
        .get(..5)
        .filter(|p| p.eq_ignore_ascii_case("jmap:"))
        .map(|_| &s[5..])
    else {
        return false;
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let [ty, account, object] = parts[..] else {
        return false;
    };
    let ok = |p: &str, chars: &[u8], max: usize| {
        !p.is_empty() && p.len() <= max && p.bytes().all(|b| chars.contains(&b))
    };
    ok(ty, TYPE_CHARS, usize::MAX) && ok(account, ID_CHARS, 255) && ok(object, ID_CHARS, 255)
}

#[test]
fn valid_uris_round_trip_through_parse_and_format() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..2000 {
        let ty = rng.string(TYPE_CHARS, 1, 20);
        let account = rng.string(ID_CHARS, 1, 255);
        let object = rng.string(ID_CHARS, 1, 255);
        let s = format!("jmap:{ty}:{account}:{object}");
        let uri = parse(&s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(uri.to_string(), s);
        assert_eq!(
            (
                uri.data_type(),
                uri.account_id().as_ref(),
                uri.object_id().as_ref()
            ),
            (ty.as_str(), account.as_str(), object.as_str())
        );
        let built = JmapUri::new(ty, Id::from(account), Id::from(object)).unwrap();
        assert_eq!(built, uri);
        assert_eq!(parse(&built.to_string()).unwrap(), built);
    }
}

#[test]
fn parser_accepts_exactly_what_the_abnf_matches() {
    // Near-miss strings: a valid shape with characters drawn from a wider
    // alphabet, so separators, percent signs, uppercase type names and
    // reserved forms all turn up.
    const NOISY: &[u8] = b"aZ09-_:%/#?.J";
    let mut rng = Rng(0x0123_4567_89ab_cdef);
    let mut accepted = 0;
    for i in 0..20000 {
        let s = match i % 3 {
            0 => format!(
                "{}:{}:{}:{}",
                ["jmap", "JMAP", "jmap/", "jmp"][rng.below(4)],
                rng.string(NOISY, 0, 4),
                rng.string(NOISY, 0, 4),
                rng.string(NOISY, 0, 4)
            ),
            1 => format!("jmap:{}", rng.string(NOISY, 0, 12)),
            _ => {
                let len = [255, 256][rng.below(2)];
                format!("jmap:t:{}:{}", rng.string(ID_CHARS, len, len), "o")
            }
        };
        let expected = abnf_matches(&s);
        assert_eq!(parse(&s).is_ok(), expected, "{s:?}");
        if expected {
            accepted += 1;
            // The scheme is case-insensitive; every other part is kept.
            assert_eq!(parse(&s).unwrap().to_string(), format!("jmap{}", &s[4..]));
        }
    }
    assert!(accepted > 100, "generator too sparse: {accepted}");
}
