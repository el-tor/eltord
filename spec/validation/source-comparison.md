# Existing implementation and proposed protocol

This is a source comparison, not runtime conformance evidence. It covers the
paid-circuit paths relevant to the twelve design checklist items. Runtime work
remains deferred. Recheck this comparison when implementation revisions change.

## Revisions inspected

Paths below are relative to the named repository, not to the spec book.

| Repository | Revision | Scope |
| --- | --- | --- |
| eltord | `18935e2f9b4bd7b297620f538e6186520b234117` (`intermitted-fix`) | Client construction/scheduling, relay watcher/ledger, RPC helpers and database |
| eltord | `20cc246` (`hidden-service-spec-revision`) | Documentation preflight baseline; runtime does not include the reliability branch changes |
| tor | `2f05d0452d0cfc1393e8d7d2e0dbf781e5230e33` (`intermitted-fix`) | Control handlers, payment events and testing-network exception |
| libeltor-sys | `543241cda3b12fff3833b6b8b52cc997a494ac4d` | Vendored Tor under `libtor-src/tor-src`; this directory is part of libeltor-sys, not an independent Git revision |
| lni | `90dd176b6c5cf1052842fbe574e9320aedf2ebe4` | Phoenixd lookup and invoice polling |
| eltor-app | `c0c0e2a894e534eadf68e553f0341c6fe520bbd2` (`intermitted-fix`) | SOCKS framing, startup/readiness, log recovery, Arti configuration and local Phoenix binaries |

The standalone and vendored `control_extendpaidcircuit.c` files compare equal.
Their `circuitbuild_relay.c` files differ: the standalone fork allows missing
payment data only with `TestingTorNetwork`; the vendored copy does not include
that exception. This is not the proposed bounded free allowance.
Source checkout state does not establish which revision a running binary embeds.
The implementation handoff must capture actual build provenance separately.

## Checklist mapping

| Design item | Existing basis to reuse | Required change or new work |
| --- | --- | --- |
| 1. Renewal RPC | `src/rpc/extend_paid_circuit.rs`, Tor `src/feature/control/control_extendpaidcircuit.c` provide authenticated control transport and circuit construction. | Existing nonzero EXTENDPAIDCIRCUIT extends a path; it is not payment renewal. Add operation-bound renewal preparation/status and per-process handles without changing path membership. |
| 2. Initial terms/activation | `src/client/circuit.rs` constructs per-hop handshake fields and ten identifiers; relay metadata and selection supply prices. | Add authenticated immutable terms and activation acknowledgments. Existing watcher start time is local event receipt, not an authenticated relay activation. Handshake verification is still a TODO in `src/relay/payments_watcher.rs`. |
| 3. Queries/events | Existing GETINFO circuit-status, STREAM subscriptions and PAYMENT_ID_HASH_RECEIVED event provide transport and lifecycle integration. | Add payment-specific revisions, snapshots and recoverable registration. Existing BUILT status and log event CIRCUIT_BUILT do not prove settlement or round activation. |
| 4. Restart/replay | `src/database.rs`, sent/received ledgers and LNI lookup provide persistence and node access. | Replace append-and-rewrite bookkeeping with atomic consumption/reservation and durable uncertainty. Do not reset a corrupt payment ledger to an empty authorization history. |
| 5. Service metadata/catalog | Inherited Tor descriptors provide authenticated publication; existing relay payment metadata is distinct. | New service capability, delegation and canonical catalog records; no current paid-service catalog implementation was found in the inspected eltord source. |
| 6. Quote transport | Inherited introduction circuits and relay framing are reusable. | New encrypted return-slot exchange, bounded reassembly and command allocations; existing payment-ID event is not a service quote transport. |
| 7. Acceptance/invoices | LNI provides wallet operations; `payments_loop.rs::pay_relay` pays an offer with the generated ID as payer note. | Add signed SCQ acceptance and invoice-to-round binding. The inspected pay_relay path unconditionally uses pay_offer; an optional BOLT11 field is not evidence of an implemented BOLT11 payment branch there. |
| 8. Status/READY | Circuit status, stream attachment and teardown are existing mechanisms. | New authenticated service attempt status/READY and accounting. Do not treat an existing control success reply as proof the end-to-end rendezvous succeeded. |
| 9. Continuation/lifetime | Reliability scheduler pays each surviving circuit every round and keeps one stream monitor for the generation. | Renew the identifier allocation in place. The existing loop stops at the minimum available identifier count; backups do not migrate established streams. |
| 10. Resource/timing bounds | Watcher uses 60-second rounds and 15-second grace; reliability scheduler subtracts 15 seconds from its capped interval. | Replace that scheduling approximation with authenticated per-hop deadlines and a bounded advance-payment policy. Add aggregate free-tier and quote-slot limits; testing-network bypass is not their implementation. |
| 11. Privacy/consistency | Shared initial payment fields, dummy handshake material and Tor encryption remain the basis. | Remove routine logging of payment payloads/preimages and compare the new states across both branches. Fixed fields alone do not establish resistance to timing correlation. |
| 12. Handoff/validation | Existing stability probes, mock-wallet tests and Tor control-error tests provide reusable regression coverage. | Add new protocol vectors and paid/restart/renewal tests, wallet capability matrix and actual embedded-build provenance. Existing tests do not certify the new protocol. |

