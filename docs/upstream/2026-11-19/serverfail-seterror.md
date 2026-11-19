<!--
DRAFT, not sent. Message to the JMAP WG list (jmap@ietf.org), from Mark.
crate-jmap decision 38; bd JMAP-76tz. Quotes checked against RFC 8620 and
draft-ietf-jmap-blobext-03 on 2026-11-19.
-->

**Subject:** A SetError for "this one object failed on the server side"?

Hi all,

Implementing Foo/set in crate-jmap (https://github.com/MarkAtwood/crate-jmap), we hit a gap in RFC 8620 §5.3 and would like to know what other implementations do, and whether it is worth fixing.

The gap: a non-atomic /set that applies some changes, then gets an unexpected storage failure on one object. No SetError type in §5.3 fits. Three ways out, none of them good:

1. Fail the whole method with "serverFail". But §3.6.2 says of serverFail that "The method call made no changes to the server's state", and the earlier creates, updates or destroys did change it.
2. Report a defined SetError such as "forbidden" or "invalidProperties". That tells the client that it is at fault and should not retry, which is false.
3. Use "serverFail" as a SetError type for that one object, and carry on with the rest. This is what crate-jmap does. blobext-03 seems to expect it too: its security considerations say servers "SHOULD reject requests that would exceed these limits with a "tooLarge" or "serverFail" error as appropriate", and the rejections it means are per-creation entries in notCreated, next to "tooLarge".

The registry has a single namespace: the "JMAP Error Codes" registry (§9.5) covers method errors and "method-specific error object (such as SetError in Section 5.3)". serverFail is registered there with reference §3.6.2 and the description "The method call made no changes to the server's state". Its use as a SetError type is undefined.

Proposal: allow "serverFail" as a SetError type (create; update; destroy). It would mean "an unexpected or unknown error occurred while processing this object; no change was made to it; retrying is expected to fail again", with the method-level rule that a description should give details. Two things go with it:

- A client that receives a SetError type it does not understand treats it as serverFail for that object. This mirrors the §3.6.2 rule for method errors; §5.3 has none for SetErrors.
- As for method-level serverFail, the description must not leak internal details, such as storage paths or other users' data.

Questions for the list:

1. What do Cyrus, Fastmail, Stalwart and others return today for an unexpected per-object failure in a non-atomic /set?
2. If this is worth doing, what is the right vehicle: an update to the registry entry, a short update draft, or text in a document already in progress (for example, the essential/portability guide)?

Mark
