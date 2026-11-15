//! `locale` and `localizations` on `Foo/set`
//! (draft-atwood-jmap-updates-00 §3 and §4.1).
//!
//! Oracle: the draft's normative text and its Appendix A.1 update at
//! jmap-chat-spec 2b85363. Expected values are written out by hand from
//! that text.

use jmap_server::{
    I18nInfo, I18nSet, JmapObject, SetErrorType, apply_patch_object, localized_update,
};
use jmap_types::PatchObject;
use jmap_types::i18n::{JMAP_I18N_URI, LocalizableProperty};
use serde_json::{Map, Value, json};

/// AddressBook-like (draft §5.3): `name` and `description` localizable.
#[derive(serde::Serialize, serde::Deserialize)]
struct Book {}

impl JmapObject for Book {
    const TYPE_NAME: &'static str = "Book";
    type Property = ();
    const LOCALIZABLE: &'static [LocalizableProperty] = &[
        LocalizableProperty::new("name", false),
        LocalizableProperty::new("description", true),
    ];
}

/// No localizable properties (like Identity, §5.1).
#[derive(serde::Serialize, serde::Deserialize)]
struct Plain {}

impl JmapObject for Plain {
    const TYPE_NAME: &'static str = "Plain";
    type Property = ();
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn with_i18n() -> Vec<String> {
    vec![JMAP_I18N_URI.to_owned()]
}

fn invalid_on(err: &jmap_server::SetError, property: &str) {
    assert_eq!(err.error_type, SetErrorType::InvalidProperties);
    assert_eq!(err.properties.as_deref(), Some(&[property.to_owned()][..]));
}

// --- handler side ------------------------------------------------------

/// §3: without the capability in `using`, "a /set that names locale or
/// localizations is rejected with an invalidProperties SetError".
#[test]
fn rejected_without_capability_in_using() {
    let set = I18nSet::new::<Book>(&[], Some(&I18nInfo::new()));
    let err = set
        .check_create(&mut obj(json!({ "name": "x", "locale": "en" })))
        .unwrap_err();
    invalid_on(&err, "locale");
    let err = set
        .check_patch(&obj(json!({ "localizations/fr/name": "y" })))
        .unwrap_err();
    invalid_on(&err, "localizations");
    // Other properties are untouched.
    set.check_create(&mut obj(json!({ "name": "x" }))).unwrap();
    set.check_patch(&obj(json!({ "name": "y" }))).unwrap();
}

/// §3: "The same applies to an account whose accountCapabilities does not
/// include urn:ietf:params:jmap:i18n, even when the capability is in
/// using".
#[test]
fn rejected_for_account_without_capability() {
    let set = I18nSet::new::<Book>(&with_i18n(), None);
    let err = set
        .check_patch(&obj(json!({ "locale": "en" })))
        .unwrap_err();
    invalid_on(&err, "locale");
}

/// A type without localizable properties is not this document's business.
#[test]
fn other_types_pass_through() {
    let set = I18nSet::new::<Plain>(&[], None);
    set.check_create(&mut obj(json!({ "locale": "whatever" })))
        .unwrap();
    set.check_patch(&obj(json!({ "locale": "whatever" })))
        .unwrap();
}

/// draft Appendix A.1's values are accepted on create, folded to canonical
/// case (§4.1 SHOULD).
#[test]
fn create_accepts_and_folds() {
    let set = I18nSet::new::<Book>(&with_i18n(), Some(&I18nInfo::new()));
    let mut create = obj(json!({
        "name": "Team contacts",
        "locale": "EN",
        "localizations": {
            "fr": { "name": "Contacts du groupe" },
            "DE-ch": { "name": "Team-Kontakte" }
        }
    }));
    set.check_create(&mut create).unwrap();
    assert_eq!(create["locale"], "en");
    assert_eq!(
        create["localizations"],
        json!({ "de-CH": { "name": "Team-Kontakte" }, "fr": { "name": "Contacts du groupe" } })
    );
}

/// §4.1: "no key may equal locale, ignoring case", rejected naming
/// `localizations`; a malformed `locale` names `locale`.
#[test]
fn create_rejects_key_equal_to_locale_and_bad_locale() {
    let set = I18nSet::new::<Book>(&with_i18n(), Some(&I18nInfo::new()));
    let err = set
        .check_create(&mut obj(
            json!({ "locale": "fr", "localizations": { "FR": { "name": "x" } } }),
        ))
        .unwrap_err();
    invalid_on(&err, "localizations");
    let err = set
        .check_create(&mut obj(json!({ "locale": "fr_FR" })))
        .unwrap_err();
    invalid_on(&err, "locale");
}

/// §4.1: "A server MAY limit the number of localizations it stores per
/// object and reject more with the same error."
#[test]
fn create_enforces_the_account_cap() {
    let info = I18nInfo::new().with_max_localizations(Some(1));
    let set = I18nSet::new::<Book>(&with_i18n(), Some(&info));
    let err = set
        .check_create(&mut obj(json!({
            "localizations": { "fr": { "name": "a" }, "de": { "name": "b" } }
        })))
        .unwrap_err();
    invalid_on(&err, "localizations");
}

/// §3: clients without the capability never see the properties; others
/// get them as returned.
#[test]
fn echo_strips_only_for_clients_without_capability() {
    let entry = json!({ "id": "ab1", "locale": "en", "localizations": null });
    let mut e = entry.clone();
    I18nSet::new::<Book>(&[], Some(&I18nInfo::new())).echo(&mut e);
    assert_eq!(e, json!({ "id": "ab1" }));
    let mut e = entry.clone();
    I18nSet::new::<Book>(&with_i18n(), Some(&I18nInfo::new())).echo(&mut e);
    assert_eq!(e, entry);
}

// --- backend side ------------------------------------------------------

fn stored() -> Map<String, Value> {
    obj(json!({
        "id": "ab1",
        "name": "Team contacts",
        "description": null,
        "locale": "en",
        "localizations": {
            "fr": { "name": "Contacts du groupe" },
            "de": { "name": "Team-Kontakte" }
        }
    }))
}

/// Apply `patch` to the stored object and run `localized_update`.
fn update(
    patch: Value,
    max: Option<usize>,
) -> (Result<bool, Box<jmap_server::SetError>>, Map<String, Value>) {
    let old = stored();
    let mut new = old.clone();
    apply_patch_object(&mut new, patch.as_object().unwrap()).unwrap();
    let patch: PatchObject = serde_json::from_value(patch).unwrap();
    let r = localized_update::<Book>(&old, &mut new, &patch, max);
    (r, new)
}

/// §4.1: a client that does not use the capability renames the object; the
/// stale translations of `name` go, and the PatchObjects left empty too.
#[test]
fn rename_without_capability_prunes_translations() {
    let (r, new) = update(json!({ "name": "Staff" }), None);
    assert!(r.unwrap());
    assert_eq!(new["localizations"], Value::Null);
    assert_eq!(new["name"], "Staff");
}

/// §4.1: a client that renames and sets `localizations/fr/name` in the
/// same update keeps its French name; the German one is pruned.
#[test]
fn rename_with_one_language_keeps_that_language() {
    let (r, new) = update(
        json!({ "name": "Staff", "localizations/fr/name": "Personnel" }),
        None,
    );
    assert!(r.unwrap());
    assert_eq!(
        new["localizations"],
        json!({ "fr": { "name": "Personnel" } })
    );
}

/// Draft Appendix A.1: setting `locale` and `localizations` as a whole.
/// No base value changed, so nothing is pruned.
#[test]
fn appendix_a_update() {
    let (r, new) = update(
        json!({
            "locale": "en",
            "localizations": {
                "fr": { "name": "Contacts du groupe" },
                "de": { "name": "Team-Kontakte" }
            }
        }),
        None,
    );
    assert!(!r.unwrap());
    assert_eq!(new["localizations"]["de"]["name"], "Team-Kontakte");
}

/// §4.1 on the patched object: changing `locale` to an existing key breaks
/// "no key may equal locale".
#[test]
fn locale_changed_to_an_existing_key_is_rejected() {
    let (r, _) = update(json!({ "locale": "fr" }), None);
    invalid_on(&r.unwrap_err(), "localizations");
}

/// The cap applies to the patched object too.
#[test]
fn update_enforces_the_cap() {
    let (r, _) = update(
        json!({ "localizations/it": { "name": "Contatti" } }),
        Some(2),
    );
    invalid_on(&r.unwrap_err(), "localizations");
}

/// A type without localizable properties is left alone.
#[test]
fn other_types_are_left_alone() {
    let old = obj(json!({ "name": "a", "localizations": "junk" }));
    let mut new = old.clone();
    new.insert("name".to_owned(), json!("b"));
    let patch: PatchObject = serde_json::from_value(json!({ "name": "b" })).unwrap();
    assert!(!localized_update::<Plain>(&old, &mut new, &patch, None).unwrap());
    assert_eq!(new["localizations"], "junk");
}
