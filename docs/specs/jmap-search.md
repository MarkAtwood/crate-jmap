---
title: JMAP Search
abbrev: JMAP Search
docname: draft-atwood-jmap-search-00
category: std
stream: IETF

ipr: trust200902

stand_alone: yes
smart_quotes: no
pi: [toc, sortrefs, symrefs]

author:
  -
    fullname: Mark Atwood
    organization: Review Commit, LLC
    city: Seattle
    region: WA
    country: US
    email: mark@reviewcommit.com

normative:
  RFC2119:
  RFC8174:
  RFC8620:
  RFC6901:
  RFC4790:
  RFC5051:

informative:
  RFC8621:
  JMAP-GRAPH-SEARCH:
    title: "JMAP Graph Search"
    author:
      fullname: Mark Atwood
    seriesinfo:
      Internet-Draft: draft-atwood-jmap-graph-search-00
    date: 2026
  JMAP-CHAT:
    title: "JMAP Chat"
    author:
      fullname: Mark Atwood
    seriesinfo:
      Internet-Draft: draft-atwood-jmap-chat-00
    date: 2026
  JMAP-METADATA:
    title: "JMAP Metadata"
    seriesinfo:
      Internet-Draft: draft-ietf-jmap-metadata-05
    date: 2026

--- abstract

This document defines JMAP Search, an extension that gives the `/query` method of any JMAP data type one shared filter language. A server lists the data types it supports in a capability, and the FilterCondition of each listed type gains a `where` property holding a JSON expression of field tests, comparison operators, and logical operators. The language is the type-neutral part of the built-in filter language of JMAP Graph Search. No new methods or result formats are defined.

--- middle

# Introduction

Each JMAP data type defines its own FilterCondition ({{RFC8620}} Section 5.5), so a client can filter only on the properties its specification chose, and every new filter needs a new specification. This document adds one more FilterCondition property, `where`, whose value is a small expression language over the object's own JSON representation. A server opts in per data type, and a client learns from the capability which types accept it.

The language is that of {{JMAP-GRAPH-SEARCH}} Section "The Built-In Filter Language" without its structural operators (`$edge`, `$reachable`, `$from`, `$to`), which only make sense in a graph.

## Design Principles

- **No new methods.** `where` is a FilterCondition property. Paging, sorting, `/queryChanges`, and result references work as for any JMAP query ({{RFC8620}} Sections 5.5 and 5.6).
- **Only what `/get` shows.** Field tests address the candidate's `/get` representation, and only objects the user could fetch are candidates.
- **Additive.** `where` combines with a type's own FilterCondition properties, which keep their meaning.
- **Same results everywhere.** Strings compare by code point unless a collation is named; there is no regular-expression operator.
- **Bounded.** Nesting depth has an advertised limit, and servers may refuse any query they consider too expensive.

# Conventions and Definitions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "NOT RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in BCP 14 {{RFC2119}} {{RFC8174}} when, and only when, they appear in all capitals, as shown here.

Terminology from {{RFC8620}} is used throughout.

Candidate:
: An object being tested against a filter during `Foo/query`, where `Foo` is the queried data type.

# Capability {#capability}

Servers supporting this specification advertise the `urn:ietf:params:jmap:search` capability.

## Session-Level Capability Object

The value of `capabilities["urn:ietf:params:jmap:search"]` at the session level is an empty JSON object `{}`.

## Account-Level Capability Object

The value of `accountCapabilities["urn:ietf:params:jmap:search"]` is a JSON object with the following fields:

`dataTypes` (String\[DataTypeSearchInfo]):
: A map from the name of a data type, as it appears in the method names of that type (for example `Email` or `Message`), to the search support for that type on this account. Only the types listed here accept `where` ({{opt-in}}).

`maxDepth` (UnsignedInt):
: The largest nesting depth ({{depth}}) the server accepts in an expression.

A DataTypeSearchInfo object has the following fields:

`supportsText` (Boolean):
: `true` if the server supports the `$text` operator ({{text}}) for this type.

Clients ignore fields they do not recognize in either object.

# The where FilterCondition Property {#filter}

For each data type listed in `dataTypes`, this document adds the following property to the FilterCondition of `Foo/query` and `Foo/queryChanges`:

