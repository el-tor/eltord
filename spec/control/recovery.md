# Durable payment authorization and recovery

**Proposed version-one contract.** This chapter governs daemon/relay accounting,
not wallet implementation. Node settlement, client authorization, reservation
and Tor activation are separate facts. Runtime replay fixes remain deferred.

## Authoritative records

Use a transactional durable ledger shared by all verifier workers for one
receiving Lightning account. Partition records by account identity, not by a
caller-supplied wallet label. A unique constraint on `(account, payment_hash)`
prevents reusing one settled invoice for multiple charges. A second unique key
on `(circuit_binding, round, charge_kind)` prevents duplicate round authorization.
`charge_kind` is HANDSHAKE or ROUND. BOLT 12 offer IDs and payer notes are not
settlement keys: resolve them to the recipient node's actual invoice/payment hash.

Before dispatching any outgoing payment, persist its quote/policy/round binding,
exact invoice digest, invoice payment hash, amount, fee limit, wallet account and
stable wallet operation ID. A retry after uncertain dispatch first queries the
wallet by operation/payment hash. If the adapter cannot determine the outcome,
retain UNKNOWN and its full reserved exposure; do not issue a replacement payment.
No callback, exception, transport timeout or cancellation proves failure.

Receiving records retain the expected amount, exact circuit/round/payment-ID
registration, invoice reference, authoritative settlement time and recipient
account. Verify node ownership, settled status, amount and applicable acceptance
window. For BOLT 12, verify that the node's invoice/payment record carries the
registered identifier under the adapter's authenticated mapping; an unauthenticated
webhook or client-supplied note is insufficient. For BOLT 11, use the expected
invoice hash. An adapter that cannot prove the association MUST refuse support.
A preimage can be checked against a hash, but is not reusable client authorization.

## State and transaction boundaries

| State | Meaning and allowed next transition |
| --- | --- |
| REGISTERED | Exact authorized circuit/round obligation exists, no payment claim yet |
| IN_FLIGHT | Outgoing intent durably recorded; wallet outcome still pending |
| UNKNOWN | Outcome ambiguous; retain full reservation and query, never duplicate |
| SETTLED_RESERVED | Node-verified funds atomically assigned to exactly one obligation |
| AUTHORIZED | Durable decision permits Tor to activate that obligation only |
| CONSUMED | Activation/use confirmed; cannot move funds to another circuit |
| FAILED_FINAL | Wallet confirms terminal failure and no settlement; release only that intent's reservation |
| CLOSED_UNRESOLVED | Circuit closed while financial outcome remains uncertain |
| RECONCILED | Final accounting recorded; refund, if policy permits, is a separate outgoing intent |

Incoming settlement and outgoing payment use separate records linked to the same
obligation; a forwarded payment is not counted as a second client charge.
Atomically claim the unique settlement, reserve its amount and create the
activation decision before notifying Tor. Persist outgoing intent and reserve
fees before wallet dispatch. Retries read these rows rather than insert another
claim. Workers use serializable transactions or equivalent constraints; application
checks outside the transaction are insufficient. Ledger unavailability stops new
paid work, not just logging. Reads and status events cannot create authorizations.

## Circuit registration and verifier RPC

Tor assigns local instance/handle/revision identifiers to relay-side circuits as
well as origin circuits. Their scopes are local; a relay handle must never be
confused with the origin's handle or transmitted to another hop. The authorized
verifier gets the registration through the [lifecycle event/query contract](lifecycle.md), then queries
its own Lightning node asynchronously. Circuit registration contains accepted
policy Q, A0, fee amount/hash, ten IDs in order, setup/funding deadlines and the
per-hop circuit binding retained inside Tor. Do not expose exporter/MAC keys or
handshake preimages in RPC replies or logs.

```text
ELTOR_PAYMENT_AUTHORIZE 1 instance handle decision round amount_msat payment_hash record_digest
```

