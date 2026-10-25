# Consistency review and remaining decisions

This review covers the reorganized Markdown and source-backed control inventory.
It is not an independent cryptographic audit or runtime conformance test.

## Resolved documentation conflicts

| Earlier wording | Resolution |
| --- | --- |
| TOPS post-round payment and stronger privacy guarantees | Retained only as historical material; current rounds, exposure and privacy requirements are authoritative. |
| Watcher uses callback time and deletes ledger rows on closure | Historical sketch; current accounting requires actual settlement, durable single-use binding and reconciliation. Runtime remediation remains deferred. |
| Kill every circuit after ten IDs or immediately on final payment | Allocation-based renewal is explicit; receipt of a payment does not end its round. |
| Original paid service diagram bypasses SCQ/authenticated return path | Moved to historical material; current bootstrap/acceptance/readiness chapters define the sequence. |
| `EXTEND_PAID_CIRCUIT` event in prose | Control inventory records the observed subscription and `EVENT_PAYMENT_ID_HASH_RECEIVED` event token separately. |
| Every setup timeout is terminal | No-READY timeout may enter bounded WAIT_READY_RECOVERY with new spending stopped. Explicit failure/closure and recovery expiry are terminal. |
| Retry the “exact READY” | Same logical signed authorization, fresh encrypted delivery as required; no repeated ciphertext or reused AEAD nonce. |
| Fixed ten-minute path versus ongoing downloads | Ten-round allocations can renew only with negotiated support and accepted finite lifetime limits. Backup circuits do not migrate streams. |

## Current review status

Concrete proposals now cover initial commitment, lifecycle/recovery, metadata,
transport, acceptance/invoices, paid introduction, service build, READY, continuation
and refunds. The [gate matrix](gates.md) separates those designs from remaining
validation. The cross-chapter audit and implementation handoff below record proposed-design
completion; no tested-runtime or deployment-ready claim is made.

Public [service codec fixtures](../vectors/service_codec_v1.json) cover eight
representative record encodings, domain-separated Ed25519 examples and four
framing sizes. The checker reproduces exact bytes/digests/signatures. It does not
exercise native wallets, runtime parsers, hs-ntor or HPKE, and the listed negative
tests remain implementation acceptance requirements.

## Readiness and spending invariant

FUNDED precedes paid construction. Billing starts at first paid relay activation.
Client connectivity requires its local Tor handshake. READY accepted on the same
live rendezvous path confirms service setup state. At the setup deadline without
READY, new spending stops; recovery uses only existing funded time. Valid READY
can complete a live recovering attempt but never reset billing or revive closure.
READY does not authorize new spending beyond the accepted round and budget policy.

## Privacy/timing assessment

Reorganization introduces no runtime wire behavior. Stable links replace positional
cross-references; proposed commands remain distinct from source-observed APIs.
Historical privacy claims are isolated. Existing requirements for independent hop
IDs, encrypted authorization, uniform padding, bounded retries and optional receiver
privacy remain. Timing correlation and privileged logging remain explicit risks.
Documentation coverage/link/render checks and their limitations are recorded in
the maintenance workflow; runtime, crypto and correlated paid tests stay deferred.

## Documentation verification for this rewrite

On 2026-10-25, the Markdown checker covered all 43 listed book chapters plus
repository entry points and the deferred backlog. mdBook 0.4.43 built locally;
the rendered checker covered 46 HTML pages with no broken local file or fragment
references. The sidebar was inspected in the local browser, including independent
El Tor numbering and the Control/RPC hierarchy.

The initial quote wire tables and fenced encoding examples were compared with
the pre-reorganization source and retained unchanged. The renewal wire document
and synthetic vector file were unchanged. The former technical companion's
sections were checked against their new destinations; obsolete walkthroughs are
retained as historical content. The readiness table now explicitly includes the
already-agreed nonterminal recovery state. Source-backed RPC documentation was
checked against the revisions named in its introduction.

