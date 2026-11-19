# Agent Instructions — jmap-graph

Umbrella facade for JMAP Graph (draft-atwood-jmap-graph-00): re-exports `jmap-graph-types`
(always), `jmap-graph-client` (`client` feature) and `jmap-graph-server`
(`server`; `memory` adds its reference backend). It holds no logic: change the component crates,
not this one. It copies the shape of `jmap-quotas`, the workspace's first
umbrella, and replaces the 0.0.1 name-claim placeholder (bd JMAP-gs76).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-graph
cargo test -p jmap-graph --features client
cargo test -p jmap-graph --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
