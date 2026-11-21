# Interop testing against Stalwart

The `jmap-interop` crate (`crate-jmap-interop/`) runs the workspace's
client crates against a real JMAP server: [Stalwart], started as a
throwaway container (bd:JMAP-cyub). The tests are opt-in. Each one is
`#[ignore]`d, so `cargo test --workspace` and the default CI never need a
container.

Stalwart is AGPL-licensed. It runs only as an external test server in a
container. Nothing in this workspace links, vendors or copies its code.

## Run

Needs podman (rootless works), curl and jq.

```sh
scripts/stalwart-up.sh
. target/interop/stalwart.env
cargo test -p jmap-interop -- --ignored
scripts/stalwart-down.sh
```

`stalwart-up.sh` prints the base URL and the credentials and writes them
to `target/interop/stalwart.env`. Without the env file, set the variables
yourself:

| Variable | Meaning |
|---|---|
| `JMAP_INTEROP_URL` | Base URL, e.g. `http://127.0.0.1:18080`; the Session is at `/.well-known/jmap` |
| `JMAP_INTEROP_USER`, `JMAP_INTEROP_PASSWORD` | First account (alice), HTTP Basic auth |
| `JMAP_INTEROP_USER2`, `JMAP_INTEROP_PASSWORD2` | Second account (bob), the recipient of submitted mail |

Any JMAP server with two such accounts that can mail each other can be
tested the same way. `--ignored` without `JMAP_INTEROP_URL` fails each
test with a message saying how to start the server.

The scripts read `STALWART_PORT` (default 18080), `STALWART_CONTAINER`
(default `crate-jmap-interop-stalwart`) and `STALWART_ENV_FILE`.
`stalwart-down.sh` is idempotent.

## What the scripts set up

`scripts/stalwart-up.sh` pins the image by digest and records the Stalwart
version next to it. The server:

- listens on `127.0.0.1` only, plain HTTP;
- keeps its data on tmpfs, so each run starts empty and `down` leaves no
  volume;
- gets a one-line `config.json` (RocksDB data store) written inside the
  container before start, so it skips bootstrap mode (the setup wizard)
  and starts normally;
- has its administrator pinned with `STALWART_RECOVERY_ADMIN` and its
  Session URLs set to the published port with `STALWART_PUBLIC_URL`;
- gets the domain `example.org` and the users `alice@example.org` and
  `bob@example.org` through Stalwart's JMAP management API
  (`urn:stalwart:jmap`: `x:Domain/set`, `x:Account/set`). Stalwart
  creates each user's default mailboxes and identity itself.

Log in with the full email address. Bare usernames need Stalwart's
default domain to be set and the settings reloaded; the scripts do not
do that.

## What the tests check

Protocol facts from RFC 8620 and RFC 8621, not Stalwart's choices: the
Session resource, authentication failure, `Core/echo`, request-level
and method-level error shapes, Mailbox, Email, Thread, Identity and
EmailSubmission methods, `/changes` state tracking, and delivery from
alice to bob. Divergences found are tracked under bd:JMAP-cyub.5.

[Stalwart]: https://stalw.art
