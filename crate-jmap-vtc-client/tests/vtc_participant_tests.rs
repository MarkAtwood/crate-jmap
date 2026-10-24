//! Wiremock integration tests for `VTCParticipant/*` client methods.
//!
//! Oracle: the request and response examples of draft-atwood-jmap-vtc-00
//! (jmap-chat-spec HEAD, line numbers cited per test) and the RFC 8620 §5
//! method shapes. Expected JSON is copied from the draft, never derived
//! from the code under test.

#[path = "helpers.rs"]
mod helpers;

use std::collections::BTreeMap;

use jmap_types::{Filter, Id, PatchObject, State};
use jmap_vtc_types::{CallRole, VtcParticipant};
use jmap_vtc_types::{
    VtcParticipantComparator, VtcParticipantFilterCondition, VtcParticipantSortProperty,
};
use serde_json::{Value, json};
use wiremock::MockServer;

/// One-entry `update` map of `id` to the single patch `key: value`.
fn update(id: &str, key: &str, value: Value) -> BTreeMap<Id, PatchObject> {
    let mut patch = PatchObject::new();
    patch.as_map_mut().insert(key.into(), value);
    BTreeMap::from([(Id::from(id), patch)])
}

/// Send `update` from the handle `sc` builds and return the invocation the
/// client sent.
async fn send_update(
    sc: impl FnOnce(&MockServer) -> jmap_vtc_client::SessionClient,
    hosting: &str,
    update: BTreeMap<Id, PatchObject>,
) -> Value {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/set",
        helpers::set_response(hosting, json!({})),
    )
    .await;
    sc(&server)
        .vtc_participant_set(None, Some(update), None, None)
        .await
        .expect("VTCParticipant/set must succeed");
    helpers::sent_invocation(&server).await
}

/// draft-atwood-jmap-vtc-00 L1898-L1904, "Alice's desktop client finds her
/// VTCParticipant record" in Bob's account, which "Alice's Session lists
/// as a shared account" (L1895).
#[tokio::test]
async fn vtc_participant_query_finds_own_record_in_hosting_account() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/query",
        json!({
            "accountId": "bob-account",
            "queryState": "q1",
            "canCalculateChanges": false,
            "position": 0,
            "ids": ["01J4XMVQK7NRYS9PPCEHTK4CD0"]
        }),
    )
    .await;
    let mut cond = VtcParticipantFilterCondition::default();
    cond.call_id = Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"));
    cond.user_id = Some("user:alice@example.com".into());

    let sc = helpers::make_shared_client(&server, "alice-account", "bob-account");
    let resp = sc
        .vtc_participant_query(
            Some(&Filter::Condition(cond)),
            None,
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("VTCParticipant/query must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCParticipant/query", {
            "accountId": "bob-account",
            "filter": {
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "userId": "user:alice@example.com"
            }
        }])
    );
    // L1907: "The response's `ids` is `["01J4XMVQK7NRYS9PPCEHTK4CD0"]`."
    assert_eq!(resp.ids, vec![Id::from("01J4XMVQK7NRYS9PPCEHTK4CD0")]);
}

/// draft-atwood-jmap-vtc-00 L1910-L1917, "Alice answers on her desktop": the
/// update goes to the hosting account (L1654).
#[tokio::test]
async fn vtc_participant_set_answer_matches_lifecycle_example() {
    let sent = send_update(
        |s| helpers::make_shared_client(s, "alice-account", "bob-account"),
        "bob-account",
        update("01J4XMVQK7NRYS9PPCEHTK4CD0", "joinMethod", json!("webrtc")),
    )
    .await;
    assert_eq!(
        sent,
        json!(["VTCParticipant/set", {
            "accountId": "bob-account",
            "update": {
                "01J4XMVQK7NRYS9PPCEHTK4CD0": {
                    "joinMethod": "webrtc"
                }
            }
        }])
    );
}

/// draft-atwood-jmap-vtc-00 L1966-L1973, "Alice mutes" — a JSON Pointer
/// patch key (RFC 8620 §5.3), sent to the hosting account.
#[tokio::test]
async fn vtc_participant_set_mute_matches_lifecycle_example() {
    let sent = send_update(
        |s| helpers::make_shared_client(s, "alice-account", "bob-account"),
        "bob-account",
        update(
            "01J4XMVQK7NRYS9PPCEHTK4CD0",
            "mediaState/audio",
            json!(false),
        ),
    )
    .await;
    assert_eq!(
        sent,
        json!(["VTCParticipant/set", {
            "accountId": "bob-account",
            "update": {
                "01J4XMVQK7NRYS9PPCEHTK4CD0": {
                    "mediaState/audio": false
                }
            }
        }])
    );
}

