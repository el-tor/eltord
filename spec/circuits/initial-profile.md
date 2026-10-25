# Initial paid-hop commitment and activation, version 1

**Proposed protocol; no runtime implementation is claimed.** This profile closes
the initial-state dependency of the renewal wire draft. It preserves the existing
per-hop initial payment payload and uses authenticated control cells on the
already constructed ntor-v3 circuit to commit terms and start paid capacity.
Bounded setup/control capacity is not permission to carry unpaid bulk traffic.

## Capability and key derivation

A relay implementing this profile advertises the following in its signed relay
descriptor, alongside `eltor-payment-renew` when renewal is supported:

```text
eltor-payment-start 1 226 227 228 229 TERMS_HEX
```

`TERMS_HEX` is the exact 55-byte accepted-policy encoding from the renewal draft,
in lowercase hex. The advertisement is authenticated through the signed relay
descriptor; an unverified microdescriptor hint is insufficient. The policy bytes
in the start and renewal advertisements MUST match. Origins pin the exact
verified bytes before paying any applicable setup fee; stale terms may be refused,
never silently replaced. This profile is paid mode 1, including an explicitly
zero-priced paid round; free-only circuits do not run paid activation.

Use the renewal draft's exact ntor-v3 transcript-derived `C`, `Q` and `A0` and
same base `PRK`/`CTX`. Derive separate keys with its `D` and HKDF conventions:

```text
K_start_req = HKDF-Expand-SHA256(PRK,
    D("eltor/payment-start/v1/request-key") || CTX, 32)
K_start_rep = HKDF-Expand-SHA256(PRK,
    D("eltor/payment-start/v1/reply-key") || CTX, 32)
```

Retain these keys only inside Tor. Do not reuse renewal MAC keys or traffic keys.
Ordinary RELAY cells, StreamID zero and exact Length 463 apply; no RELAY_EARLY,
fragmentation or cell-size change. Commands 226–229 are El Tor draft allocations,
not upstream reservations; they were absent from the inspected fork definitions.
All integers are unsigned network byte order. Reserved bytes MUST be zero.

For every request/reply below, `B=message[0:431]`, with a 32-byte MAC at offset
431. Requests use `K_start_req`, replies `K_start_rep`:

```text
mac = HMAC-SHA256(key,
    D("eltor/payment-start/v1/mac") || C || u8(command) || B)
request_digest = SHA256(D("eltor/payment-start/v1/request") || request[0:463])
```

Malformed authentication/framing is dropped under bounded abuse policy, not
reflected into an unlimited error exchange. Semantic replies have identical size.
The exporter construction and handshake integration still need independent review.

## PAYMENT_COMMIT (226)

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 1 | version=1 |
| 1 | 1 | flags=0 |
| 2 | 16 | fresh per-hop commit nonce |
| 18 | 55 | exact accepted policy bytes |
| 73 | 32 | initial allocation digest A0 |
| 105 | 326 | reserved zero |
| 431 | 32 | request MAC |

The recipient checks its retained initial ten IDs, per-hop handshake data and
policy against the authenticated request. It atomically reserves those IDs for
this circuit's rounds and records the immutable commit before accepting. A reused
ID, mismatching A0 or conflicting commit cannot overwrite a reservation. Exact
duplicates recover the same result; only one commit is allowed per hop/circuit.
Changing descriptor prices later cannot change an accepted commit.

The relay starts its finite setup clock when it accepts the initial paid circuit
construction, before commit. `setup_timeout_ms` bounds that clock; receipt of
COMMIT does not start it over. Record the remaining setup budget in the reply.
This hop timer is separate from the service SCQ's funded-reservation setup timer.
The origin must fit both. Capacity denial is an ordinary refusal, not a new charge.

## PAYMENT_COMMITTED (227)

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 1 | version=1 |
| 1 | 1 | status |
| 2 | 16 | echoed commit nonce |
| 18 | 32 | exact request digest |
| 50 | 32 | accepted Q on success, zero on refusal |
| 82 | 32 | accepted A0 on success, zero on refusal |
| 114 | 4 | conservative setup_remaining_ms, zero if expired/closed |
| 118 | 313 | reserved zero |
| 431 | 32 | reply MAC |