These checks establish document structure, retained layouts and local navigation.
They do not establish runtime payment correctness, independently reviewed crypto,
current wallet interoperability or measured privacy. Those gates remain explicit.

## Version-one cross-chapter design audit

The following is a document-level state/authority trace, not a runtime test.
It reviews the concrete profiles introduced after the book reorganization.

| Trace | Required sequence and stopping condition | Authoritative contracts |
| --- | --- | --- |
| Successful setup | Verified metadata/capabilities → padded quote → client RP established → immutable SETUP_BIND → acceptance → exact invoice → node settlement/reservation → one BUILD → all-hop commitment/activation → one JOIN → local native handshake → path-bound READY → durable ACK. No earlier stage means delivered service. | metadata, transport, payment-messages, paid-introduction, service-build, initial-profile, readiness-wire |
| Refusal before payment | Unknown version/method/reference, malformed input, budget/path refusal or missing setup binding yields non-payable refusal/drop. A zero-price quote requires offered status and explicit zero-charge records, not zeroed refusal fields. | version, metadata, wire, payment-messages |
| Lost quote/acceptance reply | Fresh outer delivery keys/nonces/token, stable logical inquiry/core/operation, bounded attempts. Exact acceptance recovers the existing invoice; changed invoice/native request does not create a replacement. | transport, payment-messages |
| Lost build or JOIN reply | Query the original Tor instance/operation. Identical resubmission cannot launch another path or native join. UNKNOWN after local tombstone loss is not proof of no side effect. | service-build, recovery |
| Setup timeout | Original FUNDED deadline expires → no new spend. Retain only already-funded live path for bounded READY recovery; no paid grace subsidy. Expired/closed attempt cannot revive after late funds or READY. | timing, readiness-wire |
| Lost READY ACK | Client has verified actual handshake; service commits READY before ACK. Duplicate READY is bound to the same path/challenge and does not buy anything. Bounded status recovery can observe persisted READY; it cannot fake local handshake completion. | readiness-wire, status |
| Daemon restart | Durable ledger plus same Tor instance/handle permits queries; recover original deadlines/counters. If they cannot be recovered conservatively, stop new spend instead of granting fresh time. | recovery, lifecycle, service-build |
| Tor restart | New instance invalidates live handles and native secrets. Old attempt closes; retain unresolved financial state. No ledger reload reconstructs old traffic keys or silently pays for replacement. | recovery, service-build |
| Partial renewal | Each half privately prepares its own hops; immutable SCQ extension requires all service-hop admission and client-side checks. One refusal stops future-period funding, not existing purchased service. Payments remain incremental; no six-hop atomic-payment guarantee. | renewal control/wire, continuation |
| Late/unknown settlement | Keep full reservation until authoritative outcome. Actual recipient-node time and registration window govern authorization, not callback time. Closure prevents activation; accounting/refund remains separate. | recovery, payment-messages, refunds |
| Cancellation/refund | Atomic no-new-spend/terminal intent races READY under one attempt record. Only reconciled unallocated principal may fund a signed refund invoice; pending payouts retain their reservation. | readiness-wire, refunds |
| Overload | Shared cell/byte/CPU/memory/storage caps refuse admission before resource growth; no automatic payment, retry-counter reset or deletion of unresolved records. Existing Tor limits also apply. | transport, resource-profile, recovery |

### Conflicts reconciled during this audit

- Registration initially described a round's payment window as ending after the
  whole purchased minute. The initial profile and registration snapshot now agree:
  later rounds must be funded by their nominal start plus grace; round one uses
  its original setup window. Purchased service duration is a different deadline.
- A full paid introduction/cookie/hs-ntor association was missing between quote and
  build. SETUP_BIND now precedes invoice issuance; native parse/verification cannot
  invoke an automatic unpaid rendezvous launch.
- An existing construction RPC alone was insufficient for lost-reply recovery.
  Service setup/build/query/join contracts now bind one durable operation to one
  process-local path and forbid automatic paid relaunch.
