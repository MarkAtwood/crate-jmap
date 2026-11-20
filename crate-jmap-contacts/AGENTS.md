# Agent Instructions — jmap-contacts

Umbrella facade for JMAP Contacts (RFC 9610): re-exports `jmap-contacts-types`
(always), `jmap-contacts-client` (`client` feature) and `jmap-contacts-server`
(`server`; `memory` adds its reference backend). It holds no logic: change the component crates,
not this one. It copies the shape of `jmap-filenode` and replaces the 0.0.1
name-claim placeholder (bd JMAP-d97x).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-contacts
cargo test -p jmap-contacts --features client
cargo test -p jmap-contacts --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
