# Recoverable service setup and build control

**Proposed local control contract; not implemented.** These commands connect the
service's durable SCQ ledger to Tor's native handshake state and one paid path.
Use the authenticated privileged control authority and canonical text/error
rules from [renewal control](renewal.md). This is not a public app API.

## Register native setup without building

```text
ELTOR_SERVICE_SETUP 1 instance operation service core circuit_auth client_key intro_key rp introduce_hex quote_expires_at
ELTOR_SERVICE_SETUP_GET 1 instance operation
```

`instance` and `operation` are 16-byte hex; all remaining identifiers before
introduce_hex are 32-byte hex. `service` is the configured onion identity;
`core` is the SCQ core digest; `circuit_auth` is the accepted circuit authorization;
`client_key` is its signing key; `intro_key` is the native introduction authentication
key; `rp` is the agreed Ed25519 relay identity. `introduce_hex` encodes the complete
native payload, 1–2037 bytes. The privileged daemon validates the outer SETUP_BIND
signature/quote first. Tor independently validates native introduction cryptography,
configured service ownership, retained intro keys/subcredential and RP consistency.
A controller assertion does not bypass the native handshake parser.

Compute setup_digest using the exact binary payload and domain from
[paid introduction](../onion-services/paid-introduction.md). Store native secrets
inside Tor, not in the reply or daemon log. This operation reserves bounded setup
state but launches no circuit, issues no invoice and sends no rendezvous join.
The setup registry shares the service's 128-record/4096-byte-per-record limits.
Its expiry is the retained quote expiry supplied by the trusted daemon after
verifying the SCQ; SETUP cannot extend it. `quote_expires_at` is u64 decimal Unix seconds. It is part of the immutable arguments, must be within 150 seconds of current time. Tor checks the bound and immutable
replay arguments; the daemon is responsible for matching it to the signed SCQ.

Successful registration/query returns:

```text
250 ELTOR_SERVICE_SETUP 1 instance operation setup_handle setup_digest state
```

`setup_handle` is a fresh nonreused 16-byte process-local handle; setup_digest is
32-byte hex; state is REGISTERED, CLAIMED or TERMINAL. Same operation and exact
arguments returns its retained result; changed arguments return OPERATION_CONFLICT.
A second operation for the same service/core/circuit_auth must recover the same
binding if identical, otherwise conflict. GET is read-only and never revalidates
into a new binding or recreates expired secrets. Unknown returns UNKNOWN_OPERATION.
An expired unclaimed binding returns DEADLINE_EXPIRED, never an implicit rebuild.

The quote/client/cap authorization assertions in SETUP are trusted local input
from the one privileged SCQ daemon, not arbitrary clients. No additional implicit
registration RPC or externally supplied trust context is required. A malicious
authorized controller is outside this local isolation model.
Tor must still reject a service identity absent from its local service config.

## Authorize one path

Use a multiline Tor control command with exactly three path rows:

```text
+ELTOR_SERVICE_BUILD 1 instance operation setup_handle attempt reservation max_cost_msat setup_remaining_ms
fingerprint terms initial_payment_payload
fingerprint terms initial_payment_payload
fingerprint terms initial_payment_payload
.
```

`operation` is a fresh 16-byte durable local ID; `attempt` and `reservation` are
32-byte opaque IDs from the accepted agreement and ledger. `max_cost_msat` is u64;
`setup_remaining_ms` is u32 in 1–60000. Each fingerprint is a 20-byte lowercase
hex relay identity resolved through verified directory information; terms is the
55-byte policy encoded in lowercase hex. The last row must resolve to the exact
registered RP identity. Each initial_payment_payload is 384 bytes, encoded as
768 lowercase hex characters: handshake hash32, preimage32 and ten IDs32 in order.
This preserves the existing paid construction fields, including fresh dummy
zero-fee material. No extra rows, blank rows, trailing tokens or partial-row
acceptance are permitted. Total command size is capped at 8192 bytes.

This sensitive construction input is the same proof material required by the
existing paid build format. It is an explicit exception to the no-preimage-input
rule for the other new control commands: it travels only over authenticated local
control, is never echoed in replies/events/logs, and must be redacted from command
traces. Wallet credentials and exporter keys remain prohibited. Do not confuse
this local secret input with permission to publish proof material in metadata.

Before BUILD, the daemon must have durably verified and reserved round-one/setup
funds, selected the exact path and checked charges/fees against SCQ and reservation
caps. Tor trusts that local financial authority, but checks service/setup ownership,
path length/RP, immutable terms/IDs, duplicate operation and deadline. Tor performs
no wallet calls. The daemon sends the conservative remaining original setup budget,
subtracting elapsed time and clock margin; Tor anchors a monotonic deadline at
first acceptance and never changes it on duplicate RPCs. No client-supplied number
can grant a new sixty seconds after FUNDED.

