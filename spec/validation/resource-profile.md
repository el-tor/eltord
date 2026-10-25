# Candidate resource and timing profile

**Version-one test profile, not measured production defaults.** This chapter
makes resource_profile=1 finite and testable. It supplies common ceilings without
claiming that every relay can afford them or every wallet completes within them.
Operators may configure smaller aggregate budgets and reject admission; they must
not secretly enlarge a peer's accepted deadlines or automatically charge for
excess free traffic. Existing Tor congestion, padding, isolation and circuit DoS
controls remain additional constraints.

## Purpose-independent free allowance

A supporting relay advertises this exact capability in its signed descriptor:

```text
eltor-free-capacity 1 2 128 600000
```

Fields are version, charged fixed cells/second, per-circuit burst in charged cells,
and maximum free-only lifetime in milliseconds. Version one requires these exact
per-circuit numbers; smaller aggregate capacity is allowed through refusal rather
than a false advertisement. Origins require support at every hop. All circuits
retain the shared per-hop payment payload; no hidden-service-purpose bit is added.

A token bucket starts with 128 tokens at circuit admission, refills continuously
at two tokens/second using monotonic elapsed time, and saturates at 128. Charge
one token for each fixed cell received or sent for that circuit, in either
direction, including padding and protocol-control cells. A forwarded cell normally
costs one ingress and one egress token. Debit atomically before the action; do not
count a cell as free merely because it was dropped after consuming parse/CPU work.
Admission/setup DoS budgets additionally cover invalid cells before a circuit exists.
Connection-wide maintenance uses the separate OR-connection budget, not a fabricated
per-circuit attribution. TLS/IP framing and variable-cell traffic count in the
aggregate byte budget below even when there is no fixed-cell token charge.

For a cell already received without sufficient credit, discard under bounded
abuse policy rather than buffer unlimited work; do not attempt to undo received
bandwidth. For an outbound cell, queue only within the common byte/time ceilings
or fail the operation. There is no unlimited special exemption for quote/control,
padding or descriptor traffic. Genuine overload may cause discovery failure.
When paid capacity becomes active, paid quota/enforcement replaces free traffic
eligibility, but does not reset global resource buckets. Missing a paid deadline
closes the circuit, never downgrades it to free. A renewed allocation does not
refill a new free burst. Free-only replacement is subject to shared admission caps.

A normal middle on a control exchange forwards eight request and eight reply
cells: 32 charged ingress/egress cells. The introduction relay also handles four
legs of eight cells, 32 charged cells. That fits one burst, but four immediate
exchanges consume the entire burst before refill. A descriptor near 50 KiB takes
roughly 25–26 data cells before framing and acknowledgments, or about 50–52 charged
forwarding cells. These are sizing estimates, not packet measurements. Count the
actual fork cells and all extra traffic in tests. A single burst is usable by
ordinary low-volume applications too; the profile does not promise that free use
has no value to an attacker.

## Shared relay ceilings

| Resource | Candidate process ceiling |
| --- | ---: |
| Free eligible live circuits | 4096 |
| Free circuit admission bucket | 16/s, burst 32 |
| Free circuit admission per OR connection | 4/s, burst 8 |
| Aggregate free traffic, received plus sent | 4 MiB/s, burst 8 MiB |
| Aggregate free traffic per OR connection | 1 MiB/s, burst 2 MiB |
| Free queued outgoing cells per circuit | 32 cells |
| Total free outgoing queue | 8 MiB |
| Oldest free queued cell | 5 seconds |
| Concurrent quote/control slots and unexpired tombstones | 4096 combined |
| Maximum allocation per quote/control slot | 16 KiB |
| Shared slot memory arena | 64 MiB |
| Expensive protocol crypto operations | 256/s, burst 512 |
| Concurrent expensive crypto jobs | 32 |
| Waiting crypto jobs | 128, at most 1 second queued |

MiB means 1048576 bytes. Charge aggregate byte buckets for actual serialized Tor
link bytes in both directions; separately measure kernel/TLS/IP overhead to set
host-level network limits. Do not advertise that these application counters bound
all traffic arriving from an attacker. Expensive crypto means a public-key
signature verification or HPKE decapsulation; charge each attempted primitive,
including failures and up to two request-key trials. Native Tor handshake work
also stays under its inherited and shared setup limits. Stop admission before
allocating beyond any memory/job limit; no unbounded thread or queue creation.

The quote transport's per-circuit/per-registration counts and rate buckets apply
in addition to these limits. The 16 KiB maximum supersedes the smaller 4 KiB quote
slot estimate when using one shared arena; a quote implementation may allocate
less, never more. Runtime accounting must include buffer capacity, map entries,
timer nodes and tombstone storage, not just payload bytes. If object overhead
would exceed the arena, admit fewer slots. There is no implied promise of 4096
simultaneous successful exchanges. Peer identities and new circuits cannot reset
process buckets or give each service its own copy of aggregate memory.

## HSDir and descriptor bounds

Inherit the current verified Tor descriptor maximum, cryptographic validation,
revision acceptance and expiry rules. Do not substitute a smaller native format.
The new metadata has its separate 16 KiB decoded/12 KiB catalog caps and must fit
inside that inherited whole-descriptor bound after base64 and encryption overhead.
No peer can request an external catalog fetch to evade the limit.

