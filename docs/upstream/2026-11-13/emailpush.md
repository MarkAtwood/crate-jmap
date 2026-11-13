<!--
DRAFT, not filed.
Target: GitHub issue on https://github.com/jmapio/jmap (editor's copy of draft-ietf-jmap-emailpush-05:
spec/emaildelivery/draft-ietf-jmap-emailpush.xml at f8f44fd, which is on jmapio/jmap master and is the latest
commit touching spec/emaildelivery). Searched existing issues read-only: none covers either point.
Section numbers follow the rendered -05 (§3 EmailPush object, §4 PushSubscription extension), as in #473.
crate-jmap decisions: 9, 10.
-->

**Title:** EmailPush: filter has no default; objectHistory can never be pushed

Two small things from implementing -05 in crate-jmap (https://github.com/MarkAtwood/crate-jmap).

**1. EmailPushConfig.filter has no stated default.**

The other two EmailPushConfig properties have one: `properties: String[] (default: [])` and `urgency: String (default: "normal")`. filter is just `filter: FilterOperator|FilterCondition|null`, and its description says "If null, all new messages delivered to the account are pushed." So it isn't said whether a client may omit it, or whether an omitted filter is an error. A server also needs a value to return from PushSubscription/get and to report in `created` (RFC 8620 §5.3: "any properties that were omitted by the client and thus set to a default by the server").

Suggest: `filter: FilterOperator|FilterCondition|null (default: null)`.

crate-jmap's implementation (in progress) treats an omitted filter as null, stores each EmailPushConfig with defaults filled in (filter null, properties [], urgency "normal"), returns that from /get, and reports it in created/updated.

**2. Suggest excluding objectHistory from properties, as bodyValues is.**

properties "is the same as the properties argument in "Email/get", ... except that the "bodyValues" property is not supported." With draft-ietf-jmap-object-history, Email/get also accepts `objectHistory` in properties, but object-history-03 §3.1 says "It is returned if and only if includeReplaced or includeDestroyed is true, regardless of whether it is listed in properties." A push has neither argument, so objectHistory can never appear in an EmailPush. If a client lists it, the Emails skip it but still carry the properties after it, which breaks §3: "The properties present in each Email object are therefore always a prefix of the requested list."

Suggest, in §4 properties: "except that the "bodyValues" property, and any property that "Email/get" returns only when given additional arguments (such as "objectHistory" [I-D.ietf-jmap-object-history]), are not supported." Then the existing invalidProperties rule ("contains a property the server does not support, including "bodyValues"") covers it. A general wording avoids a normative dependency on object-history; naming objectHistory as the example is enough.

crate-jmap's implementation (in progress) accepts objectHistory in properties and drops it from the stored list when normalizing, and the stored list is reported back in created/updated (as in 1), so the client sees it was dropped and pushes stay a prefix. If the draft says "not supported", we'd switch to rejecting it with invalidProperties.

Found implementing -05 in crate-jmap. Lines are spec/emaildelivery/draft-ietf-jmap-emailpush.xml at f8f44fd: L95, L125-126, L146-147, L171, L181.
