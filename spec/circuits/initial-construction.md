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
before relying on them. The [initial wire profile](initial-profile.md) defines commitment and capability
encoding; the renewal reply cannot retroactively establish them. ET-02/06/08
still require implementation and independent cryptographic review.

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

## Initial-profile contracts

The original [client/relay sketch](../history/paid-circuit-flow.md) contains
configuration and watcher examples, but is historical. Use the [initial wire profile](initial-profile.md),
[lifecycle controller contract](../control/lifecycle.md) and
[durable recovery rules](../control/recovery.md) for term acceptance, activation,
queries, authorization and process loss. Wallet adapters still require explicit
conformance tests for payment-ID mapping. These proposals do not alter the
source-observed legacy command or claim its runtime already implements them. Track them in the [implementation gates](../validation/gates.md).

**Privacy/timing assessment:** new control messages use uniform encrypted
per-hop framing; lifecycle/recovery chapters specify privileged observations
and conservative clocks. Source comparison and document checks do not prove
cryptographic security or runtime settlement correctness.
