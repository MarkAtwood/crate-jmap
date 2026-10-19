//! `jmap_base_client::StateChange` is a re-export of the canonical
//! `jmap_types::StateChange`, not a second copy (bd:JMAP-nbk3.1, plan D1).

/// Compiles only if both paths name the same type.
fn same_type(sc: jmap_base_client::StateChange) -> jmap_types::StateChange {
    sc
}

// RFC 8620 §7.1 (rfc8620.txt:3497-3499): "This MUST be the string
// "StateChange"."  One type carries that rule for server and client.
#[test]
fn base_client_state_change_is_jmap_types_state_change() {
    let sc: jmap_base_client::StateChange = serde_json::from_value(serde_json::json!({
        "@type": "StateChange",
        "changed": { "a3123": { "Email": "d35ecb040aab" } }
    }))
    .expect("must parse");
    assert_eq!(same_type(sc).changed["a3123"]["Email"], "d35ecb040aab");
}
