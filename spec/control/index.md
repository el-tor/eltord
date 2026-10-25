# Control and RPC extensions

**Status:** source-observed interfaces plus explicitly deferred proposals.
This chapter describes local control interfaces, not onion relay-cell commands.
It follows the separation of commands, replies and asynchronous events used in
the [Tor Control Protocol](https://spec.torproject.org/control-spec/index.html).
Inherit its authenticated connection and text framing rules.

## Interface layers

| Layer | Role | Status |
| --- | --- | --- |
| Tor control port | `EXTENDPAIDCIRCUIT`, `TEARDOWNCIRCUIT`, payment events | Present in inspected fork source |
| eltord Rust helpers | Build/send commands, parse replies, subscribe to events | Present in inspected source; not a separate JSON-RPC server |
| Proposed renewal controller interface | Register/query next per-hop allocation | Deferred; no command grammar assigned |
| SCQ, invoice, status and READY control | End-to-end paid service protocol | Proposed; not automatically local RPC commands |

Source review used `eltord` revision `555558c` and sibling `tor` revision
`2f05d04`. Source paths below are workspace-relative; source observation does not
establish parser safety, complete conformance or live support. The runtime may
embed another checkout, so a release must identify its actual build provenance.

See [commands](commands.md), [events and errors](events.md), and
[daemon helpers and future contracts](daemon.md). Never infer that a successful
command reply means payment settled or a circuit finished building.
