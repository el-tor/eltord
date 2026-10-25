# Quote transport command profile, version 1

**Proposed El Tor extension; not implemented.** This chapter assigns commands and
state transitions to the [fixed quote envelopes](wire.md). It does not change
Tor's existing INTRODUCE/RENDEZVOUS command meanings or turn INTRODUCE_ACK into
a service-authenticated quote. Later invoice/status records use their own profile;
they cannot be smuggled into these fixed initial quote records.

## Capabilities and command allocation

An eligible introduction relay includes exactly this line in its signed relay
descriptor:

```text
eltor-quote-transport 1 230 231 232 233
```

Clients and services verify the full descriptor and exact profile before using
this transport. A hint from an unverified microdescriptor or an error message
is insufficient. The service establishes its normal introduction circuit and
registration at a supporting relay; the registration's existing Ed25519 intro
authentication key selects that circuit. No new stable wallet/service identity
is exposed. The service's authenticated metadata advertises transport_profile=1
only for introduction points supporting this extension. An incompatible point
is ineligible, not grounds for ordinary-Tor fallback or paid bootstrap.

| Command | Value | Direction and purpose |
| --- | ---: | --- |
| QUOTE_REQUEST | 230 | Client origin to introduction relay |
| QUOTE_DELIVER | 231 | Introduction relay to registered service origin |
| QUOTE_RETURN | 232 | Service origin to introduction relay |
| QUOTE_REPLY | 233 | Introduction relay to requesting client origin |

All use ordinary RELAY, StreamID zero, exact Length 498, the existing 64-byte
fragment header and 434-byte slice. No RELAY_EARLY or wire ACK is added. Values
230–233 are local draft assignments, absent from standalone Tor revision
`2f05d04`'s `src/core/or/or.h`; they are not upstream reservations. Recheck the
actual embedded source at implementation time. Commands 224–229 remain assigned
to payment renewal/initial commitment and must not be reused.

The command determines the allowed local role, direction, header kind and route.
A REQUEST/DELIVER has kind 1, count 1, index 0. RETURN/REPLY has kind 2, count 2,
indices 0 then 1. Nonzero stream IDs, wrong command/direction/length/count,
unknown framing versions, nonzero reserved bytes and illegal route fields are
invalid. Authenticate/decrypt ordinary Tor relay transport before parsing.
Never trust the unencrypted interpretation of a ciphertext as a routing header.
The terminating introduction relay still cannot decrypt the end-to-end envelope.

## Admission and forwarding

For REQUEST, the route is the 32-byte intro authentication key. Resolve it to the
exact current registered service circuit instance. The client circuit and service
circuit must both be live and eligible within their existing lifetimes. Reserve
state and byte budgets before forwarding. The request carries a fresh 16-byte
client-leg token and no reply-slot handle. Allocate independent unpredictable
16-byte service-leg token and slot handle, rejecting collisions. Slots store:

- Exact client/service circuit object generations and intro registration generation.
- Both leg tokens, slot handle and immutable first-admission deadline.
- Expected reply fragment index and fixed byte budget, plus whether forwarding began.

Rewrite the route to `slot_handle16 || zero16` and token to the service-leg token,
then forward the unchanged 434-byte encrypted request as DELIVER. This routing
rewrite does not alter the signed delivery nonce or logical request inside HPKE.
The service records only its local slot context while processing the authenticated
request; it never treats an intro-supplied slot as client payment authorization.

A service RETURN must arrive on the exact mapped service circuit, with the same
slot handle/zero suffix and service-leg token. The introduction relay rewrites
the route to zero32 and token to the mapped client-leg token, forwarding slices
as REPLY in order. There is no caller-supplied destination circuit in RETURN.
The client only accepts REPLY on its active delivery's original circuit/token.
It waits for both fragments, then verifies HPKE, signature and all inner bindings
before exposing a quote. No partial response permits acceptance or payment.

## Slot state machine

| State | Input | Transition |
| --- | --- | --- |
| ABSENT | Valid admitted REQUEST | Reserve one slot, forward DELIVER, WAIT_REPLY_0 |
| WAIT_REPLY_0 | Matching RETURN index 0 | Forward REPLY index 0, WAIT_REPLY_1 |
| WAIT_REPLY_1 | Matching RETURN index 1 | Forward REPLY index 1, terminal COMPLETE |
| Any live state | Expiry, mapped circuit loss, registration replacement | Terminal ABORTED |
| Any live state | Conflicting duplicate, wrong index or excess fragment for that slot | Terminal ABORTED |
| COMPLETE/ABORTED | Any fragment for the retired slot | Drop; never recreate |

Exact duplicate REQUEST with the same circuit/token is ignored, not forwarded a
second time. A changed payload under that token aborts its slot. Exact repeated
RETURN fragments are ignored if their bytes match the already observed fragment;
a changed duplicate aborts. This requires retaining at most two 434-byte reply
slices until terminal cleanup. Ignore only duplicates for a known slot; do not
allocate state for arbitrary unknown tokens. Ordinary Tor transport is ordered;
this is replay handling, not permission to reorder fragment delivery.

