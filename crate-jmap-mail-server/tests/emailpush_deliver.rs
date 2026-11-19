//! EmailPush delivery: `email_pushes_for` (bd JMAP-r44n.1, r44n.2).
//!
//! Oracles: draft-ietf-jmap-emailpush-05 §5 Figures 2 and 3 (copied by
//! hand), §3 (`state` null without mail; size limit, splitting and the
//! prefix rule) and §4 (no push before verification; regardless of
//! `types`; `id` only when listed; empty `properties` gives `{}`);
//! docs/design/DECISIONS.md decisions 10, 16 and 20.

use std::collections::BTreeMap;

use jmap_mail_server::emailpush::{
    DeliveredEmail, EmailPushAccount, EmailPushBatch, EmailPushError, email_pushes_for,
};
use jmap_mail_types::{Email, Keyword};
use jmap_server::StoredPushSubscription;
use jmap_types::{Id, State, Urgency};
use serde_json::{Value, json};

const INBOX: &str = "674cc240-95db-49ce-a8a2-054f4f733095";
/// 2026-01-01T00:00:00Z.
const NOW: i64 = 1_767_225_600;

/// Figure 2's config, as normalization stores it.
fn figure_2_config() -> Value {
    json!({
        "filter": {"operator": "OR", "conditions": [
            {"inMailbox": INBOX}, {"hasKeyword": "$notify"}]},
        "properties": ["id", "from", "subject"],
        "urgency": "normal"
    })
}

/// A verified subscription with keys whose `emailPush` is `email_push`.
fn subscription(email_push: Value) -> StoredPushSubscription {
    StoredPushSubscription {
        subscription: serde_json::from_value(json!({
            "id": "P1", "deviceClientId": "a889-ffea-910",
            "url": "https://push.example.com/WmYD3enMWsSOE0nd",
            "keys": {"p256dh": "client-p256dh", "auth": "client-auth"},
            "verificationCode": "code", "types": [],
            "emailPush": email_push
        }))
        .unwrap(),
        expected_code: "code".to_owned(),
    }
}

/// The Figure 3 message, delivered to the inbox.
fn figure_3_email() -> Email {
    serde_json::from_value(json!({
        "id": "M699bb151ca4cbf1c24364a68", "blobId": "B1", "threadId": "T1",
        "mailboxIds": {INBOX: true}, "size": 2000,
        "receivedAt": "2026-01-01T00:00:00Z",
        "from": [{"name": "John Smith", "email": "john@example.com"}],
        "subject": "Surprise birthday party for Jane tonight",
        "preview": "Bring cake"
    }))
    .unwrap()
}

/// The §5 account: no mail, IMAP with ObjectID.
fn imap() -> EmailPushAccount {
    EmailPushAccount::new(false).with_object_ids(true)
}

fn run(
    sub: &StoredPushSubscription,
    account: &EmailPushAccount,
    emails: &[Email],
    state: Option<&State>,
    max_bytes: usize,
) -> Result<Vec<Value>, EmailPushError> {
    let delivered: Vec<DeliveredEmail<'_>> = emails.iter().map(DeliveredEmail::new).collect();
    let id = Id::from("A1412");
    let batch = EmailPushBatch::new(&id, account, &delivered, state, max_bytes);
    Ok(email_pushes_for(sub, NOW, &batch)?
        .into_iter()
        .map(|o| serde_json::to_value(&o.push).unwrap())
        .collect())
}

#[test]
fn section_5_delivery_is_figure_3() {
    let sub = subscription(json!({"A1412": figure_2_config()}));
    let got = run(
        &sub,
        &imap(),
        &[figure_3_email()],
        Some(&State::from("s7")),
        3993,
    )
    .unwrap();
    assert_eq!(
        got,
        [json!({
          "@type": "EmailPush",
          "accountId": "A1412",
          "emails": [{
            "id": "M699bb151ca4cbf1c24364a68",
            "from": [{ "name": "John Smith", "email": "john@example.com" }],
            "subject": "Surprise birthday party for Jane tonight"
          }],
          "state": null
        })]
    );
}

fn config(properties: Value) -> Value {
    json!({"A1412": {"filter": null, "properties": properties, "urgency": "normal"}})
}

#[test]
fn properties_and_implicit_id() {
    let e = [figure_3_email()];
    let got = run(&subscription(config(json!([]))), &imap(), &e, None, 3993).unwrap();
    assert_eq!(got[0]["emails"], json!([{}]));
    let got = run(
        &subscription(config(json!(["from"]))),
        &imap(),
        &e,
        None,
        3993,
    )
    .unwrap();
    assert_eq!(
        got[0]["emails"],
        json!([{"from": [{"name": "John Smith", "email": "john@example.com"}]}])
    );
}

