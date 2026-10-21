# jmap-graph-projection-server

The Mail and Contacts binding for `jmap-graph-server`: the rules of
draft-atwood-jmap-graph-projection-00 for presenting an account's RFC 8621
and RFC 9610 objects as one projected Graph under
`urn:ietf:params:jmap:graph:projection`. Each source object is a GraphNode
whose `objectUri` names it, and each id reference between objects is a
GraphEdge typed `<DataType>/<property>`.

## What it is

| Item | Spec |
|---|---|
| `Projectable` | §Which Objects Are Projected: Mailbox, Thread, Email, Identity, EmailSubmission, AddressBook, ContactCard with their registered type names (§JMAP Object URI Type Registrations); never Graph, GraphNode or GraphEdge |
| `Projectable::references`, `Reference` | §Edges: one reference per `Id`/non-null `Id\|null` property, per `Id[]` element, per `Id[T]` key; blob ids and nested ids give none; `edge_type` such as `Email/mailboxIds` |
| `object_uri`, `Reference::target_uri` | §objectUri: `jmap:<type>:<accountId>:<id>` via `jmap_types::JmapUri` |
| `projected_node`, `projected_edge` | §Nodes, §Edges: the fixed property values of derived nodes and edges |
| `node_object_uri`, `node_matches` | §objectUri, §GraphNode/query Filter: `objectUri` is one of the given values |
| `projected_graph_rights` | §Writes: `mayRead` only; the projection accepts no writes, so the graph-server handlers answer `forbidden` |
| `visible_ids` | §Visibility: a node is visible iff the caller can fetch its source object with native `/get` |

## What it's for

A deployment that serves a projected Graph implements `GraphBackend` over
its own store of projected nodes and edges and calls:

| Where | Call |
|---|---|
| A source object is created or changed | `references`, `object_uri`, `projected_node`, `projected_edge` |
| `get_objects::<Graph>` for the projected Graph | `projected_graph_rights` |
| `query_objects::<GraphNode>` | `node_matches` |
| Every GraphNode and GraphEdge method, per caller | `visible_ids` on the native backend |
| `GraphBackend::projected_graph_id` | the id advertised as `graphId` |

## How to use

```rust,ignore
// After the mail store commits an Email:
let uri = object_uri::<Email>(account_id, &email.id)?;
let node = projected_node(&new_node_id, &graph_id, &uri, &now);
for r in email.references() {
    if let Some(to) = nodes_by_uri.get(&r.target_uri(account_id)?.to_string()) {
        store_edge(projected_edge(&new_edge_id(), &graph_id, &node.id, to, &r, &now));
    }
}
```

## How it works

Edges come from a fixed table of the top-level properties RFC 8621 and
RFC 9610 define as ids of projected types (Mailbox `parentId`, Thread
`emailIds`, Email `threadId` and `mailboxIds`, EmailSubmission
`identityId`, `emailId` and `threadId`, ContactCard `addressBookIds`).
Visibility is the native backend's own `get_objects` with the caller's
context, so it follows current native rights.

## Gotchas

- Node and edge ids, and `created` times, are the deployment's: keep a
  node's id for as long as its source object exists (§Nodes SHOULD).
- A reference whose target has no node (an unprojected type) produces no
  edge; check `target_uri` against the projected nodes.
- An edge is visible only when both of its nodes are; apply `visible_ids`
  to both ends.
- AddressBook `shareWith` names Principals, which this crate does not
  project.

## References

- draft-atwood-jmap-graph-projection-00, draft-atwood-jmap-graph-00
- draft-atwood-jmap-object-uri-00
- RFC 8621 (JMAP Mail), RFC 9610 (JMAP Contacts)
