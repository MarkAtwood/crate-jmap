//! EmailPush validation and normalization (bd JMAP-r44n.5).
//!
//! Oracles: draft-ietf-jmap-emailpush-05 §4, hand-applied: the five
//! `invalidProperties` rules, "MUST include the path to each invalid
//! value", the property and condition minimums for an account without
//! `urn:ietf:params:jmap:mail`, and the §5 request; RFC 8620 §5.3 (`created`
//! / `updated` hold server-changed values); docs/design/DECISIONS.md
//! decisions 9 (normalized configs), 10 (`objectHistory`) and 16 (the
//! deployment calls `check_email_push_change`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use jmap_mail_server::emailpush::{
    EmailCondition, EmailPushAccount, EmailPushBackend, InvalidEmailPushConfig,
    check_email_push_change, check_email_push_config,
};
use jmap_mail_types::Email;
use jmap_metadata_types::DataTypeMetadataInfo;
use jmap_server::{
    BackendChangesError, BackendQueryError, BackendSetError, ChangesResult, Dispatcher, GetObject,
    JmapBackend, JmapObject, JmapRequest, PushSubscriptionBackend, QueryChangesParams,
    QueryChangesResult, QueryObject, QueryParams, QueryResult, SetError, SetErrorType,
    StoredPushSubscription, register_push_subscription_handlers,
};
use jmap_types::{Id, PushSubscription, State};
use serde_json::{Value, json};

const CORE: &str = "urn:ietf:params:jmap:core";
const EMAILPUSH: &str = "urn:ietf:params:jmap:emailpush";

#[derive(Debug)]
struct MockError;

impl std::fmt::Display for MockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mock")
    }
}

impl std::error::Error for MockError {}

/// A deployment-style backend: its create and update call
/// `check_email_push_change` and store what it leaves.
#[derive(Default)]
struct Mock {
    accounts: BTreeMap<Id, EmailPushAccount>,
    metadata: BTreeMap<Id, DataTypeMetadataInfo>,
    subs: Mutex<Vec<StoredPushSubscription>>,
}

impl Mock {
    fn with(accounts: &[(&str, EmailPushAccount)]) -> Self {
        Self {
            accounts: accounts
                .iter()
                .map(|(id, a)| (Id::from(*id), a.clone()))
                .collect(),
            ..Self::default()
        }
    }

    fn stored(&self, id: &str) -> PushSubscription {
        self.subs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.subscription.id.as_ref().map(AsRef::as_ref) == Some(id))
            .map(|s| s.subscription.clone())
            .unwrap()
    }
}

impl JmapBackend for Mock {
    type Error = MockError;
    type CallerCtx = ();

    async fn account_exists(&self, _: &(), _: &Id) -> Result<bool, MockError> {
        unreachable!()
    }
    async fn get_objects<O: GetObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: Option<&[Id]>,
        _: Option<&[String]>,
    ) -> Result<(Vec<O>, Vec<Id>), MockError> {
        unreachable!()
    }
    async fn get_state<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
    ) -> Result<State, MockError> {
        unreachable!()
    }
    async fn get_changes<O: JmapObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: jmap_server::ChangesParams<'_>,
    ) -> Result<ChangesResult, BackendChangesError<MockError>> {
        unreachable!()
    }
    async fn query_objects<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryParams<'_, O>,
    ) -> Result<QueryResult, BackendQueryError<MockError>> {
        unreachable!()
    }
    async fn query_changes<O: QueryObject + Send + Sync>(
        &self,
        _: &(),
        _: &Id,
        _: QueryChangesParams<'_, O>,
    ) -> Result<QueryChangesResult, BackendChangesError<MockError>> {
        unreachable!()
    }
    fn metadata_info<O: JmapObject>(
        &self,
        _: &(),
        account_id: &Id,
    ) -> Option<DataTypeMetadataInfo> {
        (O::TYPE_NAME == Email::TYPE_NAME)
            .then(|| self.metadata.get(account_id).cloned())
            .flatten()
    }
}

impl PushSubscriptionBackend for Mock {
    const PUSH_SUBSCRIPTION_EXTENSION_CAPABILITIES: &'static [(&'static str, &'static str)] =
        &[("emailPush", EMAILPUSH)];

