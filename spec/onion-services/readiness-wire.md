# Attempt status and rendezvous readiness, version 1

**Proposed wire/state contract; not implemented.** Use the common encrypted
[service-control records](payment-messages.md) for status and cancellation.
Use the actual completed rendezvous circuit for READY; an introduction reply or
status signature cannot prove that connection exists.

## Status and cancellation records

Allocate common control request message 4 STATUS_GET and 5 CANCEL_ATTEMPT.
All existing service/client/core/quote/circuit/issuer/acquisition bindings remain
mandatory. `round=0` selects whole-attempt state; the exact accepted caps are
retained. STATUS_GET payload is `attempt_id32`: zero requests the sole attempt
for this circuit authorization, while a nonzero value must match it exactly.
CANCEL_ATTEMPT payload is `attempt_id32 || u16 reason=1`. A zero attempt ID
cancels the exact accepted agreement identified by the common signed service,
client, quote-core and circuit-authorization bindings, including while initial
funding is pending. If that agreement already has an attempt, zero selects that
sole attempt; it is not a wildcard across agreements. A nonzero ID must match
the existing attempt. Persist the same logical mutation/operation across retries;
do not replace a zero selector with a newly learned ID on the same operation. The query never accepts a quote, creates an invoice, starts a build,
renews a round, changes a deadline or invokes a wallet payment.

For these two messages, common response status 8 STATE uses the payload below.
Pre-acceptance unknown agreement uses REFUSED reason 3 with the existing refusal
shape. Unknown nonzero attempt returns REFUSED reason 3 without altering a real
attempt. Other financial statuses from invoice messages are not substitutes for
STATE. Queries can return a newer revision than a previous cancellation reply;
financial reconciliation may progress after terminal circuit closure.

Fields in exact payload order:

```text
u8 state || u16 reason || attempt_id32
u32 current_round || u32 last_reserved_round
u8 time_fields
u64 setup_remaining_ms || u64 recovery_remaining_ms
u64 service_elapsed_upper_ms || u64 funded_remaining_ms
u64 received_msat || u64 incurred_msat || u64 refunded_msat
u64 refund_in_flight_msat || u64 outgoing_unresolved_msat
u64 reserved_unspent_msat || u64 unallocated_msat
```

This is 132 bytes. Common agreement_revision is the durable snapshot revision;
all fields are read atomically from that revision. State values:
1 ACCEPTED, 2 FUNDED, 3 BUILDING, 4 JOIN_SENT, 5 WAIT_READY_RECOVERY,
6 READY, 7 FAILED, 8 EXPIRED, 9 CANCELLED, 10 CLOSED_UNRESOLVED, 11 CLOSED.
The last five are terminal for connectivity, even if accounting later changes.
ACCEPTED has zero attempt ID until funds are reserved. FUNDED allocates the one
fresh random attempt ID and starts the original setup deadline atomically.
Repeated settlement cannot allocate another attempt. Cancellation before FUNDED
retains attempt_id=zero and current_round=0 in all later terminal snapshots;
it never allocates a build attempt just to reconcile funds. Such terminal records
have positive agreement revisions and absent setup/recovery/elapsed/funded time
bits, not invented clocks. A terminal agreement with no attempt remains queryable
with the zero selector. FAILED/EXPIRED/CANCELLED
may become CLOSED_UNRESOLVED or CLOSED after reconciliation without reconnecting.

Reason 0 none; 1 client cancellation; 2 setup timeout; 3 relay/path failure;
4 payment deadline; 5 process loss; 6 protocol/binding failure; 7 resource limit;
8 lifetime limit; 9 uncertain external outcome. Unknown values fail. Terminal
states require a nonzero reason; WAIT_READY_RECOVERY uses 2. Other states use 0.
Current round is zero before relay activation; last_reserved_round is the highest
consecutive service round with verified reserved funding, never inferred from an
invoice issued or from the next allocation being admitted.