## Important semantic differences

The reliability scheduler in `src/client/payments_loop.rs::run_circuit_rounds`
pays all surviving circuits each round. It accounts for elapsed wallet/RPC work
when waiting for the next round, rather than sleeping a full interval afterward.
Preserve this correction. Its interval is currently capped at 60 seconds and
then reduced by 15 seconds, so it is not the proposed independent relay-clock
schedule with a fixed one-round advance limit. Carrying that old formula into
unbounded renewals could keep moving payments earlier; implement the new
schedule explicitly rather than assuming the reliability fix already does it.

Phoenixd `crates/lni/phoenixd/api.rs` returns settlement timestamps and searches
transactions by hash or payer note. Its polling fix treats transient HTTP
failures as pending until the window expires. Preserve that behavior. However,
`payments_watcher.rs` judges success against `Instant::now()` captured when the
payment-ID event was handled, and logs the returned transaction rather than
validating recipient, amount and settlement-time authorization there. The new
verifier must inspect those facts, and must not treat a payer note as a unique
proof of payment. This is source evidence, not an executed replay exploit.

`src/database.rs::write_payment` appends to a vector and saves JSON by truncating
the file. It does not reject duplicate consumption keys or provide a transaction
covering invoice settlement, reservation and circuit authorization. Received
ledger initialization includes placeholder amount/offer values. Existing paid
flags and lookup methods are useful integration points, not sufficient evidence
of crash-safe single-use authorization. Keep the separately deferred replay
investigation intact; this comparison does not start that implementation.

## Integration requirements

Implementers MUST port or otherwise preserve the reliability corrections before
using this spec branch as a runtime baseline. Do not merge branches as a side
effect of documentation work. New controller contracts supplement the existing
construction command; they do not rename it or turn EXTEND into renewal.
Adapters must distinguish generated round IDs, actual invoice payment hashes,
settlement records and local operation IDs. Preserve the existing per-hop ID
format while adding explicit authenticated mapping and single-use enforcement.

## Privacy/timing assessment

This comparison changes no wire behavior. It records that current debug/info
paths expose full payment material and that callback clocks differ from the
proposed relay activation clocks. No personal data, secrets or raw runtime logs
are reproduced here. Validation performed: source reads, branch diffs, symbol
searches and comparison of the two vendored/standalone control handlers. No
wallet payment, runtime feature test or exploit was executed. Deferred checks
remain ET-01, ET-08–10, ET-18, ET-21 and ET-33 in the [backlog](../../TODO.md).

## Reliability branch integration preflight

The local `intermitted-fix` work spans eltord, tor, libeltor-sys, lni and
eltor-app. A Git merge-tree simulation against the eltord specification preflight
baseline `20cc246` completed without conflicts. The resulting virtual tree's
runtime source, lockfile, tests, tools and investigation documents exactly matched
the reliability branch; its spec, TODO, book configuration and readme exactly
matched the specification branch. No checkout, branch ref or working file was
merged. This establishes local textual compatibility, not a compiled integration
or remote merge result. Standalone Tor's baseline branch is `main`; the other
repositories retain their own histories and must be integrated separately.

Additional preservation requirements from that review:

- Preserve eltord's distinct relay selection, proper command-status parsing,
  bounded RPC calls, one stream monitor per generation and shutdown cleanup.
  New durable operation/instance contracts extend these fixes; successful control
  authentication alone must not become command or payment success.
- Preserve both Tor copies' initialized error-path pointers, actual serialization
  capacity checks and failed-new-circuit cleanup. New control handlers need the
  same failure-path regression coverage. The standalone testing-only unpaid
  exception remains distinct from the vendored behavior and from bounded free use.
- Preserve app SOCKS fragmented-frame handling and forwarding of remaining bytes,
  launch error propagation, replaced-log recovery and loopback Arti configuration.
  Existing bootstrap progress is process readiness, not paid-service READY. The
  new UI must retain that distinction rather than reuse a bootstrap completion
  callback as proof of rendezvous or settlement.
- Preserve the local Phoenix binaries and configurable embedded-wallet startup.
  Those binaries are not part of the specification branch. Their versions and
  actual invoice capabilities need separate validation before claiming support
  for invoice-before-payment, either payment method or refund receiving.
- Preserve LNI's transient-error-as-pending behavior. New durable uncertainty,
  authoritative settlement checks and single-use accounting remain required;
  the polling fix alone does not implement them.

The previously recorded mock, C error-path and free-network results are historical
evidence, not tests rerun in this preflight. No new runtime feature or protocol
change is introduced. Existing ET-01, ET-08–10, ET-18, ET-20–22 and ET-33 cover
implementation and regression validation. Retain and rerun the reliability tests
when implementation is separately authorized, then add paid-protocol conformance.

**Privacy/timing assessment:** this comparison records existing behavior only.
It adds no identifiers, message fields, logging, timers or payment authority.
Local source/diff and virtual merge checks were executed; no wallet payments,
network soak, binary rebuild or independent privacy validation was performed.
