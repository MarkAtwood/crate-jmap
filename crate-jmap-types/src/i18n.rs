//! Localized display text (draft-atwood-jmap-updates-00 §3–§5).
//!
//! The `urn:ietf:params:jmap:i18n` capability adds two properties to
//! every data type that has localizable properties, modelled on
//! JSCalendar (RFC 8984 §4.2.8 and §4.6.1):
//!
//! - `locale` (`String|null`): the [`LanguageTag`] of the base values;
//! - `localizations` (`String[PatchObject]|null`): a [`Localizations`]
//!   map from language tag to a [`PatchObject`] over the localizable
//!   properties.
//!
//! It also adds a `language` argument (a list of [`LanguageRange`]s) to
//! `/get`, `/query` and `/queryChanges`; [`lookup`] implements the
//! RFC 4647 §3.4 Lookup that picks one tag per object, and
//! [`localized_view`] builds the single-language `/get` view.
//!
//! The `/set` side is [`parse_locale`] and [`check_localizations`],
//! which turn wire values into the validated types or an
//! `invalidProperties` [`SetError`], and [`prune_stale_localizations`],
//! which drops translations of a base value that an update changed.
//!
//! A data type declares its localizable properties with
//! [`JmapObject::LOCALIZABLE`](crate::JmapObject::LOCALIZABLE).
//! Design: `docs/design/updates.md` (decisions 27 to 31).

use crate::{PatchObject, SetError, SetErrorType};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// The capability URI (draft-atwood-jmap-updates-00 §3, §10).
pub const JMAP_I18N_URI: &str = "urn:ietf:params:jmap:i18n";

/// The Session `capabilities` value of [`JMAP_I18N_URI`]: an empty
/// object (draft-atwood-jmap-updates-00 §3).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct I18nCapability {
    /// Catch-all for vendor / site / private extension fields.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

/// The `accountCapabilities` value of [`JMAP_I18N_URI`]
/// (draft-atwood-jmap-updates-00 §3).
///
/// Present on an account only when the server supports localizations
/// for it.
///
/// ```rust
/// use jmap_types::i18n::I18nAccountCapability;
///
/// let cap: I18nAccountCapability = serde_json::from_value(
///     serde_json::json!({ "preferredLanguages": ["fr-ch", "de"] }),
/// ).unwrap();
/// let langs = cap.preferred_languages.as_deref().unwrap();
/// assert_eq!(langs[0].as_str(), "fr-CH");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct I18nAccountCapability {
    /// The languages the account's owner prefers, most preferred first,
    /// or `None` (`null` on the wire) when the server has none recorded.
    /// A server returns `None` for an account the requesting user does
    /// not own (§9), and never uses it to change `/get` or `/query`
    /// results (§3.1).
    pub preferred_languages: Option<Vec<LanguageTag>>,
    /// Catch-all for vendor / site / private extension fields.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// Language tags and ranges
// ---------------------------------------------------------------------------

/// Why a string is not a well-formed [`LanguageTag`] or
/// [`LanguageRange`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageTagError {
    /// The string is empty, or has an empty subtag (`"en--US"`, `"en-"`).
    EmptySubtag,
    /// A subtag has a character other than an ASCII letter or digit, or
    /// is longer than eight characters.
    InvalidSubtag,
    /// Every subtag is well-formed but their sequence matches no
    /// production of the grammar (RFC 5646 §2.1 or RFC 4647 §2.1).
    IllFormed,
}

impl fmt::Display for LanguageTagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptySubtag => "language tag has an empty subtag",
            Self::InvalidSubtag => "language tag subtag is not 1 to 8 ASCII letters or digits",
            Self::IllFormed => "language tag does not match the RFC 5646 grammar",
        })
    }
}

impl std::error::Error for LanguageTagError {}

/// An RFC 5646 language tag, checked to be well-formed (§2.2.9) and
/// held in the canonical case of §2.1.1.
///
/// Tags are case-insensitive (§2.1.1), so parsing folds the case:
/// `"EN-us"` and `"en-US"` give equal values, both rendered `"en-US"`.
/// Equality, ordering and hashing therefore ignore the case of the
/// input. The check is well-formedness only: subtags are not looked up
/// in the IANA registry, so `"qq-Zzzz"` is accepted.
///
/// ```rust
/// use jmap_types::i18n::LanguageTag;
///
/// let tag: LanguageTag = "sr-latn-rs".parse().unwrap();
/// assert_eq!(tag.as_str(), "sr-Latn-RS");
/// assert!("en--US".parse::<LanguageTag>().is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageTag(String);