/// draft-atwood-jmap-vtc-00 L2069-L2076, "Moderator admits the guest" in
/// the moderator's own account, where the call lives (L2050).
#[tokio::test]
async fn vtc_participant_set_admit_matches_lifecycle_example() {
    let sent = send_update(
        |s| helpers::make_client(s, "mod-account"),
        "mod-account",
        update(
            "01J4XV7YT4XCA06YYKNQ01BQR0",
            "lobbyState",
            json!("admitted"),
        ),
    )
    .await;
    assert_eq!(
        sent,
        json!(["VTCParticipant/set", {
            "accountId": "mod-account",
            "update": {
                "01J4XV7YT4XCA06YYKNQ01BQR0": {
                    "lobbyState": "admitted"
                }
            }
        }])
    );
}

/// draft-atwood-jmap-vtc-00 L2084-L2091, "Guest leaves": the guest sends
/// to the moderator's hosting account.
#[tokio::test]
async fn vtc_participant_set_leave_matches_lifecycle_example() {
    let sent = send_update(
        |s| helpers::make_shared_client(s, "guest-account", "mod-account"),
        "mod-account",
        update(
            "01J4XV7YT4XCA06YYKNQ01BQR0",
            "leftAt",
            json!("2026-06-05T15:45:00Z"),
        ),
    )
    .await;
    assert_eq!(
        sent,
        json!(["VTCParticipant/set", {
            "accountId": "mod-account",
            "update": {
                "01J4XV7YT4XCA06YYKNQ01BQR0": {
                    "leftAt": "2026-06-05T15:45:00Z"
                }
            }
        }])
    );
}

/// Join request: draft-atwood-jmap-vtc-00 L2053-L2061 ("A channel member
/// joins. The call lives in the moderator's account, which the member
/// reaches as a shared account"). Response: L895-L923, "The server
/// responds with the complete VTCParticipant record", which therefore
/// decodes as a `VtcParticipant`.
#[tokio::test]
async fn vtc_participant_set_join_matches_lifecycle_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/set",
        helpers::set_response(
            "mod-account",
            json!({
                "created": {
                    "p0": {
                        "id": "01J4XMVQK7NRYS9PPCEHTK4CD0",
                        "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                        "userId": "user:alice@example.com",
                        "displayName": "Alice Chen",
                        "role": "participant",
                        "joinedAt": "2026-06-05T14:35:22Z",
                        "leftAt": null,
                        "callResponse": null,
                        "joinMethod": "webrtc",
                        "gatewayData": null,
                        "lobbyState": null,
                        "mediaState": {
                            "audio": true,
                            "video": true,
                            "screen": false,
                            "raisedHand": false
                        },
                        "speakerTimeMs": 0,
                        "kickedBy": null,
                        "e2eeFingerprint": null,
                        "unmuteRequested": false,
                        "breakoutCallId": null
                    }
                }
            }),
        ),
    )
    .await;

    let sc = helpers::make_shared_client(&server, "guest-account", "mod-account");
    let resp = sc
        .vtc_participant_set(
            Some(json!({
                "p0": {
                    "callId": "01J4XL8QN50WXV9RRBEJVS5CD0",
                    "joinMethod": "webrtc"
                }
            })),
            None,
            None,
            None,
        )
        .await
        .expect("VTCParticipant/set must succeed");

    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCParticipant/set", {
            "accountId": "mod-account",
            "create": {
                "p0": {
                    "callId": "01J4XL8QN50WXV9RRBEJVS5CD0",
                    "joinMethod": "webrtc"
                }
            }
        }])
    );
    let created = resp.created;
    let record: VtcParticipant =
        serde_json::from_value(serde_json::to_value(&created["p0"]).expect("serializes"))
            .expect("complete record must decode");
    assert_eq!(record.display_name, "Alice Chen");
    assert_eq!(record.role, CallRole::Participant);
    assert_eq!(record.speaker_time_ms, 0);
}

/// Gateway dial-out create: draft-atwood-jmap-vtc-00 L1051-L1061.
#[tokio::test]
async fn vtc_participant_set_dial_out_matches_draft_example() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/set",
        helpers::set_response("mod-account", json!({})),
    )
    .await;
    let create = json!({
        "g0": {
            "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
            "joinMethod": "pstn",
            "gatewayData": {"dialNumber": "+14155559876"},
            "displayName": "Alice (mobile)"
        }
    });
    let sc = helpers::make_client(&server, "mod-account");
    sc.vtc_participant_set(Some(create), None, None, None)
        .await
        .expect("VTCParticipant/set must succeed");
    let sent = helpers::sent_invocation(&server).await;
    assert_eq!(
        sent[1]["create"],
        json!({
            "g0": {
                "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0",
                "joinMethod": "pstn",
                "gatewayData": {"dialNumber": "+14155559876"},
                "displayName": "Alice (mobile)"
            }
        })
    );
}

