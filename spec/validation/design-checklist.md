# Protocol design checklist

Use this checklist for our next planning discussions, in order. It tracks design
completion; the [implementation backlog](../../TODO.md) separately tracks code
and tests. All boxes begin unchecked because these detailed contracts still need
completion. Existing architecture decisions remain in force.

**Current work: item 1 has a [concrete controller draft](../control/renewal.md); finalize its dependencies through items 2–4 before checking it complete. Next: item 2, initial terms and round activation.**
Discuss one item at a time. Check it only when its decisions, authoritative spec
section, failure cases and deferred implementation tasks are recorded. If an
item depends on an unresolved later contract, record that dependency and leave
its finalization open rather than inventing a default.

Before finalizing each item, reconcile it with the [existing-code comparison](source-comparison.md), including the reliability branch and vendored Tor differences. Source review is not executed conformance testing.

## Controller foundations

- [ ] **1. Renewal preparation and status query.** Define what eltord submits to
  Tor for each hop's next batch, what acceptance means, and how eltord recovers
  the result after a lost reply. Specify caller authorization, circuit/allocation
  binding, idempotency and errors. Registration must not buy rounds or rebuild
  the circuit. Start from the existing renewal wire draft. **Tasks: ET-06–08, ET-33.**
- [ ] **2. Initial relay terms and round activation.** Define how accepted prices,
  initial payment identifiers, capabilities and the round-start event are
  authenticated and retained. Distinguish settlement, reservation, dispatch and
  actual relay activation; keep hop clocks independent. This is a prerequisite
  for finalizing renewal's initial state. **Tasks: ET-01–02, ET-08, ET-15, ET-30.**
- [ ] **3. Payment-state queries and lifecycle events.** Define snapshots and
  events for round activation, batch acceptance/refusal, expiry and closure.
  Include revisions, duplicate/out-of-order delivery and query-after-disconnect
  behavior. A query must have no payment or construction side effects.
  **Tasks: ET-08–09, ET-29–30, ET-33.**
- [ ] **4. Restart, replay and uncertain outcomes.** Specify durable state,
  single-use payment authorization, clock recovery, retention and reconciliation
  after wallet/controller timeouts or crashes. Reconcile the first three items
  against these rules; do not blindly retry paid work. Preserve the separate
  deferred replay investigation. **Tasks: ET-01, ET-08–09, ET-16, ET-29–33.**

## Complete the paid onion-service contracts

- [ ] **5. Descriptor, delegation and catalog records.** Finish canonical
  encodings for capabilities, request-encryption/signing-key delegations,
  revocation, payment policies, receivers and acquisition references. Define
  validity, size bounds and strict version/feature handling.
  **Tasks: ET-13–14, ET-19, ET-24–27.**
- [ ] **6. Quote transport allocation and framing.** Allocate and verify El Tor
  command values, routing/slot semantics and parser errors against the existing
  fixed request/reply layouts. Retain uniform padded sizes and bounded reply
  slots; distinguish these relay messages from local RPC commands.
  **Tasks: ET-13, ET-23, ET-26.**
- [ ] **7. Acceptance and per-round invoices.** Finish exact acceptance/invoice
  messages and authenticated bindings for both BOLT 11 and BOLT 12. Specify
  recoverable invoice lookup, replacement of unresolved invoices and budget
  checks without duplicate payment. **Tasks: ET-14–16, ET-18, ET-27–28.**
- [ ] **8. Build status, READY and failure accounting.** Finish control message
  formats and actual-rendezvous binding; reconcile setup, funded recovery,
  READY/ACK loss, terminal closure, cancellation and refund-policy reporting.
  No readiness report is permission to exceed the accepted budget.
  **Tasks: ET-16, ET-29–33.**
- [ ] **9. Two-half continuation and lifetime policy.** Finalize SCQ continuation,
  all-hop admission and independent half schedules. Decide finite circuit age,
  renewal pricing and free-only renewal behavior without silently changing
  accepted prices or migrating existing streams. **Tasks: ET-10, ET-14, ET-17.**

## Bounds, review and implementation handoff

- [ ] **10. Resource and timing profiles.** Define measurable budgets for free
  traffic, descriptor publication/retrieval, introduction availability, pending
  circuits/slots, queues, crypto work, retries and clock uncertainty. Record a
  measurement plan for candidate 60-second setup, 15-second recovery and three
  READY retries; do not label candidates validated defaults.
  **Tasks: ET-03–05, ET-11–12, ET-21, ET-23, ET-31–32.**
- [ ] **11. Whole-protocol consistency and privacy review.** Trace complete
  success, refusal, timeout, restart and renewal flows across the message/state
  definitions. Check identity exposure, fresh encryption on retries, visible
  sizes, timing correlation, bounded resource use and immutable accepted terms.
  Record independent cryptographic review requirements and remaining questions.
  **Tasks: ET-06, ET-21–27, ET-33.**
- [ ] **12. Version-one implementation handoff.** Pin the Tor/El Tor source
  baseline and complete required capability profile; assemble canonical test
  vectors, parser/state tests, wallet interoperability and correlated paid
  Chutney/privacy test plans. Include app consent, budget and failure displays.
  Mark the profile implementation-ready only after unresolved interoperability
  decisions are closed. Implementation and executed validation require their
  own work; this checklist does not claim either is done.
  **Tasks: ET-18–22 and the preceding dependency tasks.**

## Working rule

For each item, record: agreed behavior; inputs and outputs; state transitions;
authentication and spending authority; timeout/retry/restart handling;
privacy/timing and resource assessment; authoritative spec links; and matching
implementation acceptance checks. Proposed RPC names become final only when
specified; do not imply they already exist in the runtime.

**Privacy/timing assessment:** this checklist organizes existing deferred design
work and changes no wire behavior. It preserves the separate review of identity,
linkability, padded sizes, deadlines and aggregate limits for every new contract.
