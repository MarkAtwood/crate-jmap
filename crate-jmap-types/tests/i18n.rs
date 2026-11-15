//! Localized display text (draft-atwood-jmap-updates-00 §3–§4).
//!
//! Oracles: RFC 5646 Appendix A example tags, the RFC 4647 §3.4 Lookup
//! truncation example, and the draft's text and Appendix A examples at
//! jmap-chat-spec 2b85363. Expected values are written out by hand from
//! those texts, not computed by the code under test.

use jmap_types::i18n::{
    I18nAccountCapability, I18nCapability, JMAP_I18N_URI, LanguageRange, LanguageTag,
    LanguageTagError, LocalizableProperty, Localizations, check_localizations, localized_view,
    lookup, parse_locale, prune_stale_localizations,
};
use jmap_types::{PatchObject, SetErrorType};
use serde_json::{Value, json};

/// The localizable properties of AddressBook, Principal and Calendar
/// (draft §5.3–§5.5): `name` (String) and `description` (String|null).
const NAME_DESC: &[LocalizableProperty] = &[
    LocalizableProperty::new("name", false),
    LocalizableProperty::new("description", true),
];

fn tag(s: &str) -> LanguageTag {
    s.parse().unwrap()
}

fn ranges(rs: &[&str]) -> Vec<LanguageRange> {
    rs.iter().map(|r| r.parse().unwrap()).collect()
}

fn tags(ts: &[&str]) -> Vec<LanguageTag> {
    ts.iter().map(|t| tag(t)).collect()
}

fn chosen(rs: &[&str], ts: &[&str]) -> Option<String> {
    let ts = tags(ts);
    lookup(&ranges(rs), &ts).map(|t| t.as_str().to_owned())
}

// --- capability --------------------------------------------------------

/// draft §3: the Session value "is an empty object", and the account
/// value "is also an empty object".
#[test]
fn capability_values_match_the_session_excerpt() {
    assert_eq!(JMAP_I18N_URI, "urn:ietf:params:jmap:i18n");
    assert_eq!(
        serde_json::to_value(I18nCapability::default()).unwrap(),
        json!({})
    );
    assert_eq!(
        serde_json::to_value(I18nAccountCapability::default()).unwrap(),
        json!({})
    );
    // Unknown members round-trip (workspace extras policy).
    let acct: I18nAccountCapability = serde_json::from_value(json!({ "x-site": 1 })).unwrap();
    assert_eq!(serde_json::to_value(&acct).unwrap(), json!({ "x-site": 1 }));
}

// --- RFC 5646 well-formedness ------------------------------------------

/// RFC 5646 Appendix A, "Some Valid (and Well-Formed) Tags".
#[test]
fn rfc5646_appendix_a_valid_tags_parse() {
    for t in [
        "de",
        "fr",
        "ja",
        "i-enochian",
        "zh-Hant",
        "zh-Hans",
        "sr-Cyrl",
        "sr-Latn",
        "zh-cmn-Hans-CN",
        "cmn-Hans-CN",
        "zh-yue-HK",
        "yue-HK",
        "zh-Hans-CN",
        "sr-Latn-RS",
        "sl-rozaj",
        "sl-rozaj-biske",
        "sl-nedis",
        "de-CH-1901",
        "sl-IT-nedis",
        "hy-Latn-IT-arevela",
        "de-DE",
        "en-US",
        "es-419",
        "de-CH-x-phonebk",
        "az-Arab-x-AZE-derbend",
        "x-whatever",
        "qaa-Qaaa-QM-x-southern",
        "de-Qaaa",
        "sr-Latn-QM",
        "sr-Qaaa-RS",
        "en-US-u-islamcal",
        "zh-CN-a-myext-x-private",
        "en-a-myext-b-another",
    ] {
        assert!(LanguageTag::parse(t).is_ok(), "{t} should be well-formed");
    }
}

/// RFC 5646 Appendix A, "Some Invalid Tags", the ones that are also
/// ill-formed: "de-419-DE" (two region tags) and "a-DE" (single-character
/// subtag in primary position). "ar-a-aaa-b-bbb-a-ccc" is well-formed but
/// invalid (repeated singleton), and validity is not checked.
#[test]
fn rfc5646_appendix_a_ill_formed_tags_fail() {
    assert_eq!(
        LanguageTag::parse("de-419-DE"),
        Err(LanguageTagError::IllFormed)
    );
    assert_eq!(LanguageTag::parse("a-DE"), Err(LanguageTagError::IllFormed));
    assert!(LanguageTag::parse("ar-a-aaa-b-bbb-a-ccc").is_ok());
}

