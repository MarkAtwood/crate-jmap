//! `language`, `locale` and `localizations` on `Foo/get`
//! (draft-atwood-jmap-updates-00 §3, §4.1, §4.2.1).
//!
//! Oracle: the draft's normative text and its Appendix A example at
//! jmap-chat-spec 2b85363 (AddressBook/get with `language: ["fr-CH",
//! "de"]` returns `"name": "Contacts du groupe", "locale": "fr"`).
//! Expected values are written out by hand from that text.

use std::sync::Mutex;

use jmap_server::{
    BackendChangesError, BackendQueryError, ChangesParams, ChangesResult, GetObject, I18nInfo, Id,
    JmapAccounts, JmapObject, JmapReads, QueryChangesParams, QueryChangesResult, QueryObject,
    QueryParams, QueryResult, State, handle_get, handle_get_with_metadata,
};
use jmap_types::MethodErrorType;
use jmap_types::i18n::{JMAP_I18N_URI, LocalizableProperty};
use serde_json::{Map, Value, json};

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

/// An AddressBook-like type: `name` and `description` are localizable
/// (draft §5.3). Unknown members round-trip through `extra`.
#[derive(serde::Serialize, serde::Deserialize)]
struct Book {
    id: String,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

impl JmapObject for Book {
    const TYPE_NAME: &'static str = "Book";
    type Property = ();
    const LOCALIZABLE: &'static [LocalizableProperty] = &[
        LocalizableProperty::new("name", false),
        LocalizableProperty::new("description", true),
    ];
}

impl GetObject for Book {}

/// The same shape without localizable properties (like Identity, §5.1).
#[derive(serde::Serialize, serde::Deserialize)]
struct Plain {
    id: String,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

impl JmapObject for Plain {
    const TYPE_NAME: &'static str = "Plain";
    type Property = ();
}

impl GetObject for Plain {}

/// Account `a1` supports localizations; `a2` exists but does not.
/// `get_objects` returns two stored books (the draft's Appendix A.1
/// object and one with no locale data) and records the `properties` it
/// was asked for.
#[derive(Default)]
struct Backend {
    asked: Mutex<Option<Option<Vec<String>>>>,
}

impl JmapAccounts for Backend {
    type Error = MockError;
    type CallerCtx = ();

    async fn account_exists(&self, _: &(), account_id: &Id) -> Result<bool, MockError> {
        Ok(matches!(account_id.as_ref(), "a1" | "a2"))
    }

