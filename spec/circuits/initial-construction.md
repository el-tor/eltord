# Initial construction and payment accounting

**Status: design requirements and implementation gates.** The existing control
interface is documented separately in [Control and RPC](../control/index.md).
Its presence in source does not establish conformance to the proposed profile.

## Construction contract

Each hop receives its own handshake hash/preimage and ten independent 32-byte
round identifiers in the encrypted paid construction. Preserve equal-size fields
and fresh dummy values for eligible zero-fee cases. Do not concatenate one common
identity across all hops or expose a service-purpose marker to intermediates.

Accepted relay prices, capabilities, initial round start, early acceptance window,
grace, resource limits and renewal policy must be authenticated and retained
before relying on them. The exact initial commitment and capability encoding are
still gates in ET-02/06/08; the renewal reply cannot retroactively establish them.

## Settlement and ledger contract

Use receiving-node settlement records for recipient, amount and invoice state.
A hash match or preimage alone is not client authorization or proof of single use.
Atomically bind and consume authorization for one circuit/round; reconcile
uncertain or late wallet outcomes without allocating the same funds again.
Callback arrival time cannot replace actual settlement time or an agreed round
clock. A paid round ends at its deadline, not at the instant its payment arrives.

Closing a circuit does not justify deleting unresolved accounting records.
Require finite retention and durable replay protection, with exact storage and
recovery rules in ET-01/16. The separate deferred replay-protection investigation
remains deferred; this rewrite does not fix runtime code.

## Unresolved initial-profile details

The original [client/relay sketch](../history/paid-circuit-flow.md) contains
configuration and watcher examples, but is historical. Finalize initial paid
handshake encoding, authenticated term acceptance, activation events, wallet
method mappings and controller errors before implementing the revised profile.
Renewal and service quote encodings must not be used as substitutes for these
missing contracts. Track them in the [implementation gates](../validation/gates.md).