`where` (Expression):
: A filter expression ({{language}}). The candidate matches if the expression is true for it.

Like every FilterCondition property, `where` must match together with the other properties of the same FilterCondition, and a FilterCondition with `where` may appear anywhere the type's `/query` accepts a FilterCondition, including inside FilterOperator objects. A type's own restrictions on the shape of its filter still apply: for example, {{JMAP-CHAT}} requires a `Message/query` filter to be a single FilterCondition that includes `chatId` or `hasMention: true`, and `where` does not lift that requirement. A `where` of `null` is the same as no `where`.

An expression is well-formed if: it is a JSON object; every member name is a JSON Pointer ({{RFC6901}}) used as a field test or an operator defined in {{language}}; every operator object contains only `$`-prefixed names; and every operand has the type stated for it.

# Addressable Properties {#pointers}

A field test's name is a JSON Pointer that is resolved against the candidate's JSON representation. That representation is the object the user would receive from `Foo/get` with `properties` listing every property of the type that the server supports, with every other argument of `Foo/get` at its default. So:

- The first reference token is a property name of the type, including properties added by extensions in use on the account (for example `/metadata/...` of {{JMAP-METADATA}}). Later tokens address members of objects and elements of arrays, so `/keywords/$seen` and `/to/0/email` are valid pointers into an Email {{RFC8621}}.
- A property whose value depends on another `/get` argument takes the value it has with that argument at its default; for example, `bodyValues` of an Email is empty because `fetchTextBodyValues` defaults to `false`.
- Properties that are only requested by a parameterised name, such as the `header:{name}` properties of an Email, are not part of the representation.
- A property the user would not receive, because of access control or because the server omits it for this user, is not part of the representation.

A pointer that does not resolve to a value leaves the field **missing**; this is not an error. A server MAY refuse a field test whose pointer addresses a property it does not support searching (for example one it does not index) with an `unsupportedFilter` error ({{errors}}); a server SHOULD support at least every property of the type that is a String, Number, Boolean, or `Id`, or an array or map of those.

# The Filter Language {#language}

## Expressions

An Expression is a JSON object. The expression is true if every one of its members is true. An empty object is always true. Each member is one of the following:

- A **field test**, whose name is a JSON Pointer ({{pointers}}).
- A **logical operator**: `$and`, `$or`, or `$not` ({{logical}}).
- The **text operator** `$text` ({{text}}).

A member name that begins with `$` and is not one of these is rejected as described in {{errors}}.

## Field Tests

A field test's value is either an operator object or a plain value:

- An **operator object** is a JSON object at least one of whose member names begins with `$`. An object that mixes `$`-prefixed and other member names is not well-formed. The test is true if every operator in it is true.
- Any other value is shorthand for `{"$eq": value}`.

## Comparison Operators {#comparison}

The following operators may appear in an operator object. "The field" means the value the pointer resolves to.

`$eq` (any value):
: The field is present and equal to the operand.

`$ne` (any value):
: Not `$eq`. True when the field is missing.

`$gt`, `$gte`, `$lt`, `$lte` (Number or String):
: The field and the operand are both numbers and compare numerically as stated, or are both strings and compare as stated under the test's collation ({{collation}}). Otherwise false. A `UTCDate` or `Date` property ({{RFC8620}} Section 1.4) is a string, so its values order correctly only when they have the same form.

`$in` (Array):
: The field is `$eq` to at least one element of the operand.

`$nin` (Array):
: Not `$in`. True when the field is missing.

`$exists` (Boolean):
: If `true`, the field is present (its value may be `null`); if `false`, it is missing.

`$prefix` (String):
: The field is a string that begins with the operand under the test's collation.

`$contains` (String):
: The field is a string that contains the operand as a substring under the test's collation.

`$collation` (String):
: Not a test. Names the collation algorithm used by this operator object's string operators. Its value is one of the `collationAlgorithms` in the core capability ({{RFC8620}} Section 2).

Two JSON values are equal when they have the same JSON type and: both are `null`; both are the same Boolean; both are numbers with the same numeric value; both are strings that are equal under the test's collation; both are arrays of the same length whose elements are pairwise equal; or both are objects with the same member names whose values are pairwise equal. Values of different types are never equal and never ordered. In particular, `{"$eq": null}` is true only for an explicit `null`; a missing field is tested with `$exists`.

