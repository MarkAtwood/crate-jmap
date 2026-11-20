//! JSContact URI members on ContactCard/set (RFC 9553 §1.4.4: a Resource
//! `uri` "MUST be a URI as defined in Section 3 of [RFC3986]"; §2.3.2
//! OnlineService `uri`; §2.8.3 Note `author.uri`;
//! draft-atwood-jmap-updates-00 §7.1).
//!
//! Oracle: RFC 9553 example values (§2.3.2 `xmpp:alice@example.com`,
//! §2.4.1 `webcal://...`, §2.6.1 `data:` key) as valid, and hand-written
//! invalid values.

mod common;

use common::MemoryBackend;
use jmap_contacts_server::handle_contact_card_set;
use jmap_types::Id;
use serde_json::{Value, json};

async fn set(args: Value) -> Value {
    let backend = MemoryBackend::<()>::default().with_account(&Id::from("acc1"));
    let mut args = args;
    args["accountId"] = json!("acc1");
    handle_contact_card_set(&backend, &(), &std::collections::BTreeMap::new(), args, &[])
        .await
        .expect("/set must not fail at method level")
        .primary
}

#[tokio::test]
async fn create_rejects_each_kind_of_bad_uri() {
    for (property, value) in [
        (
            "links",
            json!({ "l1": { "uri": "https://example.com/a b" } }),
        ),
        (
            "onlineServices",
            json!({ "o1": { "service": "Mastodon", "uri": "https://exämple.social/@a" } }),
        ),
        ("cryptoKeys", json!({ "k1": { "uri": "not a uri" } })),
        (
            "notes",
            json!({ "n1": { "note": "hi", "author": { "uri": "mailto:a b@example.com" } } }),
        ),
    ] {
        let mut card =
            json!({ "@type": "Card", "version": "1.0", "addressBookIds": { "ab1": true } });
        card[property] = value;
        let resp = set(json!({ "create": { "c": card } })).await;
        assert_eq!(
            resp["notCreated"]["c"]["type"], "invalidProperties",
            "{property}: {resp}"
        );
        assert_eq!(
            resp["notCreated"]["c"]["properties"],
            json!([property]),
            "{resp}"
        );
    }
}

#[tokio::test]
async fn rfc9553_example_uris_are_not_rejected() {
    let card = json!({
        "@type": "Card", "version": "1.0", "addressBookIds": { "ab1": true },
        "onlineServices": { "x1": { "service": "XMPP", "uri": "xmpp:alice@example.com" } },
        "calendars": { "calA": { "kind": "calendar", "uri": "webcal://calendar.example.com/calA.ics" } },
        "cryptoKeys": { "mykey2": { "uri": "data:application/pgp-keys;base64,LS0tLS1CRUdJTiBSU0EgUFVC" } }
    });
    let resp = set(json!({ "create": { "c": card } })).await;
    let props = &resp["notCreated"]["c"]["properties"];
    for p in ["onlineServices", "calendars", "cryptoKeys"] {
        assert!(
            !props.as_array().is_some_and(|a| a.contains(&json!(p))),
            "{resp}"
        );
    }
}

#[tokio::test]
async fn update_rejects_a_bad_uri_by_pointer_or_value() {
    let resp =
        set(json!({ "update": { "C1": { "links/l1/uri": "https://example.com/a b" } } })).await;
    assert_eq!(
        resp["notUpdated"]["C1"]["properties"],
        json!(["links"]),
        "{resp}"
    );
    let resp = set(
        json!({ "update": { "C1": { "media": { "m1": { "kind": "photo", "uri": "x y" } } } } }),
    )
    .await;
    assert_eq!(
        resp["notUpdated"]["C1"]["properties"],
        json!(["media"]),
        "{resp}"
    );
}
