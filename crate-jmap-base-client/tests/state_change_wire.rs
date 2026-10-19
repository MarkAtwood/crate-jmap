//! The base-client `StateChange` consumes and emits the RFC 8620 §7.1
//! `@type` discriminant instead of capturing it in `extra`
//! (bd:JMAP-nbk3.1).

use jmap_base_client::StateChange;
use serde_json::json;

// RFC 8620 §7.1 (rfc8620.txt:3497-3499): "@type: "String" ... This MUST be
// the string "StateChange"."  Example object at rfc8620.txt:3541-3554.
#[test]
fn state_change_type_is_not_an_extra_field() {
    let raw = json!({
        "@type": "StateChange",
        "changed": {
            "a3123": {
                "Email": "d35ecb040aab",
                "EmailDelivery": "428d565f2440",
                "CalendarEvent": "87accfac587a"
            },
            "a43461d": {
                "Mailbox": "0af7a512ce70",
                "CalendarEvent": "7a4297cecd76"
            }
        }
    });
    let sc: StateChange = serde_json::from_value(raw.clone()).expect("RFC example must parse");
    assert!(
        sc.extra.is_empty(),
        "@type must be consumed as the discriminant, found extra = {:?}",
        sc.extra
    );
    assert_eq!(serde_json::to_value(&sc).expect("serialize"), raw);

    let mut wrong = raw;
    wrong["@type"] = json!("PushVerification");
    assert!(serde_json::from_value::<StateChange>(wrong).is_err());
}
