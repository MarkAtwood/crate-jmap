# Agent Instructions — jmap-quotas

Umbrella facade for JMAP Quotas (RFC 9425): re-exports `jmap-quotas-types`
(always), `jmap-quotas-client` (`client` feature) and `jmap-quotas-server`
(`server`; `memory` adds its reference backend). It holds no logic: change
the component crates, not this one. It is the first umbrella in the
workspace; the other claimed umbrellas (`jmap-mail`, `jmap-blob`, …, bd
JMAP-amr2) copy its shape.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-quotas
cargo test -p jmap-quotas --features client
cargo test -p jmap-quotas --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
