//! Quota localization (JMAP-cayv.11).
//!
//! Oracle: draft-atwood-jmap-updates-00 §5.2 ("When the capability is not
//! in `using`, the server selects `description` from Accept-Language as
//! RFC 9425 Section 4.1 describes. When it is in `using` ... with
//! `language` null, the server returns the base values with their
//! `locale` and all `localizations`, and with `language` set, it selects
//! as Section 4.2 describes"), §4.2.1 (Accept-Language MUST NOT change
//! the result with the capability in `using`), RFC 9425 §4.1 and RFC 9110
//! §12.5.4. Expected values are hand-written.

use jmap_quotas_server::memory::{MemoryBackend, MemoryError};
use jmap_quotas_server::{QuotasBackend, handle_quota_get, handle_quota_query};
use jmap_server::{
    BackendChangesError, BackendQueryError, ChangesParams, ChangesResult, GetObject, JmapBackend,
    JmapObject, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams, QueryResult,
};
use jmap_types::{Id, State};
use serde_json::{Value, json};

const ACC: &str = "acc1";
const MAIL: &str = "urn:ietf:params:jmap:mail";
const I18N: &str = "urn:ietf:params:jmap:i18n";

/// The memory backend with the request's Accept-Language as caller
/// context.
struct WithAcceptLanguage(MemoryBackend);

impl JmapBackend for WithAcceptLanguage {
    type Error = MemoryError;
    type CallerCtx = Option<String>;

    fn accept_language(caller: &Option<String>) -> Option<&str> {
        caller.as_deref()
    }
    fn i18n_info(&self, _: &Option<String>, a: &Id) -> Option<jmap_server::I18nInfo> {
        self.0.i18n_info(&(), a)
    }
    async fn account_exists(&self, _: &Option<String>, a: &Id) -> Result<bool, MemoryError> {
        self.0.account_exists(&(), a).await
    }
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _: &Option<String>,
        a: &Id,
        ids: Option<&[Id]>,
        props: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MemoryError> {
        self.0.get_objects(&(), a, ids, props).await
    }
    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _: &Option<String>,
        a: &Id,
    ) -> Result<State, MemoryError> {
        self.0.get_state::<O>(&(), a).await
    }
    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _: &Option<String>,
        a: &Id,
        p: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MemoryError>> {
        self.0.get_changes::<O>(&(), a, p).await
    }
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &Option<String>,
        a: &Id,
        p: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MemoryError>> {
        self.0.query_objects(&(), a, p).await
    }
    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &Option<String>,
        a: &Id,
        p: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MemoryError>> {
        self.0.query_changes(&(), a, p).await
    }
}

impl QuotasBackend for WithAcceptLanguage {}

fn quota(id: &str, name: &str, de_name: &str) -> Value {
    json!({
        "id": id, "resourceType": "octets", "used": 1, "hardLimit": 10,
        "scope": "account", "name": name, "types": ["Email"],
        "description": format!("{name} storage"),
        "locale": "en",
        "localizations": {
            "de": { "name": de_name, "description": format!("{de_name} Speicher") },
            "fr": { "description": null }
        }
    })
}

fn backend() -> WithAcceptLanguage {
    let b = MemoryBackend::new().with_account(&Id::from(ACC));
    b.seed_quota(&Id::from(ACC), quota("q1", "Mail", "Post"));
    b.seed_quota(&Id::from(ACC), quota("q2", "Attachments", "Anhänge"));
    WithAcceptLanguage(b)
}

async fn get(al: Option<&str>, args: Value, caps: &[&str]) -> Result<Value, String> {
    let mut args = args;
    args["accountId"] = json!(ACC);
    let mut using = vec![
        "urn:ietf:params:jmap:core".to_owned(),
        "urn:ietf:params:jmap:quota".to_owned(),
    ];
    using.extend(caps.iter().map(|&c| c.to_owned()));
    handle_quota_get(&backend(), &al.map(str::to_owned), args, &using)
        .await
        .map(|o| o.primary)
        .map_err(|e| e.error_type.to_string())
}

