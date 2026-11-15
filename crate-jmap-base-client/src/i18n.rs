//! Client side of `urn:ietf:params:jmap:i18n`
//! (draft-atwood-jmap-updates-00): the setting the family clients'
//! `SessionClient::with_i18n` stores (decision 32 in
//! `docs/design/DECISIONS.md`).
//!
//! # Caching
//!
//! With a `language` set, `/get` returns each object's view in one
//! language, with the same `state` as the stored object (§4.2.1). A
//! client that caches objects by id and state MUST also key the cache on
//! the language, or it will mix languages. A client that wants to switch
//! language offline can fetch with `language` unset (the stored values
//! and every localization) and build views itself with
//! [`jmap_types::i18n::localized_view`].
//!
//! # Display
//!
//! The draft carries the language of display text, not its direction.
//! When a client knows no base direction it SHOULD use the first strong
//! directional character of the text, not the language tag, and isolate
//! text from other users (§8).

use jmap_types::JmapRequest;
use jmap_types::i18n::{JMAP_I18N_URI, LanguageRange};

use crate::{ClientError, Session};

/// Client-wide use of `urn:ietf:params:jmap:i18n`: adds the capability to
/// `using` (so `locale` and `localizations` are returned and accepted) and,
/// when [`language`](Self::language) is set, sends it as the `language`
/// argument of `/get`, `/query` and `/queryChanges` on types with
/// localizable properties.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct I18nSetting {
    /// The `language` argument: RFC 4647 language ranges, most preferred
    /// first. `None` returns stored values with every localization.
    pub language: Option<Vec<LanguageRange>>,
}

impl I18nSetting {
    /// The setting for `session`, with `language`.
    ///
    /// # Errors
    ///
    /// [`ClientError::InvalidSession`] when the Session does not advertise
    /// `urn:ietf:params:jmap:i18n`: a request naming an unsupported
    /// capability in `using` fails as a whole (RFC 8620 §3.6.1
    /// `unknownCapability`).
    pub fn new(
        session: &Session,
        language: Option<Vec<LanguageRange>>,
    ) -> Result<Self, ClientError> {
        if !session.capabilities.contains_key(JMAP_I18N_URI) {
            return Err(ClientError::InvalidSession(format!(
                "the session does not advertise {JMAP_I18N_URI}"
            )));
        }
        Ok(Self { language })
    }

    /// Add `urn:ietf:params:jmap:i18n` to `request.using` unless present.
    pub fn add_to_using(&self, request: &mut JmapRequest) {
        if !request.using.iter().any(|u| u == JMAP_I18N_URI) {
            request.using.push(JMAP_I18N_URI.to_owned());
        }
    }

    /// Set the `language` argument in `args` when
    /// [`language`](Self::language) is set.
    pub fn add_language(&self, args: &mut serde_json::Value) {
        if let (Some(l), Some(map)) = (&self.language, args.as_object_mut()) {
            map.insert(
                "language".to_owned(),
                serde_json::Value::Array(
                    l.iter()
                        .map(|r| serde_json::Value::String(r.as_str().to_owned()))
                        .collect(),
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(caps: serde_json::Value) -> Session {
        serde_json::from_value(json!({
            "capabilities": caps,
            "accounts": {},
            "primaryAccounts": {},
            "username": "u",
            "apiUrl": "https://example.com/api",
            "downloadUrl": "https://example.com/d/{accountId}/{blobId}/{name}?type={type}",
            "uploadUrl": "https://example.com/u/{accountId}",
            "eventSourceUrl": "https://example.com/es",
            "state": "s1"
        }))
        .unwrap()
    }

    #[test]
    fn requires_the_capability_in_the_session() {
        let without = session(json!({ "urn:ietf:params:jmap:core": {} }));
        assert!(matches!(
            I18nSetting::new(&without, None),
            Err(ClientError::InvalidSession(_))
        ));
        let with = session(json!({ "urn:ietf:params:jmap:core": {}, JMAP_I18N_URI: {} }));
        assert!(I18nSetting::new(&with, None).is_ok());
    }

    #[test]
    fn adds_capability_once_and_language() {
        let with = session(json!({ JMAP_I18N_URI: {} }));
        let s = I18nSetting::new(
            &with,
            Some(vec!["fr-CH".parse().unwrap(), "fr".parse().unwrap()]),
        )
        .unwrap();
        let mut req = JmapRequest::new(vec!["urn:ietf:params:jmap:core".into()], vec![], None);
        s.add_to_using(&mut req);
        s.add_to_using(&mut req);
        assert_eq!(req.using, vec!["urn:ietf:params:jmap:core", JMAP_I18N_URI]);
        let mut args = json!({ "accountId": "a1" });
        s.add_language(&mut args);
        assert_eq!(
            args,
            json!({ "accountId": "a1", "language": ["fr-CH", "fr"] })
        );
        let mut args = json!({ "accountId": "a1" });
        I18nSetting::new(&with, None)
            .unwrap()
            .add_language(&mut args);
        assert_eq!(args, json!({ "accountId": "a1" }));
    }
}
