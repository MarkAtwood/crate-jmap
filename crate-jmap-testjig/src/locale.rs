//! Accept-Language handling for the jig's own custom strings
//! (draft-atwood-jmap-updates-00 §7.2 and §4.3; RFC 8620 §3.8).
//!
//! The jig localizes the `title` of the problem details it sends, into
//! English (the default) or German, and labels HTTP responses with
//! Content-Language. It never lets Accept-Language change JMAP data: with
//! `urn:ietf:params:jmap:i18n` in `using` only the `language` argument does
//! that (§4.2.1), and the handlers never see the header.
//!
//! One gap follows: without the capability, RFC 9425 §4.1 selects the
//! Quota `description` by Accept-Language, but the jig's caller context is
//! `()`, so `JmapAccounts::accept_language` is `None` and the base
//! `description` is returned.

use jmap_types::i18n::{LanguageTag, accept_language_ranges, lookup};

/// A language the jig has titles for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    En,
    De,
}

impl Lang {
    /// The Content-Language value.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
        }
    }
}

/// The language for an `Accept-Language` field value (RFC 9110 §12.5.4):
/// its ranges ordered by quality (q=0 dropped), then RFC 4647 Lookup over
/// `en` and `de`; English when nothing matches or there is no header.
pub(crate) fn choose(accept_language: Option<&str>) -> Lang {
    let Some(value) = accept_language else {
        return Lang::En;
    };
    let ranges = accept_language_ranges(value);
    let available: Vec<LanguageTag> = ["en", "de"]
        .iter()
        .filter_map(|t| LanguageTag::parse(t).ok())
        .collect();
    match lookup(&ranges, &available).map(LanguageTag::as_str) {
        Some("de") => Lang::De,
        _ => Lang::En,
    }
}

/// The `title` of an `about:blank` or JMAP problem with HTTP `status`, in
/// `lang` (RFC 9457 §4.2.1: the status phrase for `about:blank`).
pub(crate) fn title(status: u16, lang: Lang) -> Option<&'static str> {
    Some(match (status, lang) {
        (400, Lang::En) => "Bad Request",
        (400, Lang::De) => "Ungültige Anfrage",
        (401, Lang::En) => "Unauthorized",
        (401, Lang::De) => "Nicht autorisiert",
        (403, Lang::En) => "Forbidden",
        (403, Lang::De) => "Verboten",
        (404, Lang::En) => "Not Found",
        (404, Lang::De) => "Nicht gefunden",
        (405, Lang::En) => "Method Not Allowed",
        (405, Lang::De) => "Methode nicht erlaubt",
        (426, Lang::En) => "Upgrade Required",
        (426, Lang::De) => "Upgrade erforderlich",
        (500, Lang::En) => "Internal Server Error",
        (500, Lang::De) => "Interner Serverfehler",
        (503, Lang::En) => "Service Unavailable",
        (503, Lang::De) => "Dienst nicht verfügbar",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 9110 §12.5.4 example shape: quality values order the ranges.
    #[test]
    fn chooses_by_quality_then_lookup() {
        assert_eq!(choose(None), Lang::En);
        assert_eq!(choose(Some("de-CH")), Lang::De);
        assert_eq!(
            choose(Some("fr-CH, fr;q=0.9, de;q=0.8, en;q=0.7")),
            Lang::De
        );
        assert_eq!(choose(Some("en;q=0.9, de")), Lang::De);
        assert_eq!(choose(Some("de;q=0, en")), Lang::En);
        assert_eq!(choose(Some("ja")), Lang::En);
        assert_eq!(choose(Some("*")), Lang::En);
        assert_eq!(choose(Some("garbage;;;")), Lang::En);
    }
}