After terminal completion retain a bounded tombstone keyed by the original
client circuit generation/token and service slot/token until the original expiry.
It prevents duplicate REQUEST from recreating a slot or replayed RETURN from
routing to a recycled circuit. Admission reserves tombstone capacity up front;
when full, refuse new slots rather than evict unexpired entries to admit more.
Circuit objects may be released while their opaque generations remain in the
bounded tombstone. Circuit IDs or memory addresses alone are not generation IDs.

A wrong-circuit RETURN for a guessed slot is dropped without aborting the real
slot; otherwise an unrelated circuit could kill another client's inquiry.
Authenticate the mapped leg before applying conflicting-fragment abort rules.
When a slot has expired, a valid late service response cannot extend it. A new
attempt requires the client's bounded retry policy and fresh keys/ciphertext.

## Candidate bounds and timing

These version-one test-profile values make the prototype contract finite. They
need load/latency measurements before being called production defaults.

| Resource or timer | Limit |
| --- | ---: |
| Slot absolute lifetime from accepted REQUEST | 30 seconds |
| Live plus unexpired terminal slots per client circuit | 2 |
| Live plus unexpired terminal slots per service registration | 128 |
| Live plus unexpired terminal slots per relay process | 4096 |
| New slot admissions per process | Token bucket 128/s, burst 256 |
| New slot admissions per OR connection | Token bucket 16/s, burst 32 |
| Slot retained state including buffers | 4096 bytes maximum |
| Service concurrently admitted quote computations | 128 |
| Service new quote computations | Token bucket 32/s, burst 64 |
| Service scheduling window after complete authenticated request | Uniform 2000–4000 ms |
| Client attempt timeout from first REQUEST dispatch | 40 seconds |
| Client backoff after failed attempt | Uniform 1000–3000 ms |
| Attempts across all points/refreshes for one inquiry | 3 total |
| Overall inquiry lifetime from first dispatch | 150 seconds maximum |

Use cryptographically suitable local randomness with integer milliseconds chosen
uniformly over the inclusive bounds. The service chooses its target at admission,
independent of whether the result is cached, new or refused. If work cannot finish
by the target, send a same-size authenticated capacity refusal at the target only
if it is ready; otherwise drop the attempt. Do not send a late success or choose
a new target based on outcome. Enqueue both reply fragments consecutively at the
target under normal congestion control; do not introduce method-dependent pacing.
If the second cannot be queued within the bounded local queue, abort transport.
No timer overrules slot expiry, quote expiry, circuit lifetime or a resource cap.

Network delay can consume the relay slot before service admission. The service
does not know the relay's exact remaining slot time, and the relay may drop an
otherwise valid delayed response. That is a bounded failure, not a reason to
share wall clocks, extend expiry or promise delivery. The 40-second client wait
is deliberately longer than the relay's slot lifetime; it is not proof of a
maximum Internet delay. No partial fragment, forwarding error or cached result
resets any timer. An invalid/unauthenticated relay error never triggers an early
retry: wait until the normal attempt deadline then use the local backoff.

The client transmits only if a full attempt fits its remaining inquiry deadline
and metadata/key validity. At most one local attempt is active. Preserve logical
request bytes, original expiry and isolation/guard policy across retries, while
replacing delivery nonce, reply key, HPKE encapsulation and leg token. A terminal
authenticated refusal ends the inquiry except the existing bounded stale-metadata
case; a refresh consumes the same remaining attempts/time. A changed capability
snapshot requires a new explicit inquiry, not silent reuse of an old approval.

Budgets count every forwarded copy and the real fork cell size, not only the
498-byte logical payload. Reserve at most one forwarded request and two reply
fragments per slot at the introduction relay; failed sends do not create more
allowance. Per-path free bandwidth, OR-connection queues, ordinary Tor congestion
control and global CPU/memory budgets remain additional constraints. Admission
is refused before exceeding any bound. No payment is automatically made to buy
extra negotiation capacity. Shared global caps matter because an attacker can
create many service identities or client circuits.

## Failure and privacy assessment

Malformed, unadmitted and over-budget traffic is silently dropped under bounded
abuse handling; no transport-generated response claims to be a service refusal.
No arbitrary redirect, wallet instruction, payment hash or user-readable error
is returned by the introduction relay. The client eventually reports bounded
transport failure. A timeout does not prove that an earlier financial operation
failed; later protocols must reconcile their durable state separately.

An introduction relay knows the local client-to-service registration association.
The extension exposes that a quote exchange occurred at that endpoint, even
though both methods/refusals have the same sizes. A malicious relay can delay,
drop and correlate. Shared timing windows reduce some cache clues but do not
prove anonymity, and overload/drop behavior may still differ. No shared stable
logical inquiry, wallet or circuit-payment identifier is exposed in routing.

ET-13/21/23/26 require command/direction/parser vectors, slot generation reuse,
wrong-circuit injection, conflicting duplicates, missing second fragments,
terminal tombstone exhaustion, fixed lengths, service overruns, queue starvation,
sequential cross-point retries and all-four-leg accounting. Compare fresh/cached/
refused timing distributions under honest and adversarial load. Document review
and source opcode checks are performed here; runtime tests remain deferred.