#[test]
fn no_push_before_verification_after_expiry_or_without_keys() {
    let e = [figure_3_email()];
    let mut sub = subscription(config(json!([])));
    sub.subscription.verification_code = None;
    assert!(run(&sub, &imap(), &e, None, 3993).unwrap().is_empty());

    let mut sub = subscription(config(json!([])));
    sub.subscription.verification_code = Some("wrong".into());
    assert!(run(&sub, &imap(), &e, None, 3993).unwrap().is_empty());

    let mut sub = subscription(config(json!([])));
    sub.subscription.expires = Some("2025-12-31T23:59:59Z".into());
    assert!(run(&sub, &imap(), &e, None, 3993).unwrap().is_empty());

    let mut sub = subscription(config(json!([])));
    sub.subscription.keys = None;
    assert!(run(&sub, &imap(), &e, None, 3993).unwrap().is_empty());
}

/// "EmailPush objects are sent regardless of the value of the "types"
/// property": `types: []` here, and `types` without Email.
#[test]
fn types_is_ignored() {
    let e = [figure_3_email()];
    let mut sub = subscription(config(json!([])));
    assert_eq!(run(&sub, &imap(), &e, None, 3993).unwrap().len(), 1);
    sub.subscription.types = Some(vec!["Mailbox".into()]);
    assert_eq!(run(&sub, &imap(), &e, None, 3993).unwrap().len(), 1);
}

#[test]
fn no_config_or_no_match_gives_nothing() {
    let e = [figure_3_email()];
    let other = json!({"B": {"filter": null, "properties": [], "urgency": "normal"}});
    assert!(
        run(&subscription(other), &imap(), &e, None, 3993)
            .unwrap()
            .is_empty()
    );
    assert!(
        run(&subscription(Value::Null), &imap(), &e, None, 3993)
            .unwrap()
            .is_empty()
    );
    let mut archived = figure_3_email();
    archived.mailbox_ids = BTreeMap::from([(Id::from("archive"), true)]);
    let sub = subscription(json!({"A1412": figure_2_config()}));
    assert!(
        run(&sub, &imap(), &[archived.clone()], None, 3993)
            .unwrap()
            .is_empty()
    );
    // ... unless it has $notify (Figure 2's OR).
    archived.keywords.insert(Keyword::from("$notify"), true);
    assert_eq!(
        run(&sub, &imap(), &[archived], None, 3993).unwrap().len(),
        1
    );
}

/// §3: state is the Email state with mail, and null without.
#[test]
fn state_follows_the_mail_capability() {
    let e = [figure_3_email()];
    let s = State::from("s7");
    let sub = subscription(config(json!([])));
    assert_eq!(
        run(&sub, &imap(), &e, Some(&s), 3993).unwrap()[0]["state"],
        Value::Null
    );
    let mail = EmailPushAccount::new(true);
    assert_eq!(
        run(&sub, &mail, &e, Some(&s), 3993).unwrap()[0]["state"],
        "s7"
    );
}

#[test]
fn urgency_is_carried() {
    let sub = subscription(json!({"A1412": {"filter": null, "properties": [], "urgency": "high"}}));
    let e = [figure_3_email()];
    let delivered = [DeliveredEmail::new(&e[0])];
    let id = Id::from("A1412");
    let account = imap();
    let batch = EmailPushBatch::new(&id, &account, &delivered, None, 3993);
    let out = email_pushes_for(&sub, NOW, &batch).unwrap();
    assert_eq!(out[0].urgency, Urgency::High);
}

/// Decision 10: a config that bypassed validation with `objectHistory`
/// still pushes the other names, in order, without it.
#[test]
fn object_history_is_never_pushed() {
    let sub = subscription(config(json!(["subject", "objectHistory", "id"])));
    let mail = EmailPushAccount::new(true).with_email_history(true);
    let got = run(&sub, &mail, &[figure_3_email()], None, 3993).unwrap();
    assert_eq!(
        got[0]["emails"],
        json!([{"subject": "Surprise birthday party for Jane tonight",
                "id": "M699bb151ca4cbf1c24364a68"}])
    );
}

#[test]
fn thread_condition_needs_thread_keywords() {
    let sub = subscription(json!({"A1412": {
        "filter": {"someInThreadHaveKeyword": "$flagged"}, "properties": [], "urgency": "normal"}}));
    let mail = EmailPushAccount::new(true);
    let e = figure_3_email();
    assert_eq!(
        run(&sub, &mail, std::slice::from_ref(&e), None, 3993),
        Err(EmailPushError::MissingThreadKeywords)
    );
    let flagged = BTreeMap::from([(Keyword::from("$flagged"), true)]);
    let thread = [&flagged];
    let delivered = [DeliveredEmail::new(&e).with_thread_keywords(&thread)];
    let id = Id::from("A1412");
    let batch = EmailPushBatch::new(&id, &mail, &delivered, None, 3993);
    assert_eq!(email_pushes_for(&sub, NOW, &batch).unwrap().len(), 1);
}