All field encodings follow the renewal controller chapter: 16-byte decision ID,
u64 round and amount, 32-byte payment hash and ledger record digest. Round zero
means the one-time handshake obligation; positive rounds mean recurring capacity.
The verifier is trusted to perform node/ledger checks; Tor does not pretend this
local command itself contacts a Lightning node or cryptographically proves funds.
The record digest is SHA256 over the canonical durable authorization record:
`u8(1)||instance16||handle16||decision16||round_u64||amount_u64||payment_hash32||Q32||A0_32`.
No native structs or JSON serialization are hashed. For an explicitly zero-priced
obligation use a zero payment_hash and a durable zero-charge authorization,
never a fake node payment. Positive amounts require nonzero hashes.

Tor verifies the authenticated verifier role, live instance/relay handle, exact
registration, expected amount, deadlines and absence of another decision for the
same obligation. The local verifier role is configured explicitly for the one
privileged daemon authority; general client applications must not get control
credentials that can authorize incoming funds. Return
`250 ELTOR_AUTHORIZED 1 instance handle decision revision` only after storing
local authorization. This may trigger a previously recorded START or boundary
activation, but never resets T0. Exact replay returns the same decision/state;
changed arguments or another decision for the obligation return
`553 OPERATION_CONFLICT`. The command cannot reopen closure, skip registration,
change rates or activate an unrequested initial START. Other strict errors follow
the renewal controller profile. A lost reply is resolved through ELTOR_PAYMENT_REGISTRATION_GET in the lifecycle chapter,
which is the registration query, which reports recorded decisions and activation state.

## Restart and uncertain activation

The daemon persists intent, bindings, node evidence and authorization before
RPC dispatch. On reconnect to the same Tor instance, query the exact handle and
decision; never authorize again under a new identity merely because a reply was
lost. The same decision can be resubmitted unchanged while the obligation is
live and within its deadline. Tor process loss invalidates all handles and
closes circuits. Its new instance MUST reject old decisions as STALE_INSTANCE.
Reconcile funds in the ledger; do not recreate a path or restore an old timer.

Tor activation is an in-memory live-circuit transition with an authenticated
status/event. Daemon confirmation may lag it. A crash between decision and event
is UNKNOWN activation, not a refundable failure. Subsequent node evidence cannot
resurrect the dead circuit. Conservatively preserve incurred/uncertain amounts
until reconciliation under the accepted policy. Lost durable ledger storage is
not a safe fresh install: restore its records or disable paid admission for the
account; never reuse that account in empty-ledger mode and claim replay safety.

## Retention and time windows

For a new initial handshake claim, require recipient-node settlement no earlier
than five minutes before the circuit registration and before its setup deadline.
Round settlement must fall within the registered early-payment window through
its funding/grace deadline as defined by the initial wire round-boundary rules.
Round one uses its original COMMIT-to-setup-deadline registration window; later
rounds close at nominal start plus grace, not at the end of the purchased minute. Bind registration to a monotonic interval plus an
anchored UTC observation; if clock uncertainty crosses an acceptance boundary,
stop and reconcile rather than widen the window. The five-minute handshake
allowance is a test-profile limit, not assurance of Lightning latency.

Keep unresolved intents, claims, authorizations and potential refunds until
resolved; they are never evicted to admit new circuits. Refuse new work when
storage budgets cannot preserve them. Retain resolved unique payment-hash
consumption tombstones at least 30 days after the later of settlement or closure.
Before deleting a tombstone, ensure no live registration, retained unresolved
operation, refundable policy or admissible payment window can reference it.
Old node settlements outside a fresh registration's window are always rejected,
even after tombstone deletion. Bound raw diagnostic retention separately and
retain only financial/replay fields needed for correctness; do not retain
preimages or traffic keys in these records.

## Privacy and validation

This ledger contains sensitive associations even though messages are encrypted.
Keep it local, restrict access, avoid routine log emission of hashes/handles and
do not share account/circuit identifiers between relay hops. Aggregate caps must
stop new work instead of discarding anti-replay state. Test concurrent duplicate
claims, one payment across two circuits, old settled payments with fresh deadlines,
callback lag, unknown wallet outcomes, clock rollback, crash at every durable/
wallet/RPC boundary, Tor versus daemon restart, zero charge and refund retries.
These are required implementation tests under ET-01/16/29/33; none are claimed
executed by writing this contract.
