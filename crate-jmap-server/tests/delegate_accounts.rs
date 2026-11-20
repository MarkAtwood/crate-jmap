//! `DelegateJmapAccounts`: a wrapper overrides the `JmapAccounts` answers
//! it changes and keeps every other one of the backend it wraps
//! (toy-consumer finding a1, bd:JMAP-saks).
//!
//! Oracle: the contract in the `DelegateJmapAccounts` docs. `Inner`
//! overrides every `JmapAccounts` method with a value no trait default
//! gives, so an answer that fell back to a default instead of reaching
//! `Inner` would show. Expected values are written out by hand.

use jmap_server::{
    BackendChangesError, BackendQueryError, ChangesParams, ChangesResult, DataTypeMetadataInfo,
    DelegateJmapAccounts, DelegateJmapReads, GetObject, I18nInfo, Id, JmapAccounts, JmapBackend,
    JmapObject, JmapReads, QueryChangesParams, QueryChangesResult, QueryObject, QueryParams,
    QueryResult, State, handle_get,
};
use jmap_types::MethodErrorType;
use serde_json::json;

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

#[derive(serde::Serialize, serde::Deserialize)]
struct Foo {
    id: String,
}

impl JmapObject for Foo {
    const TYPE_NAME: &'static str = "Foo";
    type Property = ();
}

impl GetObject for Foo {}

/// The caller context: a resolved principal and an Accept-Language.
#[derive(Clone)]
struct Caller {
    principal: Id,
    language: String,
}

fn caller() -> Caller {
    Caller {
        principal: Id::from("alice"),
        language: "de".to_owned(),
    }
}

fn metadata_info() -> DataTypeMetadataInfo {
    serde_json::from_value(json!({
        "namespaces": ["com.example"],
        "supportsVendorNamespaces": true,
        "supportsPrivate": true,
        "maxDepth": 3
    }))
    .expect("metadata fixture")
}

/// Knows account `inner-only`; overrides every `JmapAccounts` method.
struct Inner;

impl JmapAccounts for Inner {
    type Error = MockError;
    type CallerCtx = Caller;

    async fn account_exists(&self, _: &Caller, account_id: &Id) -> Result<bool, MockError> {
        Ok(account_id.as_ref() == "inner-only")
    }

    fn principal_id(caller: &Caller) -> Option<&Id> {
        Some(&caller.principal)
    }

    fn accept_language(caller: &Caller) -> Option<&str> {
        Some(&caller.language)
    }

    fn max_objects_in_set(&self, _: &Caller, _: &Id) -> u64 {
        7
    }

    fn max_objects_in_get(&self, _: &Caller, _: &Id) -> u64 {
        9
    }

    fn metadata_info<O: JmapObject>(&self, _: &Caller, _: &Id) -> Option<DataTypeMetadataInfo> {
        (O::TYPE_NAME == "Foo").then(metadata_info)
    }

    fn i18n_info(&self, _: &Caller, _: &Id) -> Option<I18nInfo> {
        Some(I18nInfo::new().with_max_localizations(Some(4)))
    }
}

impl JmapReads for Inner {
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _: &Caller,
        _: &Id,
        _: Option<&[Id]>,
        _: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        let object = serde_json::from_value(json!({ "id": "f1" })).expect("Foo fixture");
        Ok((vec![object], vec![]))
    }

    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _: &Caller,
        _: &Id,
    ) -> Result<State, MockError> {
        Ok(State::from("s1"))
    }

    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _: &Caller,
        _: &Id,
        _: ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MockError>> {
        unreachable!("not exercised")
    }

    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &Caller,
        _: &Id,
        _: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MockError>> {
        unreachable!("not exercised")
    }

    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &Caller,
        _: &Id,
        _: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MockError>> {
        unreachable!("not exercised")
    }
}

/// Overrides `account_exists` (only `outer`) and delegates the rest of
/// `JmapAccounts` and all the reads.
struct Wrapper {
    inner: Inner,
}

