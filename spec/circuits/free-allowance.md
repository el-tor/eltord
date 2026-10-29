> **Proposed protocol; implementation deferred.**

# Shared circuit construction and bounded free allowance

**Draft extension:** All El Tor circuits use the same construction and per-hop payment payload below, including circuits used only within the bounded free allowance. This section adds a protocol proposal; it does not claim that bandwidth quotas or free-resource enforcement are already implemented. Paid service retains one-minute rounds. The [renewal draft](renewal.md#circuit-renewal-draft) makes the initial ten-round allocation renewable on the same paid circuit when every hop agrees; without negotiated renewal, the existing ten-minute expiration remains in effect.

### Uniform per-hop payment payload

For each hop, preserve the handshake payment hash/preimage fields and ten distinct 32-byte round payment identifiers in the existing `EXTENDPAIDCIRCUIT` construction. Generate independent identifiers for each hop, round, and new circuit, including when no real payment is expected for the free allowance. Preserve per-hop encryption and equal-size padding. A relay decrypts only its own payment information.

Where no handshake fee applies, populate the existing handshake fields with fresh, correctly sized dummy hash/preimage values instead of omitting them. Where the agreed paid handshake fee applies, use its actual proof and verify it against the recipient's settled invoice records. A relay's advertised paid rate and paid handshake fee remain under operator control; a new relay may advertise a zero-sat handshake fee as before.

The free allowance MUST NOT use a shorter payload, missing payment fields, shared dummy constants, or a new circuit-purpose header identifying an onion service. It is available based on resource use, not an origin's claim to be a service. Ordinary clients and onion services construct their circuits the same way. Standard endpoint-specific Tor commands such as `ESTABLISH_INTRO` and descriptor publication remain necessary at their destinations; they do not require disclosing that role in the guard's or middle relay's payment payload.

### Small free capacity on ordinary circuits

Supporting relays advertise a small, bounded free traffic allowance alongside their normal paid rate. It is purpose-independent and may support low-volume communication, descriptor publication/retrieval, and introduction availability without recurring Lightning fees within those bounds. Every hop on the selected path must support and admit the allowance. The ordinary construction is shared; free usage is not a different onion-service circuit format.

Eligible setup and maintenance within the free allowance do not require a paid-service handshake charge. The preexisting handshake-fee and bandwidth-testing logic for paid service remains unchanged; this proposal does not introduce a separate paid-activation fee or promise a full-speed trial from the tiny allowance. A zero handshake fee alone does not advertise free recurring capacity, and the presence of dummy fields does not prove that a required payment was made.

Limits MUST bound per-circuit sustained/burst traffic in both directions, total relay-wide free traffic, circuit creation, concurrent/pending circuits, CPU/memory, queued work, and buffering. Include normal protocol maintenance and padding in the accounting. Many circuits or fresh identities MUST NOT multiply free resources beyond the relay's aggregate budget. Excess use is throttled or rejected with bounded queues/retries; it MUST NOT silently charge the origin. Numeric rates, bursts, and resource budgets remain configurable draft work to be selected through measurement.

Paid bandwidth beyond the allowance requires the agreed handshake and recurring payments. A circuit with a required paid round MUST close if that payment misses its bounded deadline/grace; it MUST NOT continue by falling back to free capacity. An independently admitted circuit using only the free allowance remains subject to the same small limits and aggregate budget. A dummy handshake proof or unused round identifier never counts as settled funds and cannot authorize paid capacity. Paid-round enforcement and free-resource accounting must therefore be explicit, without using a purpose-specific construction.

### Authenticated base-resource admission

The foundation profile uses one purpose-independent encrypted ntor-v3 extension,
`ELTOR_RESOURCE_PROFILE`, for **every** foundation circuit, whether it remains
free-only or later requests paid activation. It does not encode a paid/free or
onion-service selector. Origins require the signed `eltor-free-capacity`
capability from the [resource profile](../validation/resource-profile.md) at each
hop before requesting it; advertising support does not itself reserve capacity.

The local fork allocates extension field type **224 (0xe0)** in ntor-v3's
`trn_extension` namespace. This is separate from relay-command numbering and is
not an upstream allocation. The inspected source uses types 1 and 2 for congestion
control and 3 for SUBPROTO; recheck conflicts on upstream integration. The field
has length **2**, body **`01 01`** (version 1, resource profile 1). Request and
successful response use the identical field bytes **`e0 02 01 01`** exactly once
inside their normal counted extension containers. Profile 1 pins the advertised
2-cell/second, 128-cell burst, 600000 ms free lifetime candidate and the shared
aggregate accounting requirements. Existing congestion-control/SUBPROTO fields
keep their existing semantics; this extension does not enable the incompatible
small-cell format.

Append the existing canonical per-hop payment payload after the request's
extension container as before. Preserve all original hash/preimage fields and
ten independent identifiers, including fresh dummy fields where no fee applies.
The new extension and acknowledgement have the same shape for all foundation
uses. Duplicate fields, wrong length, unsupported version/profile, unsolicited
acknowledgements, malformed payment suffixes, and missing or changed required
acknowledgements fail the foundation handshake. Parse the exact extension length
and suffix boundaries rather than scanning arbitrary ciphertext/plaintext for a
marker. Successful ntor authentication binds the acknowledgement; client-side
exposure of the hop occurs only after authentication and exact acknowledgement
validation. Existing exporter transcript binding includes this exchange.

Before queueing handshake work, the relay main loop reserves provisional
aggregate and actual-link setup, CPU, pending-circuit and memory capacity. Because
the request is encrypted, this pre-work reservation cannot depend on trusting an
unverified requested profile. Pass an immutable reservation eligibility flag into
the worker; workers do not access or refill the shared resource owner. The worker
may return the acknowledgement only for a valid request with that reservation.
Before sending CREATED, the main loop must transfer the reservation into the
live admitted context; allocation failure, cancellation, stale completion or
handshake failure releases reservations exactly once after their actual work
ends. A stale worker completion cannot access freed circuit/context memory.
Admission/attempt rate charges are not refunded by early failure or reconnect.

Base admission does not require a positive paid-handshake charge and does not
validate dummy proof as payment. The acknowledged circuit begins within the
bounded allowance, with the original ten-minute free lifetime. Paid COMMIT,
recipient-backed verification of any required actual handshake proof, and paid
round activation remain separate. The immutable original dummy proof cannot be
replaced through this exchange to acquire paid authority. Once paid obligations
are accepted, their failure/expiry closes the circuit rather than reverting to
free service. No missing acknowledgement, unsupported profile or resource
exhaustion silently falls back to unrestricted legacy traffic or charges a wallet.

**Privacy/timing assessment:** all foundation uses share the same encrypted
extension and payment fields; the acknowledgement introduces no service identity,
cross-hop identifier or purpose bit. It can distinguish this protocol version
from legacy peers, not prove that all traffic patterns are indistinguishable.
Admission rejection and worker delay remain observable under load. Keep existing
padding, use aggregate bounded work, and measure successful/refused handshake
sizes and timing. Tests must cover exact request/ack bytes, authenticated
substitution, duplicate/malformed fields, suffix boundaries, missing ACK,
reservation failure/cancellation and worker completion races, plus paid/free
payload-shape parity. This contract is a proposed implementation target, not
completed live admission or anonymity validation.

### Use cases and rationale

The purpose-independent allowance supports the following use cases within the same ordinary circuit construction. These are reasons for providing the allowance, not new circuit-purpose flags or promises of unlimited free traffic.

| Use case | Why a small free allowance is useful | Boundary |
| --- | --- | --- |
| Onion-service descriptor publication and refresh | A service can upload its contact information to responsible HSDirs before receiving any customer payment. It can publish replicas and refresh changed introduction information without a continuous discovery fee. | Upload traffic, setup, and retries must fit the path allowance; HSDir storage, document size, validation, and update rates have separate bounds in spec two. |
| Client descriptor retrieval | A client can learn introduction points, accepted payment methods, and contact information before deciding whether to pay the service. | Bound complete descriptor downloads, response bursts, repeated requests, and misses. The receiver must still authenticate and verify the descriptor. |
| Service-to-introduction-point availability | The service keeps its waiting-room circuits available to receive introductions without buying full-bandwidth rounds while waiting for customers. | Count idle circuits, maintenance, bounded padding, incoming introductions, and replacement overlap against the same per-circuit and relay-wide budgets. |
| Client introduction and bounded rendezvous setup signaling | Clients and services can exchange permitted introduction/control messages and prepare a rendezvous using low-volume transport where the allowance is sufficient. | This does not waive agreed paid setup/handshake charges or provide paid rendezvous bandwidth. The client must fund any paid work on the service side before that work is incurred. Standard Tor endpoint authorization and circuit-purpose rules still apply. |
| Learning payment terms or obtaining a payment request | Where a method uses Tor transport, a bounded bootstrap exchange can provide a quote or fresh invoice before a paid data connection is established. | The quote/control chapters define authenticated bootstrap messages. BOLT 12 invoice-request onion messages use Lightning's own protocol; the Tor allowance is not a blanket subsidy for wallet traffic or Lightning routing fees. |
| Ordinary low-volume communication and circuit maintenance | Any origin can use the same small capacity for permitted low-volume messages, acknowledgments, maintenance, or idle-period padding. The relay need not identify an onion service to grant it. | Other small uses are intentionally permitted; enforce aggregate setup, state, queue, and traffic limits across circuits and identities. |

Together these uses keep basic discoverability and introduction availability affordable, avoid requiring an already paid connection merely to learn how to pay, and reuse the same encrypted/padded construction for ordinary clients and services. Users need not disclose a service identity to intermediate relays to obtain the allowance. Compatible paths must still admit free traffic; resource exhaustion can cause bounded retries or temporary unavailability, not unexpected wallet spending.

The free allowance is not a replacement for the paid-circuit bandwidth test described below. A small allowance may demonstrate basic reachability but does not establish full paid throughput. Preserve the existing operator-selected handshake fee, including zero-sat handshakes for new relays; the allowance does not introduce a separate paid-activation fee. Paid-capacity commitments and recurring paid rounds retain their own payment and closure rules.

Validate these use cases when sizing the allowance: complete descriptor transfers and copies, refresh/replacement bursts, legitimate introduction/control traffic, normal maintenance/padding, and combinations of many small ordinary circuits. Numeric limits must accommodate measured legitimate workloads while keeping the relay's aggregate contribution bounded. No free allowance implementation or measured throughput is claimed by listing these use cases.

### Shared lifetime and specification relationship

The baseline ten-minute maximum applies to free and paid circuits without negotiated renewal. The renewal draft below proposes extending an established paid circuit in additional ten-round allocations, without replacing its path or active streams. Version one replaces free-only circuits at ten minutes rather than renewing them in place, as defined by the continuation profile. Early closure remains valid for failure, poor performance, loss of required payment, user choice, or enforced resource limits. Idle periods are also normal. See [spec two](../02_paid_hidden_service.md) for the corresponding onion-service lifecycle.

`BANDWIDTH_QUOTA` below describes a separate proposed quota for paid bandwidth; its legacy zero/unlimited value MUST NOT be interpreted as an unlimited free allowance. The bounded free allowance is also distinct from the initial bandwidth-testing window on a paid circuit. Capability signaling, exact accounting/acceptance rules, and the interactions with payment watchers remain implementation work. Validate identical construction/padding, aggregate bounds, legitimate free discovery, and correct paid-circuit closure before claiming support.

## Candidate accounting profile

The [resource profile](../validation/resource-profile.md) defines the proposed
capability, cell/byte buckets, aggregate ceilings and measurement plan. These
parameters make the allowance testable; they do not establish runtime support
or imply that many small free circuits cannot be abused.
