//! Server side of the `urn:ietf:params:jmap:i18n` capability
//! (draft-atwood-jmap-updates-00 §3 and §4).
//!
//! A backend says which accounts support localizations through
//! [`JmapBackend::i18n_info`](crate::JmapBackend::i18n_info). `/get`
//! handlers build an [`I18nProjection`] per request; it reads the
//! `language` argument, rejects it where the draft says to, and shapes
//! `locale` and `localizations` on every returned object. `/set` handlers
//! build an [`I18nSet`]; backends run [`localized_update`] inside each
//! update of a type with localizable properties.
//!
//! Design: `docs/design/updates.md` (decisions 27 to 31).

use crate::{JmapObject, SetError, SetErrorType};
use jmap_types::i18n::{
    JMAP_I18N_URI, LanguageRange, LocalizableProperty, check_localizations, localized_view,
    parse_locale, prune_stale_localizations,
};
use jmap_types::{JmapError, PatchObject};
use serde_json::{Map, Value};

/// What a backend supports for `urn:ietf:params:jmap:i18n` in one
/// account, as [`JmapBackend::i18n_info`](crate::JmapBackend::i18n_info)
/// returns it.
///
/// Build one with [`I18nInfo::new`] and the `with_*` setters.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct I18nInfo {
    /// The most localizations stored per object, or `None` for no limit
    /// (§4.1: "A server MAY limit the number of localizations it stores
    /// per object").
    pub max_localizations: Option<usize>,
}

impl I18nInfo {
    /// An account that supports localizations, with no limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`max_localizations`](Self::max_localizations).
    #[must_use]
    pub fn with_max_localizations(mut self, max: Option<usize>) -> Self {
        self.max_localizations = max;
        self
    }
}

/// `true` if `using` includes `urn:ietf:params:jmap:i18n`
/// (draft-atwood-jmap-updates-00 §3). Capability URIs compare as exact
/// strings (§7.1).
pub fn uses_i18n(using: &[String]) -> bool {
    using.iter().any(|u| u == JMAP_I18N_URI)
}

#[derive(Debug, Clone, Default, PartialEq)]
enum Mode {
    /// The type has no localizable properties.
    #[default]
    Off,
    /// The request or the account does not use the capability: the two
    /// properties are not returned.
    Strip,
    /// The capability is in use and `language` is null: stored values,
    /// with `null` for an absent property.
    Stored,
    /// `language` is set: the single-language view.
    View(Vec<LanguageRange>),
}

/// How a `Foo/get` handler returns `locale` and `localizations`
/// (draft-atwood-jmap-updates-00 §3, §4.2.1).
///
/// Build one per request with [`I18nProjection::from_args`], ask the
/// backend for [`I18nProjection::backend_properties`], then call
/// [`I18nProjection::apply`] on every returned object before filtering it
/// to the requested `properties`. [`handle_get`](crate::handle_get) and
/// [`handle_get_with_metadata`](crate::handle_get_with_metadata) do all
/// three; a type-specific `/get` handler calls them itself.
#[derive(Debug, Clone, Default)]
pub struct I18nProjection {
    mode: Mode,
    properties: &'static [LocalizableProperty],
}

impl I18nProjection {
    /// Read and remove the `language` argument of a `Foo/get`,
    /// `Foo/query` or `Foo/queryChanges` for type `O`.
    ///
    /// `using` is the request's capability list and `info` the account's
    /// [`JmapBackend::i18n_info`](crate::JmapBackend::i18n_info). A
    /// missing or `null` `language` is the default and always accepted.
    ///
    /// # Errors
    ///
    /// `invalidArguments` when `language` is non-null and `O` has no
    /// localizable properties, `urn:ietf:params:jmap:i18n` is not in
    /// `using`, the account does not support it (§3: "as it would any
    /// unknown argument"), or the value is not an array of RFC 4647
    /// language ranges.
    pub fn from_args<O: JmapObject>(
        args: &mut Map<String, Value>,
        using: &[String],
        info: Option<&I18nInfo>,
    ) -> Result<Self, JmapError> {
        let language = args.remove("language");
        let ranges = match language {
            None | Some(Value::Null) => None,
            Some(v) => {
                if O::LOCALIZABLE.is_empty() {
                    return Err(JmapError::invalid_arguments(format!(
                        "language: {} has no localizable properties",
                        O::TYPE_NAME
                    )));
                }
                if !uses_i18n(using) {
                    return Err(JmapError::invalid_arguments(
                        "language requires urn:ietf:params:jmap:i18n in using",
                    ));
                }
                if info.is_none() {
                    return Err(JmapError::invalid_arguments(
                        "language: the account does not support urn:ietf:params:jmap:i18n",
                    ));
                }
                Some(parse_ranges(&v)?)
            }
        };
        let mode = if O::LOCALIZABLE.is_empty() {
            Mode::Off
        } else if !uses_i18n(using) || info.is_none() {
            Mode::Strip
        } else if let Some(r) = ranges {
            Mode::View(r)
        } else {
            Mode::Stored
        };
        Ok(Self {
            mode,
            properties: O::LOCALIZABLE,
        })
    }