impl DelegateJmapAccounts for Wrapper {
    type Target = Inner;

    fn jmap_accounts(&self) -> &Inner {
        &self.inner
    }

    async fn account_exists(&self, _: &Caller, account_id: &Id) -> Result<bool, MockError> {
        Ok(account_id.as_ref() == "outer")
    }
}

impl DelegateJmapReads for Wrapper {
    type Target = Inner;

    fn jmap_reads(&self) -> &Inner {
        &self.inner
    }
}

/// Overrides nothing but `max_objects_in_set` and `principal_id`.
struct Limiter {
    inner: Inner,
}

impl DelegateJmapAccounts for Limiter {
    type Target = Inner;

    fn jmap_accounts(&self) -> &Inner {
        &self.inner
    }

    fn principal_id(_: &Caller) -> Option<&Id> {
        None
    }

    fn max_objects_in_set(&self, _: &Caller, _: &Id) -> u64 {
        1
    }
}

/// Compile-time: the two delegates make a `JmapBackend`.
fn assert_backend<B: JmapBackend>() {}

#[test]
fn wrapper_is_a_jmap_backend() {
    assert_backend::<Wrapper>();
}

/// `account_exists` is the wrapper's; every other answer is `Inner`'s,
/// including the associated functions with no `self`.
#[tokio::test]
async fn unoverridden_answers_are_the_inner_backends() {
    let w = Wrapper { inner: Inner };
    let c = caller();
    let a = Id::from("outer");
    assert!(JmapAccounts::account_exists(&w, &c, &a).await.unwrap());
    assert!(
        !JmapAccounts::account_exists(&w, &c, &Id::from("inner-only"))
            .await
            .unwrap()
    );
    assert_eq!(
        <Wrapper as JmapAccounts>::principal_id(&c),
        Some(&Id::from("alice"))
    );
    assert_eq!(<Wrapper as JmapAccounts>::accept_language(&c), Some("de"));
    assert_eq!(JmapAccounts::max_objects_in_set(&w, &c, &a), 7);
    assert_eq!(JmapAccounts::max_objects_in_get(&w, &c, &a), 9);
    assert_eq!(
        JmapAccounts::metadata_info::<Foo>(&w, &c, &a),
        Some(metadata_info())
    );
    assert_eq!(
        JmapAccounts::i18n_info(&w, &c, &a),
        Some(I18nInfo::new().with_max_localizations(Some(4)))
    );
}

/// Each method can be overridden alone, the associated functions too;
/// the rest still come from the target.
#[tokio::test]
async fn one_method_overridden_alone() {
    let l = Limiter { inner: Inner };
    let c = caller();
    let a = Id::from("inner-only");
    assert_eq!(JmapAccounts::max_objects_in_set(&l, &c, &a), 1);
    assert_eq!(<Limiter as JmapAccounts>::principal_id(&c), None);
    assert_eq!(JmapAccounts::max_objects_in_get(&l, &c, &a), 9);
    assert_eq!(<Limiter as JmapAccounts>::accept_language(&c), Some("de"));
    assert!(JmapAccounts::account_exists(&l, &c, &a).await.unwrap());
}

/// Through a standard handler: the wrapper's accounts decide
/// `accountNotFound` (RFC 8620 §3.6.2), and the inner reads answer.
#[tokio::test]
async fn handlers_see_the_wrappers_accounts() {
    let w = Wrapper { inner: Inner };
    let out = handle_get::<Foo, _>(&w, &caller(), json!({ "accountId": "outer", "ids": null }))
        .await
        .expect("Foo/get on the wrapper's account");
    assert_eq!(out.primary["list"], json!([{ "id": "f1" }]));
    assert_eq!(out.primary["state"], "s1");

    let err = handle_get::<Foo, _>(
        &w,
        &caller(),
        json!({ "accountId": "inner-only", "ids": null }),
    )
    .await
    .expect_err("the wrapper's account_exists decides");
    assert_eq!(err.error_type, MethodErrorType::AccountNotFound);
}