/// RFC 9425 §4.1 without i18n: `description` by Accept-Language
/// (quality-ordered, RFC 9110 §12.5.4), `name` stays the base value, and
/// `locale` / `localizations` are not returned.
#[tokio::test]
async fn description_follows_accept_language_without_i18n() {
    let resp = get(
        Some("ja, de-CH;q=0.8, en;q=0.5"),
        json!({ "ids": ["q1"] }),
        &[MAIL],
    )
    .await
    .unwrap();
    let q = &resp["list"][0];
    assert_eq!(q["description"], "Post Speicher");
    assert_eq!(q["name"], "Mail");
    assert!(
        q.get("locale").is_none() && q.get("localizations").is_none(),
        "{q}"
    );

    // Only `description` asked for: still selected, nothing extra returned.
    let resp = get(
        Some("de"),
        json!({ "ids": ["q1"], "properties": ["description"] }),
        &[MAIL],
    )
    .await
    .unwrap();
    assert_eq!(
        resp["list"],
        json!([{ "id": "q1", "description": "Post Speicher" }])
    );

    // A localized null description means none in that language.
    let resp = get(Some("fr"), json!({ "ids": ["q1"] }), &[MAIL])
        .await
        .unwrap();
    assert_eq!(resp["list"][0]["description"], Value::Null);

    // No match, or no header: the base value.
    for al in [Some("ja"), None] {
        let resp = get(al, json!({ "ids": ["q1"] }), &[MAIL]).await.unwrap();
        assert_eq!(resp["list"][0]["description"], "Mail storage", "{al:?}");
        assert!(resp["list"][0].get("localizations").is_none());
    }
}

/// §4.2 / §3: `language` needs the capability in `using`.
#[tokio::test]
async fn language_without_i18n_is_invalid() {
    assert_eq!(
        get(Some("de"), json!({ "language": ["de"] }), &[MAIL]).await,
        Err("invalidArguments".to_owned())
    );
}

/// §5.2 with i18n and `language` null: base values, `locale` and all
/// `localizations`; §4.2.1: Accept-Language changes nothing.
#[tokio::test]
async fn stored_values_with_i18n_ignore_accept_language() {
    let resp = get(Some("de"), json!({ "ids": ["q1"] }), &[MAIL, I18N])
        .await
        .unwrap();
    let q = &resp["list"][0];
    assert_eq!(q["name"], "Mail");
    assert_eq!(q["description"], "Mail storage");
    assert_eq!(q["locale"], "en");
    assert_eq!(q["localizations"]["de"]["name"], "Post");
}

/// §5.2 with `language`: the single-language view (§4.2.1).
#[tokio::test]
async fn language_view_with_i18n() {
    let resp = get(
        None,
        json!({ "ids": ["q1"], "language": ["de-AT"] }),
        &[MAIL, I18N],
    )
    .await
    .unwrap();
    let q = &resp["list"][0];
    assert_eq!(
        (&q["name"], &q["description"]),
        (&json!("Post"), &json!("Post Speicher"))
    );
    assert_eq!(q["locale"], "de");
    assert_eq!(q["localizations"], Value::Null);
}

/// §4.2.2: `language` on Quota/query sorts and filters by the localized
/// `name`; Accept-Language does not.
#[tokio::test]
async fn query_by_localized_name() {
    let query = |al: Option<&'static str>, args: Value, caps: &'static [&'static str]| async move {
        let mut args = args;
        args["accountId"] = json!(ACC);
        let mut using = vec![
            "urn:ietf:params:jmap:core".to_owned(),
            "urn:ietf:params:jmap:quota".to_owned(),
        ];
        using.extend(caps.iter().map(|&c| c.to_owned()));
        handle_quota_query(&backend(), &al.map(str::to_owned), args, &using)
            .await
            .unwrap()
            .primary["ids"]
            .clone()
    };
    let by_name = json!([{ "property": "name" }]);
    // Base names: Attachments < Mail.
    assert_eq!(
        query(Some("de"), json!({ "sort": by_name }), &[MAIL]).await,
        json!(["q2", "q1"])
    );
    // German names: Anhänge < Post, same order; a filter tells them apart.
    assert_eq!(
        query(
            None,
            json!({ "filter": { "name": "post" }, "language": ["de"] }),
            &[MAIL, I18N]
        )
        .await,
        json!(["q1"])
    );
    assert_eq!(
        query(
            Some("de"),
            json!({ "filter": { "name": "post" } }),
            &[MAIL, I18N]
        )
        .await,
        json!([])
    );
}