    async fn list_push_subscriptions(
        &self,
        _: &(),
    ) -> Result<Vec<StoredPushSubscription>, MockError> {
        Ok(self.subs.lock().unwrap().clone())
    }

    async fn create_push_subscription(
        &self,
        caller: &(),
        mut new: PushSubscription,
    ) -> Result<StoredPushSubscription, BackendSetError<MockError>> {
        check_email_push_change(self, caller, None, &mut new).await?;
        let mut subs = self.subs.lock().unwrap();
        new.id = Some(Id::from(format!("P{}", subs.len() + 1)));
        let stored = StoredPushSubscription {
            subscription: new,
            expected_code: "code".to_owned(),
        };
        subs.push(stored.clone());
        Ok(stored)
    }

    async fn update_push_subscription(
        &self,
        caller: &(),
        mut sub: PushSubscription,
    ) -> Result<PushSubscription, BackendSetError<MockError>> {
        let old = self
            .subs
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.subscription.id == sub.id)
            .map(|s| s.subscription.clone())
            .ok_or_else(|| {
                BackendSetError::SetError(Box::new(SetError::new(SetErrorType::NotFound)))
            })?;
        check_email_push_change(self, caller, Some(&old), &mut sub).await?;
        let mut subs = self.subs.lock().unwrap();
        let slot = subs
            .iter_mut()
            .find(|s| s.subscription.id == sub.id)
            .unwrap();
        slot.subscription = sub.clone();
        Ok(sub)
    }

    async fn destroy_push_subscription(
        &self,
        _: &(),
        _: &Id,
    ) -> Result<(), BackendSetError<MockError>> {
        unreachable!()
    }
}

impl EmailPushBackend for Mock {
    async fn email_push_accounts(
        &self,
        _: &(),
    ) -> Result<BTreeMap<Id, EmailPushAccount>, MockError> {
        Ok(self.accounts.clone())
    }
}

/// The §5 account: no mail, IMAP with ObjectID.
fn imap_account() -> EmailPushAccount {
    EmailPushAccount::new(false).with_object_ids(true)
}

fn mail_account() -> EmailPushAccount {
    EmailPushAccount::new(true)
}

/// A subscription with keys, carrying `email_push` (absent when `None`).
fn sub(email_push: Option<Value>) -> PushSubscription {
    let mut v = json!({
        "deviceClientId": "a889-ffea-910",
        "url": "https://push.example.com/WmYD3enMWsSOE0nd",
        "keys": {"p256dh": "client-p256dh", "auth": "client-auth"},
        "types": []
    });
    if let Some(e) = email_push {
        v["emailPush"] = e;
    }
    serde_json::from_value(v).unwrap()
}

/// Runs a create through `check_email_push_change`: the stored
/// `emailPush`, or the invalidProperties list.
async fn create(m: &Mock, email_push: Value) -> Result<Value, Vec<String>> {
    let mut s = sub(Some(email_push));
    match check_email_push_change(m, &(), None, &mut s).await {
        Ok(()) => Ok(s.extra["emailPush"].clone()),
        Err(BackendSetError::SetError(e)) => {
            assert_eq!(e.error_type, SetErrorType::InvalidProperties);
            Err(e.properties.clone().unwrap())
        }
        Err(_) => panic!("backend error"),
    }
}

fn paths(v: &[&str]) -> Result<Value, Vec<String>> {
    Err(v.iter().map(|s| (*s).to_owned()).collect())
}

/// §5 Figure 2's emailPush, accepted for the §5 account (no mail,
/// ObjectID), stored with `urgency` filled.
#[tokio::test]
async fn section_5_request_is_accepted() {
    let m = Mock::with(&[("A1412", imap_account())]);
    let config = json!({
        "filter": {"operator": "OR", "conditions": [
            {"inMailbox": "674cc240-95db-49ce-a8a2-054f4f733095"},
            {"hasKeyword": "$notify"}]},
        "properties": ["id", "from", "subject"]
    });
    let mut want = config.clone();
    want["urgency"] = json!("normal");
    assert_eq!(
        create(&m, json!({"A1412": config})).await,
        Ok(json!({"A1412": want}))
    );
}

