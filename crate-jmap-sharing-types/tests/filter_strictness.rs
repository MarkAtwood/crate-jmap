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
use serde_json::{Value, json};

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

/// RFC 9670 §2.4 Principal/query FilterCondition: `accountIds`,
/// `timeZone`.
#[test]
fn principal_filter_condition_is_strict() {
    assert_strict::<jmap_sharing_types::PrincipalFilterCondition>(
        json!({ "accountIds": ["a1"], "timeZone": "Europe/London" }),
        json!({ "time_zone": "Europe/London" }),
    );
}

/// RFC 9670 §3.4 ShareNotification/query FilterCondition: `objectType`,
/// `objectAccountId`.
#[test]
fn share_notification_filter_condition_is_strict() {
    assert_strict::<jmap_sharing_types::ShareNotificationFilterCondition>(
        json!({ "objectType": "Mailbox", "objectAccountId": "a1" }),
        json!({ "object_type": "Mailbox" }),
    );
}