`time_fields` bits 0–3 indicate known setup, recovery, elapsed and funded times in
that order; other bits zero. An absent field is zero and is unknown, not unlimited.
Remaining times are conservative monotonic budgets, clipped at zero; subtract
full query RTT and the local safety margin before acting. elapsed is an upper
bound from earliest authenticated service-side paid activation. Before activation
its bit is zero. Recovery time is absent outside recovery. No query starts a clock.
Only compare the same named deadline across observations; renewed funded rounds
can have later expiry, but setup/recovery anchors never move. Status must not
claim private-hop synchronization or independently verified service expenditure.

## Accounting conservation

All accounting values are nonnegative integer millisatoshis with checked sums.
They partition received settled client principal:

```text
received = incurred + refunded + refund_in_flight + outgoing_unresolved
         + reserved_unspent + unallocated
```

A unit of principal appears in exactly one category. Outgoing unresolved includes
its reserved maximum routing fee until authoritative outcome; move confirmed
spend to incurred and unused allowance to reserved_unspent/unallocated under the
accepted policy. Refund in flight similarly remains separate from refunded.
Client-side routing fees are outside this received-principal equation and come
from the client's own wallet ledger. Never subtract unknown outcomes as failed.
No negative residual, double-counted reservation or arithmetic overflow is valid.
The client treats the signed snapshot as the service's accounting claim, not proof
that the private path spent honestly or that its own outgoing wallet payment failed.

Cancellation is admitted only for an already accepted agreement. Unknown or
unaccepted agreements return REFUSED reason 3 without allocating durable state.
For an accepted agreement, serialize cancellation, settlement/reservation and
paid-dispatch authorization through the same durable agreement record:

- If cancellation commits first, record CANCELLED/reason 1 with no-new-spend.
  Later settlement updates accounting only: do not enter FUNDED, allocate an
  attempt, start a setup timer, dispatch relay payments or launch a build.
- If funding commits first, cancel its sole attempt even if the client has not
  received its ID. Block all not-yet-dispatched work and close any existing path.
  Work already dispatched may have side effects and must be reconciled; it cannot
  be undone by cancellation or retried as a replacement.
- Exact retries recover the terminal state without resetting deadlines or
  creating another cancellation. A new operation for the same cancelled agreement
  also observes terminal state. Replayed acceptance, invoices, zero-charge
  authorization or callbacks cannot revive it.

Retain cancellation tombstones with the durable agreement/payment retention rules,
including unresolved incoming payments and invoice-creation operations. No pending
wallet result is treated as failed. Late received principal is recorded under the
original refund policy and may be reconciled/refunded without a constructed
attempt; zero remains the exact agreement's no-attempt value. Polling/cancellation
use the existing bounded control envelope, transport retries and shared limits.
Loss of a cancellation reply is uncertainty, not proof the service stopped; the
client stops new local dispatch and queries/retries the same signed operation.

Cancellation atomically records no-new-spend and terminal connectivity intent,
then closes the exact existing path. It cannot undo dispatched payments or refund
automatically. A racing READY cannot defeat a cancellation already committed.
An uncertain controller close is reconciled with the same handle; never construct
a replacement. Loss of the Tor process makes the attempt terminal, with outstanding
financial records retained. Restart of only the daemon may recover the same live
attempt if its durable state, instance and original monotonic deadlines survive;
otherwise stop spending and close conservatively. No persisted timestamp restores
traffic keys after Tor restart.

## Path-bound READY exchange

Allocate ordinary end-to-end RELAY commands 238 SERVICE_READY_CHALLENGE,
239 CLIENT_READY and 240 SERVICE_READY_ACK. StreamID=0, Length=498, no RELAY_EARLY.
They use the existing onion-service rendezvous end-to-end encryption; do not send
them through introduction reply slots or as unencrypted RPC payloads. Only the
client and service origins terminate them. Intermediates forward encrypted cells
normally. Service version one requires this profile; it is not ordinary Tor
compatibility. Values are local draft assignments, not upstream registrations.