/// RFC 5646 §2.1 `irregular` grandfathered tags, lowercase. They are the
/// only well-formed tags that the `langtag` production does not match;
/// the `regular` ones (`zh-min-nan`, `art-lojban`, ...) match it.
const IRREGULAR: &[&str] = &[
    "en-gb-oed",
    "i-ami",
    "i-bnn",
    "i-default",
    "i-enochian",
    "i-hak",
    "i-klingon",
    "i-lux",
    "i-mingo",
    "i-navajo",
    "i-pwn",
    "i-tao",
    "i-tay",
    "i-tsu",
    "sgn-be-fr",
    "sgn-be-nl",
    "sgn-ch-de",
];

/// Split on `-` and check each subtag is 1 to 8 ASCII alphanumerics.
fn subtags(s: &str) -> Result<Vec<&str>, LanguageTagError> {
    let parts: Vec<&str> = s.split('-').collect();
    for p in &parts {
        if p.is_empty() {
            return Err(LanguageTagError::EmptySubtag);
        }
        if p.len() > 8 || !p.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(LanguageTagError::InvalidSubtag);
        }
    }
    Ok(parts)
}

fn is_alpha(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_digit(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_digit())
}

/// RFC 5646 §2.1 `variant = 5*8alphanum / (DIGIT 3alphanum)`.
fn is_variant(s: &str) -> bool {
    (5..=8).contains(&s.len()) || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit())
}

