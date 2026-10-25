# Authenticated unspent-funds reconciliation and refunds

**Proposed version-one contract; not implemented.** A refund is a separate outgoing
payment under the accepted funding policy. It is never an automatic consequence
of timeout, circuit closure or a client's claim that service failed. Policy 0
promises no refund. Policy 1 returns reconciled unspent principal, with client-
authorized fees, using the bounded mechanism below. Neither policy returns money
already incurred or creates a reusable balance for another circuit.

## Eligibility and method

Finish terminal connection accounting before making funds refundable. Unresolved
onward payments stay reserved until their outcomes are known. The refundable
amount is the verified unallocated portion of the original agreement's received
principal; do not count incurred charges, prior refunds or in-flight refunds again.
A dishonest service can still refuse to pay: protocol authentication is not escrow
or an enforceable delivery/refund guarantee.

Version-one policy 1 explicitly requires the client wallet to receive a BOLT11
invoice refund, even if the original service payment used BOLT12. This requirement
is part of the funding-policy semantics and must be shown/checked before acceptance;
it is not a silent method fallback. Receiver privacy remains the receiver's
choice, including supported blinded-invoice mechanisms. Do not require public
Lightning node identity outside what the selected invoice/wallet exposes. A client
without supported receive capability must reject policy 1 before paying. Future
refund methods require a separately defined policy/profile, not arbitrary URLs.

## Requests and responses

Use the existing encrypted service-control envelope, with original quote/client/
circuit/issuer/acquisition and cap bindings, round=0. Allocate messages
10 REFUND_REQUEST and 11 REFUND_QUERY; common response status 11 REFUND_STATE.
These are endpoint record values, not new relay commands.

REFUND_REQUEST payload is:

```text
attempt_id32 || refund_id32
u64 principal_msat || u64 max_routing_fee_msat || u64 refund_expires_at
u16 invoice_length || invoice_ascii[invoice_length]
```

Invoice is valid canonical lowercase BOLT11, length 1–2048, with exact amount
principal_msat > 0 and native expiry no earlier than refund_expires_at. Validate
native signature, chain, amount and required features before dispatch. The client
signs the complete request under the accepted client authorization key. This
explicitly authorizes that invoice as the refund destination; the service must not
replace its receiver using an unauthenticated address, callback or wallet hint.
refund_id is fresh random 32 bytes, retained unchanged across retries. Expiry must
fit the common operation expiry and be at most 150 seconds from first admission.
REFUND_QUERY payload is `attempt_id32 || refund_id32` and has no financial effects.

REFUND_STATE payload is exactly:

```text
attempt_id32 || refund_id32 || u8 state || u16 reason
payment_hash32 || u64 principal_msat || u64 fee_cap_msat || u64 actual_fee_msat
```

Length is 123 bytes. States: 1 RESERVED, 2 IN_FLIGHT, 3 UNKNOWN, 4 SETTLED,
5 FAILED_FINAL, 6 REFUSED. Reasons: 0 none, 1 policy ineligible, 2 insufficient
reconciled funds, 3 conflict, 4 expired, 5 wallet failure, 6 uncertain, 7 capacity.
REFUSED requires nonzero reason; UNKNOWN uses 6; FAILED_FINAL uses 4 or 5; others
use 0. Hash is zero only when no invoice was accepted. actual_fee is zero before
settlement, not proof that a pending payment has no fees. The common durable
agreement revision prevents snapshot rollback. Unknown records return REFUSED,
not a fabricated failed payment that releases a client reservation.

## Atomic dispatch and replay rules

Require `principal_msat + max_routing_fee_msat <= unallocated_msat` with checked
arithmetic. Atomically reserve this full amount, store the immutable invoice/hash,
client authorization, refund ID and wallet operation ID, then dispatch exactly
that intent. Permit only one unresolved refund per agreement. Same refund ID and
identical logical bytes recover status; changed bytes conflict. A new refund ID
cannot bypass an unresolved older intent. Replayed client authorization cannot
redirect or enlarge a refund. Reject policy 0 requests without a wallet call.

On settlement move principal to refunded, actual routing fee to incurred, and
unused reserved fee back to unallocated. On authoritative final failure with no
settlement, release the reservation back to unallocated. On timeout/exception or
process crash retain UNKNOWN and the full refund_in_flight reservation; query the
wallet by the retained payment hash/operation before any retry. Cancellation or
invoice expiry alone does not prove the outgoing payment failed. Never produce a
second refund merely because the response was lost. The accounting conservation
equation from the status profile must hold at each transaction boundary.

After a final failed refund, the client may explicitly authorize another fresh
invoice/refund ID for the still-reconciled balance. Retain all old consumption and
wallet records. A zero remaining balance cannot generate a zero-value invoice or
another payout. No refund restarts setup, accepts a continuation, reopens a closed
circuit or authorizes applying the balance elsewhere. Issuer/quote key compromise
follows existing suspension rules; client-key loss is not permission for the
service to guess a replacement payout identity.

The durable ledger retains financial status after transport slots and Tor circuits
close. Refund transport uses new eligible introduction/control paths, with the
original client agreement bindings and current authenticated service reply keys.
Resource, retry and storage ceilings still apply. At most one refund query per
five seconds per agreement and six per delivery group; exhausting transport
retries leaves uncertainty intact. Human recovery can later query the same record
under a new bounded group, never reset the wallet operation or ledger identity.

## Privacy and validation

Refund invoices and amounts remain inside the common padded encrypted envelope.
The service necessarily sees the destination invoice and its wallet observes the
payment; optional receiver privacy is not anonymity from all counterparties.
New control paths must preserve guard/isolation rules and use fresh outer encryption.
Refund timing, retries and wallet traffic remain correlatable. Never log refund
invoices, preimages or client/quote mappings as routine diagnostics.

ET-16/18/20/21/27 require explicit policy/receive-method consent, invalid destination
and amount cases, duplicate/concurrent refund requests, invoice substitution,
unknown wallet outcome, expiry during dispatch, ledger restart, fee remainder
accounting and policy-0 refusal. All monetary tests remain deferred to mock/regtest
or explicitly authorized payment environments; none were run for this proposal.