Atomically claim the setup_handle for this attempt and record the complete build
operation before launching the one origin circuit. Allocate its circuit handle
before dispatch so GET can recover it even if construction fails immediately.
Return only:

```text
250 ELTOR_SERVICE_BUILD_QUEUED 1 instance operation attempt revision
```

QUEUED is not BUILT, paid activation or joined rendezvous. A lost reply is resolved
by exact operation query/resubmission; changed arguments or another operation
claiming the same setup/attempt/reservation conflicts. Wire retries do not select
another path, reuse a reservation on another circuit, pay another handshake fee
or reset the deadline. Version one permits one path launch; automatic native
rendezvous relaunch/fallback must be disabled for this authorized attempt.

## Query, activation and join

```text
ELTOR_SERVICE_BUILD_GET 1 instance operation
ELTOR_SERVICE_JOIN 1 instance operation
ELTOR_SERVICE_CLOSE 1 instance operation
```

GET returns a coherent local snapshot:

```text
250-ELTOR_SERVICE_BUILD 1 instance operation attempt revision
250-BINDING setup_digest reservation circuit_handle
250-STATE state reason
250-TIME setup_remaining_ms paid_remaining_ms
250 OK
```

Circuit handle is 16-byte hex or `-` if never allocated. Durations are conservative
u64 milliseconds, or `-` when not known. States: QUEUED, BUILDING, BUILT,
WAIT_FUNDING, JOIN_SENT, READY, CLOSING, CLOSED, FAILED. Reason: NONE,
PATH_FAILURE, DEADLINE, PAYMENT, CANCELLED, PROCESS_LOSS, PROTOCOL or CAPACITY.
BUILDING means Tor path construction, not permission for application traffic.
BUILT is native path completion. WAIT_FUNDING means the initial paid-hop commitments
or funding/activation observations remain incomplete. JOIN_SENT is not proof that
the client received the native rendezvous response. READY requires the path-bound
client proof defined in the readiness wire chapter. GET has no wire/wallet effects.

The daemon runs the existing proposed initial per-hop COMMIT/START control on the
returned circuit handle, pays within the accepted reservation and records its
first authenticated activation. JOIN is allowed only once all required hops have
verified paid activation, the same path is still live and the original setup
budget remains positive. It sends native RENDEZVOUS1 using the retained cookie and
handshake context exactly once, then generates the attempt's READY challenge.
The immutable join intent is recorded before send. Duplicate JOIN returns current
state and never retransmits the native join or constructs another circuit.
An uncertain native send is not a new send authorization. A failed join is terminal
for this attempt; reconcile rather than automatically rebuild.

JOIN success reply is `250 ELTOR_SERVICE_JOIN 1 instance operation revision`.
A missing funding/activation fact returns `551 INITIAL_STATE_UNCONFIRMED`; expired
budget returns DEADLINE_EXPIRED. CLOSE records terminal no-new-join/spend intent
for this operation and closes only its existing circuit, returning
`250 ELTOR_SERVICE_CLOSE 1 instance operation revision`. Repeated CLOSE is harmless;
it does not refund funds, stop an already dispatched Lightning payment or reopen
an expired path. The daemon independently persists cancellation/accounting.

## Recovery and limits

Store each operation until its circuit ends and its daemon has recovered the
result; terminal local tombstones last 60 seconds, with 4096 process-wide maximum,
sharing the controller cache bound. Financial records live longer in the ledger.
UNKNOWN_OPERATION after tombstone expiry is uncertainty, not proof of no launch.
On daemon restart, query the original instance/operation and retained ledger;
never generate another operation for the same reservation. On Tor restart, all
setup handles and native secrets are gone, the instance changes and old operations
are rejected. Mark the old attempt terminal and reconcile, without reconstructing
its path or clocks from the ledger. A replacement requires a new approved SCQ.

At most 128 pending service builds per process, sharing the service setup and
control-operation ceilings. GET shares the existing 64/s authority query budget.
All mutating operations validate full framing, ownership and budget before any
side effect. Use the standard error tokens plus `553 WRONG_SERVICE`,
`553 BAD_PATH` and `552 UNKNOWN_SETUP`; errors never partially accept path rows.
No service build command grants wallet permission independently of its reservation.

## Privacy and conformance

The daemon and local Tor necessarily know the private service path; public status
must not expose it. Do not log native introduction bytes, cookies, handshake proofs,
private paths or reservation mappings. Local opaque handles never appear in relay
payment fields. Lost replies and restart recovery must not produce recognizable
bursts of new paid builds. This contract uses existing controller authentication,
not a claim that all control clients are mutually isolated.

ET-08/16/29/30/33 require exact duplicate BUILD after lost reply, changed-row
conflicts, malformed third row with no partial launch, reused reservation, wrong
RP, JOIN before activation, native join uncertainty, deadline replay, lost GET,
tombstone expiry and Tor/daemon restart tests. Compare against the inherited
service launch/retry code and retain the reliability fixes. Documentation defines
these requirements; no controller implementation or paid build was executed.
