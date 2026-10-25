# Renewal controller protocol, version 1

**Proposed interface; not implemented.** This chapter specifies local Tor control
commands used by eltord. The existing [renewal relay wire protocol](../01a_circuit_renewal_wire.md)
remains authoritative for encrypted per-hop messages. These RPC names are El Tor
extensions, not upstream Tor command assignments.

## Authority and encoding

Use an authenticated local Tor control connection. This profile treats authorized
control peers as one privileged daemon authority, as Tor control access already
permits circuit management; it does not promise per-wallet multi-tenant isolation.
The deployment MUST restrict control access to that authority. Wallet credentials,
preimages and exporter/MAC keys are never arguments or outputs of the commands
in this chapter. The separate service-build construction command documents its
required private proof input explicitly; no command logs or echoes those proofs.

Commands are ASCII, one line terminated by CRLF, with one space between positional
arguments. No optional positional arguments, quoting, duplicate keys or extra
arguments are accepted for these commands. Hex fields use lowercase hexadecimal
with exactly twice their binary width; decimal integers have no sign or leading
zeros except zero itself. Reject overflows, unsupported versions and malformed
fields before mutation. Limit command lines to 16384 bytes including CRLF.
Sizes and quotas here are proposed conformance limits, requiring implementation
tests, not a claim about the existing parser.

## Circuit binding

```text
ELTOR_CIRCUIT_BIND 1 CircuitID
```

This read-only command resolves a current live origin circuit. Tor assigns each
process a fresh random 16-byte `instance` and each origin circuit a fresh random
16-byte `handle` at creation. These are local controller identifiers, never sent
to transport relays or used as Lightning payment IDs. Handles are not recycled
within an instance. `CircuitID` uses Tor's existing origin-circuit identifier and
may be reused by Tor; all later mutation commands MUST use `instance` and `handle`.

Success is exactly:

```text
250 ELTOR_CIRCUIT 1 instance handle hop_count revision
```

`hop_count` is u16; `revision` is u64, initially one and incremented for every
externally observable payment-state change. Repeating BIND does not change state,
revision, lifetime or authority. A closed/unknown circuit returns `552 UNKNOWN_CIRCUIT`.
Binding itself does not establish paid-profile or renewal support. The daemon
must compare the returned handle against its retained circuit ownership/context;
after losing an old instance, it cannot rebind a reused numeric ID and treat it
as recovery of the old circuit.

## Prepare one hop

```text
ELTOR_RENEW_PREPARE 1 instance handle hop operation period first_round nonce terms parent ids
```

| Field | Type | Meaning |
| --- | --- | --- |
| `instance`, `handle` | 16-byte hex each | Exact local process and live origin-circuit instance |
| `hop` | u16 decimal | One-based position on the fixed existing path |
| `operation` | 16-byte hex | Fresh local operation ID, retained for retries; never sent to a relay |
| `period` | u64 decimal | Next ten-round allocation, starting at one |
| `first_round` | u64 decimal | Exactly `10 * period + 1`, checked for overflow |
| `nonce` | 16-byte hex | Fresh independent nonce for this hop/allocation |
| `terms` | 32-byte hex | Accepted renewal terms digest Q |
| `parent` | 32-byte hex | Previous admitted allocation digest |
| `ids` | 320-byte hex | Ten ordered, nonzero, distinct per-hop payment IDs |

Tor validates the live handle, hop, negotiated paid profile, retained initial
terms/start commitment, sequence, parent, identifier uniqueness and local budget
before accepting an operation. Tor derives activation/round fields and generates
the relay request/MAC using its own per-hop state. The daemon cannot inject keys,
MACs, raw relay requests, a new path or a new start clock through this API.
If the initial commitment is absent, return `551 INITIAL_STATE_UNCONFIRMED`;
never infer it from an arbitrary ledger row or controller assertion.

The local operation identity is `(instance, handle, hop, operation)`. Store the
canonical arguments and resulting immutable relay request before first dispatch.
An exact duplicate returns the same local operation and current revision without
another immediate dispatch. Different arguments for that identity return
`553 OPERATION_CONFLICT`. A different operation for a hop/period already queued
or admitted also returns conflict, even if its payment IDs happen to match.
Do not let a new operation ID reset retry counters or pending-allocation limits.

Success is exactly:

```text
250 ELTOR_RENEW_QUEUED 1 instance handle hop operation revision
```

This is local acceptance only. It is not relay admission, payment settlement,
allocation activation, or authority to pay the next period. Tor owns bounded
wire retransmission under the relay protocol (at most four transmissions per
hop/period, with the original timing/deadline rules); repeated RPC calls cannot
add transmissions. The daemon queries state after a missing reply rather than
creating another operation. A controller connection closing does not cancel the
stored operation or recreate its deadline.

## Query a hop and operation

```text
ELTOR_PAYMENT_GET 1 instance handle hop operation
```

`operation` is a 16-byte hex ID, or the literal `-` to request a hop snapshot
without selecting an operation. GET MUST NOT dispatch, retry, register IDs,
activate capacity, change revisions or invoke a wallet. It returns a coherent
snapshot under one circuit revision. An unknown operation returns
`552 UNKNOWN_OPERATION`; it is not evidence that a payment failed. The daemon
may resubmit its exact persisted PREPARE only while the same live instance,
handle and original admission deadline remain valid.

