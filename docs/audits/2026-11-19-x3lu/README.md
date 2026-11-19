# MUST audit of the chat-spec guides and compliance tests (bd JMAP-x3lu)

2026-11-19, jmap-chat-spec 120db2c. Every MUST line in `jmap-chat-compliance-tests.md` and the `*-guide.md` files that have one, mapped to code (COVERED, with file:line), a bead, a GAP (new bead), or OUT OF SCOPE with a reason. Guides with no MUST line are not listed.

| Table | Docs | MUST lines | Covered | Gap | Out of scope |
|---|---|---|---|---|---|
| [compliance.md](compliance.md), [leads.md](leads.md) | jmap-chat-compliance-tests.md | 81 | 69 (+1 split) | 3 (4 rows) | 7 |
| [push.md](push.md) | push-platform, chat-push-platform, vtc-ring-push guides | 78 | 18 | 36: deploying server, optional crate JMAP-xxnf (decision 40) | 24 |
| [chat.md](chat.md) | chat wss, implementer, did, filenode, calendars, federation, tasks, metadata; did; cid guides | 73 | 13 | 3 | 57 |
| [scene-vtc.md](scene-vtc.md) | scene wss, implementer, 2d, simulation, scene-vtc; vtc recording, implementer, e2ee, gateway, chat-integration, chat-delegation guides | 59 | 30 | 0 | 29 |

New beads: JMAP-uf12 (SpaceBan sweep), JMAP-egy7 (ChatContact.serverUrl), JMAP-8ar3 (groupUpdate roles), JMAP-c70f (ChatStreamEnable capability check), JMAP-rigq (testjig pushState resume), JMAP-b9qs (CustomEmoji overQuota), JMAP-ssda and JMAP-zr5h (tests for covered but unasserted MUSTs), JMAP-ugi1 (errors in the compliance doc, for Mark).

The push table's 36 GAP rows are FCM, APNs, WNS, ADM, HPK and MiPush delivery, which no crate implements (only Web Push, bead JMAP-gxxm). Decision 40: out of scope for the core kit (owner: the deploying server), with an optional support crate tracked as epic JMAP-xxnf.