/// `true` when `parts` (already 1*8alphanum each) match the RFC 5646
/// `langtag` production.
fn is_langtag(parts: &[&str]) -> bool {
    let mut i = 0;
    let n = parts.len();
    // language = 2*3ALPHA ["-" extlang] / 4ALPHA / 5*8ALPHA
    let lang = parts[0];
    if !is_alpha(lang) || lang.len() < 2 {
        return false;
    }
    i += 1;
    if lang.len() <= 3 {
        // extlang = 3ALPHA *2("-" 3ALPHA)
        let mut ext = 0;
        while ext < 3 && i < n && parts[i].len() == 3 && is_alpha(parts[i]) {
            ext += 1;
            i += 1;
        }
    }
    // script = 4ALPHA
    if i < n && parts[i].len() == 4 && is_alpha(parts[i]) {
        i += 1;
    }
    // region = 2ALPHA / 3DIGIT
    if i < n
        && ((parts[i].len() == 2 && is_alpha(parts[i]))
            || (parts[i].len() == 3 && is_digit(parts[i])))
    {
        i += 1;
    }
    // *("-" variant)
    while i < n && is_variant(parts[i]) {
        i += 1;
    }
    // *("-" extension); extension = singleton 1*("-" (2*8alphanum))
    while i < n && parts[i].len() == 1 && !parts[i].eq_ignore_ascii_case("x") {
        i += 1;
        let start = i;
        while i < n && parts[i].len() >= 2 {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    // ["-" privateuse]
    if i < n && parts[i].eq_ignore_ascii_case("x") {
        return is_privateuse(&parts[i..]);
    }
    i == n
}

/// RFC 5646 §2.1 `privateuse = "x" 1*("-" (1*8alphanum))`; `parts[0]`
/// is the `x`.
fn is_privateuse(parts: &[&str]) -> bool {
    parts.len() >= 2
}

/// RFC 5646 §2.1.1 canonical case: lowercase, except that a two-letter
/// subtag is uppercase and a four-letter subtag titlecase when it is not
/// the first subtag and no singleton precedes it.
fn canonical_case(parts: &[&str]) -> String {
    let mut out = String::new();
    let mut after_singleton = false;
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push('-');
        }
        if i > 0 && !after_singleton && p.len() == 2 {
            out.push_str(&p.to_ascii_uppercase());
        } else if i > 0 && !after_singleton && p.len() == 4 {
            let lower = p.to_ascii_lowercase();
            let mut chars = lower.chars();
            if let Some(c) = chars.next() {
                out.push(c.to_ascii_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(&p.to_ascii_lowercase());
        }
        if p.len() == 1 {
            after_singleton = true;
        }
    }
    out
}

impl LanguageTag {
    /// Parse `s` as an RFC 5646 `Language-Tag` and fold it to canonical
    /// case.
    ///
    /// # Errors
    ///
    /// [`LanguageTagError`] when `s` is not well-formed.
    pub fn parse(s: &str) -> Result<Self, LanguageTagError> {
        let parts = subtags(s)?;
        let lower = s.to_ascii_lowercase();
        let ok = if parts[0].eq_ignore_ascii_case("x") {
            is_privateuse(&parts)
        } else {
            IRREGULAR.contains(&lower.as_str()) || is_langtag(&parts)
        };
        if ok {
            Ok(Self(canonical_case(&parts)))
        } else {
            Err(LanguageTagError::IllFormed)
        }
    }

    /// The tag in canonical case.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for LanguageTag {
    type Err = LanguageTagError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for LanguageTag {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for LanguageTag {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageTag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(de::Error::custom)
    }
}

/// An RFC 4647 §2.1 basic language range: `"*"`, or a primary subtag
/// of 1 to 8 letters followed by subtags of 1 to 8 letters or digits.
///
/// Held in the canonical case of a [`LanguageTag`] (RFC 5646 §2.1.1),
/// so that equal ranges render equally; matching ignores case either
/// way (RFC 4647 §2).
///
/// ```rust
/// use jmap_types::i18n::LanguageRange;
///
/// let r: LanguageRange = "DE-ch".parse().unwrap();
/// assert_eq!(r.as_str(), "de-CH");
/// assert!("*".parse::<LanguageRange>().unwrap().is_wildcard());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageRange(String);

impl LanguageRange {
    /// Parse `s` as an RFC 4647 §2.1 `language-range`.
    ///
    /// # Errors
    ///
    /// [`LanguageTagError`] when `s` is not a basic language range.
    pub fn parse(s: &str) -> Result<Self, LanguageTagError> {
        if s == "*" {
            return Ok(Self(s.to_owned()));
        }
        let parts = subtags(s)?;
        if !is_alpha(parts[0]) {
            return Err(LanguageTagError::IllFormed);
        }
        Ok(Self(canonical_case(&parts)))
    }

    /// The range in canonical case.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `true` for the wildcard range `"*"`.
    pub fn is_wildcard(&self) -> bool {
        self.0 == "*"
    }
}

impl FromStr for LanguageRange {
    type Err = LanguageTagError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for LanguageRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for LanguageRange {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageRange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(de::Error::custom)
    }
}

/// RFC 4647 §3.4 Lookup: the tag in `available` that best matches the
/// priority list `ranges`, or `None` when nothing matches and the
/// default applies.
///
/// Each range is tried in order. A range that matches no tag is
/// shortened by its last subtag, and also by a single-letter subtag left
/// at its end, until it matches or is empty; ranges are never widened,
/// so `"de"` does not match `"de-CH"`. A `"*"` that other ranges follow
/// is skipped; a `"*"` with no range after it ends the search with the
/// default. Matching ignores case.
///
/// draft-atwood-jmap-updates-00 §4.2.1 passes the `localizations` keys
/// plus the object's `locale` as `available`, and uses the object's
/// `locale` as the default.
///
/// ```rust
/// use jmap_types::i18n::{LanguageRange, LanguageTag, lookup};
///
/// let tags: Vec<LanguageTag> = ["fr", "de"].iter().map(|t| t.parse().unwrap()).collect();
/// let ranges: Vec<LanguageRange> = ["fr-CH", "de"].iter().map(|r| r.parse().unwrap()).collect();
/// assert_eq!(lookup(&ranges, &tags).map(|t| t.as_str()), Some("fr"));
/// ```
pub fn lookup<'a, I>(ranges: &[LanguageRange], available: I) -> Option<&'a LanguageTag>
where
    I: IntoIterator<Item = &'a LanguageTag>,
    I::IntoIter: Clone,
{
    let available = available.into_iter();
    for (i, range) in ranges.iter().enumerate() {
        if range.is_wildcard() {
            if i + 1 == ranges.len() {
                return None;
            }
            continue;
        }
        let mut candidate = range.as_str();
        loop {
            if let Some(tag) = available
                .clone()
                .find(|t| t.as_str().eq_ignore_ascii_case(candidate))
            {
                return Some(tag);
            }
            let Some(cut) = candidate.rfind('-') else {
                break;
            };
            candidate = &candidate[..cut];
            // Drop a singleton left at the end ("en-a-bbb" -> "en-a" -> "en").
            if let Some(cut) = candidate.rfind('-')
                && candidate.len() - cut == 2
            {
                candidate = &candidate[..cut];
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Localizable properties and the localizations map
// ---------------------------------------------------------------------------

/// One localizable property of a data type: its wire name and whether
/// its type allows `null` (draft-atwood-jmap-updates-00 §4.1).
///
/// Every localizable property in the draft is a `String` or a
/// `String|null`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalizableProperty {
    /// Wire name, e.g. `"name"`.
    pub name: &'static str,
    /// `true` for a `String|null` property, whose localized value may be
    /// `null` ("no value in that language").
    pub nullable: bool,
}

impl LocalizableProperty {
    /// A property named `name`, nullable when `nullable`.
    pub const fn new(name: &'static str, nullable: bool) -> Self {
        Self { name, nullable }
    }
}

/// The `localizations` property: a map from [`LanguageTag`] to a
/// [`PatchObject`] that gives the localizable properties in that
/// language (draft-atwood-jmap-updates-00 §4.1).
///
/// Deserializing parses each key as a tag in canonical case and rejects
/// two keys that differ only in case. It does not check the patches;
/// [`check_localizations`] does that against the data type's
/// [`LocalizableProperty`] list.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Localizations(BTreeMap<LanguageTag, PatchObject>);

impl Localizations {
    /// The patch for `tag`, if any.
    pub fn get(&self, tag: &LanguageTag) -> Option<&PatchObject> {
        self.0.get(tag)
    }

    /// The localizations, in canonical tag order.
    pub fn iter(&self) -> impl Iterator<Item = (&LanguageTag, &PatchObject)> {
        self.0.iter()
    }

    /// The language tags, in canonical order.
    pub fn tags(&self) -> impl Iterator<Item = &LanguageTag> + Clone {
        self.0.keys()
    }

    /// `true` when there is no localization.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of localizations.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl<'de> Deserialize<'de> for Localizations {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Localizations;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map from language tag to PatchObject")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Localizations, A::Error> {
                let mut out = BTreeMap::new();
                while let Some((k, v)) = m.next_entry::<String, PatchObject>()? {
                    let tag = LanguageTag::parse(&k).map_err(de::Error::custom)?;
                    if out.insert(tag, v).is_some() {
                        return Err(de::Error::custom(format!(
                            "duplicate localizations key (ignoring case): {k}"
                        )));
                    }
                }
                Ok(Localizations(out))
            }
        }
        d.deserialize_map(V)
    }
}

// ---------------------------------------------------------------------------
// /set: validation and pruning
// ---------------------------------------------------------------------------

fn invalid(property: &str, description: String) -> Box<SetError> {
    Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties([property])
            .with_description(description),
    )
}

/// Parse a `/set` `locale` value: `null` or a language tag.
///
/// # Errors
///
/// An `invalidProperties` [`SetError`] naming `locale` when `value` is
/// neither `null` nor a well-formed language tag.
pub fn parse_locale(value: &Value) -> Result<Option<LanguageTag>, Box<SetError>> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) => LanguageTag::parse(s)
            .map(Some)
            .map_err(|e| invalid("locale", format!("locale {s:?}: {e}"))),
        other => Err(invalid(
            "locale",
            format!(
                "locale must be a String or null, not {}",
                crate::json_type_name(other)
            ),
        )),
    }
}