    /// The `language` ranges, when the request set them.
    pub fn language(&self) -> Option<&[LanguageRange]> {
        match &self.mode {
            Mode::View(r) => Some(r),
            _ => None,
        }
    }

    /// The `properties` to ask the backend for, given the client's
    /// `requested` list: the view needs `locale` and `localizations` even
    /// when the client did not ask for them. `None` (all properties)
    /// stays `None`.
    pub fn backend_properties(&self, requested: Option<&[String]>) -> Option<Vec<String>> {
        let mut props = requested?.to_vec();
        if matches!(self.mode, Mode::View(_)) {
            for p in ["locale", "localizations"] {
                if !props.iter().any(|q| q == p) {
                    props.push(p.to_owned());
                }
            }
        }
        Some(props)
    }

    /// Shape `locale` and `localizations` on one serialized object.
    ///
    /// Without the capability in use they are removed. With it, an absent
    /// one is returned as `null` (its default, §4.1), and with `language`
    /// set the object becomes its single-language view
    /// ([`localized_view`]). `state` is not affected (§4.2.1).
    pub fn apply(&self, item: &mut Map<String, Value>) {
        match &self.mode {
            Mode::Off => {}
            Mode::Strip => {
                item.remove("locale");
                item.remove("localizations");
            }
            Mode::Stored => {
                for p in ["locale", "localizations"] {
                    item.entry(p).or_insert(Value::Null);
                }
            }
            Mode::View(ranges) => localized_view(item, self.properties, ranges),
        }
    }
}