/// RFC 5646 §2.1 grammar edges: subtags are 1*8 alphanumerics; an
/// extension needs at least one subtag; private use needs one too; at
/// most three extlangs.
#[test]
fn rfc5646_grammar_edges() {
    assert_eq!(LanguageTag::parse(""), Err(LanguageTagError::EmptySubtag));
    assert_eq!(
        LanguageTag::parse("en-"),
        Err(LanguageTagError::EmptySubtag)
    );
    assert_eq!(
        LanguageTag::parse("en_US"),
        Err(LanguageTagError::InvalidSubtag)
    );
    assert_eq!(
        LanguageTag::parse("en-abcdefghi"),
        Err(LanguageTagError::InvalidSubtag)
    );
    assert_eq!(LanguageTag::parse("en-a"), Err(LanguageTagError::IllFormed));
    assert_eq!(
        LanguageTag::parse("en-a-x"),
        Err(LanguageTagError::IllFormed)
    );
    assert_eq!(LanguageTag::parse("x"), Err(LanguageTagError::IllFormed));
    assert_eq!(LanguageTag::parse("en-x"), Err(LanguageTagError::IllFormed));
    assert!(LanguageTag::parse("zh-abc-def-ghi").is_ok());
    assert_eq!(
        LanguageTag::parse("zh-abc-def-ghi-jkl"),
        Err(LanguageTagError::IllFormed)
    );
    // An irregular grandfathered tag; "oed" alone is not a variant.
    assert_eq!(tag("EN-gb-OED").as_str(), "en-GB-oed");
    assert_eq!(
        LanguageTag::parse("en-GB-oxd"),
        Err(LanguageTagError::IllFormed)
    );
}

/// RFC 4647 §2.1: `language-range = (1*8ALPHA *("-" 1*8alphanum)) / "*"`.
#[test]
fn rfc4647_basic_ranges() {
    assert!(LanguageRange::parse("*").is_ok());
    assert!(LanguageRange::parse("de-CH").is_ok());
    assert!(LanguageRange::parse("de-*").is_err());
    assert!(LanguageRange::parse("").is_err());
}

// --- RFC 4647 §3.4 Lookup ----------------------------------------------

/// RFC 4647 §3.4 example: "zh-Hant-CN-x-private1-private2" is tried as
/// itself, then "zh-Hant-CN-x-private1", then "zh-Hant-CN" (the trailing
/// "x" is removed with "private1"), then "zh-Hant", then "zh".
#[test]
fn rfc4647_lookup_truncation_example() {
    let r = ["zh-Hant-CN-x-private1-private2"];
    assert_eq!(
        chosen(&r, &["zh-Hant-CN-x-private1"]).as_deref(),
        Some("zh-Hant-CN-x-private1")
    );
    assert_eq!(
        chosen(&r, &["zh-Hant-CN", "zh"]).as_deref(),
        Some("zh-Hant-CN")
    );
    assert_eq!(chosen(&r, &["zh"]).as_deref(), Some("zh"));
    assert_eq!(chosen(&r, &["zh-Hans"]), None);
}

/// draft §4.2.1: Lookup "never adds" subtags: "de" does not match "de-CH".
#[test]
fn lookup_never_widens() {
    assert_eq!(chosen(&["de"], &["de-CH"]), None);
    assert_eq!(
        chosen(&["de-CH", "de", "en"], &["de", "en"]).as_deref(),
        Some("de")
    );
}

/// draft §4.2.1: "Lookup skips a range "*" that other ranges follow, and
/// a final "*" selects the default".
#[test]
fn lookup_wildcard_positions() {
    assert_eq!(chosen(&["*", "fr"], &["fr", "de"]).as_deref(), Some("fr"));
    assert_eq!(
        chosen(&["it", "*", "de"], &["fr", "de"]).as_deref(),
        Some("de")
    );
    assert_eq!(chosen(&["it", "*"], &["fr", "de"]), None);
    assert_eq!(chosen(&["*"], &["fr"]), None);
}

#[test]
fn lookup_ignores_case() {
    assert_eq!(chosen(&["FR-ch"], &["fr"]).as_deref(), Some("fr"));
}

// --- /get view ---------------------------------------------------------

/// draft Appendix A.2: AddressBook/get with `language: ["fr-CH", "de"]`
/// returns `"name": "Contacts du groupe", "locale": "fr"`. The stored
/// object is the one Appendix A.1 builds; its base `name` is not shown
/// in the draft, "Team contacts" is this test's choice.
#[test]
fn draft_appendix_a_view() {
    let mut obj = json!({
        "id": "ab1",
        "name": "Team contacts",
        "description": null,
        "locale": "en",
        "localizations": {
            "fr": { "name": "Contacts du groupe" },
            "de": { "name": "Team-Kontakte" }
        }
    });
    localized_view(
        obj.as_object_mut().unwrap(),
        NAME_DESC,
        &ranges(&["fr-CH", "de"]),
    );
    assert_eq!(obj["name"], "Contacts du groupe");
    assert_eq!(obj["locale"], "fr");
    assert_eq!(obj["description"], Value::Null);
    // decision 30: the view holds one language.
    assert_eq!(obj["localizations"], Value::Null);
}

