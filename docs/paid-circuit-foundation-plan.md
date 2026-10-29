# Paid circuit foundation implementation plan

Status: active implementation planning; no foundation capability is implemented
or validated by this document. Work stays local on `paid-circuit-foundation` in
each affected repository. Paid onion-service implementation follows this work.

## Baseline

The implementation starts from the merged reliability and Tor 0.5 upgrade,
not the older revisions recorded during specification drafting:

| Repository | Starting commit |
| --- | --- |
| tor | 7f6086c31d1674ad11ba69659cda8ef715cd40e0 |
| libeltor-sys | abeed0e4af241fd8e07d28073f2da4c89236185d |
| libeltor | d5eb363404082632e186b246bce6cefedcf47c0f |
| eltord | f87b2184a854a0591d19bfc28bc6bfeb2296dbba |
| eltor-app | 87e3733729a344376376914e7afe20788064f08c |
| lni | ac6e0eeb5b9103fdc2610c313535baf639145191 |

Record actual source revisions, lockfiles, build configuration and executable
digests with each integration run. Existing sibling Cargo overrides are explicit
local build inputs; ordinary remote dependency pins do not automatically select
these sibling revisions. Preserve the committed native application lockfile.

## Dependency order and completion evidence

1. **Durable accounting and recipient evidence (ET-01, ET-05).** Introduce a
   transactional ledger with unique account/payment-hash and circuit/round/charge
   claims. Record outgoing intent before dispatch, retain ambiguous outcomes,
   and refuse new paid work on unavailable/corrupt storage. Recipient-node
   settlement, amount, time and registered identifier association must be
   independently verified. Test concurrent claims, delayed callbacks, restart,
   corruption and every dispatch/authorization crash boundary. Preserve the
   separate deferred replay investigation as supporting tracking context.
2. **Initial construction, activation and controller recovery (ET-02, ET-08,
   ET-33).** Implement the specified instance-scoped registration, immutable
   initial terms, verifier authorization and activation contracts. Test stale
   handles, changed retries, event gaps and lost replies without duplicate
   authorization or restarted deadlines. Keep per-hop identifiers independent.
3. **Renewal (ET-06–10).** Implement reviewed key derivation, canonical codecs,
   per-hop admission, prepare/status RPCs and incremental scheduling. Begin
   preparation at rounds 7, 17 and so on. Demonstrate an existing stream through
   rounds 10–11 and 20–21, partial refusal, lost acknowledgments and finite
   lifetime termination. Registering identifiers does not buy future rounds.
4. **Bounded free capacity (ET-03–05).** Advertise and admit the capability at
   every hop, enforce both-direction traffic and overhead plus aggregate relay
   state/CPU/setup/queue ceilings. Test concurrent circuits and identities,
   saturation and honest recovery. The Chutney unpaid testing exception does
   not count as implementing this allowance.
5. **Bindings, application and integrated validation.** Synchronize standalone
   and vendored C deliberately, expose required controller contracts, and make
   client status reflect actual activation and failures. Preserve wallet polling
   reliability. Run native codec/state tests and deterministic payment fault
   tests, then correlated local Chutney, desktop activation and Firefox checks.
   Label simulated wallet evidence separately from real Lightning settlement;
   do not spend real wallet funds without authorization.

The authoritative requirements remain [the implementation TODO](../TODO.md),
[initial profile](../spec/circuits/initial-profile.md),
[renewal wire](../spec/01a_circuit_renewal_wire.md),
[durable recovery](../spec/control/recovery.md), and
[resource profile](../spec/validation/resource-profile.md). Passing an isolated
module test does not complete a task until its production path is integrated.

## Initial source findings

`src/database.rs` stores independent in-memory snapshots and truncates JSON on
save; it cannot provide atomic cross-worker redemption. The received ledger
initializer can reset malformed storage to an empty array. The payment watcher
still has a handshake-verification TODO. These paths must not serve as the
authority for the new profile. Migration must not invent missing settlement
evidence or treat an existing receiving account as safe empty state.

The source reviews identified two admission gaps: durable reservation must precede
Tor's COMMITTED/ADMITTED reply, and free-only admission needs an explicit
authenticated contract distinct from paid activation. The former is now specified
in the lifecycle chapter; the latter remains an integration decision. Initial
implementation slices are the transactional ledger, strict C profile codecs and
shared free-capacity accounting. These are foundations for live integration, not
standalone completion of the goal.

Wallet review also found that generic BOLT12 search and pay-offer interfaces do
not prove exact recipient association or provide invoice-before-dispatch and
uncertain-outcome recovery. Implement capability-specific adapters; do not
advertise a payment method until its required evidence and recovery are verified.

## Privacy and timing assessment

This plan changes no wire format, timing rule or runtime behavior. Implementation
must retain uniform construction, independent hop identifiers, bounded fixed
control framing, explicit clocks and idempotent retries. Financial associations
remain local and must not appear in routine logs. Storage and overload failures
must stop new paid work without discarding unresolved accounting. Independent
cryptographic/privacy review and measurements remain distinct from local tests.

