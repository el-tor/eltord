# Daemon helpers and proposed interfaces

The `eltord/src/rpc` module wraps Tor's control port. It is not evidence of a
separate network JSON-RPC API. Its generic client authenticates, sends the supplied
command and ordinarily quits; subscriptions use a persistent control connection.

| Helper / operation | Control operation | Classification |
| --- | --- | --- |
| `extend_paid_circuit` | Multiline `EXTENDPAIDCIRCUIT` | El Tor extension |
| `teardown_circuit` | `TEARDOWNCIRCUIT` | El Tor extension |
| Current consensus | `GETINFO ns/all` | Inherited Tor command |
| Relay descriptors | `GETINFO desc/all-recent` | Inherited command; El Tor descriptor fields are separate extensions |
| Wait for circuit | `GETINFO circuit-status` | Inherited Tor command |
| Bootstrap progress | `GETINFO status/bootstrap-phase` | Inherited Tor command |
| Configuration lookup | `GETCONF` | Inherited Tor command |
| Manual stream attachment | `SETCONF __LeaveStreamsUnattached=1`, `SETEVENTS STREAM`, `ATTACHSTREAM` | Existing helpers; backup selection does not migrate open streams |

Source inventory: `eltord/src/rpc/mod.rs` and its exported modules at the revision
identified in the chapter introduction. App bindings and manager methods are
separate APIs; document any future public transport explicitly.

## Contract required for new control operations

For renewal, initial activation, quote/round status and restart recovery, define:
command grammar, authenticated caller scope, required capability, canonical
arguments, stable idempotency binding, replies/errors, asynchronous events,
lookup after uncertain dispatch, clock semantics and bounded resource use.
Separate permission to register IDs from permission to spend or build. Querying
state must not cause new financial effects or reset expiry.

These definitions remain deferred in ET-08/29–32 and the control documentation
conformance task. No runtime controller implementation is added by this rewrite.
