//! Filter conditions reject keys they do not define.
//!
//! Oracle: RFC 8620 §5.5 — "unsupportedFilter: The "filter" is
//! syntactically valid, but the server cannot process it." A key the
//! condition type does not define must fail the decode instead of becoming
//! an all-`None` condition that matches every record; servers map the
//! failure to `unsupportedFilter`. An operator object is not a
//! FilterCondition, so a bare condition type must reject it too. Inputs are
//! hand-written JSON; control keys are the property names the cited spec
//! defines.

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

fn assert_strict<T: DeserializeOwned>(known: Value, typo: Value) {
    if let Err(e) = serde_json::from_value::<T>(known.clone()) {
        panic!("{known} must decode: {e}");
    }
    for bad in [
        typo,
        json!({ "bogus": 1 }),
        json!({ "operator": "AND", "conditions": [] }),
    ] {
        assert!(
            serde_json::from_value::<T>(bad.clone()).is_err(),
            "{bad} must be rejected"
        );
    }
}

/// RFC 8621 §2.3 Mailbox/query FilterCondition: `role`, `hasAnyRole`.
#[test]
fn mailbox_filter_condition_is_strict() {
    assert_strict::<jmap_mail_types::MailboxFilterCondition>(
        json!({ "role": "inbox", "hasAnyRole": true }),
        json!({ "has_any_role": true }),
    );
}

/// RFC 8621 §7.3 EmailSubmission/query FilterCondition: `identityIds`,
/// `undoStatus`.
#[test]
fn email_submission_filter_condition_is_strict() {
    assert_strict::<jmap_mail_types::EmailSubmissionFilterCondition>(
        json!({ "identityIds": ["i1"], "undoStatus": "pending" }),
        json!({ "identity_ids": ["i1"] }),
    );
}
