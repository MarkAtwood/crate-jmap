//! `VTCCall/signal` and `VTCCall/exportMediaSecret`
//! (draft-atwood-jmap-vtc-00 §`VTCCall/signal`,
//! §`VTCCall/exportMediaSecret`).
//!
//! Both are the backend's to carry out, and a backend that offers
//! neither — `MemoryBackend`, which takes both default implementations —
//! answers `notFound`. What the handlers own is the argument checking,
//! which is what these tests pin: a signal whose kind and payload
//! disagree never reaches the backend, because a client that sent an
//! offer with no session description has a call that will not connect and
//! this is the only chance to say so.

mod common;

use common::*;
use jmap_server::HandlerOutput;
use jmap_vtc_server::{handle_vtc_call_export_media_secret, handle_vtc_call_signal};
use serde_json::{Value, json};

/// `VTCCall/signal` with `args`, as `BOB`; the method error or the
/// response.
async fn signal(
    backend: &jmap_vtc_server::memory::MemoryBackend,
    mut args: Value,
) -> Result<Value, String> {
    args["accountId"] = json!(ACCOUNT);
    match handle_vtc_call_signal(backend, &jmap_types::Id::from(BOB), args).await {
        Ok(HandlerOutput { primary, .. }) => Ok(primary),
        Err(error) => Err(serde_json::to_value(&error).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned()),
    }
}

#[tokio::test]
async fn a_signal_whose_kind_and_payload_disagree_is_refused() {
    let backend = backend();
    let call = create(
        &backend,
        BOB,
        json!({"callType": "room", "mediaTypes": ["audio"]}),
    )
    .await;
    let call_id = call["id"].as_str().unwrap().to_owned();

    // An offer with no sdp, and an offer carrying a candidate instead.
    for args in [
        json!({"callId": call_id, "signal": "offer"}),
        json!({"callId": call_id, "signal": "offer", "candidate": {"candidate": "x"}}),
        json!({"callId": call_id, "signal": "candidate", "sdp": "v=0\r\n"}),
        // A kind this library does not know takes no payload: nothing
        // can be made of one.
        json!({"callId": call_id, "signal": "rollback", "sdp": "v=0\r\n"}),
    ] {
        assert_eq!(
            signal(&backend, args.clone()).await,
            Err("invalidArguments".to_owned()),
            "{args}"
        );
    }

    // And the arguments the method needs at all.
    assert_eq!(
        signal(&backend, json!({"signal": "offer", "sdp": "v=0\r\n"})).await,
        Err("invalidArguments".to_owned()),
        "no callId"
    );
    assert_eq!(
        signal(&backend, json!({"callId": call_id, "sdp": "v=0\r\n"})).await,
        Err("invalidArguments".to_owned()),
        "no signal"
    );
}

#[tokio::test]
async fn a_backend_with_no_relay_declines_a_well_formed_signal() {
    let backend = backend();
    let call = create(
        &backend,
        BOB,
        json!({"callType": "room", "mediaTypes": ["audio"]}),
    )
    .await;
    let call_id = call["id"].as_str().unwrap().to_owned();

    // Well-formed, and the default implementation declines it: the
    // deployment does not offer the method.
    assert_eq!(
        signal(
            &backend,
            json!({"callId": call_id, "signal": "offer", "sdp": "v=0\r\n"})
        )
        .await,
        Err("invalidArguments".to_owned()),
        "the backend's notFound becomes invalidArguments carrying its description"
    );
    assert_eq!(
        signal(
            &backend,
            json!({"callId": call_id, "signal": "candidate",
                   "candidate": {"candidate": "candidate:1 1 UDP 1 192.0.2.1 5000 typ host"}})
        )
        .await,
        Err("invalidArguments".to_owned())
    );
}

#[tokio::test]
async fn exporting_a_media_secret_checks_its_arguments_and_the_backend_declines() {
    let backend = backend();
    let call = create(
        &backend,
        BOB,
        json!({"callType": "room", "mediaTypes": ["audio"]}),
    )
    .await;
    let call_id = call["id"].as_str().unwrap().to_owned();
    let export = |args: Value| {
        let backend = &backend;
        async move {
            let mut args = args;
            args["accountId"] = json!(ACCOUNT);
            handle_vtc_call_export_media_secret(backend, &jmap_types::Id::from(BOB), args)
                .await
                .map(|HandlerOutput { primary, .. }| primary)
                .map_err(|error| {
                    serde_json::to_value(&error).unwrap()["type"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
        }
    };

    assert_eq!(
        export(json!({})).await,
        Err("invalidArguments".to_owned()),
        "no callId"
    );
    assert_eq!(
        export(json!({"callId": call_id, "epoch": "soon"})).await,
        Err("invalidArguments".to_owned()),
        "an epoch that is not a number"
    );
    assert_eq!(
        export(json!({"callId": call_id, "epoch": -1})).await,
        Err("invalidArguments".to_owned()),
        "an epoch below zero"
    );
    // Well-formed, and the default implementation declines it.
    assert_eq!(
        export(json!({"callId": call_id})).await,
        Err("invalidArguments".to_owned())
    );
}
