//! Object history for SceneAsset, SceneRegion and SceneObject
//! (draft-ietf-jmap-object-history-03 with the -04 editor's copy changes;
//! docs/design/object-history.md; docs/design/DECISIONS.md decision 22).
//! SceneAvatar keeps none.
//!
//! §8.2: "History MUST respect current access". SceneRegion and SceneObject
//! entries pass the same region-access and visibility checks as current
//! objects ([`is_visible`]) and the same redaction (`invitedUserIds` only
//! for a caller who may update the region); an entry the caller may not see
//! is `notFound`, and a destroyed query leaves out ids the caller may not
//! see.

use std::collections::BTreeSet;
use std::sync::Arc;

use jmap_object_history_types::{HistoryGetArguments, HistoryObject};
use jmap_scene_types::{SceneAsset, SceneObject, SceneRegion};
use jmap_server::{
    CallContext, ClosureHandler, Dispatcher, HandlerFuture, HandlerOutput, HistoryGetParams,
    JmapHandler, NoRestoreHooks, ObjectHistoryBackend, attach_object_history,
    enforce_max_objects_in_get, extract_account_id, history_get, history_query,
    history_query_changes, history_set, not_found_json, optional_arg, parse_history_get_arguments,
    require_history_support, serialize_value, server_fail_from_backend,
    strip_object_history_property,
};
use jmap_types::{Id, JmapError};
use serde_json::{Value, json};

use crate::helpers::{Visible, is_visible};
use crate::{
    SceneBackend, asset, handle_scene_asset_get, handle_scene_asset_query,
    handle_scene_asset_query_changes, handle_scene_object_get, handle_scene_object_query,
    handle_scene_object_query_changes, handle_scene_object_set, handle_scene_region_get,
    handle_scene_region_query, handle_scene_region_query_changes, handle_scene_region_set,
    register_scene_handlers,
};

/// A history `Foo/get` for a type whose current objects pass
/// [`is_visible`] and are redacted: `plain` when no history argument is
/// set.
async fn history_get_visible<O, B, F, Fut>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
    plain: F,
) -> Result<HandlerOutput, JmapError>
where
    O: Visible + HistoryObject,
    B: SceneBackend + ObjectHistoryBackend,
    F: FnOnce(Value) -> Fut,
    Fut: std::future::Future<Output = Result<HandlerOutput, JmapError>>,
{
    let Value::Object(mut map) = args else {
        return plain(args).await;
    };
    let history = parse_history_get_arguments(&mut map, using)?;
    strip_object_history_property(&mut map);
    let Some(history) = history else {
        return plain(Value::Object(map)).await;
    };
    let (account_id, mut args) = extract_account_id(Value::Object(map))?;
    if !backend
        .account_exists(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?
    {
        return Err(JmapError::account_not_found());
    }
    require_history_support::<O, B>(backend, caller, &account_id)?;
    enforce_max_objects_in_get(&args, backend.max_objects_in_get(caller, &account_id))?;
    let ids: Option<Vec<Id>> = optional_arg(&mut args, "ids", || {
        JmapError::invalid_arguments("ids must be an Id array")
    })?;
    let properties: Option<Vec<String>> = optional_arg(&mut args, "properties", || {
        JmapError::invalid_arguments("properties must be a string array")
    })?;
    let result = backend
        .get_object_history::<O>(
            caller,
            &account_id,
            HistoryGetParams::new(&history).ids(ids.as_deref()),
        )
        .await
        .map_err(JmapError::from)?;
    let mut list = Vec::with_capacity(result.entries.len());
    let mut not_found = result.not_found;
    let mut shown: BTreeSet<Id> = BTreeSet::new();
    let mut hidden: BTreeSet<Id> = BTreeSet::new();
    for entry in result.entries {
        let id = entry.object.id().clone();
        if !is_visible(backend, caller, &account_id, &entry.object)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            hidden.insert(id);
            continue;
        }
        let (object, _) = entry
            .object
            .redact(backend, caller, &account_id)
            .await
            .map_err(|e| server_fail_from_backend(&e))?;
        let mut item = serialize_value(&object)?;
        if let Value::Object(m) = &mut item {
            if let Some(props) = &properties {
                m.retain(|k, _| k == "id" || props.iter().any(|p| p == k));
            }
            attach_object_history(m, serialize_value(&entry.object_history)?, None);
        }
        shown.insert(id);
        list.push(item);
    }
    // An id with no visible entry is `notFound`, like an unreadable current
    // object.
    not_found.extend(hidden.into_iter().filter(|id| !shown.contains(id)));
    let state = backend
        .get_state::<O>(caller, &account_id)
        .await
        .map_err(|e| server_fail_from_backend(&e))?;
    Ok(HandlerOutput::new(json!({
        "accountId": account_id.as_ref(),
        "state": state.as_ref(),
        "list": list,
        "notFound": not_found_json(&not_found),
    })))
}