## Incremental implementation evidence

- LNI commit `38044c6` validates Phoenixd's exact incoming payment-hash response,
  explicit settlement status, checked amounts and an exact payer-note candidate
  followed by a hash lookup. Six focused tests passed, including a local HTTP
  response with a substituted payment hash. No real wallet was contacted. This
  does not establish account provisioning, fee-limited outgoing dispatch, durable
  consumption or end-to-end BOLT12 support.
- Daemon commit `45324b6` adds fixed typed admission bindings, canonical receipt
  parsing and a bounded response reader. Three focused tests passed, including
  stale/mismatched receipts, integer overflow and an oversized reply without a
  newline. No live Tor handler is enabled by adding this helper.
- The durable admission bridge documentation passes the repository documentation
  checks. Its runtime ledger/C handshake remains under implementation.
- LNI commit `b87d51b` additionally verifies the recipient settlement preimage
  against the exact payment hash. The six focused adapter tests still pass.
- Daemon commit `2cfa530` adds the receiving SQLite ledger with atomic batch
  reservation, unique settlement redemption, explicit zero-charge decisions,
  immutable activation windows and closed-instance tombstones. Twenty-one
  ledger tests and the three admission-client tests pass. Production wallet
  account binding, outgoing accounting and live controller integration remain
  incomplete.
- The new C schedule passes four focused tests for prerequisite-gated activation,
  unchanged T0 through late funding, renewal across rounds 10–11 and 20–21,
  setup/lifetime expiry, clock rollback and a delayed timer skipping an unpaid
  round. These are state-machine tests, not proof that an actual network stream
  survives renewal. Full ten-round renewal allocations must fit the accepted
  lifetime, matching the existing wire validator. Live handlers, durable
  consumption events and transport acceptance remain to be integrated.
- Tor commit `0996e6dcf0` parses and serializes exact signed payment capability
  terms, retains them in full-descriptor path metadata, and permits hard-lifetime
  truncation of an active round's enforcement interval. Thirteen profile and
  signed-descriptor tests passed independently. Runtime advertisement remains
  disabled pending complete admission and resource enforcement.
- Tor commit `f179acfb63` adds relay admission/activation/renewal state and the
  bounded privileged control registry, queries, decisions, authorization and
  events. The agent's combined suite passed 109 tests; an independent rerun of
  the six relay-state and seven control tests passed. These exercise real local
  control-to-state calls but not a live three-hop paid connection. Origin
  construction/activation, shared resource accounting and daemon orchestration
  remain outstanding. Embedded source mirrors are committed as `ac51be3` and
  `b80f0e2`; a new embedded build is still required before transport acceptance.

- The storage gate implements the resource profile's 1 GiB ledger ceiling,
  80% admission cutoff and 95% dispatch cutoff. Schema 3 reserves future write
  headroom transactionally, checks database/sidecar allocation and filesystem
  availability, and detects deletion or replacement of the live ledger file.
  Explicit migrations retain replay claims and unresolved financial records;
  ordinary reopen does not migrate or create a missing ledger. An independent
  run of all 61 ledger tests passed, including physical storage pressure,
  migration, recovery and uncertain wallet outcomes. The 256 KiB per-obligation
  reserve is a conservative test estimate requiring deployment measurements;
  it cannot guarantee space against unrelated writers on a shared filesystem.
  Live daemon orchestration and paid network acceptance remain incomplete.

- Schema 4 retains immutable uncertainty intervals for construction, COMMIT and
  activation. Explicit migration preserves old financial history, but live
  recovery refuses legacy initial windows without interval provenance. The
  registration decoder binds C/Q/A0, ordered IDs, amounts and state to saved
  reservations; observation brackets include the complete paced control exchange.
  The consumption bridge preflights all reported authorizations against durable
  recipient proofs before recording actual consumption, and replay after restart
  is idempotent. The combined ledger suite passed 66 tests and the payment RPC
  suite passed 34 tests, including registration sockets, clock discontinuities,
  mismatched consumption proofs and closed bindings. These are local integration
  tests; the production coordinator and live paid-network acceptance are pending.

- Schema 5 adds a durable, bounded native invoice-discovery cursor and candidate
  mapping, per-circuit closure tombstones, and an origin control-operation
  journal. Candidate hashes remain lookup hints: the relay must still obtain
  exact native recipient evidence before authorization. COMMIT and START retain
  their original operation identities; a claimed dispatch survives restart and
  cannot be claimed again by a second controller. Read-only recovery does not
  authorize payment. Origin records reserve conservative future-write headroom
  under the same ledger storage ceiling, and closure prevents later dispatch.
  Explicit migration preserves existing obligations and replay records; normal
  reopen never upgrades a database. The combined ledger suite passed 75 tests,
  including wrong-account migration, closed-circuit dispatch, changed operation
  identity, and corruption checks. These local tests do not establish live
  activation, renewal, or real Lightning settlement acceptance. The live relay
  coordinator and origin orchestration remain in progress.

