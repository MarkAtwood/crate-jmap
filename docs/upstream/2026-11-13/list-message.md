<!--
DRAFT, not sent. Optional message to the JMAP WG list, from Mark, after the issues are filed.
Replace each <link ...> placeholder with the filed issue URL or errata ID.
-->

**Subject:** Implementation findings: object-history, conditional, emailpush, blobext, RFC 9404

Hi all,

These all fell out of implementing the drafts in crate-jmap (https://github.com/MarkAtwood/crate-jmap). I've filed them as issues and errata; each says what crate-jmap does in the meantime, so there's a working reading to argue with.

object-history-03 <link object-history issue>:
- Do restores count toward maxObjectsInSet? RFC 8620 defines it over create, update and destroy.
- Foo/queryChanges for an onlyDestroyed query: §4 says it "works as for any other query", but queryChanges has no onlyDestroyed argument, so a destroyed query and a live one can't be told apart.
- A destroyed object whose final version was discarded while older versions remain: restore needs the final version, but notFound is only for objects with no history at all.

conditional-02 and object-history-03, together <link conditional issue> <link object-history issue>:
- Does atomic cover restores, and should atomicFailure carry notRestored? Conditional names only create, update and destroy.

emailpush-05 <link jmapio/jmap issue>:
- EmailPushConfig.filter has no stated default (properties and urgency have one).
- objectHistory can never be returned in a push, so listing it breaks the "always a prefix" promise; suggest excluding it as bodyValues is.

blobext-03 <link blobext issue>:
- Eight small gaps (examples vs text, ids null, cycles in Blob/set, unresolved #refs, null DSO keys, and so on), what a server does when a client sends both blob and blob2, and whether conditional's atomic applies to Blob/convert.

RFC 9404 errata <errata ID> <errata ID>:
- The §4.3.1 Blob/lookup example returns notFound, which §4.3 forbids because it leaks which blobs exist.
- The Blob/upload examples in §4.1.2 and §4.2.2 carry oldState, which §4.1 says isn't there.

Happy to talk through any of these at the JMAP session at IETF 127 (Thursday 2026-11-19, 09:00 PT).

Mark
