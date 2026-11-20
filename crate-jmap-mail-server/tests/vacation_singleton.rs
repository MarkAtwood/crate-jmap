//! VacationResponse/set on an account with no stored singleton
//! (RFC 8621 §8).
//!
//! Oracles: RFC 8621 §8.1 "There MUST only be exactly one
//! VacationResponse object in an account"; RFC 8620 §5.3 a rejected
//! update changes nothing.

use jmap_mail_server::memory::MemoryBackend;
use jmap_mail_server::{handle_vacation_get, handle_vacation_set};
use jmap_object_history_server::HistoryRetention;
use jmap_types::Id;
use serde_json::{Value, json};

async fn set(b: &MemoryBackend<()>, patch: Value) -> Value {
    handle_vacation_set(
        b,
        &(),
        json!({"accountId": "acc", "update": {"singleton": patch}}),
    )
    .await
    .unwrap()
    .primary
}

/// A rejected first update stores nothing, and the next update works,
/// with object history on for VacationResponse (where a destroyed
/// singleton used to trip the never-destroyed assertion).
#[tokio::test]
async fn rejected_first_update_then_valid_update() {
    let b = MemoryBackend::<()>::default()
        .with_account(&Id::from("acc"))
        .with_object_history(
            serde_json::from_value(
                json!({"maxHistoryDuration": 2_592_000, "supportedTypes": ["VacationResponse"]}),
            )
            .unwrap(),
            HistoryRetention::new().max_duration(Some(std::time::Duration::from_secs(2_592_000))),
        );
    let resp = set(&b, json!({"isEnabled": "yes"})).await;
    assert_eq!(
        resp["notUpdated"]["singleton"]["type"], "invalidProperties",
        "{resp}"
    );
    assert_eq!(
        resp["oldState"], resp["newState"],
        "nothing changed: {resp}"
    );

    let resp = set(&b, json!({"isEnabled": true, "subject": "Away"})).await;
    assert!(resp["updated"].get("singleton").is_some(), "{resp}");
    let got = handle_vacation_get(&b, &(), json!({"accountId": "acc"}))
        .await
        .unwrap()
        .primary;
    assert_eq!(got["list"][0]["isEnabled"], true, "{got}");
    assert_eq!(got["list"][0]["subject"], "Away", "{got}");
}