/// Check a `/set` `localizations` value against the restrictions of
/// draft-atwood-jmap-updates-00 §4.1 and return it with its keys in
/// canonical case (which servers SHOULD store).
///
/// `locale` is the object's `locale` after the same create or update.
/// `null` gives `Ok(None)`. Otherwise the value must be an object whose
/// keys are language tags, no two equal ignoring case and none equal to
/// `locale`, and whose values are objects naming only properties in
/// `properties`, each a String, or `null` when that property is
/// nullable. The defining specification's length and syntax limits on
/// localized values are the caller's to apply.
///
/// # Errors
///
/// An `invalidProperties` [`SetError`] naming `localizations`, whose
/// description says which rule failed.
pub fn check_localizations(
    locale: Option<&LanguageTag>,
    value: &Value,
    properties: &[LocalizableProperty],
) -> Result<Option<Localizations>, Box<SetError>> {
    let bad = |d: String| invalid("localizations", d);
    let obj = match value {
        Value::Null => return Ok(None),
        Value::Object(o) => o,
        other => {
            return Err(bad(format!(
                "localizations must be an object or null, not {}",
                crate::json_type_name(other)
            )));
        }
    };
    let mut out = BTreeMap::new();
    for (key, patch) in obj {
        let tag = LanguageTag::parse(key).map_err(|e| bad(format!("key {key:?}: {e}")))?;
        if locale == Some(&tag) {
            return Err(bad(format!("key {key:?} equals locale")));
        }
        let Value::Object(patch) = patch else {
            return Err(bad(format!("value for {key:?} is not a PatchObject")));
        };
        for (prop, v) in patch {
            let Some(p) = properties.iter().find(|p| p.name == prop) else {
                return Err(bad(format!(
                    "{key:?}: {prop:?} is not a localizable property"
                )));
            };
            match v {
                Value::String(_) => {}
                Value::Null if p.nullable => {}
                Value::Null => {
                    return Err(bad(format!("{key:?}: {prop:?} cannot be null")));
                }
                other => {
                    return Err(bad(format!(
                        "{key:?}: {prop:?} must be a String, not {}",
                        crate::json_type_name(other)
                    )));
                }
            }
        }
        if out
            .insert(tag, PatchObject::from_map(patch.clone()))
            .is_some()
        {
            return Err(bad(format!(
                "key {key:?} duplicates another key ignoring case"
            )));
        }
    }
    Ok(Some(Localizations(out)))
}