fn parse_ranges(v: &Value) -> Result<Vec<LanguageRange>, JmapError> {
    let bad = || JmapError::invalid_arguments("language must be an array of language ranges");
    let Value::Array(items) = v else {
        return Err(bad());
    };
    items
        .iter()
        .map(|i| {
            i.as_str().ok_or_else(bad).and_then(|s| {
                LanguageRange::parse(s)
                    .map_err(|e| JmapError::invalid_arguments(format!("language {s:?}: {e}")))
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// /set
// ---------------------------------------------------------------------------

fn invalid(property: &str, description: impl Into<String>) -> Box<SetError> {
    Box::new(
        SetError::new(SetErrorType::InvalidProperties)
            .with_properties([property])
            .with_description(description),
    )
}

/// The wire property a `/set` create key or update patch path writes, when
/// it is `locale` or `localizations` (`"localizations/fr/name"` writes
/// `localizations`).
fn localized_root(key: &str) -> Option<&'static str> {
    match key.split('/').next() {
        Some("locale") => Some("locale"),
        Some("localizations") => Some("localizations"),
        _ => None,
    }
}

/// How a `Foo/set` handler treats `locale` and `localizations`
/// (draft-atwood-jmap-updates-00 §3, §4.1).
///
/// Build one per request with [`I18nSet::new`]. Call
/// [`I18nSet::check_create`] on each create object and
/// [`I18nSet::check_patch`] on each update patch before handing them to the
/// backend, and [`I18nSet::echo`] on each object returned in `created` or
/// `updated`. The checks that need the patched object (a key equal to the
/// new `locale`, pruning) run in the backend through [`localized_update`],
/// inside the same atomic update.
#[derive(Debug, Clone, Default)]
pub struct I18nSet {
    /// `None`: the type has no localizable properties.
    properties: Option<&'static [LocalizableProperty]>,
    /// The request uses the capability and the account supports it.
    allowed: bool,
    max_localizations: Option<usize>,
}

impl I18nSet {
    /// The handling for type `O`, given the request's `using` and the
    /// account's [`JmapBackend::i18n_info`](crate::JmapBackend::i18n_info).
    pub fn new<O: JmapObject>(using: &[String], info: Option<&I18nInfo>) -> Self {
        Self {
            properties: (!O::LOCALIZABLE.is_empty()).then_some(O::LOCALIZABLE),
            allowed: uses_i18n(using) && info.is_some(),
            max_localizations: info.and_then(|i| i.max_localizations),
        }
    }

    fn gate(&self, key: &str) -> Result<(), Box<SetError>> {
        match localized_root(key) {
            Some(root) if self.properties.is_some() && !self.allowed => Err(invalid(
                root,
                format!(
                    "{root} requires urn:ietf:params:jmap:i18n in using and an account that \
                     supports it"
                ),
            )),
            _ => Ok(()),
        }
    }

    /// Check a create object and fold its tags to canonical case.
    ///
    /// Without the capability in use, naming `locale` or `localizations`
    /// is `invalidProperties` (§3). With it, `locale` must be `null` or a
    /// language tag, and `localizations` must follow §4.1
    /// ([`check_localizations`]) and the account's
    /// [`I18nInfo::max_localizations`]. Length and syntax limits of the
    /// defining specification on localized values are the type's handler's
    /// to apply.
    ///
    /// # Errors
    ///
    /// An `invalidProperties` [`SetError`] naming `locale` or
    /// `localizations`.
    pub fn check_create(&self, create: &mut Map<String, Value>) -> Result<(), Box<SetError>> {
        let Some(properties) = self.properties else {
            return Ok(());
        };
        for key in create.keys() {
            self.gate(key)?;
        }
        if !self.allowed {
            return Ok(());
        }
        check_and_fold(create, properties, self.max_localizations)
    }

    /// Check an update patch before the backend applies it.
    ///
    /// Without the capability in use, a patch path into `locale` or
    /// `localizations` is `invalidProperties` (§3). Everything that depends
    /// on the patched object is [`localized_update`]'s.
    ///
    /// # Errors
    ///
    /// An `invalidProperties` [`SetError`] naming `locale` or
    /// `localizations`.
    pub fn check_patch(&self, patch: &Map<String, Value>) -> Result<(), Box<SetError>> {
        if self.properties.is_none() {
            return Ok(());
        }
        patch.keys().try_for_each(|k| self.gate(k))
    }

    /// Shape a `created` or `updated` entry for the client: without the
    /// capability in use, `locale` and `localizations` are removed (§3); with
    /// it they are left as the backend returned them.
    pub fn echo(&self, entry: &mut Value) {
        if self.properties.is_some()
            && !self.allowed
            && let Value::Object(m) = entry
        {
            m.remove("locale");
            m.remove("localizations");
        }
    }
}

/// Parse `locale` and check `localizations` in `obj`, writing both back in
/// canonical case.
fn check_and_fold(
    obj: &mut Map<String, Value>,
    properties: &[LocalizableProperty],
    max: Option<usize>,
) -> Result<(), Box<SetError>> {
    let locale = match obj.get("locale") {
        Some(v) => parse_locale(v)?,
        None => None,
    };
    if let Some(tag) = &locale {
        obj.insert("locale".to_owned(), Value::String(tag.as_str().to_owned()));
    }
    let Some(value) = obj.get("localizations") else {
        return Ok(());
    };
    let locs = check_localizations(locale.as_ref(), value, properties)?;
    if let (Some(locs), Some(max)) = (&locs, max)
        && locs.len() > max
    {
        return Err(invalid(
            "localizations",
            format!("at most {max} localizations per object"),
        ));
    }
    let folded = match locs {
        Some(l) => serde_json::to_value(&l)
            .map_err(|_| invalid("localizations", "localizations could not be serialized"))?,
        None => Value::Null,
    };
    obj.insert("localizations".to_owned(), folded);
    Ok(())
}

/// The draft-atwood-jmap-updates-00 §4.1 rules that need the patched
/// object, for a backend's `update_object` of a type with
/// [`JmapObject::LOCALIZABLE`] properties.
///
/// `old` is the stored object and `new` the result of applying `patch` to
/// it (for example with [`apply_patch_object`](crate::apply_patch_object)),
/// both serialized. It checks `locale` and `localizations` in `new`
/// ([`check_localizations`], so no key may equal the new `locale`), applies
/// `max_localizations` ([`I18nInfo::max_localizations`]), folds tags to
/// canonical case, and then drops translations made stale by a changed
/// base value ([`prune_stale_localizations`]). Run it inside the same
/// atomic update, before validating and writing `new`.
///
/// Pruning applies to every client, including those that do not use the
/// capability: that is the case §4.1 is for.
///
/// Returns `true` when pruning removed anything. The backend MUST then
/// return the updated object (`Ok(Some(_))`) from `update_object`, so that
/// the handler returns `localizations` in `updated` to clients that use the
/// capability (§4.1, RFC 8620 §5.3).
///
/// # Errors
///
/// An `invalidProperties` [`SetError`] naming `locale` or
/// `localizations`; the backend returns it as the update's SetError.
pub fn localized_update<O: JmapObject>(
    old: &Map<String, Value>,
    new: &mut Map<String, Value>,
    patch: &PatchObject,
    max_localizations: Option<usize>,
) -> Result<bool, Box<SetError>> {
    if O::LOCALIZABLE.is_empty() {
        return Ok(false);
    }
    check_and_fold(new, O::LOCALIZABLE, max_localizations)?;
    Ok(prune_stale_localizations(old, new, patch, O::LOCALIZABLE))
}

// ---------------------------------------------------------------------------
// /query and /queryChanges: queryState
// ---------------------------------------------------------------------------

/// Separates the backend's queryState from the language in a wrapped state.
/// Language ranges never contain `|`, so the last occurrence is the mark.
const LANG_MARK: &str = "|lang=";

fn join_ranges(language: &[LanguageRange]) -> String {
    language
        .iter()
        .map(LanguageRange::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

/// The `queryState` to return from a `Foo/query` whose `language` was
/// `language` (draft-atwood-jmap-updates-00 §4.2.3: "The queryState of a
/// /query result reflects the language it was computed with").
///
/// With `language` `None` the backend's `state` is returned unchanged.
/// Otherwise it is wrapped as `<state>|lang=<r1>,<r2>`, the ranges in
/// canonical case (decision 28). Backend query states MUST NOT contain
/// `|lang=`. [`unwrap_query_state`] reverses it.
///
/// ```rust
/// use jmap_server::{localized_query_state, unwrap_query_state};
/// use jmap_types::State;
/// use jmap_types::i18n::LanguageRange;
///
/// let de: Vec<LanguageRange> = vec!["DE-ch".parse().unwrap(), "de".parse().unwrap()];
/// let wrapped = localized_query_state(&State::from("17"), Some(&de));
/// assert_eq!(wrapped.as_ref(), "17|lang=de-CH,de");
/// assert_eq!(unwrap_query_state(&wrapped, Some(&de)).unwrap().as_ref(), "17");
/// assert!(unwrap_query_state(&wrapped, None).is_err());
/// ```
pub fn localized_query_state(
    state: &jmap_types::State,
    language: Option<&[LanguageRange]>,
) -> jmap_types::State {
    match language {
        None => state.clone(),
        Some(l) => {
            jmap_types::State::from(format!("{}{LANG_MARK}{}", state.as_ref(), join_ranges(l)))
        }
    }
}

/// The backend's queryState inside a `Foo/queryChanges` `sinceQueryState`,
/// checked against the request's `language`.
///
/// # Errors
///
/// `cannotCalculateChanges` when the language that `sinceQueryState` was
/// computed with differs from `language` (draft-atwood-jmap-updates-00
/// §4.2.3), including when one of them has a language and the other not.
pub fn unwrap_query_state(
    since: &jmap_types::State,
    language: Option<&[LanguageRange]>,
) -> Result<jmap_types::State, JmapError> {
    let s = since.as_ref();
    let (inner, was) = match s.rfind(LANG_MARK) {
        Some(i) => (&s[..i], Some(&s[i + LANG_MARK.len()..])),
        None => (s, None),
    };
    let now = language.map(join_ranges);
    if was != now.as_deref() {
        return Err(JmapError::cannot_calculate_changes());
    }
    Ok(jmap_types::State::from(inner))
}
