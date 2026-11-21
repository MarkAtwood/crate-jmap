# jmap-interop

Opt-in interop tests of the workspace's JMAP client crates against an
external JMAP server, by default Stalwart in a throwaway container.
Test-only: `publish = false`, never released.

## What it is

- `src/lib.rs`: shared helpers. `InteropEnv` reads the server's base URL
  and two accounts' credentials from `JMAP_INTEROP_URL`,
  `JMAP_INTEROP_USER`/`_PASSWORD` and `JMAP_INTEROP_USER2`/`_PASSWORD2`,
  and builds a Basic-auth `jmap_base_client::JmapClient` per account.
  `unique` makes object names unique per call, since the server keeps
  state between tests.
- `tests/`: `#[ignore]`d tokio tests that drive `jmap-base-client` and
  `jmap-mail-client` against the server and assert RFC 8620 and RFC 8621
  facts: Session discovery and auth, `Core/echo`, error shapes, Mailbox,
  Email, Thread, Identity, EmailSubmission and `/changes`.

## What it's for

Finding where the client crates and a real, independently written server
disagree, which the wiremock tests in the client crates and the
`jmap-testjig` tests (both built from this workspace's own reading of the
RFCs) cannot. Depends on `jmap-base-client`; its tests also use
`jmap-mail-client`. Nothing depends on it.

## How to use

```sh
scripts/stalwart-up.sh
. target/interop/stalwart.env
cargo test -p jmap-interop -- --ignored
scripts/stalwart-down.sh
```

[docs/interop/README.md](../docs/interop/README.md) describes the
scripts, the variables and what the container is set up with.

## How it works

Each test calls `InteropEnv::require()`, builds a client for alice (or
bob), fetches the Session and binds it with `with_mail_session`. Tests
create their own Mailboxes and Emails under unique names and remove them
afterwards, so they can run in parallel and against a server that is not
empty. Assertions use only what the RFCs require; values the RFCs leave
to the server (ids, state strings, default mailbox names) are only
compared with each other.

## Gotchas

- `cargo test -p jmap-interop` without `--ignored` runs only the helper
  unit tests. With `--ignored` but without `JMAP_INTEROP_URL`, every
  interop test fails with instructions.
- Known server divergences (`known_divergence`) only print a line and
  pass; `JMAP_INTEROP_STRICT=1` makes them fail. The list for Stalwart is
  in docs/interop/README.md.
- Tests that talk to the server hold `exclusive()`, so they run one at a
  time: Stalwart refuses more than `maxConcurrentRequests` (4) at once.
- Stalwart is AGPL-licensed: it runs only in the container. Do not add a
  dependency on any Stalwart crate.
- The Stalwart container speaks plain HTTP on 127.0.0.1. Use full email
  addresses as usernames.

## References

- [RFC 8620] — JMAP Core
- [RFC 8621] — JMAP for Mail
- [Stalwart]

[RFC 8620]: https://www.rfc-editor.org/rfc/rfc8620
[RFC 8621]: https://www.rfc-editor.org/rfc/rfc8621
[Stalwart]: https://stalw.art