/// `VTCParticipant/get` "Standard JMAP `/get` ({{RFC8620}} Section 5.1)"
/// (L826); RFC 8620 §5.1 `properties` argument.
#[tokio::test]
async fn vtc_participant_get_sends_ids_and_properties() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/get",
        json!({
            "accountId": "bob-account",
            "state": "s202",
            "list": [],
            "notFound": ["01J4XV7YT4XCA06YYKNQ01BQR0"]
        }),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_participant_get(
            Some(&[Id::from("01J4XV7YT4XCA06YYKNQ01BQR0")]),
            Some(&["id", "userId", "displayName", "role"]),
        )
        .await
        .expect("VTCParticipant/get must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCParticipant/get", {
            "accountId": "bob-account",
            "ids": ["01J4XV7YT4XCA06YYKNQ01BQR0"],
            "properties": ["id", "userId", "displayName", "role"]
        }])
    );
    assert_eq!(resp.not_found, vec![Id::from("01J4XV7YT4XCA06YYKNQ01BQR0")]);
}

/// `VTCParticipant/changes` (L830), RFC 8620 §5.2.
#[tokio::test]
async fn vtc_participant_changes_sends_since_state() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/changes",
        json!({
            "accountId": "bob-account",
            "oldState": "s201",
            "newState": "s202",
            "hasMoreChanges": false,
            "created": [],
            "updated": ["01J4XMVQK7NRYS9PPCEHTK4CD0"],
            "destroyed": []
        }),
    )
    .await;
    let sc = helpers::make_client(&server, "bob-account");
    sc.vtc_participant_changes(&State::from("s201"), None)
        .await
        .expect("VTCParticipant/changes must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCParticipant/changes", {
            "accountId": "bob-account",
            "sinceState": "s201"
        }])
    );
}

/// `VTCParticipant/query` filter properties `callId` (L1076) and `isActive`
/// (L1085, "When `true`, filter to participants with `joinedAt != null`
/// and `leftAt == null`"); sort property `joinedAt` (L1096).
#[tokio::test]
async fn vtc_participant_query_sends_filter_and_sort() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/query",
        json!({
            "accountId": "bob-account",
            "queryState": "q7",
            "canCalculateChanges": false,
            "position": 0,
            "ids": ["01J4XMVQK7NRYS9PPCEHTK4CD0", "01J4XB0B000000000000000000"]
        }),
    )
    .await;
    let mut cond = VtcParticipantFilterCondition::default();
    cond.call_id = Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"));
    cond.is_active = Some(true);
    let sort = VtcParticipantComparator::new(VtcParticipantSortProperty::JoinedAt);

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_participant_query(
            Some(&Filter::Condition(cond)),
            Some(&[sort]),
            &jmap_base_client::QueryParams::default(),
        )
        .await
        .expect("VTCParticipant/query must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCParticipant/query", {
            "accountId": "bob-account",
            "filter": { "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0", "isActive": true },
            // RFC 8620 §5.5 isAscending "(default: true)": the default is omitted.
            "sort": [{ "property": "joinedAt" }]
        }])
    );
    assert_eq!(resp.ids.len(), 2);
}

/// `VTCParticipant/queryChanges` (L1100), RFC 8620 §5.6: the `filter` sent
/// must be the one the original query used.
#[tokio::test]
async fn vtc_participant_query_changes_sends_filter() {
    let server = MockServer::start().await;
    helpers::respond(
        &server,
        "VTCParticipant/queryChanges",
        json!({
            "accountId": "bob-account",
            "oldQueryState": "q7",
            "newQueryState": "q8",
            "removed": ["01J4XB0B000000000000000000"],
            "added": []
        }),
    )
    .await;
    let mut cond = VtcParticipantFilterCondition::default();
    cond.call_id = Some(Id::from("01J4XKZQN4MWVT8PPBEHTJ3AB0"));

    let sc = helpers::make_client(&server, "bob-account");
    let resp = sc
        .vtc_participant_query_changes(
            &State::from("q7"),
            None,
            Some(&Filter::Condition(cond)),
            None,
            None,
            None,
        )
        .await
        .expect("VTCParticipant/queryChanges must succeed");
    assert_eq!(
        helpers::sent_invocation(&server).await,
        json!(["VTCParticipant/queryChanges", {
            "accountId": "bob-account",
            "sinceQueryState": "q7",
            "filter": { "callId": "01J4XKZQN4MWVT8PPBEHTJ3AB0" }
        }])
    );
    assert_eq!(resp.removed, vec![Id::from("01J4XB0B000000000000000000")]);
}
