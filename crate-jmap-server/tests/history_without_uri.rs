//! The decision-18 dispatcher check: object-history arguments without
//! `urn:ietf:params:jmap:object-history` in `using` (bd JMAP-tna1.3.25).
//!
//! Oracle: docs/design/DECISIONS.md decision 18 ("one dispatcher check on
//! */get, */query, */set for every deployment: includeReplaced: true,
//! includeDestroyed: true, onlyDestroyed: true, non-null destroyedAfter /
//! destroyedBefore, non-empty restore ⇒ invalidArguments"; defaults
//! accepted and removed), extended to */queryChanges by the -04 editor's
//! copy §4, and object-history §3 (the four other `history*` arguments are
//! ignored when both flags are false).

use std::sync::{Arc, Mutex};

use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, JmapHandler,
};
use jmap_types::{JmapRequest, State};
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Value>>>;

fn probe(seen: &Seen) -> Arc<dyn JmapHandler<()>> {
    Arc::new(ClosureHandler::new(
        Arc::clone(seen),
        |seen: Arc<Mutex<Vec<Value>>>, cx: CallContext<()>| {
            seen.lock().unwrap().push(cx.args);
            Box::pin(async { Ok(HandlerOutput::new(json!({}))) }) as HandlerFuture
        },
    ))
}

/// Dispatch one call; the response arguments and what the handler saw.
async fn call(
    history_aware: bool,
    method: &str,
    args: Value,
    with_uri: bool,
) -> (Value, Option<Value>) {
    let seen: Seen = Arc::default();
    let mut d: Dispatcher<()> = Dispatcher::new();
    if history_aware {
        d.register_history_aware(method, probe(&seen));
    } else {
        d.register(method, probe(&seen));
    }
    let mut using = vec!["urn:ietf:params:jmap:core"];
    if with_uri {
        using.push("urn:ietf:params:jmap:object-history");
    }
    let req: JmapRequest =
        serde_json::from_value(json!({"using": using, "methodCalls": [[method, args, "0"]]}))
            .unwrap();
    let resp = serde_json::to_value(d.dispatch(req, (), State::from("s")).await).unwrap();
    let got = seen.lock().unwrap().first().cloned();
    (resp["methodResponses"][0].clone(), got)
}

#[tokio::test]
async fn requests_for_history_are_rejected_without_the_uri() {
    let cases = [
        ("Note/get", json!({"includeReplaced": true})),
        ("Note/get", json!({"includeDestroyed": true})),
        ("Note/get", json!({"includeDestroyed": "yes"})),
        ("Note/query", json!({"onlyDestroyed": true})),
        (
            "Note/query",
            json!({"destroyedAfter": "2026-01-01T00:00:00Z"}),
        ),
        ("Note/query", json!({"destroyedBefore": false})),
        ("Note/queryChanges", json!({"onlyDestroyed": true})),
        ("Note/set", json!({"restore": {"x": null}})),
        ("Note/set", json!({"restore": []})),
    ];
    for aware in [false, true] {
        for (method, args) in &cases {
            let (resp, seen) = call(aware, method, args.clone(), false).await;
            assert_eq!(resp[0], "error", "{method} {args}");
            assert_eq!(resp[1]["type"], "invalidArguments", "{method} {args}");
            assert!(seen.is_none(), "handler ran for {method} {args}");
        }
    }
}

#[tokio::test]
async fn defaults_reach_the_handler_without_the_keys() {
    let cases = [
        (
            "Note/get",
            json!({"includeReplaced": false, "includeDestroyed": null, "historyLimit": "x"}),
            json!({"historyLimit": "x"}),
        ),
        (
            "Note/query",
            json!({"onlyDestroyed": false, "destroyedAfter": null}),
            json!({}),
        ),
        (
            "Note/queryChanges",
            json!({"destroyedBefore": null}),
            json!({}),
        ),
        ("Note/set", json!({"restore": {}}), json!({})),
        (
            "Note/set",
            json!({"restore": null, "update": {}}),
            json!({"update": {}}),
        ),
        (
            "Note/changes",
            json!({"restore": {"x": null}}),
            json!({"restore": {"x": null}}),
        ),
    ];
    for aware in [false, true] {
        for (method, args, want) in &cases {
            let (resp, seen) = call(aware, method, args.clone(), false).await;
            assert_eq!(resp[0], *method, "{method} {args}: {resp}");
            assert_eq!(seen.as_ref(), Some(want), "{method} {args}");
        }
    }
}

#[tokio::test]
async fn with_the_uri_the_check_does_nothing() {
    let args = json!({"includeDestroyed": true, "includeReplaced": false});
    let (_, seen) = call(false, "Note/get", args.clone(), true).await;
    assert_eq!(seen, Some(args));
    let args = json!({"restore": {"x": null}});
    let (_, seen) = call(true, "Note/set", args.clone(), true).await;
    assert_eq!(seen, Some(args));
}