- Policy-1 refunds lacked concrete transport and fee reconciliation. The refund
  profile now requires explicit receiving capability, client-signed destination,
  one unresolved payout and disjoint accounting categories.
- General controller text prohibited proof inputs even though construction must
  carry the preserved handshake proof. The service-build command documents that
  privileged input exception explicitly, without permitting proof output/logging.
- Old overview paragraphs still called schemas, opcodes and lifetime choices open.
  Their authoritative linked profiles now define them; independent review and
  runtime measurements remain gates rather than missing wire definitions.

### Observer and privacy audit

A guard/middle receives its own payment fields and sees timing/volume, not a shared
SCQ/attempt identifier. The introduction endpoint knows local client/service circuit
associations, the quote-versus-control framing class and slot timing. The service
knows the accepted client authorization, RP, quote and wallet association. Wallet
counterparties observe native payment data; blinding is optional and implementation-
checked. HSDirs store authenticated encrypted descriptors and can observe size,
publication cadence and blinded storage keys. Local controllers/ledgers hold
sensitive circuit/payment mappings and must restrict access/redact diagnostics.

Per-hop identifiers remain independent; stable endpoint identities stay inside
protected payloads. Fresh outer encryption/token/key material prevents intentional
ciphertext reuse across retries; logical state remains stable to prevent charges
from duplicating. Fixed request/reply shapes cover success/refusal for each profile,
but the initial quote and larger control profile are distinguishable. BOLT11 and
BOLT12 native wallet traffic, missing replies, key trials, congestion and timing
still differ. Jitter, padding and a common scheduler are not a proof against
colluding relays or an observer watching both ends. Longer paths-in-use and periodic
renewal/draining may add correlation opportunities; finite ages do not remove them.
These limitations are recorded rather than converted into anonymity guarantees.

### Remaining review and deployment gates

No unassigned El Tor command, unspecified message capacity or intentionally
unbounded deadline is relied on by the traced proposed flow. Exact runtime behavior
must still be independently checked against these requirements. The ntor exporter,
HPKE/library validation, native handshake integration, wallet adapters and parser
implementations require independent review and conformance suites in the handoff.
Resource/lifetime/timing numbers remain test candidates; honest success rates and
privacy effects require measurements. Failure of those tests requires an explicit
spec revision, not an assertion that this draft proved safety or performance.

## Local handoff and integration preflight

The final local preflight reviewed the handoff, design checklist, deployment gates
and remaining-work language across the overview and technical chapters. Stale
statements calling already specified controller, lifecycle, continuation and READY
contracts unfinished were replaced with authoritative links. Historical chapters
received whitespace cleanup only; their superseded behavior remains nonnormative.

Executed checks:

- Markdown coverage: 62 files, 59 book chapters and 33 deferred implementation IDs.
- Service codec fixtures: eight representative records, digest/signature examples
  and four framing examples reproduced; no runtime or HPKE interoperability test.
- Local mdBook build and all 62 rendered pages: no broken local references.
- Full diff against local master: documentation/specification and book tooling only,
  no runtime source changes; whitespace checks pass, including historical chapters.
- All branch commits inspected use Eltor Dev as both author and committer with the
  repository-local development email. No new tracked absolute home paths were found.
- Local master `8963f4c1bfd711f7138db21b90718abf7895205b` is an ancestor of the
  specification branch, so that local baseline requires no conflict resolution.
  No remote fetch, push, merge or publication was performed; remote divergence
  has not been evaluated. Disabled push configuration and the rejecting hook remain.

This is a prepared proposed-specification handoff, not independent protocol signoff
or deployment approval. Before any separately authorized publication/integration,
recheck the target revision and diff, rerun the document checks and review the
remaining implementation/validation gates. The local-only workspace rule remains
in force. The implementation backlog stays unchecked.

**Privacy/timing assessment:** these corrections change navigation, status wording
and historical whitespace only. Wire bytes, state transitions, payment authority,
privacy assumptions, timing and retry policies are unchanged. No new runtime
requirement or corresponding implementation task is introduced.