Status: 0 COMMITTED, 1 TERMS_MISMATCH, 2 BAD_INITIAL_BINDING, 3 CAPACITY,
4 EXPIRED, 5 CLOSED. Unknown statuses fail. Validate echoed nonce/request digest,
policy and allocation against the original hop; a success on another hop is
irrelevant. Subtract the full elapsed time since the earliest transmission of
this exact request from the returned remaining budget and clamp at zero. No
subsequent reply extends a previously learned deadline.

Commit is not settlement or paid activation. Collect all required hop commitments
before intentionally beginning round-one payments. An already-paid handshake fee
can still be lost if commitment fails; that exposure belongs in accepted setup
fees. Existing fee proof must be verified against the receiving node and consumed
at most once; copied proofs do not authorize multiple constructions. No new
handshake fee is charged by COMMIT or its retries.

## PAYMENT_START_STATUS (228)

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 1 | version=1 |
| 1 | 1 | action: 0 START, 1 QUERY |
| 2 | 16 | per-hop request nonce |
| 18 | 32 | accepted Q |
| 50 | 32 | accepted A0 |
| 82 | 8 | requested round: START requires 1; QUERY requires 0 |
| 90 | 341 | reserved zero |
| 431 | 32 | request MAC |

START requests activation of the initial allocation. Retain one immutable START
intent per committed circuit. The relay activates round one only after that
intent, the required handshake payment and round-one settlement/authorization
are all valid and durably consumed/reserved for this circuit. Whichever required
fact arrives last triggers the transition; no blocking wallet call runs on the
cell handler. Expired setup or terminal closure defeats activation even if funds
arrive later. Zero-priced obligations use explicit authorized zero-charge records,
not invented Lightning settlements. Record `T0` at the actual activation event.

QUERY is read-only and returns the latest authenticated hop state. It cannot
latch START, invoke a wallet, resend payment, change a clock or activate a round.
Use a fresh QUERY nonce for each new observation, while wire retries of the same
query retain its immutable plaintext with fresh ordinary circuit encryption.
After T0, per-round activation follows accepted deadlines and verified funding;
no new START is needed each minute. Missing required payment closes under the
existing grace without free fallback. A final round is not over when prepaid.

## PAYMENT_STARTED_STATUS (229)

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 1 | version=1 |
| 1 | 1 | state |
| 2 | 16 | echoed request nonce |
| 18 | 32 | exact request digest |
| 50 | 32 | accepted Q, zero if uncommitted |
| 82 | 32 | accepted A0, zero if uncommitted |
| 114 | 8 | current round, zero before activation |
| 122 | 8 | elapsed_ms since T0, zero before activation |
| 130 | 4 | current round_remaining_ms, zero before activation/after deadline |
| 134 | 4 | remaining time to current enforcement deadline including grace |
| 138 | 4 | setup_remaining_ms, zero after activation or expiry |
| 142 | 8 | per-hop state revision, starts at 1 and increases on transitions |
| 150 | 281 | reserved zero |
| 431 | 32 | reply MAC |

State: 0 UNCOMMITTED, 1 COMMITTED, 2 WAIT_PAYMENT, 3 ACTIVE, 4 GRACE,
5 CLOSED, 6 EXPIRED, 7 CONFLICT. WAIT_PAYMENT means START intent exists but
verified funding/fee authorization is incomplete. ACTIVE does not prove end-to-end
service delivery. GRACE is not paid capacity purchased beyond the accepted round.
Unknown state or invalid field combinations fail. CLOSED/EXPIRED are terminal.
A different START after an intent is recorded returns CONFLICT without replacing
it. A valid QUERY can observe state without altering it.

