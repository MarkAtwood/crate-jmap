# Agent Instructions — jmap-tasks

Umbrella facade for JMAP Tasks (the local JMAP Tasks profile (docs/specs/jmap-tasks-local.md), based on draft-ietf-jmap-tasks-06): re-exports `jmap-tasks-types`
(always), `jmap-tasks-client` (`client` feature) and `jmap-tasks-server`
(`server`; `memory` adds its reference backend). It holds no logic: change the component crates,
not this one. It copies the shape of `jmap-filenode` and replaces the 0.0.1
name-claim placeholder (bd JMAP-unaa).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-tasks
cargo test -p jmap-tasks --features client
cargo test -p jmap-tasks --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
