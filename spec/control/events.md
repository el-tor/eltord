# Events, replies and errors

## Payment identifier event

**Source-observed.** Subscribe using:

```text
SETEVENTS PAYMENT_ID_HASH_RECEIVED
```

The fork emits:

```text
650 EVENT_PAYMENT_ID_HASH_RECEIVED P_CIRC_ID=PreviousID N_CIRC_ID=NextID PAYMENT_HASH=Payload
```

The subscription name and emitted event token differ. Both must be documented
exactly. Sources: `tor/src/feature/control/control_events.c` and
`eltord/src/relay/payments_watcher.rs`. The older narrative's
`EXTEND_PAID_CIRCUIT` event name is not the observed wire token.

These IDs identify local relay-side circuit associations, not a shared global
circuit identity. The payload reports received payment information; it is not
proof of Lightning settlement, recipient ownership, client authorization or
single-use redemption. Never treat event arrival time as settlement time.

## Proposed lifecycle events

The [lifecycle chapter](lifecycle.md) defines the new ELTOR_PAYMENT_STATE
event, inventory resynchronization and local initial-state queries. It is distinct
from the source-observed event above and does not imply existing runtime support.

## Reply handling

Inherit Tor's numeric reply and asynchronous event framing. Parse complete replies
and correlate asynchronous state separately. `250 EXTENDED` acknowledges a command
success path, not readiness; `250 OK` teardown is not financial reconciliation.
A lost control connection leaves an operation uncertain until state is queried.
Do not replay a paid construction blindly after a timeout.

## Privacy and conformance

The control port is a privileged local interface. Payment payloads, preimages,
authorization and circuit mappings must not become public routing metadata or
routine logs in the revised implementation. Existing source contains verbose
payment logging; this chapter does not endorse it. Track redaction, strict parsing,
event ordering, restart reconciliation and correlation-safe diagnostics in the
controller implementation task. Documentation review sent no commands to a live
control port and triggered no payment or teardown.
