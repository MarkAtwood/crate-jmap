# Agent Instructions — jmap-conditional

Facade for JMAP conditional and atomic Foo/set
(draft-ietf-jmap-conditional-02): re-exports `jmap_types::conditional` as
`types` (always) and the conditional items of `jmap-server` as `server`
(`server` feature; `scoped-state` adds `ScopedState`, `ScopeGate` and
`ScopedGuard`), plus `CAPABILITY`, the one item the 0.0.1 placeholder
published. The extension is a module of `jmap-types` and items of
`jmap-server` (docs/design/conditional.md, DECISIONS.md decisions 1, 17
and 50), not a crate family. It holds no logic: change those crates, not
this one. It copies the shape of `jmap-quotas`, the workspace's first
umbrella, and replaces the 0.0.1 name-claim placeholder (bd JMAP-gs76.11).

When `jmap-server` adds or removes a conditional item, update the
`server` module's list here in the same change.

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-conditional
cargo test -p jmap-conditional --features server
cargo test -p jmap-conditional --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