    fn i18n_info(&self, _: &(), account_id: &Id) -> Option<I18nInfo> {
        (account_id.as_ref() == "a1").then(I18nInfo::new)
    }
}

impl JmapReads for Backend {
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: Option<&[Id]>,
        properties: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        *self.asked.lock().unwrap() = Some(properties.map(<[String]>::to_vec));
        let list = vec![
            serde_json::from_value(json!({
                "id": "ab1",
                "name": "Team contacts",
                "description": null,
                "locale": "en",
                "localizations": {
                    "fr": { "name": "Contacts du groupe" },
                    "de": { "name": "Team-Kontakte" }
                }
            }))
            .unwrap(),
            // locale and localizations absent, as a backend that skips
            // serializing None would store it.
            serde_json::from_value(json!({ "id": "ab2", "name": "Other", "description": "d" }))
                .unwrap(),
        ];
        Ok((list, vec![]))
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
    ) -> Result<State, MockError> {
        Ok(State::from("s42"))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MockError>> {
        unreachable!("only /get is exercised")
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MockError>> {
        unreachable!("only /get is exercised")
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MockError>> {
        unreachable!("only /get is exercised")
    }
}

fn with_i18n() -> Vec<String> {
    vec![
        "urn:ietf:params:jmap:core".to_owned(),
        JMAP_I18N_URI.to_owned(),
    ]
}

fn without_i18n() -> Vec<String> {
    vec!["urn:ietf:params:jmap:core".to_owned()]
}

async fn get_book(
    backend: &Backend,
    args: Value,
    using: &[String],
) -> Result<Value, MethodErrorType> {
    handle_get_with_metadata::<Book, _>(backend, &(), args, using)
        .await
        .map(|o| o.primary)
        .map_err(|e| e.error_type)
}

/// draft Appendix A.2: the request and response, with `properties`.
/// The backend is asked for `locale` and `localizations` too, since the
/// view needs them; the response holds only the requested ones.
#[tokio::test]
async fn appendix_a_view() {
    let backend = Backend::default();
    let resp = get_book(
        &backend,
        json!({
            "accountId": "a1",
            "ids": ["ab1"],
            "properties": ["name", "locale"],
            "language": ["fr-CH", "de"]
        }),
        &with_i18n(),
    )
    .await
    .unwrap();
    assert_eq!(resp["state"], "s42");
    assert_eq!(
        resp["list"][0],
        json!({ "id": "ab1", "name": "Contacts du groupe", "locale": "fr" })
    );
    let asked = backend.asked.lock().unwrap().clone().unwrap().unwrap();
    for p in ["name", "locale", "localizations"] {
        assert!(
            asked.iter().any(|a| a == p),
            "backend not asked for {p}: {asked:?}"
        );
    }
}

/// §4.2.1: with no range matching, the base values with their `locale`;
/// the view's `localizations` is null (decision 30). An object with no
/// stored locale gets `locale: null`.
#[tokio::test]
async fn view_without_match_and_without_locale() {
    let resp = get_book(
        &Backend::default(),
        json!({ "accountId": "a1", "language": ["ja"] }),
        &with_i18n(),
    )
    .await
    .unwrap();
    assert_eq!(resp["list"][0]["name"], "Team contacts");
    assert_eq!(resp["list"][0]["locale"], "en");
    assert_eq!(resp["list"][0]["localizations"], Value::Null);
    assert_eq!(resp["list"][1]["locale"], Value::Null);
    assert_eq!(resp["list"][1]["name"], "Other");
}

/// §4.1 defaults: with the capability and `language` null, the stored
/// values are returned, and an absent property as `null`.
#[tokio::test]
async fn stored_values_with_null_language() {
    let resp = get_book(
        &Backend::default(),
        json!({ "accountId": "a1", "language": null }),
        &with_i18n(),
    )
    .await
    .unwrap();
    assert_eq!(resp["list"][0]["locale"], "en");
    assert_eq!(
        resp["list"][0]["localizations"]["de"]["name"],
        "Team-Kontakte"
    );
    assert_eq!(resp["list"][1]["locale"], Value::Null);
    assert_eq!(resp["list"][1]["localizations"], Value::Null);
}

/// §3: without the capability in `using` the server "neither returns nor
/// accepts locale and localizations, and it rejects the language
/// argument with invalidArguments".
#[tokio::test]
async fn without_capability_in_using() {
    let backend = Backend::default();
    let resp = get_book(&backend, json!({ "accountId": "a1" }), &without_i18n())
        .await
        .unwrap();
    assert!(resp["list"][0].get("locale").is_none());
    assert!(resp["list"][0].get("localizations").is_none());
    assert_eq!(resp["list"][0]["name"], "Team contacts");

    let err = get_book(
        &backend,
        json!({ "accountId": "a1", "language": ["fr"] }),
        &without_i18n(),
    )
    .await
    .unwrap_err();
    assert_eq!(err, MethodErrorType::InvalidArguments);
}

/// §3: "The same applies to an account whose accountCapabilities does not
/// include urn:ietf:params:jmap:i18n, even when the capability is in
/// using".
#[tokio::test]
async fn account_without_capability() {
    let backend = Backend::default();
    let resp = get_book(&backend, json!({ "accountId": "a2" }), &with_i18n())
        .await
        .unwrap();
    assert!(resp["list"][0].get("locale").is_none());
    let err = get_book(
        &backend,
        json!({ "accountId": "a2", "language": ["fr"] }),
        &with_i18n(),
    )
    .await
    .unwrap_err();
    assert_eq!(err, MethodErrorType::InvalidArguments);
}

/// §4.2: `language` is an argument only of types with localizable
/// properties; elsewhere it is unknown. `null` is the default.
#[tokio::test]
async fn type_without_localizable_properties() {
    let backend = Backend::default();
    let err = handle_get_with_metadata::<Plain, _>(
        &backend,
        &(),
        json!({ "accountId": "a1", "language": ["fr"] }),
        &with_i18n(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type, MethodErrorType::InvalidArguments);

    let resp = handle_get_with_metadata::<Plain, _>(
        &backend,
        &(),
        json!({ "accountId": "a1", "language": null }),
        &with_i18n(),
    )
    .await
    .unwrap()
    .primary;
    // Not localizable: stored members pass through untouched.
    assert_eq!(resp["list"][0]["locale"], "en");
}

/// `handle_get` sees no `using`, so it behaves as without the capability.
#[tokio::test]
async fn plain_handle_get_treats_capability_as_absent() {
    let backend = Backend::default();
    let err = handle_get::<Book, _>(
        &backend,
        &(),
        json!({ "accountId": "a1", "language": ["fr"] }),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error_type, MethodErrorType::InvalidArguments);
    let resp = handle_get::<Book, _>(&backend, &(), json!({ "accountId": "a1" }))
        .await
        .unwrap()
        .primary;
    assert!(resp["list"][0].get("localizations").is_none());
}

/// §4.2: `language` is `String[]` of RFC 4647 §2.1 ranges.
#[tokio::test]
async fn malformed_language() {
    for bad in [json!("fr"), json!([1]), json!(["fr_CH"]), json!(["de-*"])] {
        let err = get_book(
            &Backend::default(),
            json!({ "accountId": "a1", "language": bad }),
            &with_i18n(),
        )
        .await
        .unwrap_err();
        assert_eq!(err, MethodErrorType::InvalidArguments, "language {bad}");
    }
}

/// §4.2.1: "Lookup skips a range "*" that other ranges follow, and a
/// final "*" selects the default".
#[tokio::test]
async fn wildcard_ranges() {
    let resp = get_book(
        &Backend::default(),
        json!({ "accountId": "a1", "ids": ["ab1"], "language": ["*", "de"] }),
        &with_i18n(),
    )
    .await
    .unwrap();
    assert_eq!(resp["list"][0]["name"], "Team-Kontakte");
    assert_eq!(resp["list"][0]["locale"], "de");

    let resp = get_book(
        &Backend::default(),
        json!({ "accountId": "a1", "ids": ["ab1"], "language": ["it", "*"] }),
        &with_i18n(),
    )
    .await
    .unwrap();
    assert_eq!(resp["list"][0]["name"], "Team contacts");
    assert_eq!(resp["list"][0]["locale"], "en");
}

/// `I18nProjection::without_language` is `from_args` on arguments with no
/// `language`, for each mode that can arise (§3: a missing `language` is
/// the default): stored values with the capability in use, the two
/// properties stripped without it, and untouched for a type with no
/// localizable properties. Expected objects are written by hand.
#[test]
fn without_language_matches_from_args_with_no_language() {
    use jmap_server::I18nProjection;

    let stored = json!({
        "id": "ab1",
        "name": "Team contacts",
        "locale": "en",
        "localizations": { "fr": { "name": "Contacts du groupe" } }
    });
    let info = I18nInfo::new();
    let cases: [(&[String], Option<&I18nInfo>, Value); 3] = [
        (
            &with_i18n(),
            Some(&info),
            json!({
                "id": "ab1",
                "name": "Team contacts",
                "locale": "en",
                "localizations": { "fr": { "name": "Contacts du groupe" } }
            }),
        ),
        (
            &without_i18n(),
            Some(&info),
            json!({ "id": "ab1", "name": "Team contacts" }),
        ),
        (
            &with_i18n(),
            None,
            json!({ "id": "ab1", "name": "Team contacts" }),
        ),
    ];
    for (using, info, expected) in cases {
        let built = I18nProjection::without_language::<Book>(using, info);
        let parsed = I18nProjection::from_args::<Book>(&mut Map::new(), using, info).unwrap();
        for projection in [built, parsed] {
            let mut item = stored.as_object().unwrap().clone();
            projection.apply(&mut item);
            assert_eq!(Value::Object(item), expected);
        }
    }

    let plain = I18nProjection::without_language::<Plain>(&with_i18n(), Some(&info));
    let mut item = stored.as_object().unwrap().clone();
    plain.apply(&mut item);
    assert_eq!(
        Value::Object(item),
        stored,
        "no localizable properties: untouched"
    );
}