#[tokio::test]
async fn unknown_account() {
    let m = Mock::with(&[("A", mail_account())]);
    assert_eq!(
        create(&m, json!({"X": {}})).await,
        paths(&["emailPush", "emailPush/X"])
    );
}

#[tokio::test]
async fn non_object_values() {
    let m = Mock::with(&[("A", mail_account())]);
    assert_eq!(
        create(&m, json!({"A": null})).await,
        paths(&["emailPush", "emailPush/A"])
    );
    assert_eq!(create(&m, json!("x")).await, paths(&["emailPush"]));
    assert_eq!(create(&m, json!([])).await, paths(&["emailPush"]));
}

#[tokio::test]
async fn bad_filters() {
    let m = Mock::with(&[("A", mail_account()), ("I", EmailPushAccount::new(false))]);
    let bad = |a: &str| paths(&["emailPush", &format!("emailPush/{a}/filter")]);
    assert_eq!(
        create(&m, json!({"A": {"filter": {"foo": 1}}})).await,
        bad("A")
    );
    // inMailbox without mail and without ObjectID is OPTIONAL, and this
    // account does not declare it.
    assert_eq!(
        create(&m, json!({"I": {"filter": {"inMailbox": "m"}}})).await,
        bad("I")
    );
    // A filter nested 33 FilterOperators deep.
    let mut deep = json!({"hasKeyword": "$x"});
    for _ in 0..33 {
        deep = json!({"operator": "NOT", "conditions": [deep]});
    }
    assert_eq!(create(&m, json!({"A": {"filter": deep}})).await, bad("A"));
    let mut ok = json!({"hasKeyword": "$x"});
    for _ in 0..32 {
        ok = json!({"operator": "NOT", "conditions": [ok]});
    }
    assert!(create(&m, json!({"A": {"filter": ok}})).await.is_ok());
}

