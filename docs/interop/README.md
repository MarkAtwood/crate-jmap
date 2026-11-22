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
`JMAP_INTEROP_STRICT=1` turns known divergences into failures (see
below).

The scripts read `STALWART_PORT` (default 18080), `STALWART_CONTAINER`
(default `crate-jmap-interop-stalwart`) and `STALWART_ENV_FILE`.
`stalwart-down.sh` is idempotent.

On GitHub the `interop` workflow (`.github/workflows/interop.yml`) does
the same on demand and weekly; it never runs on push.

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
alice to bob.

Tests run one at a time within each test binary
(`jmap_interop::exclusive`): Stalwart advertises
`maxConcurrentRequests: 4` and answers the `limit` problem above it.

## Differential tests

`tests/differential.rs` (bd:JMAP-cyub.4) sends the same requests to
`jmap-testjig`, the workspace's reference server, and to the configured
server, then compares the responses. The testjig runs in the test
process (`jmap_testjig::spawn_in_process`, a fresh in-memory jig per
test on a free loopback port, bearer token), so only the external server
needs starting. Both are seeded the same way through the API: each test
creates its own Mailboxes and Emails under unique names.

Each scenario turns the responses into named facts with the
server-specific values normalized out: ids become the aliases the test
gave the objects, states are only compared with each other on the same
server, capability limits are reduced to their JSON types, and keywords
the server sets itself (`$hasnoattachment`) are dropped. Compared:

- the Session shape (RFC 8620 §2, RFC 8621 §1.3);
- request-level errors (`notJSON`, `notRequest`, `unknownCapability`),
  `Core/echo` and `createdIds` (RFC 8620 §3.4, §3.6.1, §4);
- method-level errors for the same invalid calls (`unknownMethod`,
  `accountNotFound`, `invalidArguments`, `invalidResultReference`,
  `unsupportedSort`, `unsupportedFilter`, a never-issued `sinceState`);
- Mailbox `/set` create, update and destroy, the `created` contents,
  `/get` properties, `/changes` after each `/set`, `/query` order and
  filters, and SetErrors (`invalidProperties`, `notFound`,
  `mailboxHasChild`, `mailboxHasEmail`);
- Email `/set`, `/get`, `/query` order, filters, `position` and
  `anchor`, result references (`/ids`, `/list/*/threadId` into
  `Thread/get`), creation ids across calls, Email and Mailbox `/changes`
  after `/set` (with `updatedProperties`), and SetErrors.

A fact that differs fails the test unless `KNOWN` in the test file
classifies it: an RFC deviation of the external server or a known
testjig bug (both reported with `known_divergence`), or a difference the
RFCs allow (printed only).

## Known divergences

A server behaviour that contradicts an RFC and is already tracked
(bd:JMAP-cyub.5) is reported with `jmap_interop::known_divergence`: the
test prints it to standard error and passes. Set `JMAP_INTEROP_STRICT=1`
to make each one fail instead (add `--nocapture` to see them in a green
run). Known for Stalwart v1.1.1:

| RFC | Expected | Stalwart v1.1.1 |
|---|---|---|
| RFC 8620 §3.6.1 | an unsupported `using` capability is `unknownCapability` | `notRequest` (400, problem details) |
| RFC 8620 §5.3 | every `/set` response has `oldState` (`String\|null`) and `newState` | `Identity/set` has no `oldState`; `Identity/set` and `EmailSubmission/set` without a change omit both; a successful `EmailSubmission/set` create has no `oldState` |
| RFC 8620 §3.6.1 | a body that does not parse as I-JSON is `notJSON` | `notRequest` |
| RFC 8620 §3.3 | an empty `createdIds` in the Request is echoed as `{}` | no `createdIds` in the Response (support.stalw.art t/2474) |
| RFC 8620 §1.2, §3.4 | a `createdIds` value containing `-` (a valid Id character) is accepted and echoed | the whole Request is refused with `notRequest` (t/2474) |
| RFC 8621 §2 | `isSubscribed` SHOULD default to `true` for a Mailbox the user creates | `false` |

Differences the RFCs allow, found by the differential tests:

| RFC | testjig | Stalwart v1.1.1 |
|---|---|---|
| RFC 8620 §3.3 | a method whose capability is not in `using` runs | `unknownMethod` |
| RFC 8620 §5.2, §3.6.2 | a `sinceState` it never issued is `cannotCalculateChanges` | `invalidArguments` |
| RFC 8620 §5.3 | `created` also echoes the sent `name` and `parentId` | only the properties not sent |
| RFC 8620 §5.3 | a child created in the same `Mailbox/set` that destroys its parent is created, the destroy fails with `mailboxHasChild` | the create fails with `willDestroy` (a SetError §5.3 lists for updates) |
| draft-ietf-jmap-metadata-05 §4.1 | no `maySetMetadata` (Mailbox not a listed metadata type) | `maySetMetadata` in `myRights` |

The clients tolerate the missing `newState` (decision 67 in
`docs/design/DECISIONS.md`): `jmap_types::SetResponse` decodes it as the
empty state, so `jmap-mail-client`'s `email_submission_set` succeeds, and
`jmap_base_client::extract_response` logs a `WARN` event
(`set response has no newState`, with fields
`method`, `account_id` and `missing`). The empty state is not a real
one: re-fetch the state with `Foo/get` rather than passing it to
`Foo/changes`. The interop tests still report the divergence.

[Stalwart]: https://stalw.art