The service binds one random 32-byte `challenge` to its exact live rendezvous
circuit object generation, accepted core and attempt after sending the rendezvous
join. Client Tor must independently complete the inherited handshake for the
intended service before processing a challenge. Service Tor accepts CLIENT_READY
only on the exact circuit on which that challenge was issued. A valid signature
arriving through a different circuit or introduction path is rejected.

All three messages use this same 212-byte body:

```text
u16 version=1 || u8 message || u8 flags=0
service_identity32 || quote_core_digest32 || circuit_authorization_id32
attempt_id32 || challenge32
u64 signer_generation || u64 agreement_revision
client_auth_key32
```

Message is 1 challenge, 2 READY, 3 ACK. Append a 64-byte signature then 222 zero
padding bytes to Length 498. Sign `D("eltor/scq/v1/ready-path") || u8(command) ||
L(body)`. The signature binds the command; an ACK cannot become a challenge.
Service messages use a currently valid delegated signer and its generation.
Client READY uses client_auth_key and signer_generation=0. The client echoes the
challenge's agreement_revision; ACK reports the revision after durable READY
acceptance. All service/core/circuit/attempt/client/challenge bindings must match.
Invalid key/signature/version/length/flags/padding is rejected before mutation.

The client verifies the challenge's service signature and attempt binding against
its accepted SCQ and actual handshake. It then sends one logically immutable
signed READY; duplicates re-encrypt through the advancing Tor circuit cipher,
never replay captured encrypted cells. The service validates challenge, circuit,
client signature and unexpired state, then atomically records READY before ACK.
An exact duplicate READY on that live path returns current authenticated ACK
without another transition, new charge, build, invoice or clock reset. Once READY
is recorded, a lost ACK does not undo it or kill a valid stream.

At the original setup deadline, absent READY stops new spending. Retain the same
path only until the earliest of setup deadline+15000 ms, already-funded path
expiry and a stricter resource/closure limit. Unpaid grace is not funded time.
No remaining time means immediate terminal closure. A valid READY can complete
within this recovery interval; at or after its end it cannot. Resuming funding
after completion still requires safe deadlines, admitted rounds and original caps.
Neither READY nor ACK authorizes exceeding them.

## Retry limits and observation

Service challenge has initial transmission plus at most three retransmissions;
client READY independently has the same four-transmission ceiling. Retry intervals
are uniformly sampled 2000–3000 ms after each prior transmission, clipped by the
original setup/recovery/funded deadline; this is a candidate test profile.
A duplicate challenge does not reset the client's READY counter or trigger an
immediate extra retry. A matching ACK stops READY retries. The service stops
challenge retransmission after accepting READY. At most four ACKs are emitted
for the four permitted READY deliveries; extra duplicates are dropped. Routine
signer rotation can change service signature/generation while retaining challenge
and attempt, but cannot restart retry counters. No failure triggers another path.

If all ACKs are lost, a bounded STATUS_GET may confirm persisted READY. It does
not replace the requirement that the client completed its local handshake. If
no safe answer is available, report uncertain service readiness, preserve live
funded streams where allowed, and stop new automatic spending that depends on
confirmation. Original paid deadlines still apply. Persist counters/state across
daemon restart; if exact deadlines cannot be recovered, do not grant extra time.

## Privacy and validation

The READY messages have equal length and carry stable bindings only inside the
existing rendezvous encryption. Their timing and existence can still be observed
or correlated, and the service learns the client key already accepted in the SCQ.
Fresh challenge per actual attempt prevents cross-circuit use without adding a
public identity. Status accounting is privileged encrypted data, not routine logs.
Candidate retries add bounded traffic; measure congestion and fingerprinting
against the ordinary paid circuit profile before deployment.

ET-16/21/29–33 require signature/size vectors, wrong-path valid READY, challenge
substitution, simultaneous cancellation/READY, lost ACKs, late completion at each
deadline, accounting partition checks, process versus daemon restart and no-spend
queries. Actual hs-ntor setup/introduction binding is a separate prerequisite;
these messages cannot manufacture a rendezvous cookie or authorize a second build.
Documentation and arithmetic checks do not validate the runtime handshake.