/// -05 §4: an account without mail supports at least these ten.
#[tokio::test]
async fn non_mail_required_conditions() {
    let m = Mock::with(&[("I", EmailPushAccount::new(false))]);
    for c in [
        json!({"hasKeyword": "$a"}),
        json!({"notKeyword": "$a"}),
        json!({"from": "x"}),
        json!({"to": "x"}),
        json!({"cc": "x"}),
        json!({"bcc": "x"}),
        json!({"subject": "x"}),
        json!({"header": ["X"]}),
        json!({"minSize": 1}),
        json!({"maxSize": 1}),
    ] {
        assert!(
            create(&m, json!({"I": {"filter": c.clone()}}))
                .await
                .is_ok(),
            "{c}"
        );
    }
    assert!(
        create(&m, json!({"I": {"filter": {"text": "x"}}}))
            .await
            .is_err()
    );
    let declared = Mock::with(&[(
        "I",
        EmailPushAccount::new(false).with_filter_conditions([EmailCondition::Text]),
    )]);
    assert!(
        create(&declared, json!({"I": {"filter": {"text": "x"}}}))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn bad_properties() {
    let m = Mock::with(&[("A", mail_account()), ("I", EmailPushAccount::new(false))]);
    let bad = |a: &str| paths(&["emailPush", &format!("emailPush/{a}/properties")]);
    // "including "bodyValues"".
    assert_eq!(
        create(&m, json!({"A": {"properties": ["bodyValues"]}})).await,
        bad("A")
    );
    // id needs mail or ObjectID.
    assert_eq!(
        create(&m, json!({"I": {"properties": ["id"]}})).await,
        bad("I")
    );
    assert_eq!(
        create(&m, json!({"A": {"properties": ["nonsense"]}})).await,
        bad("A")
    );
    assert_eq!(
        create(&m, json!({"A": {"properties": "from"}})).await,
        bad("A")
    );
    // The eight required names pass without mail.
    let eight = json!([
        "keywords",
        "from",
        "to",
        "cc",
        "subject",
        "preview",
        "receivedAt",
        "sentAt"
    ]);
    assert!(
        create(&m, json!({"I": {"properties": eight}}))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn header_forms() {
    let m = Mock::with(&[
        ("A", mail_account()),
        ("I", EmailPushAccount::new(false)),
        (
            "D",
            EmailPushAccount::new(false).with_properties(["header:Subject:asText"]),
        ),
    ]);
    let p = json!({"properties": ["header:Subject:asText"]});
    assert!(create(&m, json!({"A": p.clone()})).await.is_ok());
    assert_eq!(
        create(&m, json!({"I": p.clone()})).await,
        paths(&["emailPush", "emailPush/I/properties"])
    );
    assert!(create(&m, json!({"D": p})).await.is_ok());
    // RFC 8621 §4.1.2.4: Date is not an address field.
    assert_eq!(
        create(
            &m,
            json!({"A": {"properties": ["header:Date:asAddresses"]}})
        )
        .await,
        paths(&["emailPush", "emailPush/A/properties"])
    );
}

#[tokio::test]
async fn bad_urgency() {
    let m = Mock::with(&[("A", mail_account())]);
    assert_eq!(
        create(&m, json!({"A": {"urgency": "urgent"}})).await,
        paths(&["emailPush", "emailPush/A/urgency"])
    );
}

/// "MUST include the path to each invalid value": every bad path, in map
/// order, with `emailPush` first.
#[tokio::test]
async fn every_bad_path() {
    let m = Mock::with(&[("A", mail_account()), ("B", mail_account())]);
    assert_eq!(
        create(
            &m,
            json!({
                "A": {"properties": ["bodyValues"], "urgency": "urgent"},
                "B": {"filter": {"foo": 1}},
                "C": {}
            })
        )
        .await,
        paths(&[
            "emailPush",
            "emailPush/A/properties",
            "emailPush/A/urgency",
            "emailPush/B/filter",
            "emailPush/C",
        ])
    );
}

/// -05 §4 rule 5: "the "properties" list also includes "keys"".
#[tokio::test]
async fn keys_required() {
    let m = Mock::with(&[("A", mail_account())]);
    let mut s = sub(Some(json!({"A": {}})));
    s.keys = None;
    let Err(BackendSetError::SetError(e)) = check_email_push_change(&m, &(), None, &mut s).await
    else {
        panic!("accepted")
    };
    assert_eq!(e.properties.unwrap(), ["emailPush", "keys"]);
    // null emailPush needs no keys.
    let mut s = sub(Some(Value::Null));
    s.keys = None;
    assert!(check_email_push_change(&m, &(), None, &mut s).await.is_ok());
}

#[tokio::test]
async fn metadata_conditions() {
    let mut m = Mock::with(&[("A", mail_account()), ("P", mail_account())]);
    m.metadata.insert(
        Id::from("P"),
        serde_json::from_value(json!({"maxDepth": null, "supportsPrivate": true})).unwrap(),
    );
    let exists = json!({"filter": {"metadataExists": "example.com:x"}});
    assert_eq!(
        create(&m, json!({"A": exists.clone()})).await,
        paths(&["emailPush", "emailPush/A/filter"])
    );
    assert!(create(&m, json!({"P": exists})).await.is_ok());
    // Without supportsPrivate, the privateMetadata* conditions are not
    // supported (draft-ietf-jmap-metadata-05 §1.2.1).
    m.metadata.insert(
        Id::from("P"),
        serde_json::from_value(json!({"maxDepth": null})).unwrap(),
    );
    assert_eq!(
        create(
            &m,
            json!({"P": {"filter": {"privateMetadataExists": "example.com:x"}}})
        )
        .await,
        paths(&["emailPush", "emailPush/P/filter"])
    );
}

/// Decision 9: defaults filled.
#[tokio::test]
async fn normalization() {
    let m = Mock::with(&[("A", mail_account())]);
    assert_eq!(
        create(&m, json!({"A": {}})).await,
        Ok(json!({"A": {"filter": null, "properties": [], "urgency": "normal"}}))
    );
    assert_eq!(create(&m, json!({})).await, Ok(json!({})));
}

/// Decision 10: `objectHistory` passes where Email/get accepts it and is
/// dropped from the stored list.
#[tokio::test]
async fn object_history_property() {
    let m = Mock::with(&[
        ("H", mail_account().with_email_history(true)),
        ("A", mail_account()),
        ("I", EmailPushAccount::new(false).with_email_history(true)),
    ]);
    let p = json!({"properties": ["from", "objectHistory", "subject"]});
    assert_eq!(
        create(&m, json!({"H": p.clone()})).await,
        Ok(json!({"H": {"filter": null, "properties": ["from", "subject"], "urgency": "normal"}}))
    );
    for a in ["A", "I"] {
        assert_eq!(
            create(&m, json!({a: p.clone()})).await,
            paths(&["emailPush", &format!("emailPush/{a}/properties")])
        );
    }
}

/// The account check runs only when the change includes emailPush.
#[tokio::test]
async fn unchanged_email_push_skips_the_account_check() {
    let m = Mock::with(&[]);
    let old = sub(Some(
        json!({"R": {"filter": null, "properties": [], "urgency": "normal"}}),
    ));
    let mut new = old.clone();
    new.types = Some(vec!["Email".into()]);
    assert!(
        check_email_push_change(&m, &(), Some(&old), &mut new)
            .await
            .is_ok()
    );
    assert_eq!(new.extra, old.extra);

    let mut changed = old.clone();
    changed.extra["emailPush"]["R"]["urgency"] = json!("high");
    let Err(BackendSetError::SetError(e)) =
        check_email_push_change(&m, &(), Some(&old), &mut changed).await
    else {
        panic!("accepted")
    };
    assert_eq!(e.properties.unwrap(), ["emailPush", "emailPush/R"]);
}

#[test]
fn pure_config_check() {
    let a = mail_account();
    let err: InvalidEmailPushConfig = check_email_push_config(&json!(1), &a, None).unwrap_err();
    assert!(err.properties.is_empty());
    let err = check_email_push_config(&json!({"urgency": 1, "filter": 1}), &a, None).unwrap_err();
    assert_eq!(err.properties, ["filter", "urgency"]);
    let cfg = check_email_push_config(&json!({"urgency": "high"}), &a, None).unwrap();
    assert_eq!(cfg.urgency, jmap_types::Urgency::High);
}

// ---- End to end through PushSubscription/set ----

async fn call(m: &Arc<Mock>, method: &str, args: Value) -> Value {
    let mut d = Dispatcher::new();
    register_push_subscription_handlers(&mut d, Arc::clone(m));
    let req: JmapRequest = serde_json::from_value(
        json!({"using": [CORE, EMAILPUSH], "methodCalls": [[method, args, "0"]]}),
    )
    .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let inv = &resp["methodResponses"][0];
    assert_eq!(inv[0], method, "{inv}");
    inv[1].clone()
}

fn create_args(email_push: Value) -> Value {
    let s = serde_json::to_value(sub(Some(email_push))).unwrap();
    json!({"create": {"c": s}})
}

#[tokio::test]
async fn set_rejects_and_normalizes_end_to_end() {
    let m = Arc::new(Mock::with(&[("A", mail_account())]));
    let r = call(&m, "PushSubscription/set", create_args(json!({"X": {}}))).await;
    assert_eq!(
        r["notCreated"]["c"],
        json!({"type": "invalidProperties", "properties": ["emailPush", "emailPush/X"]}),
        "{r}"
    );

    let normal = json!({"A": {"filter": null, "properties": [], "urgency": "normal"}});
    let r = call(&m, "PushSubscription/set", create_args(json!({"A": {}}))).await;
    assert_eq!(r["created"]["c"]["emailPush"], normal, "{r}");
    let id = r["created"]["c"]["id"].as_str().unwrap().to_owned();
    assert_eq!(m.stored(&id).extra["emailPush"], normal);
    let r = call(
        &m,
        "PushSubscription/get",
        json!({"ids": [&id], "properties": ["emailPush"]}),
    )
    .await;
    assert_eq!(r["list"][0]["emailPush"], normal);

    // A patch the server normalizes: the whole normalized map is echoed.
    let r = call(
        &m,
        "PushSubscription/set",
        json!({"update": {&id: {"emailPush/A": {"urgency": "high"}}}}),
    )
    .await;
    let high = json!({"A": {"filter": null, "properties": [], "urgency": "high"}});
    assert_eq!(r["updated"][&id], json!({"emailPush": high}), "{r}");

    // A patch already in normalized form: nothing to echo.
    let r = call(
        &m,
        "PushSubscription/set",
        json!({"update": {&id: {"emailPush/A/urgency": "low"}}}),
    )
    .await;
    assert_eq!(r["updated"][&id], Value::Null, "{r}");
    assert_eq!(m.stored(&id).extra["emailPush"]["A"]["urgency"], "low");

    // A rejected patch leaves the stored value.
    let r = call(
        &m,
        "PushSubscription/set",
        json!({"update": {&id: {"emailPush/A/urgency": "urgent"}}}),
    )
    .await;
    assert_eq!(
        r["notUpdated"][&id]["properties"],
        json!(["emailPush", "emailPush/A/urgency"]),
        "{r}"
    );
    assert_eq!(m.stored(&id).extra["emailPush"]["A"]["urgency"], "low");
}

// ---- r44n.3: pruning revoked accounts ----

#[test]
fn prune_removes_only_revoked_accounts() {
    use jmap_mail_server::emailpush::prune_email_push;
    let accounts = BTreeMap::from([(Id::from("A"), mail_account())]);
    let cfg = json!({"filter": null, "properties": [], "urgency": "normal"});

    let mut s = sub(Some(json!({"A": cfg.clone(), "R": cfg.clone()})));
    assert!(prune_email_push(&mut s, &accounts));
    assert_eq!(s.extra["emailPush"], json!({"A": cfg.clone()}));
    // Nothing more to remove.
    assert!(!prune_email_push(&mut s, &accounts));
    assert_eq!(s.extra["emailPush"], json!({"A": cfg.clone()}));

    // -05 §4 "remove that account from the map": the map stays, empty.
    let mut s = sub(Some(json!({"R": cfg})));
    assert!(prune_email_push(&mut s, &accounts));
    assert_eq!(s.extra["emailPush"], json!({}));

    for v in [None, Some(Value::Null)] {
        let mut s = sub(v.clone());
        assert!(!prune_email_push(&mut s, &accounts));
        assert_eq!(s.extra.get("emailPush"), v.as_ref());
    }
}

/// RFC 8620 §1.4: `before` / `after` are UTCDates ("date-time" with "Z").
/// -05 §4: an invalid filter is invalidProperties at its path. A
/// non-ASCII value used to be stored and then panic every delivery.
#[tokio::test]
async fn bad_filter_dates() {
    let m = Mock::with(&[("A", mail_account())]);
    let bad = paths(&["emailPush", "emailPush/A/filter"]);
    for date in [
        "202\u{e9}-01-02T03:04:05Z",
        "garbage",
        "2026-01-02",
        "2026-13-45T03:04:05Z",
        "2026-01-02T03:04:05+02:00",
    ] {
        for key in ["before", "after"] {
            let filter = json!({"operator": "OR", "conditions": [{key: date}]});
            assert_eq!(
                create(&m, json!({"A": {"filter": filter}})).await,
                bad,
                "{key}: {date}"
            );
        }
    }
    assert!(
        create(
            &m,
            json!({"A": {"filter": {"after": "2026-01-02T03:04:05.5Z"}}})
        )
        .await
        .is_ok()
    );
}

/// -05 §4: the emailPush rules apply when "The change includes
/// "emailPush"". An update of `types` on the listed object, which
/// `list_push_subscriptions` has pruned of revoked account R, does not
/// include it, even though the stored object still holds R and A's config
/// is no longer valid for A (it lost ObjectID, so `inMailbox` is out).
#[tokio::test]
async fn update_of_other_property_ignores_pruned_and_stale_configs() {
    let m = Mock::with(&[("A", EmailPushAccount::new(false))]);
    let a = json!({"filter": {"inMailbox": "m"}, "properties": [], "urgency": "normal"});
    let stored = sub(Some(json!({"A": a.clone(), "R": a.clone()})));
    let mut new = sub(Some(json!({"A": a})));
    new.types = Some(vec!["Email".into()]);
    assert!(
        check_email_push_change(&m, &(), Some(&stored), &mut new)
            .await
            .is_ok()
    );
}