/// draft §4.2.1 step 2: "When no range matches, it chooses the object's
/// locale, which may be null", and step 3 returns base values then.
#[test]
fn view_falls_back_to_locale() {
    let base = json!({
        "name": "Team contacts",
        "locale": "en",
        "localizations": { "fr": { "name": "Contacts du groupe" } }
    });
    let mut obj = base.clone();
    localized_view(obj.as_object_mut().unwrap(), NAME_DESC, &ranges(&["ja"]));
    assert_eq!(obj["name"], "Team contacts");
    assert_eq!(obj["locale"], "en");

    // Asking for the base language itself gives the base values.
    let mut obj = base.clone();
    localized_view(obj.as_object_mut().unwrap(), NAME_DESC, &ranges(&["en-GB"]));
    assert_eq!(obj["name"], "Team contacts");
    assert_eq!(obj["locale"], "en");

    // Unknown locale, no match: locale stays null.
    let mut obj = json!({ "name": "X", "locale": null, "localizations": null });
    localized_view(obj.as_object_mut().unwrap(), NAME_DESC, &ranges(&["fr"]));
    assert_eq!(
        obj,
        json!({ "name": "X", "locale": null, "localizations": null })
    );
}

/// draft §4.1: `{"description": null}` "says there is no description in
/// that language"; a property the PatchObject omits keeps the base value.
#[test]
fn view_applies_null_and_keeps_omitted() {
    let mut obj = json!({
        "name": "Team",
        "description": "Our team",
        "locale": "en",
        "localizations": { "fr": { "description": null } }
    });
    localized_view(obj.as_object_mut().unwrap(), NAME_DESC, &ranges(&["fr"]));
    assert_eq!(obj["name"], "Team");
    assert_eq!(obj["description"], Value::Null);
    assert_eq!(obj["locale"], "fr");
}

// --- /set validation ---------------------------------------------------

fn check_err(locale: Option<&str>, value: Value) -> String {
    let locale = locale.map(tag);
    let e = check_localizations(locale.as_ref(), &value, NAME_DESC).unwrap_err();
    assert_eq!(e.error_type, SetErrorType::InvalidProperties);
    assert_eq!(
        e.properties.as_deref(),
        Some(&["localizations".to_owned()][..])
    );
    e.description.unwrap_or_default()
}

/// draft Appendix A.1 is accepted.
#[test]
fn check_accepts_appendix_a_update() {
    let v = json!({
        "fr": { "name": "Contacts du groupe" },
        "de": { "name": "Team-Kontakte" }
    });
    let locs = check_localizations(Some(&tag("en")), &v, NAME_DESC)
        .unwrap()
        .unwrap();
    assert_eq!(locs.len(), 2);
    assert_eq!(
        check_localizations(None, &Value::Null, NAME_DESC).unwrap(),
        None
    );
}

/// draft §4.1: "Servers SHOULD store tags in the canonical case".
#[test]
fn check_returns_canonical_keys() {
    let v = json!({ "DE-ch": { "name": "Team" } });
    let locs = check_localizations(None, &v, NAME_DESC).unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&locs).unwrap(),
        json!({ "de-CH": { "name": "Team" } })
    );
}

/// draft §4.1 restrictions, each rejected with invalidProperties naming
/// `localizations`.
#[test]
fn check_rejects_each_restriction() {
    // Keys that differ only in case.
    check_err(None, json!({ "de-CH": {}, "de-ch": {} }));
    // A key equal to locale, ignoring case.
    check_err(Some("fr"), json!({ "FR": { "name": "x" } }));
    // A key that is not a language tag.
    check_err(None, json!({ "not a tag": { "name": "x" } }));
    // A PatchObject key that is not a localizable property.
    check_err(None, json!({ "fr": { "sortOrder": 3 } }));
    // null for a property whose type does not allow null.
    check_err(None, json!({ "fr": { "name": null } }));
    // A value not of the property's type.
    check_err(None, json!({ "fr": { "description": 7 } }));
    // Not a PatchObject.
    check_err(None, json!({ "fr": "Équipe" }));
    // Not an object at all.
    check_err(None, json!(["fr"]));
}

