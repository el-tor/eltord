(01) El Tor - Paid Circuit Protocol 
==============================


## Shared circuit construction and bounded free allowance

**Draft extension:** All El Tor circuits use the same construction and per-hop payment payload below, including circuits used only within the bounded free allowance. This section adds a protocol proposal; it does not claim that bandwidth quotas or free-resource enforcement are already implemented. Paid service retains one-minute rounds. The [renewal draft](#circuit-renewal-draft) makes the initial ten-round allocation renewable on the same paid circuit when every hop agrees; without negotiated renewal, the existing ten-minute expiration remains in effect.

### Uniform per-hop payment payload

For each hop, preserve the handshake payment hash/preimage fields and ten distinct 32-byte round payment identifiers in the existing `EXTENDPAIDCIRCUIT` construction. Generate independent identifiers for each hop, round, and new circuit, including when no real payment is expected for the free allowance. Preserve per-hop encryption and equal-size padding. A relay decrypts only its own payment information.

Where no handshake fee applies, populate the existing handshake fields with fresh, correctly sized dummy hash/preimage values instead of omitting them. Where the agreed paid handshake fee applies, use its actual proof and verify it against the recipient's settled invoice records. A relay's advertised paid rate and paid handshake fee remain under operator control; a new relay may advertise a zero-sat handshake fee as before.

The free allowance MUST NOT use a shorter payload, missing payment fields, shared dummy constants, or a new circuit-purpose header identifying an onion service. It is available based on resource use, not an origin's claim to be a service. Ordinary clients and onion services construct their circuits the same way. Standard endpoint-specific Tor commands such as `ESTABLISH_INTRO` and descriptor publication remain necessary at their destinations; they do not require disclosing that role in the guard's or middle relay's payment payload.

### Small free capacity on ordinary circuits

Supporting relays advertise a small, bounded free traffic allowance alongside their normal paid rate. It is purpose-independent and may support low-volume communication, descriptor publication/retrieval, and introduction availability without recurring Lightning fees within those bounds. Every hop on the selected path must support and admit the allowance. The ordinary construction is shared; free usage is not a different onion-service circuit format.

Eligible setup and maintenance within the free allowance do not require a paid-service handshake charge. The preexisting handshake-fee and bandwidth-testing logic for paid service remains unchanged; this proposal does not introduce a separate paid-activation fee or promise a full-speed trial from the tiny allowance. A zero handshake fee alone does not advertise free recurring capacity, and the presence of dummy fields does not prove that a required payment was made.

Limits MUST bound per-circuit sustained/burst traffic in both directions, total relay-wide free traffic, circuit creation, concurrent/pending circuits, CPU/memory, queued work, and buffering. Include normal protocol maintenance and padding in the accounting. Many circuits or fresh identities MUST NOT multiply free resources beyond the relay's aggregate budget. Excess use is throttled or rejected with bounded queues/retries; it MUST NOT silently charge the origin. Numeric rates, bursts, and resource budgets remain configurable draft work to be selected through measurement.

Paid bandwidth beyond the allowance requires the agreed handshake and recurring payments. A circuit with a required paid round MUST close if that payment misses its bounded deadline/grace; it MUST NOT continue by falling back to free capacity. An independently admitted circuit using only the free allowance remains subject to the same small limits and aggregate budget. A dummy handshake proof or unused round identifier never counts as settled funds and cannot authorize paid capacity. Paid-round enforcement and free-resource accounting must therefore be explicit, without using a purpose-specific construction.

### Use cases and rationale

The purpose-independent allowance supports the following use cases within the same ordinary circuit construction. These are reasons for providing the allowance, not new circuit-purpose flags or promises of unlimited free traffic.

| Use case | Why a small free allowance is useful | Boundary |
| --- | --- | --- |
| Onion-service descriptor publication and refresh | A service can upload its contact information to responsible HSDirs before receiving any customer payment. It can publish replicas and refresh changed introduction information without a continuous discovery fee. | Upload traffic, setup, and retries must fit the path allowance; HSDir storage, document size, validation, and update rates have separate bounds in spec two. |
| Client descriptor retrieval | A client can learn introduction points, accepted payment methods, and contact information before deciding whether to pay the service. | Bound complete descriptor downloads, response bursts, repeated requests, and misses. The receiver must still authenticate and verify the descriptor. |
| Service-to-introduction-point availability | The service keeps its waiting-room circuits available to receive introductions without buying full-bandwidth rounds while waiting for customers. | Count idle circuits, maintenance, bounded padding, incoming introductions, and replacement overlap against the same per-circuit and relay-wide budgets. |
| Client introduction and bounded rendezvous setup signaling | Clients and services can exchange permitted introduction/control messages and prepare a rendezvous using low-volume transport where the allowance is sufficient. | This does not waive agreed paid setup/handshake charges or provide paid rendezvous bandwidth. The client must fund any paid work on the service side before that work is incurred. Standard Tor endpoint authorization and circuit-purpose rules still apply. |
| Learning payment terms or obtaining a payment request | Where a method uses Tor transport, a bounded bootstrap exchange can provide a quote or fresh invoice before a paid data connection is established. | Exact authenticated bootstrap messages remain design work. BOLT 12 invoice-request onion messages use Lightning's own protocol; the Tor allowance is not a blanket subsidy for wallet traffic or Lightning routing fees. |
| Ordinary low-volume communication and circuit maintenance | Any origin can use the same small capacity for permitted low-volume messages, acknowledgments, maintenance, or idle-period padding. The relay need not identify an onion service to grant it. | Other small uses are intentionally permitted; enforce aggregate setup, state, queue, and traffic limits across circuits and identities. |

Together these uses keep basic discoverability and introduction availability affordable, avoid requiring an already paid connection merely to learn how to pay, and reuse the same encrypted/padded construction for ordinary clients and services. Users need not disclose a service identity to intermediate relays to obtain the allowance. Compatible paths must still admit free traffic; resource exhaustion can cause bounded retries or temporary unavailability, not unexpected wallet spending.

The free allowance is not a replacement for the paid-circuit bandwidth test described below. A small allowance may demonstrate basic reachability but does not establish full paid throughput. Preserve the existing operator-selected handshake fee, including zero-sat handshakes for new relays; the allowance does not introduce a separate paid-activation fee. Paid-capacity commitments and recurring paid rounds retain their own payment and closure rules.

Validate these use cases when sizing the allowance: complete descriptor transfers and copies, refresh/replacement bursts, legitimate introduction/control traffic, normal maintenance/padding, and combinations of many small ordinary circuits. Numeric limits must accommodate measured legitimate workloads while keeping the relay's aggregate contribution bounded. No free allowance implementation or measured throughput is claimed by listing these use cases.

### Shared lifetime and specification relationship

The baseline ten-minute maximum applies to free and paid circuits without negotiated renewal. The renewal draft below proposes extending an established paid circuit in additional ten-round allocations, without replacing its path or active streams. Free-only circuit renewal remains an open decision; introduction and descriptor-upload circuits retain their existing ten-minute policy for now. Early closure remains valid for failure, poor performance, loss of required payment, user choice, or enforced resource limits. Idle periods are also normal. See [spec two](02_paid_hidden_service.md) for the corresponding onion-service lifecycle.

`BANDWIDTH_QUOTA` below describes a separate proposed quota for paid bandwidth; its legacy zero/unlimited value MUST NOT be interpreted as an unlimited free allowance. The bounded free allowance is also distinct from the initial bandwidth-testing window on a paid circuit. Capability signaling, exact accounting/acceptance rules, and the interactions with payment watchers remain implementation work. Validate identical construction/padding, aggregate bounds, legitimate free discovery, and correct paid-circuit closure before claiming support.

Client Flow
==========
A client wants to build a paid circuit.

Steps:
1. Relay Descriptor Lookup
2. Handshake Fee
3. Circuit build
4. Test Bandwidth
5. Init Payments Ledger
6. Client Bandwidth Watcher
7. Renew the existing circuit when negotiated; otherwise replace it

<b>1. Relay Descriptor Lookup</b>

A client lookups the Relays by:

- `PAYMENT_RATE` - rate the relay charges in msats per payment interval (default=1000)
- `PAYMENT_INTERVAL` - seconds per each payment interval (default=60)
- `PAYMENT_INTERVAL_MAX_ROUNDS` - baseline allocation of payment identifiers (default=10), limited by the initial onion-cell payload. Without negotiated renewal, the circuit expires after this allocation. The renewal draft proposes subsequent allocations on the same circuit; capability/configuration encoding is not yet defined.
- `HANDSHAKE_FEE` - a fee in msats the relay might charge to do bandwidth testing in the pre-build handshake step (default=0)
- `BANDWIDTH_QUOTA` - a quota set in KBytes on how much bandwidth a client can use per payment interval. *future work, not being used yet (default=0) unlimited

Select Relays:

Select 3 (or more) relays (entry,middle,exit) and lookup the BOLT 12 offer (or lnurl*) to pay. 
This info can be found locally in the `cached-microdesc-consensus` file that is downloaded from the Directory Authorities.
Relay selection is a topic for more research. For now, it's a random algo, excluding relays that charge exuberant fees. 
A client can set a `CIRCUIT_MAX_FEE` in msats to stay under. Maybe something like 20 sats for the initial ten-minute allocation; renewal must also respect an explicit cumulative spending limit.
It makes reasonable sense for a user to desire to keep monthly expenses to about what they are willing to pay for a centralized VPN service (average $10/month).
Maybe they will pay for privacy and splurge to $20-$30 a month. Since El Tor is based on usage, this amount can vary based on how long you use a circuit. A
future iteration could even include have a `BANDWIDTH_QUOTA` per circuit. But since El Tor is a free market, prices will vary.

<b>2. Handshake Fee</b>

During the handshake, in the circuit build step, a client can test the bandwidth. 
A relay can charge a fee for this step and there are incentives on why, or why not, the relay may want to charge a fee. 

Fee incentives:

A relay charging a handshake fee might make sense in a few senerios:
- if you are a mature relay that a lot of clients already trust you might want to charge a fee. The client will be willing to pay this fee
for the relays high service level and honest bandwidth.
- to prevent "free loader" clients. A "free loader" client is one that builds a circuit (without a handshake fee) then uses it for free until payment is 
required for the first round beyond the inital bandwidth test. Then the free loader prematurley kills the circuit before paying the first interval round. 
In Tor, relays cannot see the client IP (unless you are a guard), so there is no simple way to prevent a "free loader" except charging a fee. 
Free loading might be discouraged if the relay sets a small enough interval round, lets say 15 seconds. 
This could prevent the free loader from getting any meaningful bandwidth usage before he is disconnected. But the interval must be long enough to allow a lightning payment to go thru.


No Fee incentives:

Setting a fee probably does not make sense:
- if you are a noobie relay because nobody trusts your advertisted bandwdith yet. The relay might take the fee and run away
- to allow clients to do free bandwidth testing 
- if you want more clients connecting to you, leading to higher profits

<b>3. Circuit build</b>

Now that you know the handshake fee the next step is to build the circuit:

- a. Config 
    - a1. A typical relay might charge 1000 msats `PAYMENT_RATE` per interval of 60 seconds `PAYMENT_INTERVAL` for an initial allocation of ten rounds `PAYMENT_INTERVAL_MAX_ROUNDS`.
    Typically the `HANDSHAKE_FEE` is 0.
    - a2. A `HANDSHAKE_FEE` might be required by the relay
- b. Lightning Payment Setup
    - b1. For BOLT 12 Offer: Create 10 random `PAYMENT_ID` hashes (32 byte) for each relay to include later (see step 6) 
    in the encypted onion message of a BOLT 12 payment. Make sure each `PAYMENT_ID` is unique to avoid giving up privacy in correlation attempts. 
    This PAYMENT_ID is used as a lookup by the relay
    to verify payment. 
    - b2. For LNURL: create 10 invoices and concatinate the 10 payment hashes as the `PAYMENT_ID` in the chronological order that you are going 
    to pay them for each interval.
- c. Handshake fee
    - c1. If NO handshake fee is required the `handshake_fee_payment_hash` and `handshake_fee_preimage` is a random hash. 
    This is a dummy hash to pad for privacy reasons to prevent against a malicious relay that might contol two hops to prevent correlation and timing attacks.
    - c2. If a handshake fee is required, then the client pays the relay out of band and inserts a valid payment proof in the `handshake_fee_payment_hash` and `handshake_fee_preimage`
- d. Next, call the <b>EXTENDPAIDCIRCUIT</b> RPC
    ```
    EXTENDPAIDCIRCUIT 0
    fingerprint_entry_guard handshake_fee_payment_hash handshake_fee_preimage 10_payment_ids_concatinated
    fingerprint_middle_relay handshake_fee_payment_hash handshake_fee_preimage 10_payment_ids_concatinated
    fingerprint_exit_relay handshake_fee_payment_hash handshake_fee_preimage 10_payment_ids_concatinated
    ```
The `EXTENDPAIDCIRCUIT` RPC command builds an onion layer for each of the relays. 

- Relay 1 (entry) gets the payment hashes and preimages wrapped up into the onion message for hop 1. 
- Relay 2 (middle) gets his payment hashes and preimage wrapped up in hop 2 of the onion message. 
- Repeat for N middle relays. 
- Finally the exit relay gets his payment hashes and preimage wrapped up in hop 3 (or N). 
- *This works a little bit different for Hidden Services because there is no exit relay as they build a circuit to the introductory and rendevous points.

Each relay receives independent payment identifiers through its own encrypted onion layer. Unique per-hop values avoid shared payment identifiers across hops, and equal-size padding avoids fee-dependent payload lengths. Zero-fee handshakes retain the same fields using dummy values. Each relay decrypts only the payment data intended for that hop. These construction requirements also apply to the bounded free allowance and the circuits in [spec two](02_paid_hidden_service.md).

<b>4. Test Bandwidth</b>

After the circuit it built, the client can test that the circuit's bandwidth is as advertised for the first 10 seconds or so 
(make sure to pay before 1 min (or interval due) , *remember that it might take some time to find a lightning route).

<b>5. Init Payments Ledger</b>

Add the newly built circuit data to the `payments-ledger` to track each round by `CIRC_ID` and `PAYMENT_ID`. 
Extract each of the 10 `PAYMENT_ID`'s from the RPC call and assisgn a round number to each in order. 
Record 0 for each `UPDATED_AT` to signal that the round has not been paid yet.
See diagram below:

`payments-ledger`
```
PAYMENT_ID    CIRC_ID          ROUND      RELAY_FINGERPRINT    UPDATED_AT  
------------- -----------  ------------   -----------------   ------------  
   111           456             1             ENTRY_N             0
   222           456             1             MIDDLE_N            0
   333           456             1             Exit_N              0
   444           456             2             ENTRY_N             0
   555           456             2             MIDDLE_N            0
   777           456             2             Exit_N              0
   ...           ...             .                .                .
   999           456             10                                0
```

This kicks off the "Client Bandwidth Watcher".

<b>6. Client Bandwidth Watcher </b>

The client watcher is responsible for testing bandwidth every interval and handles payment for the next round of bandwidth.

```
LOOP every interval (1 min default) up until MAX rounds (10 default), then kill circuit
    - if good bandwidth 
        - then pay each relay with their respective PAYMENT_ID for the ROUND 
    - else if bad bandwidth - kill circuit
LOOP
```

<b>6. Repeat</b>

After the circuit is expired, build a new one and repeat. 

The watcher and relay-auditor pseudocode describe the baseline expiration behavior. When renewal is negotiated, use the state and boundary rules below instead of unconditionally killing a circuit at the end of its initial allocation. Receiving an early payment for the final round does not mean that round has finished.

## Circuit renewal draft

### Purpose and scope

A healthy paid circuit SHOULD be renewable in place so an active download or other established stream is not deliberately interrupted merely because the initial ten payment identifiers have run out. Keep one-minute paid rounds and supply a fresh batch of up to ten identifiers per hop for the next payment period. Registering identifiers is not payment for ten minutes: the payer continues purchasing individual rounds and may stop at any time.

This is a proposed extension, not existing implementation support. Every hop MUST explicitly negotiate a compatible renewal capability before relying on it. The initial construction, ten identifiers per hop, dummy zero-fee handshake fields, encryption, and equal-size padding stay unchanged. Renewal does not add hops or rebuild the circuit: the existing `EXTENDPAIDCIRCUIT` command with an existing circuit ID must not be assumed to replenish payment identifiers. The renewal message/control interface still needs specification.

An ordinary backup circuit can accept new connections and eligible connection-establishment retries; it does not transfer an already open stream to another circuit. After circuit loss, download resumption depends on the application and server, such as HTTP range support. In-place renewal preserves the existing circuit and its stream state; it cannot preserve a connection after a relay or either endpoint actually fails.

### Proposed renewal sequence

1. **Prepare early.** Before the current allocation ends, the origin decides whether to continue within its explicit spending policy and requests the next allocation from every hop. Leave enough time for negotiation, acknowledgment, and the existing advance-payment lead time; do not wait until the final round ends. Starting around round nine is a candidate for a ten-round allocation, not a fixed guarantee: admission must finish before advance funding for round eleven is due. Exact preparation lead time remains to be measured.
2. **Bind fresh identifiers.** Deliver each hop's independent next batch through authenticated, encrypted signaling on the existing circuit. Bind the request to the protocol version, a circuit-instance binding established during setup, recipient hop, hop-specific unique renewal identifier, next period number, monotonic round range, accepted price/limits, and activation boundary. A controller-local `CIRC_ID` alone is insufficient because IDs can be reused. Never reuse old identifiers or expose another hop's identifiers.
3. **Acknowledge admission.** Each hop validates the sequence, resource limits, rate commitment, and deadline, then records and acknowledges the same bounded pending allocation. The origin must receive valid acknowledgments from every hop before treating the path as renewable or initiating payment for its next period. An acknowledgment admits identifiers and terms; it is not proof of settlement or delivered bandwidth. Lost acknowledgments are retried idempotently or queried without adding another allocation.
4. **Fund one round at a time.** After all-hop admission, pay the next period's first round with the agreed advance notice and verify settlement under the recipient's normal payment rules. Continue the existing schedule; paying early does not start the new period early. The new batch might cover global rounds 11–20, with fresh per-hop identifiers, rather than reusing rounds 1–10 from the old batch. Do not prepay all ten rounds or replenish a shared cross-circuit balance.
5. **Continue or expire.** At the agreed boundary, each relay activates its admitted next allocation only with the required funding and authorization. Preserve existing path, stream IDs, cryptographic state, quotas, and payment deadlines. If negotiation or required payment fails, retain service through the already authorized interval, then close at its agreed boundary/deadline and bounded grace. There is no automatic eleventh round, free fallback, or new grace period caused by a renewal retry.

No distributed all-hop atomicity is assumed. A hop can acknowledge and subsequently fail, and a subset of payments can settle before another fails. Stop further spending, preserve uncertain commitments, and reconcile actual charges under the accepted failure policy; no automatic refund or uninterrupted-delivery guarantee follows from admission. A backup may be prepared within budget for new connections, but must not be described as migrating the old download.

### Payload size and private continuity

Do not put an entire day's identifiers in the initial handshake or one all-hop renewal message. Send a separate circuit-control message addressed to each existing hop, using Tor's layered relay-cell routing with stream ID zero. That relay associates the message with its existing local circuit state; no public service identity, wallet identity, or globally shared circuit identifier is needed to find the previous allocation. The origin already has the per-hop cryptographic context. Allocate a negotiated command for this purpose rather than reusing circuit-extension commands or depending on a finite `RELAY_EARLY` allowance for repeated renewal.

Ten 32-byte identifiers require **320 bytes per hop** when encoded as binary, not 640 bytes of hexadecimal text. A conservative candidate fits within standard Tor's 498-byte relay message data budget without enlarging cells. The local Tor fork currently defines a larger cell body in workspace-relative `tor/src/core/or/or.h`; renewal must not rely on that extra space, and this proposal does not change its cell size. The following is a sizing sketch, not an assigned wire format or approved cryptographic construction:

| Candidate field | Bytes |
| --- | ---: |
| Version and flags | 2 |
| Period number and first global round | 16 |
| Identifier count | 1 |
| Fresh hop-specific renewal nonce | 16 |
| Circuit-relative activation boundary and round duration | 12 |
| Agreed-terms digest | 32 |
| Previous allocation digest, computed from this hop's private allocation | 32 |
| Ten binary payment identifiers | 320 |
| Proposed authentication tag | 32 |
| **Total message data** | **463** |

This leaves 35 bytes of standard relay payload for padding; the existing 11-byte relay header is outside that 498-byte budget. The digest and authentication inputs must cover the exact canonical encoding, direction/domain, hop-local circuit instance, and accepted terms. The hop's current allocation digest plus monotonic period/round state links renewal to its previous allocation without sharing a linkable token with another hop. Acknowledgments must commit to the exact admitted request and use the same private binding. Period counters are sequence numbers, not secrets or globally unique identities.

Key derivation/export and tag validation require a reviewed, domain-separated construction for the negotiated Tor handshake. Do not reuse a raw traffic key as a new MAC key, treat an unauthenticated digest as authorization, or assume Tor's four-byte relay digest alone defines the new payment-authorization protocol. The initial payment payload remains unchanged; whether the needed circuit binding/key can be derived without extra handshake data must be resolved before finalizing this sketch. If required metadata no longer fits, revise the bounded encoding explicitly rather than overflowing a cell or silently adding unbounded fragmentation.

Generate independent random identifiers and renewal nonces for each hop, and compute each allocation digest solely from that hop's private allocation. Do not copy a single renewal token or previous-allocation digest to every hop, or send the client/service SCQ identifier to transport relays as their renewal identifier. A relay necessarily knows that its own circuit continues; the privacy goal is to avoid adding a common identifier that lets different hops match their payment records. Reusing a path for longer also extends the period over which that hop can observe it. Encryption and equal-size messages do not prevent colluding relays from using timing or traffic volume.

For scale, 24 hours contains 1,440 one-minute rounds. At three hops that is 138,240 bytes (135 KiB) of raw identifiers over the whole day, excluding framing, replies, and forwarding overhead; two three-hop rendezvous halves double this raw total. The origin and each hop need only the active and pending batches for live admission, not a day's future identifiers. Historical payment/replay records still need bounded retention and safe expiry. This calculation is not approval of a 24-hour circuit lifetime; the cumulative age policy remains open.

### Price, consent, and resource protection

Renewal requires every hop to honor the accepted price commitment. This draft preserves the existing fixed price on the continuing circuit; a changed descriptor cannot silently reprice it. A hop may decline a future allocation under its advertised policy, without shortening already purchased service. Whether to allow separately accepted price changes at later period boundaries is an open decision, not permission to change existing terms. If the path cannot continue on accepted terms, use a separately authorized replacement.

Do not repeat the original build/testing handshake fee merely to register another allocation on the same circuit. Any proposed renewal-specific fee would require a separate explicit design decision and payer approval. Renewal MUST NOT restart a free full-bandwidth test, reset bandwidth buckets or aggregate budgets, bypass circuit-age policy, or turn an unpaid paid circuit into a free circuit. Each continuation must fit cumulative spending and outstanding-payment caps; permission to renew is bounded by the payer's configured budget, not a commitment to unlimited future rounds.

Keep at most the active and one pending allocation per hop, with bounded request sizes, retries, queues, acknowledgment state, and replay records. Enforce relay-wide state/CPU/traffic limits across circuits and identities. Preserve the recipient's settled-invoice verification and single-use consumption rules for every fresh identifier. Duplicate requests, reordered messages, wallet callbacks, and restarts MUST NOT buy extra rounds, reset the clock, or extend expired authorization. Persist or safely reconcile the ledger before authorizing more spending; fail closed if the required state is unavailable. Closed circuits cannot be resurrected by late settlement or renewal messages.

Use the same renewal format and padding rules across ordinary and onion-service circuits, without an origin-purpose marker. Authenticate acknowledgments to their intended hop without revealing the rest of the path. Fixed message framing alone does not eliminate traffic correlation; preserving long-running circuits changes path exposure and requires review alongside existing padding and isolation rules.

### Open decisions and validation

Before implementation, specify capability/version negotiation, the circuit-instance binding, bounded message encoding and authentication, idempotent status queries, admission/activation state transitions, exact advance deadlines, and ledger recovery. Decide a maximum cumulative circuit age/number of renewals, how to stop assigning new streams to aging circuits, and whether free-only circuits can renew. Renewal is not authorization to keep one path forever. Legacy or incompatible paths retain replacement behavior.

Test a continuous transfer across rounds 10→11 and 20→21, retaining the same circuit and established stream. Test refusal at any hop, missing/duplicate/reordered acknowledgments, partial admission and settlement, late payment, stale messages and circuit-ID reuse, replay, crash/restart, budget exhaustion, quota-reset attempts, bounded state under floods, and genuine relay failure. Verify no round beyond the accepted allocation is charged and no new ten-round lump payment is introduced. These are future acceptance tests, not executed results. Onion connections add the two-half coordination requirements in [spec two](02_paid_hidden_service.md#renewing-both-rendezvous-circuit-halves).

References: [Tor relay-cell format](https://spec.torproject.org/tor-spec/relay-cells.html) defines circuit-control stream IDs, framing, and digests; [relay-cell routing](https://spec.torproject.org/tor-spec/routing-relay-cells.html) describes addressing an existing hop through layered encryption; [setting circuit keys](https://spec.torproject.org/tor-spec/setting-circuit-keys.html) describes handshake-specific key derivation. [Tor stream attachment rules](https://spec.torproject.org/control-spec/commands.html#attachstream) restrict attaching already open streams; [HTTP range requests](https://www.rfc-editor.org/rfc/rfc9110.html#section-14) provide optional application-level partial retrieval after interruption.

Relay Flow
=========
A relay wants to start sharing his bandwidth. Configure payment preferences and rate.  

Steps:
1. Set Torrc Config
2. Handshake
3. Watch emitted Event `PAYMENT_ID_HASH_RECEIVED`
4. Start Relay Payment Watcher
5. Init Payments Ledger
6. Start Lightning payment watcher
7. Payment Ledger Cron (Auditor Loop)

<b>1. Config</b>

torrc
```
# Static lightning offer code
PAYMENT_BOLT12_OFFER lno***

# BIP-353 name that uses DNS to map to a BOLT 12 offer
PAYMENT_BOLT12_BIP353 name@domain.com

# Rate the relays charges in msats per payment interval (default=1000)
PAYMENT_RATE 1000

# Seconds per each payment interval (default=60)
PAYMENT_INTERVAL 60

# Initial payment-ID allocation (default=10); expires unless renewal is negotiated.
# Renewal is a draft extension; no new configuration support is claimed here.
PAYMENT_INTERVAL_MAX_ROUNDS 10

# We recommend to set this 0 to allow the client to test the bandwidth. 
# Setting this might make your relay less desirable as a noobie relay, but can be useful if you are being spammed or are a mature relay
HANDSHAKE_FEE 0 

# A quota set in KBytes on how much bandwidth a client can use per payment interval. *future work, not being implemented yet (default=0) unlimited
BANDWIDTH_QUOTA 0
```

Eventually support BOLT 11 because some implementations support blinded paths!
```
PAYMENT_BOLT11_LNURL lnurl*** 
PAYMENT_BOLT11_LIGHTNING_ADDRESS name@domain.com
```

<b>2. Handshake</b>

A relay receives a handshake cell to extend/create a circuit from a client willing to pay for bandwidth.

Handshake Fee check: A relay receives an onion message to `EXTENDPAIDCIRCUIT`. Verify the `handshake_fee_payment_hash` and `handshake_fee_preimage`. 
If no fee applies under the agreed allowance or handshake terms, retain the padded fields and do not treat dummy values as a real payment. The bounded free allowance above is distinct from paid service: required paid fees still need verification, while eligible free-only use is governed by resource limits rather than unpaid paid-round deadlines.

<b>3. Watch emitted Event PAYMENT_ID_HASH_RECEIVED</b>

The tor daemon will emit the event `EXTEND_PAID_CIRCUIT` for the relay watcher to verify against the lighting database.

<b>4. Relay Payment Watcher</b> 

The relay has an event watcher running that tracks payments and verifies against the remote lightning database. It also listens for events emitted
from the tor daemon `EXTEND_PAID_CIRCUIT`. The relay payment wacher can write to the `payments-ledger` 

On `EXTEND_PAID_CIRCUIT` event received:
- If the relay requires a handshake fee then check that the payment `handshake_fee_payment_hash` is valid and belongs to you by checking the lightning database. 
    - If not valid, then kill the circuit. 
    - If good, then add the newly built circuit to the `payments-ledger` to track each round (in step 5)

<b>5. Init Payments Ledger</b>

Add the newly built circuit data to the `payments-ledger` to track each round by `CIRC_ID` and `PAYMENT_ID`. 
Extract each of the 10 (or `PAYMENT_INTERVAL_MAX_ROUNDS`) `PAYMENT_ID` from event and assisgn a round number to each in order. 
Record 0 for each `UPDATED_AT` to note that the round has not been paid yet. Mark the `RELAY_FINGERPRINT` as `ME` since you are the one getting paid.
See diagram below:

`payments-leger`
```
PAYMENT_ID    CIRC_ID          ROUND      RELAY_FINGERPRINT    UPDATED_AT  
------------- -----------  ------------   -----------------   ------------   
   111           456             1               ME                0
   222           456             2               ME                0
   ...           ...             .               ME                0
   999           456             10              ME                0
```

<b>6. Lightning Payment Watcher</b>

After you add each `PAYMENT_ID` to the `payments-ledger` kick off a "lightning watcher".

Watch your lightning node for incoming payments that includes a 32 byte `PAYMENT_ID` hash inside a BOLT 12 message (payer_note).
Also watch BOLT 11 invoices, if used, for that same `PAYMENT_ID`, but this value is actually the payment_hash of the invoice.

Payment Proof:

When a lightning payment id (hash) matches then call the `Payment Ledger Cron` in step 7.


<b>7. Payment Ledger Cron (Auditor Loop)</b>

A loop is running every minute (or configured interval) to audit the payments ledger to make sure the pay streams are coming in for active circuits.
```
Loop each `CIRC_ID` and find the smallest round that has an `UPDATED_AT` field that is not 0 (if none then its the first ROUND, no payments received yet)
    ROUND=N
    check in the lightning database if the invoice with that `PAYMENT_ID` was paid:
    if yes paid
        - check if the client missed the payment window (NOW - UPDATED_AT > 1 min (or PAYMENT_INTERVAL) )
            - if yes, missed payment window, then kill the circuit and remove the rows from the ledger for the CIRC_ID
            - else 
                Update the UPDATED_AT field with the current unix timestamp for the ROUND. thus incrementing the ROUND
                - if round is greater than or equal to 10 (or MAX_ROUNDS) 
                    - if yes, then kill the circuit and remove the rows from the ledger for the CIRC_ID
                    - if no, return
    if no 
        - check if the client is outside the payment window (NOW - UPDATED_AT > 1 min (or PAYMENT_INTERVAL) )
            - if no, return
            - if yes, then kill the circuit and remove the rows from the ledger for the CIRC_ID
LOOP
```

BOLT 12 Tests between implementations
-------------------------------------
Can we send a message with 32 byte PAYMENT_ID in each of the implementations? (test using BOLT12 playground)
```
CLN
    - CREATE: lightning-cli --network regtest offer 0 clndesc
    - LIST: lightning-cli --network regtest listinvoices
    - PAY: lightning-cli fetchinvoice -k "offer"="lno***" "amount_msat"="2000" "payer_note"="<PAYMENT_ID>"

Phoenixd 
    - CREATE: Autogenerated or with elcair `./bin/eclair-cli eclair1 tipjarshowoffer`
    - LIST: Subscribe to webhook: `websocat --basic-auth :<phoenixd_api_password> ws://127.0.0.1:9740/websocket`
    - PAY: curl -X POST http://localhost:9740/payoffer \
            -u :<phoenixd_api_password> \
            -d amountSat=2 \
            -d offer=lno
            -d message='<PAYMENT_ID>'
LNDK
    - CREATE: ./bin/ldknode-cli lndk1 offer
    - LIST: ??
    - PAY: ??

Strike (only works with LNURL currently)
    TODO TEST LNURL + Blinded Paths
    TEST Paying to BOLT12

```

- CLN -> Phoenixd
    - [X] BOLT12 message works
    - [X] Message Field Name=payer_note
 
- CLN -> LNDK
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Phoenixd -> CLN
    - [X] BOLT12 message works
    - [X] Message Field Name=payerNote

- Phoenixd -> LNDK
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- LNDK -> CLN
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- LNDK -> Phoenixd
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Strike -> CLN
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Strike -> Phoenixd
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Strike -> LNDK
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?


### Compatiabilty issue notes

- CLN (current Umbrel version) cannot pay a lno without a description. Phoenixd does not include an offer description when they automatically create the BOLT 12 offer.