/// A history `Foo/query` for a visibility-checked type: a destroyed
/// query's `ids` leave out what the caller may not see in its final
/// version (`total` is then omitted rather than reveal the count).
async fn history_query_visible<O, B, F, Fut>(
    backend: &B,
    caller: &B::CallerCtx,
    args: Value,
    using: &[String],
    plain: F,
) -> Result<HandlerOutput, JmapError>
where
    O: Visible + HistoryObject + jmap_server::QueryObject,
    B: SceneBackend + ObjectHistoryBackend,
    F: FnOnce(Value) -> Fut,
    Fut: std::future::Future<Output = Result<HandlerOutput, JmapError>>,
{
    let destroyed = args.get("onlyDestroyed") == Some(&Value::Bool(true));
    let account = args.get("accountId").and_then(Value::as_str).map(Id::from);
    let mut out = history_query::<O, B, _, _>(backend, caller, args, using, plain).await?;
    let (true, Some(account)) = (destroyed, account) else {
        return Ok(out);
    };
    let ids: Vec<Id> = out.primary["ids"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(Id::from).collect())
        .unwrap_or_default();
    let mut finals = HistoryGetArguments::new();
    finals.include_destroyed = true;
    let entries = backend
        .get_object_history::<O>(
            caller,
            &account,
            HistoryGetParams::new(&finals).ids(Some(&ids)),
        )
        .await
        .map_err(JmapError::from)?
        .entries;
    let mut visible: BTreeSet<Id> = BTreeSet::new();
    for e in &entries {
        if is_visible(backend, caller, &account, &e.object)
            .await
            .map_err(|e| server_fail_from_backend(&e))?
        {
            visible.insert(e.object.id().clone());
        }
    }
    out.primary["ids"] = json!(
        ids.into_iter()
            .filter(|id| visible.contains(id))
            .collect::<Vec<_>>()
    );
    if let Value::Object(m) = &mut out.primary {
        m.remove("total");
    }
    Ok(out)
}

/// [`register_scene_handlers`], then history-aware `/get`, `/set`,
/// `/query` and `/queryChanges` of SceneAsset, SceneRegion and SceneObject,
/// each wrapping the unchanged plain handler, through
/// `Dispatcher::register_history_aware` (docs/design/DECISIONS.md decision
/// 1). `SceneObject/interact` is not a `/set` and is unchanged.
pub fn register_scene_handlers_with_history<B>(
    dispatcher: &mut Dispatcher<B::CallerCtx>,
    backend: Arc<B>,
) where
    B: SceneBackend + ObjectHistoryBackend + 'static,
{
    register_scene_handlers(dispatcher, Arc::clone(&backend));
    macro_rules! reg {
        ($method:expr, |$b:ident, $a:ident, $ctx:ident, $using:ident $(, $field:ident)*| $body:expr) => {{
            let h: Arc<dyn JmapHandler<B::CallerCtx>> = Arc::new(ClosureHandler::new(
                Arc::clone(&backend),
                move |$b: Arc<B>, cx: CallContext<B::CallerCtx>| {
                    let CallContext {
                        args: $a,
                        caller: $ctx,
                        using: $using,
                        $($field,)*
                        ..
                    } = cx;
                    Box::pin(async move { $body }) as HandlerFuture
                },
            ));
            dispatcher.register_history_aware($method, h);
        }};
    }
    // SceneAsset: no per-caller visibility.
    reg!("SceneAsset/get", |b, a, ctx, using| {
        history_get::<SceneAsset, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_asset_get(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneAsset/query", |b, a, ctx, using| {
        history_query::<SceneAsset, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_asset_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneAsset/queryChanges", |b, a, ctx, using| {
        history_query_changes::<SceneAsset, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_asset_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneAsset/set", |b, a, ctx, using, created_ids| {
        history_set::<SceneAsset, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            asset::scene_asset_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
    // SceneRegion
    reg!("SceneRegion/get", |b, a, ctx, using| {
        history_get_visible::<SceneRegion, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_region_get(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneRegion/query", |b, a, ctx, using| {
        history_query_visible::<SceneRegion, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_region_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneRegion/queryChanges", |b, a, ctx, using| {
        history_query_changes::<SceneRegion, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_region_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneRegion/set", |b, a, ctx, using, created_ids| {
        history_set::<SceneRegion, B, _, _>(&*b, &ctx, a, &using, &NoRestoreHooks, |a| {
            handle_scene_region_set(&*b, &ctx, &created_ids, a)
        })
        .await
    });
    // SceneObject: parent restored before child (-04 §5.1).
    reg!("SceneObject/get", |b, a, ctx, using| {
        history_get_visible::<SceneObject, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_object_get(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneObject/query", |b, a, ctx, using| {
        history_query_visible::<SceneObject, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_object_query(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneObject/queryChanges", |b, a, ctx, using| {
        history_query_changes::<SceneObject, B, _, _>(&*b, &ctx, a, &using, |a| {
            handle_scene_object_query_changes(&*b, &ctx, a)
        })
        .await
    });
    reg!("SceneObject/set", |b, a, ctx, using, created_ids| {
        history_set::<SceneObject, B, _, _>(
            &*b,
            &ctx,
            a,
            &using,
            &jmap_server::ParentRestoreHooks("parentId"),
            |a| handle_scene_object_set(&*b, &ctx, &created_ids, a),
        )
        .await
    });
}
