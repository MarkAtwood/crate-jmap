//! Typed `*_set` options (bd JMAP-y1yu.21).

mod helpers;

/// y1yu.21 acceptance: `options` adds `ifUnchangedBy`, `atomic` and the
/// conditional URI (draft-ietf-jmap-conditional-02 §3, §4), and `None` or
/// default options send the same request as before.
#[tokio::test]
async fn set_options_on_the_wire() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"methodResponses": [], "sessionState": "s"})),
        )
        .mount(&server)
        .await;
    let client = helpers::make_client(&server);
    let mut conditional = jmap_types::conditional::ConditionalSetArgs::default();
    conditional.atomic = true;
    conditional.if_unchanged_by.insert("x".to_owned(), None);
    let mut set = jmap_base_client::SetOptions::default();
    set.conditional = conditional;
    for options in [
        None,
        Some(jmap_base_client::SetOptions::default()),
        Some(set),
    ] {
        // The empty response is an error; only the request matters.
        let _ = client
            .share_notification_set(vec![jmap_types::Id::from("x")], None, options)
            .await;
    }
    let bodies: Vec<serde_json::Value> = server
        .received_requests()
        .await
        .expect("recording")
        .iter()
        .filter(|r| r.method == wiremock::http::Method::POST)
        .map(|r| serde_json::from_slice(&r.body).expect("JSON body"))
        .collect();
    assert_eq!(bodies.len(), 3);
    assert_eq!(bodies[0], bodies[1], "default options change nothing");
    let mut want = bodies[0].clone();
    want["using"]
        .as_array_mut()
        .expect("using")
        .push("urn:ietf:params:jmap:conditional".into());
    want["methodCalls"][0][1]["atomic"] = true.into();
    want["methodCalls"][0][1]["ifUnchangedBy"] = serde_json::json!({"x": null});
    assert_eq!(bodies[2], want);
}