## Arrays {#arrays}

When the field is an array and the operand of `$eq`, `$gt`, `$gte`, `$lt`, `$lte`, `$prefix`, or `$contains` is not an array, the operator is true if it is true for at least one element of the field. `$in` is true if `$eq`, with this rule applied, is true for at least one element of the operand. `$ne` and `$nin` are the negations of `$eq` and `$in` with this rule applied, so `{"$ne": "x"}` on an array field is true only if no element equals `"x"`. Elements are not searched recursively, so a test on a member of the objects in an array (such as the `email` of every address in an Email's `to`) needs one pointer per index.

## Logical Operators {#logical}

`$and` (Expression\[]):
: True if every expression in the array is true. An empty array is true.

`$or` (Expression\[]):
: True if at least one expression in the array is true. An empty array is false.

`$not` (Expression):
: True if the expression is false.

## Collation {#collation}

When `$collation` is absent, strings are compared as sequences of Unicode code points: two strings are equal only if they are identical, they are ordered by code point, and `$prefix` and `$contains` match code-point sequences exactly. This keeps results identical across servers and is correct for ids, which are case-sensitive. When `$collation` is present, every string operator in the same operator object, including `$eq`, `$ne`, `$in`, and `$nin`, uses it. A plain value (the `$eq` shorthand) always uses code-point comparison. Servers SHOULD support `i;ascii-casemap` and `i;octet` ({{RFC4790}}) and `i;unicode-casemap` ({{RFC5051}}).

## Full-Text Matching {#text}

`$text` (String):
: True if the candidate matches the given text. What constitutes a match (tokenization, stemming, language handling, which properties are considered) is server-defined. For a type that has its own full-text FilterCondition property, such as the `text` property of `Email/query` ({{RFC8621}} Section 4.4.1), `$text` SHOULD match as that property does.

A server sets `supportsText: true` for a type only if it supports this operator for that type.

## Nesting Depth {#depth}

The nesting depth of an expression is 0 if it has no logical operator, and otherwise 1 more than the largest nesting depth of the expressions that are operands of its `$and`, `$or`, and `$not` members. So `{}` and `{"/size": {"$gt": 10}}` have depth 0, and `{"$not": {"$or": [{"/a": 1}]}}` has depth 2. A server MUST reject an expression whose nesting depth exceeds the advertised `maxDepth` with a `queryTooComplex` error ({{errors}}), so that clients can rely on the advertised limit. The nesting of FilterOperator objects around the FilterCondition is not counted.

# Opting In a Data Type {#opt-in}

A data type accepts `where` on an account exactly when the server lists it in that account's `dataTypes`, and only in requests whose `using` includes `urn:ietf:params:jmap:search`. Any data type with a `/query` method may be listed; its specification needs no change. A specification that defines a new data type MAY state properties that are not addressable ({{pointers}}) or a default `$text` behavior.

GraphNode and GraphEdge are searched with the `where` of {{JMAP-GRAPH-SEARCH}}, which is this language plus structural operators. A server MUST NOT list them in `dataTypes`, so that `where` has one meaning per type.

A type that has no `/query` method gains none from this document.

# Visibility {#visibility}

The candidates of a query are exactly the objects of the queried type that the user could fetch with `/get`, and each is tested against the representation that user would receive ({{pointers}}). An object or property the user cannot see MUST have no effect on any result.

# Query Changes {#query-changes}

Servers MAY return `cannotCalculateChanges` ({{RFC8620}} Section 5.6) for any `/queryChanges` call whose filter uses `where`, and then SHOULD return `canCalculateChanges: false` from the `/query` with that filter. Because an expression depends only on the candidate's own representation, a server that tracks changes to the type's properties can also calculate them as for any other filter.

# Errors {#errors}

A server rejects a `Foo/query` or `Foo/queryChanges` call with:

- `unsupportedFilter` ({{RFC8620}} Section 5.5) if a FilterCondition includes `where` and either the request's `using` does not include `urn:ietf:params:jmap:search` or the type is not in the account's `dataTypes`; if `where` uses a `$`-prefixed name that is not an operator defined in this document (so that a future extension is reported as unsupported rather than malformed, and the structural operators of {{JMAP-GRAPH-SEARCH}} are unsupported here); if the expression uses `$text` and the type's `supportsText` is `false`; if an operator object's `$collation` is not in `collationAlgorithms`; or if the server refuses a pointer as described in {{pointers}}.
- `invalidArguments` ({{RFC8620}} Section 3.6.2) if `where` is not well-formed (other than by using an unknown operator).
- `queryTooComplex` if the expression's nesting depth exceeds `maxDepth`, or the query exceeds another server limit, such as a limit on evaluation time or work.

This document uses one method-level error, which {{JMAP-GRAPH-SEARCH}} also defines:

`queryTooComplex`:
: The query is valid, but the server will not evaluate it because it exceeds a server limit, such as `maxDepth` or a limit on evaluation time or work. The server MAY include a `description` property explaining the limit.

# Examples

## Large flagged Emails

Emails over 1 MB that are flagged and whose subject starts with "invoice", ignoring case, inside a mailbox:

~~~json
["Email/query", {
  "accountId": "A1",
  "filter": {
    "inMailbox": "M1",
    "where": {
      "/keywords/$flagged": true,
      "/size": { "$gt": 1000000 },
      "/subject": {
        "$prefix": "invoice",
        "$collation": "i;unicode-casemap"
      }
    }
  }
}, "0"]
~~~

## Chat Messages with a body type

Messages in a Chat whose body is Markdown, or that have at least one reaction (`reactions` is always present, so the test is that it is not the empty map):

~~~json
["Message/query", {
  "accountId": "A1",
  "filter": {
    "chatId": "C1",
    "where": {
      "$or": [
        { "/bodyType": "text/markdown" },
        { "/reactions": { "$ne": {} } }
      ]
    }
  }
}, "0"]
~~~

# Relationship to Other Documents

- {{JMAP-GRAPH-SEARCH}}: its built-in language is this language plus structural operators and its own `maxDepth` (a traversal depth). Its `where` is defined on GraphNode/query and GraphEdge/query under `urn:ietf:params:jmap:graph:search`.
- {{JMAP-METADATA}}: on a type with metadata, `/metadata/{namespace}/...` field tests filter on metadata values. The six metadata FilterCondition properties keep their meaning; `metadataExists` corresponds to `$exists: true`, and `metadataTextEquals` to a plain string value.

# Security Considerations {#security}

## Resource Consumption

`$contains`, `$text`, and field tests on unindexed properties may require scanning every candidate. The advertised `maxDepth` bounds nesting. Servers SHOULD also bound evaluation time and work, rejecting queries that exceed their limits with `queryTooComplex`, and MAY apply the rate limits they apply to other methods. The language has no regular-expression operator, because regular-expression dialects differ between implementations and some patterns take time exponential in the input.

## Information Disclosure

Search reveals information through which objects match, not only through what the results contain. The rule of {{visibility}} keeps hidden objects and properties from influencing results; a server that applies per-user access control to properties MUST apply it to field tests too.

# IANA Considerations {#iana}

## JMAP Capability Registration

IANA is requested to register the following entry in the "JMAP Capabilities" registry:

Capability Name:
: `urn:ietf:params:jmap:search`

Intended Use:
: common

Change Controller:
: IETF

Specification document:
: This document.

Security and privacy considerations:
: See {{security}} of this document.

## JMAP Error Code Registration

If {{JMAP-GRAPH-SEARCH}} has not registered it, IANA is requested to register `queryTooComplex` in the "JMAP Error Codes" registry ({{RFC8620}} Section 9.5), with Intended Use "common", Change Controller "IETF", Reference "This document, {{errors}}", and Description "The query is valid, but the server will not evaluate it because it exceeds a server limit."

--- back

# Design Notes

## Why a Pointer and Not a Property Name

JSON Pointer reaches into maps and arrays (keywords, mailboxIds, reactions, metadata namespaces) without a new syntax, and it is what {{JMAP-GRAPH-SEARCH}} uses, so one evaluator serves both documents.

## Future Work

- A standard `Foo/query` and `Foo/queryChanges`, whose filter supports only `where`, for listed types that have no `/query` today.
- Advertising, per type, which pointers a server indexes.
