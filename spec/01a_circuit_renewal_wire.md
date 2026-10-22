# Paid circuit renewal wire protocol — draft v1

This is the proposed wire companion to [spec one](01_paid_circuits.md#circuit-renewal-draft). It defines `PAYMENT_RENEW` and `PAYMENT_RENEWED` for replenishing payment identifiers on an existing circuit. It is not deployed Tor functionality, an upstream command allocation, or a completed cryptographic review. Implementation is deferred in the [El Tor TODO](../TODO.md). No production feature should be advertised until the prerequisites, security review, and acceptance tests below pass.

## 1. Scope and negotiation

Keep the original path, streams, traffic keys, per-hop initial payment fields, and ten identifiers per hop. Send new control messages to each existing hop; do not rebuild, add a hop, migrate a live stream, restart a bandwidth test, or prepay ten rounds. Prepare at the start of round seven in each ten-round allocation. Continue paying individual 60-second rounds with the already agreed advance-payment and grace rules.

V1 supports paid circuits established with the authenticated `ntor-v3` handshake (`HTYPE=0x0003`). It does not define an exporter for TAP, CREATE_FAST, older ntor, the client/service hs-ntor exchange, or a free-only renewal mode. Both onion rendezvous halves use their own origin-to-relay ntor-v3 contexts. The end-to-end SCQ is not a transport-hop identifier.

A supporting relay proposes the following optional line in its signed relay descriptor:

```text
eltor-payment-renew 1 224 225 TERMS_HEX
```

`TERMS_HEX` is exactly 110 lowercase hexadecimal characters encoding the 55-byte policy below. The descriptor signature authenticates this advertisement; it does not reserve capacity or prove admission. A client must retrieve and verify the applicable signed descriptor, rather than trusting an unauthenticated hint. Changes to directory/microdescriptor propagation and parsers are implementation work; this line is not currently understood by those components. Unknown versions are ignored and never assumed compatible.

For a new circuit, the origin and relay retain a snapshot of the accepted policy, the original ten identifiers in order, and the initial round schedule. A relay cannot change that snapshot by publishing a new descriptor. An authenticated renewal acknowledgment confirms admission under the snapshot. A mismatch means no renewal; it does not silently reprice existing service. If initial terms/schedule cannot be established unambiguously, the implementation MUST NOT advertise this feature. Establishing that initial commitment is a deployment prerequisite, not something a renewal acknowledgment retroactively guarantees.

The proposed opcode pair is private to this El Tor draft:

| Relay command | Proposed value | Direction |
| --- | ---: | --- |
| `PAYMENT_RENEW` | 224 (`0xe0`) | Circuit origin to one existing hop |
| `PAYMENT_RENEWED` | 225 (`0xe1`) | That hop to its circuit origin |

These values were absent from the inspected fork's relay-command definitions. They are not a reserved upstream/private-use range. Check for conflicts before implementation; changing them changes the authenticated protocol and requires a new advertised profile/version. Do not send them to unadvertised or incompatible relays. Lack of an acknowledgment means renewal failed, not that an unknown command was accepted.

## 2. Framing and common encoding

Use the ordinary outer `RELAY` cell, a zero `StreamID`, and the normal recognized/digest fields and per-hop layered encryption. The new relay-command byte is inside that encryption; it is not a new plaintext link-cell type. Implementations MUST explicitly send these messages as ordinary `RELAY`, including to intermediate hops, without consuming the finite `RELAY_EARLY` allowance. Keep existing congestion, bounded control-traffic, padding, and queue policies; the command is not an unlimited flow-control bypass.

Both message types have relay `Length=463`. There is no fragmentation, implicit variable-length extension, hexadecimal on-wire identifier array, or concatenated all-hop payload. Standard Tor's 498-byte relay data area leaves 35 bytes for normal relay padding. In this fork's larger cells, use the same 463-byte message and its normal remaining padding; this proposal does not resize cells. Receivers ignore outer padding and never use it as application data.

All integers are unsigned, big-endian, and fixed-width. Offsets below start at the message's first data byte, excluding the 11-byte relay header. `u8/u16/u32/u64` mean 1/2/4/8-byte integers. Byte strings have their stated exact sizes. Reject arithmetic overflow, wrong lengths, unsupported flags, nonzero reserved bytes, zero/repeated identifiers, or extra/truncated data. Payment identifiers and renewal nonces are independently generated for each hop; reject duplicates within a batch and identifiers already registered or consumed in another allocation according to the durable payment ledger.

For formulas, `||` means concatenation; `D(s)=u16(len(ASCII(s))) || ASCII(s)` with no terminating NUL. `H=SHA256` and `MAC=HMAC-SHA256` with the full 32-byte result. `ENCAP64(b)=u64(len(b)) || b`.

## 3. Canonical terms and private circuit binding

The signed policy and retained accepted snapshot use this exact encoding:

| Offset | Bytes | Field and v1 constraint |
| ---: | ---: | --- |
| 0 | 1 | `version=1` |
| 1 | 1 | `mode=1`: ordinary paid capacity, without an origin-role marker |
| 2 | 1 | `batch_count=10` |
| 3 | 8 | `rate_msat`: accepted amount per paid round |
| 11 | 4 | `round_ms=60000` |
| 15 | 4 | `lead_ms=60000`: advance funding, not late grace |
| 19 | 4 | `grace_ms`: explicitly accepted, at most `round_ms`; 15000 is the existing baseline candidate |
| 23 | 8 | `paid_quota_bytes`: per-round paid quota, zero means unlimited paid quota, never unlimited free traffic |
| 31 | 8 | `max_age_ms`: explicit finite cumulative age from the hop's initial round start |
| 39 | 4 | `max_periods`: finite total ten-round allocations including initial period zero, at least two |
| 43 | 8 | `initial_handshake_fee_msat`: existing initial fee, not charged again on renewal |
| 51 | 4 | `setup_timeout_ms`: finite setup deadline, never reset by renewal |

There is no new default for maximum age/periods: operators must choose explicit finite limits before enabling this profile. Reject a next allocation whose full end would exceed either accepted limit. Grace or retry cannot extend the hard age limit. Ordinary Tor age, isolation, resource, and failure policies can impose tighter limits. Defining these fields is not approval of a 24-hour path. A zero-rate policy still needs explicit valid authorization; dummy values never satisfy a required positive payment.

The hop's initial round start is `T0`, a monotonic local event retained from the initial accepted schedule. Renewal never resets it. The origin need not share its absolute clock: round positions are relative to that hop's `T0`, and replies provide conservative timing information below. Free test/build time, paid start signaling, and their deadlines must be resolved in initial circuit establishment before this profile can be deployed. This wire extension does not reinterpret an old watcher's callback time as an agreed start or settlement time.

Use a separate proposed exporter from the successfully authenticated ntor-v3 `ntor_key_seed`; do not repurpose traffic keys or alter existing Tor key output:

```text
CTX = H(ENCAP64(client_HDATA) || ENCAP64(server_HDATA))
PRK = HKDF-Extract-SHA256(
    salt=H(D("eltor/payment-renew/v1/extract")),
    IKM=ntor_key_seed)
K_request = HKDF-Expand-SHA256(PRK,
    D("eltor/payment-renew/v1/request-key") || CTX, 32)
K_reply = HKDF-Expand-SHA256(PRK,
    D("eltor/payment-renew/v1/reply-key") || CTX, 32)
C = HKDF-Expand-SHA256(PRK,
    D("eltor/payment-renew/v1/circuit-binding") || CTX, 32)
Q = H(D("eltor/payment-renew/v1/terms") || accepted_terms_55)
A0 = H(D("eltor/payment-renew/v1/initial") || C || Q || initial_ids_320)
```

`client_HDATA` and `server_HDATA` are exactly this hop's ntor-v3 handshake data, excluding CREATE2/CREATED2 framing, channel circuit IDs, and link headers. `initial_ids_320` is the ordered decoded binary array from this hop's existing payment payload. `C` remains local and is never sent to another hop. Distinct handshakes provide distinct bindings even if controller circuit IDs are reused. No new bytes need to be added to the initial payment payload for this proposed exporter.

Derive only after successful handshake authentication and preserve existing ntor-v3 error checks. Erase the exporter seed/PRK/transcripts when no longer needed; retain only required circuit-local derived state. Never export seed or MAC keys to the payment daemon, wallet, controller logs, or another relay. The C/Rust boundary should pass validated identifiers/status, not secret keys. Current Tor interfaces do not expose this exporter: both handshake paths and key lifecycle require implementation and cryptographic review before enabling v1. The construction is a reviewable draft using RFC 5869, not an existing Tor exporter or a proof of its own security.

## 4. `PAYMENT_RENEW` request

Period zero is the initial ten rounds. For a request for period `P>=1`, `first_round=10*P+1`, the ordered identifiers authorize only that period's ten separate rounds, and `activation_ms=(first_round-1)*round_ms` relative to `T0`.

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 1 | `version=1` |
| 1 | 1 | `flags=0` |
| 2 | 8 | `period=P` |
| 10 | 8 | `first_round` |
| 18 | 1 | `count=10` |
| 19 | 16 | `renewal_nonce`: fresh per hop and allocation |
| 35 | 8 | `activation_ms` |
| 43 | 4 | `round_ms=60000` |
| 47 | 32 | `terms_digest=Q` |
| 79 | 32 | `previous_allocation_digest`: `A0` initially, then the last admitted allocation's digest |
| 111 | 320 | Ten ordered, distinct 32-byte payment identifiers |
| 431 | 32 | `request_mac` |

```text
B = request[0:431]
request_mac = MAC(K_request,
    D("eltor/payment-renew/v1/request-mac") || C || u8(224) || B)
R = H(D("eltor/payment-renew/v1/request-hash") || request[0:463])
A_next = H(D("eltor/payment-renew/v1/allocation") || C || B)
```

`A_next` is a commitment to this exact private allocation. It is not a payment receipt or globally shared circuit ID. The next request refers to it only on this hop. Validate the parent digest, exact next period, round arithmetic, policy, timing, identifier freshness, and available capacity before admitting it. Accept only one pending allocation. A new nonce or different bytes for an already admitted period is a conflict, not a replacement or another purchase.

## 5. `PAYMENT_RENEWED` acknowledgment or refusal

The reply authenticates the exact request and carries a status. Successful admission MUST be recorded before replying. On refusal the existing paid allocation remains valid until its normal deadline; refusal is not an instruction to immediately destroy healthy paid streams.

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 1 | `version=1` |
| 1 | 1 | `flags=0` |
| 2 | 1 | `status` below |
| 3 | 1 | Reserved, zero |
| 4 | 8 | Requested `period` |
| 12 | 16 | Echoed hop-specific `renewal_nonce` |
| 28 | 32 | `request_digest=R` |
| 60 | 32 | `allocation_digest=A_next` on success; all zero on refusal |
| 92 | 8 | `relay_elapsed_ms`: monotonic elapsed time from `T0` when generating this reply |
| 100 | 8 | `activation_ms`: requested allocation boundary |
| 108 | 8 | `admit_before_ms=activation_ms-lead_ms`, relative to `T0` |
| 116 | 32 | This circuit's accepted `terms_digest=Q` |
| 148 | 8 | Requested `first_round` |
| 156 | 1 | `count=10` |
| 157 | 274 | Reserved fixed-size fill, all zero |
| 431 | 32 | `reply_mac` |

```text
reply_mac = MAC(K_reply,
    D("eltor/payment-renew/v1/reply-mac") || C || u8(225) || reply[0:431])
```

The authenticated status values are:

| Value | Status | Meaning |
| ---: | --- | --- |
| 0 | `ADMITTED` | Exact allocation recorded, still pending; no claim of settlement |
| 1 | `ACTIVE` | Exact allocation was admitted and activated; no guarantee of delivery or future funding |
| 2 | `POLICY_LIMIT` | Declined under accepted age/period/admission policy |
| 3 | `TOO_LATE` | New admission missed its deadline |
| 4 | `CONFLICT` | Different request for an already reserved period |
| 5 | `BAD_PARENT` | Previous-allocation binding does not match |
| 6 | `TERMS_MISMATCH` | Request does not match the circuit's accepted policy |
| 7 | `BAD_SEQUENCE` | Incorrect period, round range, activation, or invalid/reused identifiers |
| 8 | `CAPACITY` | Bounded pending/state budget unavailable |
| 9 | `CLOSING` | Circuit cannot continue under existing closure state |

Unknown statuses are failure. Reply only to a correctly framed, authenticated v1 request; malformed lengths/flags, invalid MACs, unsupported versions, wrong directions, and nonzero stream IDs are dropped without allocating state or reflecting unbounded error traffic. Apply existing protocol-abuse limits. Authenticated semantic refusals use the same 463-byte format and rate limits as success. For checked requests whose round arithmetic is invalid, use `BAD_SEQUENCE` and zero the activation/deadline fields rather than overflowing; echo only fields that were safely parsed.

The origin verifies version, MAC, request digest, nonce, period, terms, activation, count, reserved bytes, and the expected allocation digest for success. A reply on another hop or circuit, or a reply matching only the period number, is insufficient. A MAC-valid `ACTIVE` reply does not replace recipient-node settlement verification or authorize an unpaid round.

## 6. Timing, retransmission, and state transitions

Round seven begins at `T0+360000` for the initial ten-round allocation. Prepare then, or earlier if an explicitly configured policy needs more margin. For period one, activation is `T0+600000`, and new admission must complete strictly before `T0+540000` with the candidate 60-second funding lead. Then fund round eleven during round ten under the agreed payment policy. Do not pay for round eleven in round seven merely because its identifiers are ready.

The origin requires successful admission from every hop before beginning next-period payments. Onion connections additionally require the service's authenticated readiness under the accepted SCQ extension. If readiness cannot be established before the earliest required funding deadline, do not initiate another period. Existing-round payments and deadlines continue normally.

`relay_elapsed_ms` lets the origin bound remaining time without synchronized clocks. For an acknowledgment matching a request, conservatively subtract the full local elapsed time since that exact request's earliest transmission from `admit_before_ms-relay_elapsed_ms`, clamping at zero. This covers a delayed or replayed response to an earlier retransmission. Never extend a deadline already learned from an earlier valid reply. Stop if the conservative remaining budget is insufficient. Exact operational safety margins require measured transport/wallet delay; timestamps do not guarantee timely settlement.

Use the same immutable request bytes and nonce for retransmission, freshly wrapped in normal relay encryption/padding. Never replay captured encrypted cells, which would corrupt running cipher/digest state. The draft retry ceiling is four transmissions per hop and period, initially at 0, 10, 30, and 60 seconds after preparation starts, each subject to the earlier deadline and resource limits. Stop on success/refusal or loss of need/budget; no unbounded polling. These tuning values must be tested before deployment. A duplicate request is the status query; no third wire command is required.

The relay's bounded state machine is:

| State/event | Action |
| --- | --- |
| Active allocation plus fresh valid next-period request, before admission deadline | Atomically check ledger uniqueness, store one pending allocation and its exact request/digest, then reply `ADMITTED` |
| Exact duplicate of the pending request | Return its current status; no new reservation, fees, identifiers, or deadline |
| Different request for that pending period | Reply `CONFLICT`; preserve the original allocation |
| Boundary reached, required first-round funding and authorization valid | Activate pending allocation, retain stream/cipher state, continue per-round enforcement |
| Boundary/funding deadline reached without required authorization/payment | Close according to the original bounded grace; no free fallback or fresh grace |
| Exact duplicate of the newly active request | Reply `ACTIVE`; do not activate or debit twice |
| Older period, skipped period, wrong parent, or late new request | Refuse without changing purchased service or resurrecting a closed circuit |
| Circuit failure or process loss | Stop new work, close lost live circuits, reconcile durable payment commitments; do not restore streams or recreate grace from the ledger |

Check an exact already admitted request before treating it as a new late admission. This permits status recovery after a lost acknowledgment while preventing a late different allocation. If the origin no longer has enough time to pay safely, it still must not initiate next-period payment. Keep at most the active request and one pending request plus bounded replay/payment records. A stored old success cannot authorize a different period/circuit or cause a second debit. Enforce aggregate traffic, state, setup, and CPU limits across all circuits.

Admission is not settlement. The payment daemon must check its recipient node, amount, accepted payment window, circuit/round binding, and single redemption before authorizing capacity. Early observed payment does not mean the current round has ended. Uncertain in-flight payments remain reserved until reconciled, including after abandonment or closure. Do not evict safety-critical commitments merely to keep live admission arrays small. No two-hop or all-hop atomic settlement is promised; the accepted financial exposure in specs one/two still applies.

An accepted pending allocation does not oblige the payer to buy it. If the origin stops, no cancellation message or automatic payment is implied; the circuit expires under the unchanged funding rules. No original handshake fee is charged again. Activation must not reset paid/free quotas, padding budgets, age, or cumulative spending caps.

## 7. Controller boundary and onion-service use

The Tor C implementation owns the per-hop secret keys, authentication, hop-local circuit mapping, and relay command parsing. Eltord needs a distinct authenticated local-control request/event interface to register batches and observe each hop's status; `EXTENDPAIDCIRCUIT` remains a build/add-hop operation. The exact text/binary local RPC grammar is intentionally separate future work. Controllers must not fabricate relay admission from a local successful write, and a Tor event is not a Lightning settlement receipt.

Spec two applies the same wire protocol independently to both rendezvous halves. Its SCQ extension/acceptance/readiness messages are authenticated end-to-end between client and service, not forwarded as shared identifiers to transport relays. Those client/service message encodings remain a separate TODO. Both halves preserve their actual hop-relative schedules; readiness must be obtained before the earliest funding deadline. Failure on either half stops further renewal and invokes reconciliation, not live-stream migration.

Free-only introduction-circuit renewal, cumulative age defaults, and explicit repricing at renewal boundaries remain unchosen policies. V1 keeps the continuing circuit's accepted rate and refuses instead of silently changing it. New application streams must retain existing Tor isolation rules; selecting an age/draining policy must not imply that all future browsing shares one renewed path.

## 8. Validation, vectors, and deployment gates

The [public synthetic vectors](vectors/payment_renewal_v1.json) specify terms, exporter outputs, two independent hop requests, an admitted reply, and malformed/semantic mutation cases. Their seeds/transcripts are artificial inputs for the renewal codec/exporter only, not valid ntor handshakes or wallet secrets. Values are test data, never production identifiers or keys. Codec consistency checks do not validate handshake integration, anonymity, live Lightning routing, or a running renewal implementation.

Before claiming interoperability, implement independent C/Rust encoders and parsers matching the vectors, cross-language known-answer checks, strict length/offset/endian and MAC tests, and fuzzing for overflow, malformed fill/counts, replay, conflict, and reordered replies. Test transport delivery to every existing hop, ordinary `RELAY` behavior after all `RELAY_EARLY` cells are exhausted, unchanged cell sizes/padding, bounded queues, and unsupported/downgraded peers.

Test live streams across 10→11 and 20→21 with the same circuit/stream state, every refusal/timeout path, duplicate requests/callbacks, pending/active status recovery, partial settlement, budgets, quota reset attempts, stale circuit-ID reuse, crashes, genuine relay failure, and both rendezvous halves. Sweep RTT, controller latency, wallet event delays, delayed/late settlement, grace, and preparation timings. Measure privacy effects against ordinary renewed clients and idle/short-lived circuits; encrypted framing is not a claim of traffic indistinguishability.

Deployment requires review of the proposed exporter, opcode/capability negotiation, initial terms/start commitment, safe ledger replay protection, bounded resource enforcement, and the above tests. These gates and the remaining local RPC/SCQ wire work are tracked separately so documentation cannot be mistaken for implementation completion.

References: [Tor relay cells](https://spec.torproject.org/tor-spec/relay-cells.html), [routing relay cells](https://spec.torproject.org/tor-spec/routing-relay-cells.html), [ntor-v3 handshake](https://spec.torproject.org/tor-spec/create-created-cells.html#the-ntor-v3-handshake), and [RFC 5869](https://www.rfc-editor.org/rfc/rfc5869.html). The additional exporter and payment messages above are El Tor proposals, not guarantees supplied by those sources.