#[test]
fn locale_parses_or_names_locale() {
    assert_eq!(parse_locale(&Value::Null).unwrap(), None);
    assert_eq!(parse_locale(&json!("EN-us")).unwrap(), Some(tag("en-US")));
    let e = parse_locale(&json!("en_US")).unwrap_err();
    assert_eq!(e.properties.as_deref(), Some(&["locale".to_owned()][..]));
    assert!(parse_locale(&json!(3)).is_err());
}

/// Workspace rule: wire maps serialize in sorted key order.
#[test]
fn localizations_serialize_sorted() {
    let locs: Localizations =
        serde_json::from_value(json!({ "fr": {}, "de": {}, "en-US": {} })).unwrap();
    assert_eq!(
        serde_json::to_string(&locs).unwrap(),
        r#"{"de":{},"en-US":{},"fr":{}}"#
    );
}

// --- pruning -----------------------------------------------------------

fn stored() -> Value {
    json!({
        "name": "Team",
        "description": "Our team",
        "locale": "en",
        "localizations": {
            "fr": { "name": "Équipe", "description": "Notre équipe" },
            "de": { "name": "Team-DE" }
        }
    })
}

/// Apply a flat patch of top-level and `localizations/...` paths, enough
/// for these tests, and run the pruning.
fn prune(patch: Value) -> (bool, Value) {
    let old = stored();
    let mut new = old.clone();
    for (k, v) in patch.as_object().unwrap() {
        let segs: Vec<&str> = k.split('/').collect();
        let mut target = new.as_object_mut().unwrap();
        for s in &segs[..segs.len() - 1] {
            target = target.get_mut(*s).unwrap().as_object_mut().unwrap();
        }
        target.insert(segs[segs.len() - 1].to_owned(), v.clone());
    }
    let patch: PatchObject = serde_json::from_value(patch).unwrap();
    let removed = prune_stale_localizations(
        old.as_object().unwrap(),
        new.as_object_mut().unwrap(),
        &patch,
        NAME_DESC,
    );
    (removed, new["localizations"].clone())
}

/// draft §4.1: a renamed base value drops the stale `name` translations
/// of every untouched language and drops PatchObjects left empty.
#[test]
fn prune_drops_untouched_translations() {
    let (removed, locs) = prune(json!({ "name": "Staff" }));
    assert!(removed);
    assert_eq!(locs, json!({ "fr": { "description": "Notre équipe" } }));
}

/// Setting `localizations` as a whole protects every language.
#[test]
fn prune_skips_when_localizations_set_whole() {
    let (removed, _) = prune(json!({
        "name": "Staff",
        "localizations": { "fr": { "name": "Personnel" } }
    }));
    assert!(!removed);
}

/// Setting `localizations/<tag>` protects that language only.
#[test]
fn prune_skips_language_set_by_key() {
    let (removed, locs) = prune(json!({
        "name": "Staff",
        "localizations/fr": { "name": "Personnel" }
    }));
    assert!(removed);
    assert_eq!(locs, json!({ "fr": { "name": "Personnel" } }));
}

/// Setting `localizations/<tag>/<prop>` protects that property in that
/// language only; another changed property in the same language is
/// still pruned.
#[test]
fn prune_skips_property_set_inside_language() {
    let (removed, locs) = prune(json!({
        "name": "Staff",
        "description": "All staff",
        "localizations/fr/name": "Personnel"
    }));
    assert!(removed);
    assert_eq!(locs, json!({ "fr": { "name": "Personnel" } }));
}

/// No base value changed (only `locale`, gap G3): nothing is pruned.
#[test]
fn prune_nothing_when_base_unchanged() {
    let (removed, locs) = prune(json!({ "locale": "en-GB" }));
    assert!(!removed);
    assert_eq!(locs, stored()["localizations"]);
}

// --- /query: localized_value ---------------------------------------------

/// draft §4.2.2: a `/query` with `language` compares "the value that /get
/// with the same language would return, not the base value".
#[test]
fn localized_value_matches_the_view() {
    let obj = json!({
        "name": "Team contacts",
        "description": "Our team",
        "locale": "en",
        "localizations": {
            "fr": { "name": "Contacts du groupe", "description": null },
            "de": { "name": "Team-Kontakte" }
        }
    });
    let o = obj.as_object().unwrap();
    let v = |p: &str, r: &[&str]| jmap_types::i18n::localized_value(o, p, &ranges(r));
    assert_eq!(v("name", &["fr-CH"]), Some(json!("Contacts du groupe")));
    // A property the PatchObject omits keeps the base value.
    assert_eq!(v("description", &["de"]), Some(json!("Our team")));
    // A null in the PatchObject: no value in that language.
    assert_eq!(v("description", &["fr"]), Some(Value::Null));
    // No match: the base value.
    assert_eq!(v("name", &["ja"]), Some(json!("Team contacts")));
    // Absent property.
    assert_eq!(v("color", &["fr"]), None);
}