Candidate HSDir ceilings: 64 concurrent uploads, 64 concurrent reads, 64 MiB
validated descriptor cache, 8 MiB provisional parse buffers, and the shared crypto
budget. One body is bounded by the inherited descriptor maximum before allocation.
For each blinded descriptor identity/time-period key, admit a burst of three
updates and refill one update token per 60 seconds. Equivalent duplicate content
may recover existing storage state but still consumes request/CPU bandwidth budget;
a lower revision never replaces a higher one. Distinct keys share process limits.
Keep only the newest accepted revision for each relevant inherited storage key.

Expire entries using inherited validity. Under cache pressure evict expired first,
then least-recently-accessed valid entries, without growing the cache or retaining
unlimited revisions. Bound failed-request logging. Rejected upload/read status
follows inherited directory transport behavior, not a demand for automatic payment.
An attacker can still compete with honest services; free publication is not a
claim that Sybil or storage DoS is solved. Test key churn as well as per-key limits.

## Service, wallet and ledger budgets

The service has 128 concurrent quote computations, 128 pre-acceptance setup
bindings and 128 pending builds, subject to a combined 64 MiB service-control
working arena. Admit at most 1024 live accepted agreements per configured service,
4096 per process, and at most two unresolved round invoice obligations per agreement.
Each wallet account has at most 32 concurrent RPCs and a queue of 128 intents;
queuing does not extend financial expiry. Use at most 16 MiB for transient wallet
responses and parser buffers. Wallet artifacts remain capped at the message
profile limits. Do not dispatch when those limits would be exceeded.

For a test deployment provision a 1 GiB durable ledger budget. At 80% usage stop
new agreements and new period admissions while retaining capacity to reconcile
existing obligations. At 95%, stop new financial dispatch even on existing
agreements, preserve uncertain state and let paid paths expire. Never erase
unresolved records to admit new work. Operators must provision additional storage
or close admission before exhaustion; storage failure is fail-closed for new spend,
not permission to restart with an empty ledger. The 30-day resolved replay
retention rule still applies. Measure transaction/log amplification; the capacity
thresholds are not proof that any chosen storage implementation can finish safely.

## Deadline and clock profile

The authoritative per-message chapters retain their exact clocks:

- Paid round 60 seconds; advance window 60 seconds; candidate grace 15 seconds,
  bounded by accepted relay policy. Admission preparation starts at round seven.
- Service setup 60 seconds from durable FUNDED reservation. Initial invoice/setup
  binding validity before FUNDED is separate and cannot reset this clock.
- READY recovery up to 15 seconds, clipped by already-funded lifetime; three
  retransmissions beyond the initial READY, using the specified 2–3 second delays.
- Quote/control slot 30 seconds, response target uniform 2–4 seconds, client
  attempt 40 seconds, three attempts and backoff 1–3 seconds, inquiry at most 150
  seconds. Every attempt must fit the remaining authoritative deadline.
- UTC uncertainty at most 5 seconds for metadata/settlement validation. Monotonic
  interval scheduling uses a local safety margin of at least 1000 ms plus measured
  scheduler uncertainty, in addition to subtracting the full observed RTT.

A process cannot assume wall-clock accuracy from the existence of a timestamp.
If the whole uncertainty interval does not fit a validity window, stop new work
that depends on it. Clock rollback or lost monotonic origin after restart does
not grant a fresh deadline. Network and wallet delays are unbounded in principle;
these policies choose when to stop, not guarantee successful Lightning settlement.
For two-stage funding, both windows and all caps must remain safe; operator funds
are not a hidden latency subsidy. Consider measured larger margins only through
an explicit profile/policy change, not silent per-request deadline extensions.

## Measurement plan and acceptance evidence

Use local Chutney and mock/regtest wallets first, retaining separate paid and
free-path runs. Collect aggregate counters without logging preimages, raw native
introductions, exporter keys, reusable client IDs or private relay lists.
For each test record source revisions, actual embedded binary provenance, traffic
load, host budget, sample count and latency/error distributions. Report p50/p95/p99,
maximum observed queue/slot/ledger usage, dropped operations and reason, alongside
confidence/sample limitations; do not infer a network guarantee from a percentile.

Required scenarios: full-size descriptors with maximum metadata; all payment
methods and refusal/cached replies; complete acceptance→binding→invoice→status
flows; two-hop-key decryption trials; renewal at differing hop clocks; concurrent
honest load plus new-circuit/key churn; crypto failures and partial fragments;
slow wallets, delayed settlement, lost controller replies, process/daemon restart;
clock rollback; ledger near both watermarks; free-burst aggregation; and long
streams with draining and hard-age termination. Verify admitted honest traffic
fits real token/queue budgets, memory never exceeds the configured arena, and no
overload path dispatches duplicate payments or extends deadlines. If the candidate
profile cannot meet an agreed useful success rate, revise it explicitly before
release rather than labeling dropped traffic acceptable by definition.

Compare endpoint-visible sizes and timing for fresh/cached/refused requests and
ordinary low-volume circuits. Fixed cells and random windows do not prove privacy
against colluding endpoints or traffic observers. Record this limitation in the
release profile. ET-03–05/11/12/21/23/31/32 own implementation/measurement work.
Documentation and sizing estimates are the only evidence produced by this chapter.