- The origin control client now saves exact operation arguments and claims its
  dispatch in the durable journal before sending. Reopening after a lost receipt
  queries the original operation, validates its kind and (for COMMIT) policy
  digest, and never automatically sends a replacement START. An unknown remote
  operation remains unresolved. Four origin RPC tests pass, including a real
  local socket disconnect followed by database reopen and read-only recovery,
  substituted operation kind, and unknown-operation responses. A recovered
  VERIFIED operation still does not imply ACTIVE or authorize wallet spending;
  full-path commitment and live client orchestration remain outstanding.

### Explicit foundation relay startup configuration

The integration path checks authenticated `GETCONF ElTorFoundation` before
starting a payment watcher. Only an explicit zero value with no foundation
configuration selects the legacy path. Enabled mode, an unknown response, or
incomplete foundation setup cannot silently fall back. Reconnection verifies the
mode again before recovering the new control connection.

For the isolated foundation test deployment, `ELTOR_FOUNDATION_CONFIG` names a
private JSON file with exactly these required fields:

- `ledger_path`: an existing authoritative ledger; relative paths resolve beside
  the configuration file, not against the daemon's working directory.
- `node_id` and `chain`: the pinned native CLN compressed public key and one of
  `bitcoin`, `testnet`, `regtest`, or `signet`. The adapter verifies the account.
- `cln_url` and `cln_rune`: native read-only receiving RPC configuration. The
  adapter still enforces HTTPS or numeric loopback HTTP and required capabilities.
- `clock_uncertainty_ms`: an explicitly supplied conservative UTC error bound,
  at most 5000 ms. Choosing a number does not establish clock accuracy; the
  deployment must support that bound, and observation brackets independently
  reject discontinuities.

The file is limited to 8 KiB, rejects unknown fields, and on Unix must be private
(no group/other permission bits), regular, and opened without following a
symlink. Its values are not logged. Startup only reopens the ledger: it never
provisions, migrates, replaces, or resets it. Operator provisioning and final
acceptance configuration remain to be integrated. The supervisor bounds polling
and reconnect backoff and retains durable accounting across control loss.
This remains an integration path, not evidence of completed paid acceptance.

### Client startup integration checkpoint

The client now requires an authenticated `GETCONF ElTorFoundation` result before
loading a legacy wallet. Only an explicit disabled profile, with no foundation
configuration requested, reaches the legacy loop. Unknown replies, lost control
connections, and conflicting configuration stop wallet activity. The enabled
profile enters the durable origin coordinator only with an explicit private
client plan and an existing account ledger. This startup wiring is still under
integration test; it is not evidence of completed live activation.
The socket regression covers disabled/enabled/unknown/rejected modes and checks
that mode selection sends no activation or wallet command. Constructor and
signed-descriptor preflight helpers are tested, but live construction, recovery,
renewal, and end-to-end acceptance remain required.

The private `ELTOR_FOUNDATION_CLIENT_PLAN` JSON file selects one path with
`scope` (nonzero 32-byte lowercase hex), ordered unique `fingerprints` (20-byte
lowercase hex), and integer `lifetime_msat`, `outstanding_msat`, `per_round_msat`,
and `routing_fee_per_hop_msat` caps. The per-round cap includes relay prices and
routing allowances. It must not exceed the outstanding cap, which must not
exceed the lifetime cap. Zero caps allow only zero-cost work. The file is bounded
to 8 KiB, rejects unknown fields and symlinks, and requires private Unix
permissions. Account credentials remain in `ELTOR_FOUNDATION_CONFIG`.

Startup reads signed relay prices before construction, persists the approved
path and original clock, and claims construction once on the same authenticated
control connection. Recovery reuses the retained scope, signed prices, payment
identifiers and limits. Changed plan settings are refused. A claimed construction
without an accepted plan remains uncertain and cannot trigger another build.
This initial integration supports zero handshake fees and up to the initial ten
rounds; nonzero handshake funding and renewal orchestration remain required.

Executed checkpoint: the combined client suite passes 16 tests, including
positive-price native HTTP fixtures, all-hop commitment, loss of payment and
START replies, retained constructor ownership, and private-plan validation.
The startup entry point compiles inside its spawned asynchronous task. A
client-plan file alone also counts as foundation intent, so a missing account
configuration cannot redirect that requested path into legacy payments.
These tests do not establish live Tor activation or real Lightning settlement.

Privacy/timing assessment: the selected path, wallet references, identifiers and
scope remain in private local files and ledger records, not diagnostics. Recovery
does not requote or generate replacement identifiers. Reconnect backoff is
bounded and never extends the accepted payment window. Live traffic timing and
resource measurements remain part of the acceptance work.