Success uses Tor multiline reply framing, with these lines in this order:

```text
250-ELTOR_PAYMENT 1 instance handle hop revision
250-PROFILE terms active_period active_allocation current_round
250-SCHEDULE round_remaining_ms admission_remaining_ms hard_age_remaining_ms
250-OP operation state period request_digest allocation_digest relay_status transmissions
250 OK
```

All counters/rounds/remaining durations are u64 decimal except `transmissions`
(u8) and `relay_status` (u8). Digests are 32-byte hex. The literal `-` represents
an unavailable/not-applicable value, never numeric zero. For GET with `operation=-`,
the entire OP line after its prefix is `- - - - - - -`. An unconfirmed initial
profile returns `551 INITIAL_STATE_UNCONFIRMED` instead of invented timing.

`state` is exactly QUEUED, SENT, ADMITTED, ACTIVE, REFUSED, UNCERTAIN or CLOSED.
`request_digest` becomes available when the immutable authenticated request is
formed; `allocation_digest` is present only after validated success. `relay_status`
is present only for an authenticated relay result and uses the relay-wire enum.
QUEUED/SENT are local progress. ADMITTED/ACTIVE require a fully validated matching
relay reply. UNCERTAIN means transmission may have succeeded but timely admission
cannot be established; it cannot authorize next-period spending. REFUSED retains
the verified refusal. CLOSED means the live circuit is gone and no further work
is possible. A later valid reply can refine an uncertain observation only while
the same circuit remains live; it cannot restore an elapsed funding deadline.

Durations are conservative local remaining budgets at snapshot generation,
clamped at zero. Account for the relay elapsed-time bound using the earliest
transmission of the exact request as specified by the relay wire protocol.
The controller subtracts the full local GET round-trip time and its configured
safety margin before scheduling work. Later snapshots cannot extend previously
learned absolute deadlines. Use `-` when no safe bound exists, and stop new
spending that requires that bound. Do not export relay wall clocks or pretend
all hops share one schedule.

## Failure, restart and bounded state

| Code and token | Meaning; mutation policy |
| --- | --- |
| `512 BAD_ARGUMENT` | Framing/width/enum/arithmetic failure; no mutation |
| `551 INITIAL_STATE_UNCONFIRMED` | Initial paid contract/start is unavailable; no renewal |
| `551 UNSUPPORTED_PROFILE` | Required hop capability absent; no fallback |
| `552 UNKNOWN_CIRCUIT` | Handle not live; never bind by a replacement numeric ID |
| `552 STALE_INSTANCE` | Different Tor process; old circuits cannot be restored |
| `552 UNKNOWN_OPERATION` | No retained operation under this live binding |
| `553 OPERATION_CONFLICT` | Existing operation/period has different ownership or arguments |
| `553 BAD_SEQUENCE` | Wrong next period, parent, terms or reused IDs; no replacement |
| `551 DEADLINE_EXPIRED` | Insufficient safe admission time; no new dispatch |
| `451 CAPACITY` | Admission budget exhausted; no partial acceptance or automatic retry |

Parse the numeric code and exact token, not free-form diagnostic text. V1 adds no
optional diagnostics to these replies. Errors do not change an already accepted
operation. The first recorded valid result wins; conflicting authenticated replies
require stopping future funding and reporting protocol failure, not overwriting
state with a more favorable answer.

Retain active and pending per-hop operations while the circuit is live. On close,
keep local status tombstones for 60 seconds with a process-wide ceiling of 4096
entries; evict oldest terminal controller tombstones on overflow. GET may then
return UNKNOWN_CIRCUIT. These caches are not the payment/replay ledger, which
must retain unresolved financial commitments separately. Max 16 hops per command
profile, one pending allocation per hop, and 4096 queued/in-flight renewal
operations per process; reject admission before exceeding these limits.

The daemon persists the exact intended arguments before PREPARE and records
responses/queries. A daemon restart reconnects and queries the same instance/
handle/operation. A Tor restart generates a new instance and loses live circuits;
do not restore traffic keys, live streams or timers from accounting records.
Old commitments remain for wallet reconciliation. A new circuit requires its own
explicit authorization. No cancellation RPC is needed for an unused batch:
registration does not purchase capacity and expires under existing round rules.

## Privacy and validation

These IDs and status digests are privileged local data; never put them in public
headers, wallet notes or routine logs. Tor retains exporter keys and each relay
sees only its own independent request. Local polling can affect timing/load:
limit GET to one request per hop per second and 64 requests per second per control
authority, with no queue accumulation on excess; return `451 CAPACITY`.
Independent review and measurements must assess these proposed resource limits.

Conformance cases include exact duplicate after lost QUEUED reply; conflict with
changed IDs; stale process/handle; numeric circuit-ID reuse; a GET that causes no
wire or wallet traffic; partial all-hop admission; delayed/replayed relay reply;
expired admission budget; daemon versus Tor restart; tombstone eviction with
financial records intact; line/hex overflow; and no key material in output.
These are specification acceptance tests, not tests executed against runtime.
Implementation remains deferred under ET-08/09/33. Initial terms and activation are specified in the [initial wire profile](../circuits/initial-profile.md);
[initial operations and lifecycle events](lifecycle.md) and
[durable recovery](recovery.md) define the related controller contracts.
Whole-profile clock, parser and restart conformance remains part of final review.
