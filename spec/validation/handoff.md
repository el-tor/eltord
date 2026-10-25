# Proposed version-one implementation handoff

This is a specification handoff, not a runtime implementation or deployment
approval. The twelve design workstreams have concrete chapters; the [cross-chapter audit](review.md#version-one-cross-chapter-design-audit)
records design signoff. Preserve local reliability fixes and
implement only under a separate authorization. No live payments are implied.

## Source baseline and integration order

Use the exact revisions in [source comparison](source-comparison.md), including
eltord `18935e2f9b4bd7b297620f538e6186520b234117`, standalone Tor
`2f05d0452d0cfc1393e8d7d2e0dbf781e5230e33`, vendored libeltor-sys
`543241cda3b12fff3833b6b8b52cc997a494ac4d`, and LNI
`90dd176b6c5cf1052842fbe574e9320aedf2ebe4`. These are inspected source baselines,
not proof of the current executable's embedded revision. The spec branch was
based on master and must not silently replace the reliability branch at build time.
Record commit IDs, local changes, dependency lockfiles, build flags and executable
digests for each implementation test. Preserve the standalone/vendored testing-only
unpaid bootstrap distinction; do not confuse it with bounded-free conformance.

Implement in dependency order: durable ledger and verifier mappings; strict
construction/initial activation and control recovery; renewal; free resource
accounting; metadata/crypto/transport; acceptance and native setup; service build
and READY; continuation/refund/UI; then correlated paid validation. Implementing
a later layer must not bypass an unfinished earlier authorization gate. Keep
legacy observed RPC behavior distinct from the new capability-gated profile.

## Required capability set

A version-one paid service is enabled only when all used components support:

1. Verified signed relay descriptors with bounded-free, initial paid commitment
   and renewal capability lines, and matching immutable terms on every selected
   paid hop. Renewal is required for a service advertising feature bit 0.
2. At each selected introduction endpoint, quote commands 230–233 and service
   control commands 234–237, their exact framing and aggregate resource profile.
3. Service metadata version 1, canonical capability/catalog records, fixed HPKE
   suite and separate delegated signature/request keys, with exact version match.
4. Native v3 introduction/rendezvous validation plus immutable paid SETUP_BIND,
   recoverable single-build control, and end-to-end READY commands 238–240.
5. At least the selected wallet method's full invoice acquisition, native
   validation, recipient settlement lookup and recoverable dispatch capabilities.
   BOLT12 pay-offer-only APIs do not satisfy invoice-before-payment validation.
6. Durable unique consumption/reservation and uncertain-outcome accounting,
   atomic attempt state, finite lifetime/clock/resource policies and strict parsing.

Advertise only methods actually tested by that integration. Do not send a full
client wallet/version fingerprint, silently downgrade, switch payment method,
fall back to ordinary Tor, or interpret missing capability as free paid service.
The catalog's policy-1 refund additionally requires the disclosed BOLT11 receive
capability even when the original method is BOLT12. This is explicit policy consent,
not a fallback. Keep receiver blinding optional and capability-checked.

## Design evidence by checklist item

| Item | Authoritative design | Implementation acceptance evidence required |
| --- | --- | --- |
| 1 | [Renewal control](../control/renewal.md) | Duplicate PREPARE, lost reply, query with no side effects, instance/handle reuse |
| 2 | [Initial wire](../circuits/initial-profile.md), [lifecycle](../control/lifecycle.md) | Exact terms/A0, independent activation clocks, zero-price and paid admission |
| 3 | [Lifecycle](../control/lifecycle.md), [service build](../control/service-build.md) | Event gaps, snapshot races, coherent revisions, no event-driven double dispatch |
| 4 | [Recovery](../control/recovery.md) | Atomic single-use claims, actual node timestamps, every crash boundary and unknown outcome |
| 5 | [Metadata](../onion-services/metadata.md) | Canonical catalog/key records, rollback/expiry, bounded parsing, exact hash references |
| 6 | [Transport](../onion-services/transport.md), [control framing](../onion-services/payment-messages.md) | Direction/slot isolation, tombstone exhaustion, fixed shapes, complete fragment accounting |
| 7 | [Acceptance wire](../onion-services/payment-messages.md), [paid introduction](../onion-services/paid-introduction.md) | Exact native invoice binding, one unresolved invoice, setup-before-invoice, no premature spend |
| 8 | [Readiness](../onion-services/readiness-wire.md), [refunds](../onion-services/refunds.md) | Actual-path READY, cancellation races, bounded recovery, accounting conservation |
| 9 | [Continuation](../onion-services/continuation.md) | Two-half clocks, partial admission, immutable prices/caps, draining and finite age |
| 10 | [Resource profile](resource-profile.md) | Real cell/queue/CPU/storage counters, honest success under hostile concurrency |
| 11 | [Review](review.md) | Cross-layer success/failure traces and independent privacy/crypto review |
| 12 | This handoff, [source comparison](source-comparison.md), [vectors](../vectors/service_codec_v1.json) | Independently implemented codecs, native vectors, wallet matrix and reproducible paid test report |

## Conformance test groups

These are required future executable suites, not assertions that runtime tests ran.

- **V1-CODEC:** reproduce public renewal and service fixtures in independent C/Rust
  implementations; compare exact field offsets, endian encodings, digest domains,
  signatures, padding and fragment counts. Apply truncated/oversize/extra-field,
  unknown-enum, wrong-direction/domain, nonzero-padding and integer-overflow cases.
  Neither peer may parse one record as another, accept a refusal as zero-price
  service or enlarge envelopes based on method. Exercise every allocated command.
- **V1-CRYPTO:** run upstream RFC 9180 HPKE, RFC 8032 Ed25519 and RFC 5869 HKDF
  known-answer vectors plus protocol binding vectors. Independently review the
  ntor-v3 exporter proposal and hs-ntor reuse. Test swapped keys, invalid points,
  all-zero X25519 output, signature malleability handling, replay, key rotation,
  revocation suppression/rollback and stale caches. Fail before side effects.
- **V1-LEDGER:** crash before/after intent, dispatch, settlement, reservation,
  Tor authorization, activation and refund. Race two workers on the same invoice
  and on different invoices for one round. Restart daemon versus Tor separately.
  Preserve UNKNOWN and spent/reserved conservation; old settled payment cannot
  authorize a fresh circuit. Ledger corruption/pressure never permits empty-state
  paid admission. Compare authoritative node times with delayed callbacks.
- **V1-SETUP:** trace descriptor→quote→client RP→SETUP_BIND→acceptance→invoice→
  settlement→FUNDED→one BUILD→per-hop activation→one JOIN→native client handshake→
  challenge/READY/ACK. Drop each reply in turn. Substitute cookie/RP/native key,
  send valid READY on another path, expire at every boundary and cancel concurrently.
  Assert zero paid build before reserve, at most one attempt/path and immutable T0.
- **V1-RENEW:** long transfer through 10→11 and 20→21 with skewed hop schedules,
  one-hop refusal, partial funding, unknown payment, lost acknowledgment, cap/age
  exhaustion and stream draining. Already-paid traffic is not cut merely because
  future renewal was refused; failed paths are not magically migrated.
- **V1-WALLET:** each supported BOLT11/BOLT12 adapter must expose invoice creation/
  lookup, verified exact amount/hash/chain/issuer association, settled state/time,
  durable dispatch identity and fee limits. Test expired/incorrect invoices,
  unsupported required features, duplicate offer requests, transient errors and
  unknown outcomes. For policy 1, test refund receiving and fee remainders. Record
  actual node/wallet versions; do not infer support from interface field names.
- **V1-LOAD-PRIVACY:** use resource-profile workloads to measure full fork-cell
  traffic, burst aggregation, incomplete slots, key churn, crypto failure, stale
  metadata and ledger pressure. Compare fresh/cached/refused sizes/timing and
  ordinary low-volume versus service circuits. Respect guard/isolation behavior;
  do not claim resistance to a global observer from fixed padding or jitter.

## App and operator handoff

Eltor-app must show both own-path and SCQ costs, one-time versus recurring fees,
current-plus-next exposure, finite maximum age/rounds and total/outstanding caps.
Expose offered/accepted/invoice/pending/funded/building/locally-connected/READY
as distinct states. Do not display connected from a payment callback or a service
JOIN_SENT report. Explain unknown payments without an automatic retry-payment
button that changes operation identity. Stop renewal, cancel and request eligible
refund are distinct actions with their original reservations preserved.

Show renewal refusal and predictable hard-age termination before a long transfer
is cut; describe backups as paths for new streams, not seamless live migration.
Display unsupported wallet/capability failures before payment. Public service
status omits private relay identities and local handles; diagnostic correlation
uses opt-in local bounded logs without preimages or exporter/handshake secrets.

## Validation performed and release gates

The documentation tools check chapter coverage, local references and rendered
links. The service fixture tool verifies representative deterministic bytes,
digests, Ed25519 examples and fragment slicing. Renewal fixtures use public
synthetic exporter inputs, not live ntor handshakes. These are useful specification
evidence, but do not cover every runtime parser/state transition, wallet semantics,
HPKE interoperability or measured privacy. Negative cases listed in fixtures are
requirements for implementation tests, not tests executed by listing them.

Do not enable or label the protocol deployable until independent cryptographic
review, C/Rust byte agreement, durable-state fault injection, wallet matrix and
correlated paid Chutney/load reports meet the stated invariants. Free-mode browsing
and the earlier reliability tests cannot certify these new paid contracts.
All ET tasks remain unchecked implementation work. This handoff changes no source
runtime, sends no payments and authorizes no publication or remote push.

**Privacy/timing assessment:** this artifact organizes existing requirements and
does not add wire behavior. It preserves explicit observer limits and separates
source, document-fixture, independent-review and runtime measurement evidence.