/// What a `/set` update `patch` sets inside `localizations`
/// (draft-atwood-jmap-updates-00 §4.1).
enum Touched {
    /// `localizations` as a whole.
    All,
    /// Per language: `None` for the whole PatchObject of that language,
    /// `Some(props)` for those properties inside it.
    Some(BTreeMap<LanguageTag, Option<Vec<String>>>),
}

fn touched(patch: &PatchObject) -> Touched {
    let mut out: BTreeMap<LanguageTag, Option<Vec<String>>> = BTreeMap::new();
    for key in patch.as_map().keys() {
        let mut segs = key.splitn(3, '/');
        if segs.next() != Some("localizations") {
            continue;
        }
        let Some(tag) = segs.next() else {
            return Touched::All;
        };
        let Ok(tag) = LanguageTag::parse(tag) else {
            continue;
        };
        match segs.next() {
            None => {
                out.insert(tag, None);
            }
            Some(prop) => {
                // JSON Pointer unescaping (RFC 6901 §4) of the property name.
                let prop = prop.replace("~1", "/").replace("~0", "~");
                let entry = out.entry(tag).or_insert_with(|| Some(Vec::new()));
                if let Some(props) = entry {
                    props.push(prop);
                }
            }
        }
    }
    Touched::Some(out)
}

