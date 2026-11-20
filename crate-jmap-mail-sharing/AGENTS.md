# Agent Instructions — jmap-mail-sharing

Facade for JMAP for Mail Sharing (draft-ietf-jmap-mail-sharing-02):
re-exports `jmap-sharing-types` (always), `jmap-sharing-client` (`client`
feature) and `jmap-sharing-server` (`server`; `memory` adds its reference
backend), the same set as the `jmap-sharing` umbrella, plus `CAPABILITY`
(`urn:ietf:params:jmap:mail:share`), the one item the 0.0.1 placeholder
published. The Mailbox `shareWith` / `mayShare` properties are typed in
`jmap-mail-types` and enforced by `jmap-mail-server` (bd JMAP-irij), not
here. It holds no logic: change the component crates, not
this one. It copies the shape of `jmap-quotas`, the workspace's first
umbrella, and replaces the 0.0.1 name-claim placeholder (bd JMAP-gs76.15,
DECISIONS.md decision 50).

This project uses **bd** (beads) for issue tracking. Run `bd prime` for
full workflow context.

## Build & Test

```bash
cargo test -p jmap-mail-sharing
cargo test -p jmap-mail-sharing --features client
cargo test -p jmap-mail-sharing --all-features
```

## Restrictions

- Push freely — `git push`, no `pull --rebase` ritual. If a push is rejected for any reason, stop and ask the human; do NOT `pull --rebase`, merge, force-push or `git reset` (workspace AGENTS.md "Git Commit and Push Policy")
- No code beyond re-exports.
- Do not use TodoWrite or markdown task lists — use `bd create`