Validate request association, MAC, Q/A0 and monotonic revision. A lower revision
cannot roll state backward. For repeated identical revision, elapsed/remaining
observations may advance conservatively, but never extend learned deadlines.
Subtract full local round-trip elapsed time from remaining budgets; reject
inconsistent elapsed/round arithmetic and stop when uncertainty prevents a safe
funding decision. The relay's T0 is authoritative; controller callback receipt
is not T0. The service's SCQ first-minute anchor uses the earliest actual
service-side paid activation conservatively mapped to its local clock.

## Round boundaries and observation rules

For elapsed time `e >= 0`, the scheduled round is `floor(e/60000)+1`.
At a nominal boundary Tor advances this scheduled round, without moving T0.
If that round has a verified authorization and its allocation is admitted, enter
ACTIVE and consume only that round's authorization. Otherwise enter GRACE, with
its enforcement deadline equal to the nominal boundary plus `grace_ms`.
A verified authorization arriving before that deadline activates the remainder
of the same round; it never grants a fresh sixty seconds. At the deadline close
if still unauthorized. A late node settlement cannot reopen the circuit.
The last admitted round ends at its nominal end; if there is no admitted next
allocation, close there without treating grace as a free extra allocation.
Finite max_age/max_periods additionally bound all activity.

In ACTIVE, round_remaining_ms measures time to this round's nominal end and
current enforcement_remaining_ms measures time to that end plus grace. In GRACE,
current_round is the new unfunded scheduled round, round_remaining_ms still
measures its nominal end, but enforcement_remaining_ms measures only the grace
remaining after its start. This distinction avoids accidentally granting an
entire unpaid minute. EXPIRED/CLOSED expose zero remaining budgets; retained
current_round and elapsed values describe the final observation. Before T0,
current_round and elapsed are zero and round/enforcement remaining are zero.

For accounting, a round's latest settlement acceptance time is its nominal start
plus grace, except round one, whose initial acceptance is bounded by setup.
The advance window opens one round before the nominal start, clamped at T0.
These acceptance windows are separate from how long the already-paid round can
carry traffic. A verified decision must reach Tor before its enforcement deadline;
evidence arriving later is reconciled financially, not used to revive capacity.

The controller converts each relay observation into a conservative interval for
T0 using its local send/receive interval and authenticated elapsed value. Select
the earliest lower bound across the service's hops for the SCQ billing anchor;
never claim precision greater than the observation permits. This bounds funding
conservatively and must be disclosed with the accepted timing margin. No fresh
query moves an established anchor later. Compare remaining-time observations
only for the same identified deadline/round: the next paid round legitimately
has a different end, but retries of a round cannot extend its end.

## Retry and accounting boundary

COMMIT and START each allow at most four transmissions per hop, at 0, 1, 3 and
7 seconds after first dispatch, clipped by the original safe setup deadline.
These are test-profile candidates. QUERY allows one new observation per hop per
second, at most 16 during setup, within aggregate control budgets. Duplicate
requests do not reset counters. No retransmission is a second financial operation.
Each authenticated reply reuses the request binding but normal relay encryption
must advance; never replay captured ciphertext cells.

The privileged recipient daemon supplies Tor only verified ledger decisions bound
to its live circuit/round registration. A settled hash alone is not that decision.
The [recovery ledger contract](../control/recovery.md) governs single-use records,
uncertain outcomes, activation journaling and process loss. Tor restart closes
live circuits; no descriptor, RPC or ledger reload revives their traffic keys.

## Privacy and conformance

The existing initial payment fields remain uniform; this exchange is per-hop
and encrypted, with fixed-size semantic responses and independent nonces. It
reveals paid-profile participation to its recipient hop, not an onion-service
role. Registration, payment and activation timing remain correlatable; fixed
sizes are not an anonymity guarantee. Bound pending state and daemon verification
queues; malformed requests do not trigger wallet work. Required tests include
price substitution, A0 mismatch, reused fee/round proofs, lost replies, delayed
settlement, zero-price activation, duplicate START, read-only QUERY, expired
setup, independent hop clocks, replay/restart and contradictory authenticated
states. Independent review and paid-runtime tests remain deployment gates.