/// Decision 20: without mail and ObjectID the four required Email ids are
/// placeholders; a valid config never names them, so none is pushed.
#[test]
fn placeholder_ids_are_never_pushed() {
    let mut e = figure_3_email();
    e.id = Id::from("placeholder");
    e.thread_id = Id::from("placeholder");
    let sub = subscription(config(json!([
        "keywords",
        "from",
        "to",
        "cc",
        "subject",
        "preview",
        "receivedAt",
        "sentAt"
    ])));
    let got = run(&sub, &EmailPushAccount::new(false), &[e], None, 3993).unwrap();
    assert!(!got[0].to_string().contains("placeholder"), "{}", got[0]);
}

// ---- r44n.2: size limit, splitting, prefix truncation, send-time check ----

/// Every EmailPush for `emails` under `max_bytes`, as sent.
fn pushes(
    properties: Value,
    emails: &[Email],
    max_bytes: usize,
) -> Vec<jmap_mail_types::EmailPush> {
    let sub = subscription(config(properties));
    let delivered: Vec<DeliveredEmail<'_>> = emails.iter().map(DeliveredEmail::new).collect();
    let id = Id::from("A1412");
    let account = EmailPushAccount::new(true);
    let s = State::from("s9");
    let batch = EmailPushBatch::new(&id, &account, &delivered, Some(&s), max_bytes);
    email_pushes_for(&sub, NOW, &batch)
        .unwrap()
        .into_iter()
        .map(|o| o.push)
        .collect()
}

fn len(push: &jmap_mail_types::EmailPush) -> usize {
    serde_json::to_vec(push).unwrap().len()
}

fn email_with(id: &str, subject: &str, preview: &str) -> Email {
    let mut e = figure_3_email();
    e.id = Id::from(id);
    e.subject = Some(subject.to_owned());
    e.preview = Some(preview.to_owned());
    e
}

/// "if multiple Email objects would not fit in a single EmailPush, the
/// server MUST split them": every Email arrives complete, in order, and no
/// push is over the limit.
#[test]
fn many_small_emails_are_split() {
    let emails: Vec<Email> = (0..10)
        .map(|i| email_with(&format!("M{i}"), &format!("Subject {i}"), "p"))
        .collect();
    let props = json!(["id", "subject"]);
    let all = pushes(props.clone(), &emails, 100_000);
    assert_eq!(all.len(), 1);
    let one_size = len(&all[0]);
    let max = one_size / 3;
    let parts = pushes(props, &emails, max);
    assert!(parts.len() >= 3, "{}", parts.len());
    let mut seen = Vec::new();
    for p in &parts {
        assert!(len(p) <= max, "{} > {max}", len(p));
        assert_eq!(p.state, Some(State::from("s9")), "same state in every part");
        for e in &p.emails {
            assert_eq!(e.len(), 2, "complete: {e:?}");
            seen.push(e["id"].as_str().unwrap().to_owned());
        }
    }
    let want: Vec<String> = (0..10).map(|i| format!("M{i}")).collect();
    assert_eq!(seen, want);
}

/// A push of exactly the limit is allowed ("MUST NOT exceed").
#[test]
fn exact_fit_is_one_push() {
    let emails = [email_with("M1", "a", "p"), email_with("M2", "b", "p")];
    let props = json!(["id", "subject"]);
    let all = pushes(props.clone(), &emails, 100_000);
    let exact = len(&all[0]);
    assert_eq!(pushes(props.clone(), &emails, exact).len(), 1);
    assert_eq!(pushes(props, &emails, exact - 1).len(), 2);
}

/// "In some cases, just a single Email may be too large": it goes alone,
/// holding a prefix of the requested list; a small Email beside it is not
/// cut.
#[test]
fn oversized_email_goes_alone_as_a_prefix() {
    let big = email_with("BIG", &"s".repeat(400), "tiny");
    let small = email_with("SMALL", "hi", "tiny");
    let props = json!(["id", "subject", "preview"]);
    let max = 300;
    let parts = pushes(props, &[small.clone(), big, small], max);
    assert_eq!(parts.len(), 3, "{parts:?}");
    for p in &parts {
        assert!(len(p) <= max);
    }
    assert_eq!(parts[0].emails.len(), 1);
    assert_eq!(parts[0].emails[0].len(), 3, "small Email not cut");
    // The big one: `id` fits, `subject` does not, so `preview` (tiny) is
    // dropped too: no skipping.
    assert_eq!(
        Value::Object(parts[1].emails[0].clone()),
        json!({"id": "BIG"})
    );
    assert_eq!(parts[2].emails[0].len(), 3, "small Email not cut");
}

