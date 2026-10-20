# jmap-scene-client

## What it is

Client support for the JMAP Scene extension ([draft-atwood-jmap-scene-00]).
`AssetFetcher` fetches a SceneAsset or SceneObject `assetUri` under the
draft's §assetUri Is Untrusted rules.

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

[draft-atwood-jmap-scene-00]: https://github.com/MarkAtwood/jmap-chat-spec
