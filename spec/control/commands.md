# Commands

## EXTENDPAIDCIRCUIT

**Source-observed.** Registered as a multiline control command in
`tor/src/feature/control/control_cmd.c`; implemented in
`tor/src/feature/control/control_extendpaidcircuit.c`.

```text
+EXTENDPAIDCIRCUIT CircuitID
RelayFingerprint PaymentPayloadHex
RelayFingerprint PaymentPayloadHex
RelayFingerprint PaymentPayloadHex
.
```

Use normal control-protocol line framing. `CircuitID=0` requests a new origin
circuit; a nonzero ID selects an existing circuit for path extension. It does
**not** mean renewal of payment identifiers in place. Each hop row contains two
space-separated tokens. The client concatenates the handshake payment hash,
handshake preimage and ten round identifiers as hex in the second token; do not
insert spaces between those fields. With twelve 32-byte fields this is 768 hex
characters. Fingerprints identify the ordered relay path.

`eltord/src/client/circuit.rs` constructs this form;
`eltord/src/rpc/extend_paid_circuit.rs` sends it. The command queues circuit work
and responds `250 EXTENDED CircuitID` on its success path. Await circuit state
separately. There is no transactional guarantee that a timeout means nothing ran.

Observed errors include `512` for absent/invalid router rows or buffer limits,
`552` for invalid/unknown circuit, missing router/descriptor or oversized payment
data, and `551` for circuit creation/extension/handshake-start failure. The current
parser can skip malformed rows; this behavior is not approval of partial-path
acceptance in a future strict profile. Exact validation and recovery belong in
the controller conformance task before implementations rely on this interface.

## TEARDOWNCIRCUIT

**Source-observed.** Registered as a one-line command; handler is
`tor/src/feature/control/control_teardowncircuit.c`.

```text
TEARDOWNCIRCUIT CircuitID [Reason] [PeerIdentity=Identity] [LogOnly] [FORCE_IMMEDIATE]
```

The handler reads an optional reason (default `control_request`), optional peer
identity for relay-circuit lookup, and mode flags from its parsed keyword list.
`ALL` is a special circuit selector that requests teardown of all eligible
circuits. It is an administrative operation, not a normal retry or recovery step.
The exact accepted flag grammar must be verified against command parsing as part
of conformance; this source inventory is not a recommendation to use force mode.

Ordinary operation initiates DESTROY/close handling for an active circuit.
`LogOnly` records processing without immediate teardown; `FORCE_IMMEDIATE` takes
the handler's immediate-destruction path. Successful responses begin `250 OK`
with descriptive text that varies by branch. Unknown circuit returns `552`.
An inactive circuit can return `250 OK` without new teardown work. A successful
reply is not a refund, settlement result or proof every peer has already closed.

The daemon helper currently sends only `TEARDOWNCIRCUIT CircuitID` and checks
for `250 OK`. Payment reconciliation is a separate protocol responsibility.

## Proposed commands

`PAYMENT_RENEW` and `PAYMENT_RENEWED` are proposed encrypted relay commands, not
control-port commands. The local interface to submit/query their batches is still
ET-08. SCQ acceptance, invoice lookup and READY also need explicit interface
contracts; do not expose invented command names as implemented APIs.