/// "omit that property and all subsequent properties": a large property 2
/// followed by a tiny property 3 drops both.
#[test]
fn large_property_then_tiny_property_drops_both() {
    let e = email_with("M1", &"x".repeat(500), "p");
    let parts = pushes(json!(["id", "subject", "preview"]), &[e], 200);
    assert_eq!(
        Value::Object(parts[0].emails[0].clone()),
        json!({"id": "M1"})
    );
}

#[test]
fn limit_too_small() {
    let sub = subscription(config(json!([])));
    let e = [figure_3_email()];
    assert_eq!(
        run(&sub, &imap(), &e, None, 20),
        Err(EmailPushError::LimitTooSmall)
    );
    // The smallest push that fits: one `{}`.
    let exact = serde_json::to_vec(&json!({
        "@type": "EmailPush", "accountId": "A1412", "emails": [{}], "state": null
    }))
    .unwrap()
    .len();
    assert_eq!(run(&sub, &imap(), &e, None, exact).unwrap().len(), 1);
    assert_eq!(
        run(&sub, &imap(), &e, None, exact - 1),
        Err(EmailPushError::LimitTooSmall)
    );
}

/// Decision 16: a config stored without validation that fails the config
/// check for the live account gets no push.
#[test]
fn invalid_stored_config_gets_no_push() {
    let e = [figure_3_email()];
    let body_values = subscription(config(json!(["bodyValues"])));
    assert!(
        run(&body_values, &imap(), &e, None, 3993)
            .unwrap()
            .is_empty()
    );
    // `id` needs mail or ObjectID (decision 20).
    let id = subscription(config(json!(["id", "from"])));
    let bare = EmailPushAccount::new(false);
    assert!(run(&id, &bare, &e, None, 3993).unwrap().is_empty());
    assert_eq!(run(&id, &imap(), &e, None, 3993).unwrap().len(), 1);
    let urgent = subscription(json!({"A1412": {"urgency": "urgent"}}));
    assert!(run(&urgent, &imap(), &e, None, 3993).unwrap().is_empty());
    // inMailbox without mail or ObjectID.
    let mailbox = subscription(json!({"A1412": {"filter": {"inMailbox": INBOX}}}));
    assert!(run(&mailbox, &bare, &e, None, 3993).unwrap().is_empty());
}

/// Pushes for the Figure 3 Email (receivedAt 2026-01-01T00:00:00Z, given
/// `keywords`) under `filter`, with a config stored without validation.
fn pushes_with_filter(filter: Value, keywords: Value) -> usize {
    let mut email = figure_3_email();
    email.keywords = serde_json::from_value(keywords).unwrap();
    let sub = subscription(json!({"A1412": {
        "filter": filter, "properties": ["subject"], "urgency": "normal"
    }}));
    // A mail account: before/after are not among the conditions an
    // account without mail must support (-05 §4).
    let mail = EmailPushAccount::new(true).with_object_ids(true);
    run(&sub, &mail, &[email], None, 3993).unwrap().len()
}

/// A stored `before` that is not a UTCDate (here with a multi-byte
/// character across a field boundary) never panics and sends nothing,
/// while a valid one in the same position does send.
#[test]
fn malformed_stored_date_sends_nothing() {
    assert_eq!(
        pushes_with_filter(json!({"before": "2027-01-02T03:04:05Z"}), json!({})),
        1
    );
    assert_eq!(
        pushes_with_filter(json!({"before": "202\u{e9}-01-02T03:04:05Z"}), json!({})),
        0
    );
    assert_eq!(
        pushes_with_filter(json!({"after": "garbage"}), json!({})),
        0
    );
}

/// RFC 8621 §4.4.1: `before` "must be before this date-time", `after`
/// "must be the same or after"; fractional seconds count (RFC 8620 §1.4).
#[test]
fn before_after_compare_fractional_seconds() {
    let at = |key: &str, date: &str| pushes_with_filter(json!({key: date}), json!({}));
    assert_eq!(at("before", "2026-01-01T00:00:00.5Z"), 1);
    assert_eq!(at("after", "2026-01-01T00:00:00.5Z"), 0);
    assert_eq!(at("before", "2026-01-01T00:00:00Z"), 0);
    assert_eq!(at("after", "2026-01-01T00:00:00Z"), 1);
}

/// RFC 8621 §4.1.1: "a keyword is a case-insensitive string".
#[test]
fn keyword_conditions_ignore_case() {
    let flagged = json!({"$flagged": true});
    assert_eq!(
        pushes_with_filter(json!({"hasKeyword": "$Flagged"}), flagged.clone()),
        1
    );
    assert_eq!(
        pushes_with_filter(json!({"notKeyword": "$FLAGGED"}), flagged),
        0
    );
}
