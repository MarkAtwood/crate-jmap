# jmap-object-history-client — Implementation Plan

Design: `docs/design/object-history.md` (§3 crate placement, §8 client bindings).
Spec: `~/PROJECT/IETF/specs/draft-ietf-jmap-object-history-03.txt` plus the -04 editor's copy changes
listed in the design note §2a.

| Step | Bead | State |
|---|---|---|
| Scaffold: manifest, empty `lib.rs`, README | JMAP-tna1.3.1 | done |
| History get, destroyed query, restore over base-client `SetCall`, paging helpers | JMAP-tna1.3.12 | open |
