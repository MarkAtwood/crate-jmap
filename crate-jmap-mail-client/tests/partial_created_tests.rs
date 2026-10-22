//! /set and /copy `created` values are partial objects (RFC 8620 §5.3,
//! §5.4), and SearchSnippet/get has its own response shape (RFC 8621 §5.1).
//!
//! Oracle: the server responses are the published RFC 8621 §7 (pages
//! 66-67) and §5.2 examples, with their `...` elisions removed. Each
//! `created` entry carries only `id`, `blobId`, `threadId` and `size`,
//! none of the properties the client sent, so it must parse.

#[path = "helpers.rs"]
mod helpers;

use jmap_mail_client::EmailCopyParams;
use jmap_mail_types::query::EmailFilterCondition;
use jmap_types::{query::Filter, Id};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn server_replying(name: &str, args: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    let body = json!({
        "sessionState": "s1",
        "methodResponses": [[name, args, "r1"]]
    });
    Mock::given(method("POST"))
        .and(path("/api/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;
    server
}

/// RFC 8621 §7 page 66: Email/set created holds only server-set
/// properties plus a destroyed id.
#[tokio::test]
async fn email_set_parses_rfc8621_partial_created() {
    let server = server_replying(
        "Email/set",
        json!({
            "accountId": "ue150411c",
            "oldState": "780839",
            "newState": "780842",
            "created": {
                "k1546": {
                    "id": "Md45b47b4877521042cec0938",
                    "blobId": "Ge8de6c9f6de198239b982ea214e0f3a704e4af74",
                    "threadId": "Td957e72e89f516dc",
                    "size": 11721
                }
            },
            "destroyed": [ "Mf40b5f831efa7233b9eb1c7f" ]
        }),
    )
    .await;
    let sc = helpers::make_client(&server);
    let resp = sc
        .email_set(
            Some(json!({ "k1546": { "mailboxIds": { "mb1": true } } })),
            None,
            Some(vec![Id::from("Mf40b5f831efa7233b9eb1c7f")]),
            None,
        )
        .await
        .expect("a partial created object must parse");
    let k = &resp.created.as_ref().expect("created")["k1546"];
    assert_eq!(
        k.id.as_ref().map(|i| i.as_ref()),
        Some("Md45b47b4877521042cec0938")
    );
    assert_eq!(k.extra["threadId"], "Td957e72e89f516dc");
    assert_eq!(k.extra["size"], 11721);
    assert_eq!(
        resp.destroyed.as_deref(),
        Some([Id::from("Mf40b5f831efa7233b9eb1c7f")].as_slice())
    );
}

/// RFC 8621 §7 page 67: the Email/copy response, verbatim.
#[tokio::test]
async fn email_copy_parses_rfc8621_copy_response() {
    let server = server_replying(
        "Email/copy",
        json!({
            "fromAccountId": "ue150411c",
            "accountId": "u6c6c41ac",
            "oldState": "7ee7e9263a6d",
            "newState": "5a0d2447ed26",
            "created": {
                "k45": {
                    "id": "M138f9954a5cd2423daeafa55",
                    "blobId": "G6b9fb047cba722c48c611e79233d057c6b0b74e8",
                    "threadId": "T2f242ea424a4079a",
                    "size": 11721
                }
            },
            "notCreated": null
        }),
    )
    .await;
    let sc = helpers::make_client(&server);
    let params = EmailCopyParams {
        from_account_id: Id::from("ue150411c"),
        if_from_in_state: None,
        if_in_state: None,
        on_success_destroy_original: None,
        destroy_from_if_in_state: None,
        extra: serde_json::Map::new(),
    };
    let resp = sc
        .email_copy(
            params,
            json!({ "k45": { "id": "Md45b47b4877521042cec0938", "mailboxIds": { "75a4c956": true } } }),
        )
        .await
        .expect("the RFC 8621 Email/copy response must parse");
    assert_eq!(resp.from_account_id.as_ref(), "ue150411c");
    assert_eq!(resp.account_id.as_ref(), "u6c6c41ac");
    let k45 = &resp.created.as_ref().expect("created")["k45"];
    assert_eq!(
        k45.id.as_ref().map(|i| i.as_ref()),
        Some("M138f9954a5cd2423daeafa55")
    );
    assert_eq!(
        k45.extra["blobId"],
        "G6b9fb047cba722c48c611e79233d057c6b0b74e8"
    );
    assert!(resp.not_created.is_none());
}

/// RFC 8621 §5.2 example response: the typed list replaces the caller's
/// `response["list"].as_array()` step.
#[tokio::test]
async fn search_snippet_get_returns_typed_list() {
    let server = server_replying(
        "SearchSnippet/get",
        json!({
            "accountId": "ue150411c",
            "list": [{
                "emailId": "M44200ec123de277c0c1ce69c",
                "subject": null,
                "preview": null
            }, {
                "emailId": "M7bcbcb0b58d7729686e83d99",
                "subject": "The <mark>Foo</mark>sball competition",
                "preview": "...year the <mark>foo</mark>sball competition will be held in the Stadium de ..."
            }],
            "notFound": null
        }),
    )
    .await;
    let sc = helpers::make_client(&server);
    let mut cond = EmailFilterCondition::default();
    cond.text = Some("foo".into());
    let resp = sc
        .search_snippet_get(
            &Filter::Condition(cond),
            Some(&[
                Id::from("M44200ec123de277c0c1ce69c"),
                Id::from("M7bcbcb0b58d7729686e83d99"),
            ]),
        )
        .await
        .expect("SearchSnippet/get must succeed");
    assert_eq!(resp.account_id.as_ref(), "ue150411c");
    assert_eq!(resp.list.len(), 2);
    assert_eq!(resp.list[1].email_id.as_ref(), "M7bcbcb0b58d7729686e83d99");
    assert_eq!(
        resp.list[1].subject.as_deref(),
        Some("The <mark>Foo</mark>sball competition")
    );
    assert!(resp.not_found.is_none());
}
