# Agent Instructions — jmap-websocket

RFC 8887 JMAP over WebSocket (`urn:ietf:params:jmap:websocket`), bd
JMAP-tzcc. A support crate: no `*-types` or `*-server` crate depends on
it. The normative text is RFC 8887
(https://www.rfc-editor.org/rfc/rfc8887.txt); cite its sections in docs
and tests.

Scope:

- The frame types and the capability are always built and stay free of
  async and of `jmap-server`, so a client crate can depend on the crate
  with `default-features = false`.
- `server` holds the per-connection driver, `PushHub` and the parsing
  rules. It is framework-agnostic (`Stream` of `Inbound`, `Sink` of
  `Outbound`); library adapters go behind their own feature, like
  `tungstenite`.
- The opening handshake, authentication and TLS stay the deployment's.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-websocket
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- Tests quote RFC 8887 examples verbatim; never derive an oracle from the code under test.
- Do not use TodoWrite or markdown task lists — use `bd create`
