# jmap-scene-client

## What it is

Client support for the JMAP Scene extension ([draft-atwood-jmap-scene-00]).
`JmapSceneExt` binds the `/get`, `/changes`, `/set`, `/query` and
`/queryChanges` methods of SceneAsset, SceneRegion, SceneObject and
SceneAvatar over `jmap-base-client`. `AssetFetcher` fetches a SceneAsset or
SceneObject `assetUri` under the draft's §assetUri Is Untrusted rules.

## Scene methods

`with_scene_session` binds a `JmapClient` to a session; each method on the
returned `SessionClient` targets the primary `urn:ietf:params:jmap:scene`
account, or the account chosen with `for_account`. A region's records live
in its hosting account, which other users reach as a shared account
(§Accounts), so pass that account's id to `for_account` to enter or query
a region hosted elsewhere.

```rust,no_run
use jmap_scene_client::JmapSceneExt;
use jmap_scene_types::{Filter, SceneObjectFilterCondition, WithinRadius};

# async fn run(client: jmap_base_client::JmapClient) -> Result<(), jmap_base_client::ClientError> {
let session = client.fetch_session().await?;
let sc = client.with_scene_session(session);
let mut cond = SceneObjectFilterCondition::default();
cond.region_id = Some("01J5ABC0000000000000000001".into());
cond.within_radius = Some(WithinRadius::new([5.0, 0.0, -2.0], 20.0));
let nearby = sc
    .scene_object_query(
        Some(&Filter::Condition(cond)),
        None,
        &jmap_base_client::QueryParams {
            limit: Some(25),
            ..Default::default()
        },
    )
    .await?;
# Ok(()) }
```

`SceneObject/interact` is not bound yet.

## What it's for

`assetUri` is peer-supplied. `AssetFetcher::fetch` refuses any scheme but
`https:` before connecting, refuses redirects to non-HTTPS endpoints, stops
reading past a caller-supplied size limit, and checks the body against the
SceneAsset `sha256` when one is given. The bytes it returns are still
untrusted: §Asset Parsing requires parsing them in a sandbox.

## How to use

```rust,no_run
use jmap_cid_types::Sha256;
use jmap_scene_client::AssetFetcher;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let fetcher = AssetFetcher::new()?;
let sha256: Sha256 =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".parse()?;
let glb = fetcher
    .fetch("https://cdn.example.com/assets/sculpture-001.glb", Some(&sha256), 2_483_712)
    .await?;
# Ok(()) }
```

Pass a `reqwest::ClientBuilder` to `AssetFetcher::with_builder` to set a
timeout, root certificates or a proxy.

## Scene WebSocket events

`SceneWsExt` adds [draft-atwood-jmap-scene-wss-00] ephemeral Scene events
to a `jmap_base_client::WsSession`. `send_scene_stream_enable` and
`send_scene_stream_disable` start, replace and cancel the subscription;
`send_scene_interaction` submits a `SceneInteractionEvent`.
`next_scene_frame` yields `SceneAvatarEvent`, `SceneObjectEvent` and
`SceneInteractionEvent` frames typed. Every other frame (`StateChange`,
`Response`, a `RequestError` rejecting a `SceneStreamEnable`, VTC or Chat
events, unrecognized `@type`s, and Scene events that do not parse) arrives
unchanged as `SceneWsFrame::Other`.

```rust,no_run
use jmap_base_client::WsSession;
use jmap_scene_client::{SceneWsExt, SceneWsFrame};
use jmap_scene_types::SceneStreamEnable;

# async fn run(mut ws: WsSession) -> Result<(), jmap_base_client::ClientError> {
let mut enable = SceneStreamEnable::default();
enable.region_ids = Some(vec!["01J5ABC0000000000000000001".into()]);
ws.send_scene_stream_enable(&enable).await?;
while let Some(frame) = ws.next_scene_frame().await {
    match frame? {
        SceneWsFrame::Avatar(e) => println!("{} {:?}", e.display_name, e.event),
        _ => {}
    }
}
# Ok(()) }
```

[draft-atwood-jmap-scene-00]: https://github.com/MarkAtwood/jmap-chat-spec
[draft-atwood-jmap-scene-wss-00]: https://github.com/MarkAtwood/jmap-chat-spec