/// Drop translations made stale by a `/set` update
/// (draft-atwood-jmap-updates-00 §4.1).
///
/// `old` and `new` are the serialized object before and after `patch`
/// was applied. For each property of `properties` whose base value
/// differs between them, and each language whose PatchObject the patch
/// did not set (not `localizations` as a whole, not
/// `localizations/<tag>`, and not `localizations/<tag>/<property>`),
/// the property is removed from that language's PatchObject in `new`,
/// and a PatchObject left empty is removed. A `localizations` left
/// empty becomes `null`.
///
/// Returns `true` when anything was removed: the caller then includes
/// `localizations` in the `updated` entry it returns to clients that use
/// the capability. Malformed stored keys are left as they are.
///
/// ```rust
/// use jmap_types::PatchObject;
/// use jmap_types::i18n::{LocalizableProperty, prune_stale_localizations};
/// use serde_json::json;
///
/// const PROPS: &[LocalizableProperty] = &[LocalizableProperty::new("name", false)];
/// let old = json!({"name": "Team", "localizations": {"fr": {"name": "Équipe"}}});
/// let mut new = json!({"name": "Staff", "localizations": {"fr": {"name": "Équipe"}}});
/// let patch: PatchObject = serde_json::from_value(json!({"name": "Staff"})).unwrap();
/// let removed = prune_stale_localizations(
///     old.as_object().unwrap(), new.as_object_mut().unwrap(), &patch, PROPS);
/// assert!(removed);
/// assert_eq!(new["localizations"], json!(null));
/// ```
pub fn prune_stale_localizations(
    old: &Map<String, Value>,
    new: &mut Map<String, Value>,
    patch: &PatchObject,
    properties: &[LocalizableProperty],
) -> bool {
    let changed: Vec<&str> = properties
        .iter()
        .map(|p| p.name)
        .filter(|n| old.get(*n) != new.get(*n))
        .collect();
    if changed.is_empty() {
        return false;
    }
    let touched = match touched(patch) {
        Touched::All => return false,
        Touched::Some(t) => t,
    };
    let Some(Value::Object(locs)) = new.get_mut("localizations") else {
        return false;
    };
    let mut removed = false;
    locs.retain(|key, entry| {
        let Ok(tag) = LanguageTag::parse(key) else {
            return true;
        };
        let set_here = touched.get(&tag);
        if matches!(set_here, Some(None)) {
            return true;
        }
        let Value::Object(p) = entry else {
            return true;
        };
        for name in &changed {
            let protected =
                matches!(set_here, Some(Some(props)) if props.iter().any(|q| q == name));
            if !protected && p.remove(*name).is_some() {
                removed = true;
            }
        }
        !p.is_empty()
    });
    if locs.is_empty() {
        new.insert("localizations".to_owned(), Value::Null);
    }
    removed
}

// ---------------------------------------------------------------------------
// /get: the single-language view
// ---------------------------------------------------------------------------

/// The tag that §4.2.1 chooses for `obj` under `ranges`, and the
/// PatchObject stored for it when it is a key of `localizations`.
fn choose(
    obj: &Map<String, Value>,
    ranges: &[LanguageRange],
) -> (Option<LanguageTag>, Option<Map<String, Value>>) {
    let locale = obj
        .get("locale")
        .and_then(Value::as_str)
        .and_then(|s| LanguageTag::parse(s).ok());
    let mut localizations: BTreeMap<LanguageTag, Map<String, Value>> =
        match obj.get("localizations") {
            Some(Value::Object(m)) => m
                .iter()
                .filter_map(|(k, v)| Some((LanguageTag::parse(k).ok()?, v.as_object()?.clone())))
                .collect(),
            _ => BTreeMap::new(),
        };
    let available: Vec<&LanguageTag> = localizations.keys().chain(locale.as_ref()).collect();
    let chosen = lookup(ranges, available.iter().copied())
        .or(locale.as_ref())
        .cloned();
    let patch = chosen.as_ref().and_then(|t| localizations.remove(t));
    (chosen, patch)
}

/// The value that the `/get` view under `ranges` ([`localized_view`])
/// gives `property` of the serialized object `obj`, for sorting and
/// filtering a `/query` with `language` (draft-atwood-jmap-updates-00
/// §4.2.2). `None` when the property is absent; `Some(&Value::Null)` when
/// the chosen language has no value for it.
///
/// ```rust
/// use jmap_types::i18n::{LanguageRange, localized_value};
/// use serde_json::json;
///
/// let obj = json!({"name": "Inbox", "locale": "en",
///                  "localizations": {"de": {"name": "Posteingang"}}});
/// let de: Vec<LanguageRange> = vec!["de-AT".parse().unwrap()];
/// assert_eq!(localized_value(obj.as_object().unwrap(), "name", &de), Some(json!("Posteingang")));
/// ```
pub fn localized_value(
    obj: &Map<String, Value>,
    property: &str,
    ranges: &[LanguageRange],
) -> Option<Value> {
    match choose(obj, ranges).1.and_then(|mut p| p.remove(property)) {
        Some(v) => Some(v),
        None => obj.get(property).cloned(),
    }
}

