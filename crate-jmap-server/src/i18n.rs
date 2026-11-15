//! Server side of the `urn:ietf:params:jmap:i18n` capability
//! (draft-atwood-jmap-updates-00 §3 and §4).
//!
//! A backend says which accounts support localizations through
//! [`JmapBackend::i18n_info`](crate::JmapBackend::i18n_info). `/get`
//! handlers build an [`I18nProjection`] per request; it reads the
//! `language` argument, rejects it where the draft says to, and shapes
//! `locale` and `localizations` on every returned object.
//!
//! Design: `docs/design/updates.md` (decisions 27 to 31).

use crate::JmapObject;
use jmap_types::JmapError;
use jmap_types::i18n::{
    JMAP_I18N_URI, LanguageRange, LanguageTag, LocalizableProperty, localized_view,
};
use serde_json::{Map, Value};

/// What a backend supports for `urn:ietf:params:jmap:i18n` in one
/// account, as [`JmapBackend::i18n_info`](crate::JmapBackend::i18n_info)
/// returns it.
///
/// Build one with [`I18nInfo::new`] and the `with_*` setters.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct I18nInfo {
    /// The `preferredLanguages` of the account capability
    /// (draft-atwood-jmap-updates-00 §3): the owner's languages, most
    /// preferred first, or `None` when none are recorded. MUST be `None`
    /// for an account the caller does not own (§9: sharing an account
    /// must not reveal its owner's languages). Handlers never use it to
    /// change `/get` or `/query` results (§3.1).
    pub preferred_languages: Option<Vec<LanguageTag>>,
    /// The most localizations stored per object, or `None` for no limit
    /// (§4.1: "A server MAY limit the number of localizations it stores
    /// per object").
    pub max_localizations: Option<usize>,
}

impl I18nInfo {
    /// An account that supports localizations, with no recorded
    /// language preference and no limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`preferred_languages`](Self::preferred_languages).
    #[must_use]
    pub fn with_preferred_languages(mut self, langs: Option<Vec<LanguageTag>>) -> Self {
        self.preferred_languages = langs;
        self
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
