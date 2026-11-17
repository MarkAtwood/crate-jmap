# jmap-object-history-server — Implementation Plan

Design: `docs/design/object-history.md` (§3 crate placement, §7 memory reference backend).
Spec: `~/PROJECT/IETF/specs/draft-ietf-jmap-object-history-03.txt` plus the -04 editor's copy changes
listed in the design note §2a.

| Step | Bead | State |
|---|---|---|
| Scaffold: manifest, empty `lib.rs`, README | JMAP-tna1.3.1 | done |
| `select_versions`, `VersionLog`, retention, rights cut, `fold_restore` | JMAP-tna1.3.2 | done |