/// Turn the serialized object `obj` into its view in one language
/// (draft-atwood-jmap-updates-00 §4.2.1).
///
/// The tag is chosen by [`lookup`] over the keys of `localizations` and
/// the object's `locale`, falling back to `locale` (which may be
/// `null`). When the chosen tag is a key of `localizations`, its
/// PatchObject is applied to the properties in `properties`; a `null`
/// value there sets the property to `null`. `locale` is set to the
/// chosen tag, and `localizations` to `null`, since the view holds one
/// language only (decision 30).
///
/// Call it before filtering the object to the requested `properties`.
/// Stored values that do not parse (a malformed `locale` or key) are
/// treated as absent.
///
/// ```rust
/// use jmap_types::i18n::{LanguageRange, LocalizableProperty, localized_view};
/// use serde_json::json;
///
/// const PROPS: &[LocalizableProperty] = &[LocalizableProperty::new("name", false)];
/// let mut obj = json!({
///     "name": "Team contacts", "locale": "en",
///     "localizations": {"fr": {"name": "Contacts du groupe"}, "de": {"name": "Team-Kontakte"}},
/// });
/// let ranges: Vec<LanguageRange> = ["fr-CH", "de"].iter().map(|r| r.parse().unwrap()).collect();
/// localized_view(obj.as_object_mut().unwrap(), PROPS, &ranges);
/// assert_eq!(obj, json!({"name": "Contacts du groupe", "locale": "fr", "localizations": null}));
/// ```
pub fn localized_view(
    obj: &mut Map<String, Value>,
    properties: &[LocalizableProperty],
    ranges: &[LanguageRange],
) {
    let (chosen, patch) = choose(obj, ranges);
    if let Some(patch) = patch {
        for p in properties {
            if let Some(v) = patch.get(p.name) {
                obj.insert(p.name.to_owned(), v.clone());
            }
        }
    }
    obj.insert(
        "locale".to_owned(),
        chosen.map_or(Value::Null, |t| Value::String(t.0)),
    );
    obj.insert("localizations".to_owned(), Value::Null);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tag(s: &str) -> LanguageTag {
        LanguageTag::parse(s).unwrap()
    }

    #[test]
    fn canonical_case_follows_rfc5646_2_1_1() {
        // The examples of RFC 5646 §2.1.1.
        assert_eq!(tag("mn-cyrl-mn").as_str(), "mn-Cyrl-MN");
        assert_eq!(tag("MN-cYRL-mn").as_str(), "mn-Cyrl-MN");
        assert_eq!(tag("en-ca-x-ca").as_str(), "en-CA-x-ca");
        assert_eq!(tag("SGN-BE-FR").as_str(), "sgn-BE-FR");
        assert_eq!(tag("az-latn-x-latn").as_str(), "az-Latn-x-latn");
        assert_eq!(tag("I-KLINGON").as_str(), "i-klingon");
    }

    #[test]
    fn case_variants_are_equal() {
        assert_eq!(tag("EN-us"), tag("en-US"));
    }

    #[test]
    fn range_rejects_digit_primary_and_accepts_wildcard() {
        assert_eq!(
            LanguageRange::parse("1de"),
            Err(LanguageTagError::IllFormed)
        );
        assert!(LanguageRange::parse("*").unwrap().is_wildcard());
        assert_eq!(
            LanguageRange::parse("de--ch"),
            Err(LanguageTagError::EmptySubtag)
        );
    }

    #[test]
    fn localizations_rejects_case_duplicate_keys_on_deserialize() {
        let r: Result<Localizations, _> = serde_json::from_value(json!({"de-CH": {}, "de-ch": {}}));
        assert!(r.is_err());
    }

    #[test]
    fn touched_reads_each_path_form() {
        let p: PatchObject = serde_json::from_value(json!({
            "localizations/FR": {"name": "x"},
            "localizations/de/description": null,
            "name": "y",
        }))
        .unwrap();
        let Touched::Some(t) = touched(&p) else {
            panic!("whole-map set reported for per-language paths");
        };
        assert_eq!(t.get(&tag("fr")), Some(&None));
        assert_eq!(
            t.get(&tag("de")),
            Some(&Some(vec!["description".to_owned()]))
        );
    }
}
