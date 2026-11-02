# Paid circuit foundation acceptance status

Status: local paid-circuit foundation implementation and acceptance complete. All work remains on
`paid-circuit-foundation`; paid onion-service implementation is outside this
phase. The [implementation plan](paid-circuit-foundation-plan.md) retains the
source baseline, dependency order and detailed historical evidence. The
[implementation TODO](../TODO.md) and [handoff](../spec/validation/handoff.md)
remain the requirement references.

## Next session: manual real-Lightning test

Start by helping the operator configure one paid Chutney relay and pin it in
an explicit ordered foundation client path. Other selected hops remain zero-priced.
Use `PaymentBolt12Offer` for the receiving offer, `ElTorFoundationTerms` for
foundation pricing, and separate private controller/account configuration with
an existing durable ledger. The current integrated foundation wallet path is
Core Lightning; Phoenixd UI support alone does not establish verifier support.
Choose the receiving wallet and a separate payer, agree a small explicit spend
cap before dispatch, and verify Firefox uses the exact approved circuit. Observe
recipient settlement, all-hop activation, renewal and closure when funding stops.
Preserve the original free-testnet configurations and financial histories.

This is a next-session reminder, not an executed test or authorization for
unattended payments. Nothing is configured, funded or sent by recording it.

| Requirement | Implementation and executed evidence | Scope and limits |
| --- | --- | --- |
| Recipient-backed validation and replay protection | Exact CLN native recipient lookup, settlement/preimage/hash/amount/time checks, transactional unique redemption and authenticated consumption; positive socket, concurrent claim, corruption and lost-receipt/reopen regressions. | Deterministic native wallet fixtures; real settlement is not claimed. Unsupported adapters cannot enable this profile. |
| Durable accounting and recovery | Receiving/outgoing ledgers, explicit migrations, storage gates, original operation claims, lookup-only uncertain-outcome recovery and original clock retention. Live restart closed the prior instance without adding construction, operation, invoice, outgoing-intent or budget records; SQLite integrity passed. | No missing circuit or authorization acknowledgment implies a refund or consumption. |
| Initial construction and activation | Exact construction/COMMIT/START identities, immutable terms/clocks, pending-BIND recovery and paced control retries; positive-price lost-pay/START tests and 200-candidate/two-page receiver scheduling regression. Final financial core reached authenticated all-hop activation. | Failed attempts and original reservations remain retained; no automatic replacement build. |
| Renewal and active streams | Final financial-core 22-minute transfer: 1,351,680 bytes with matching SHA-256 and 44 unchanged native bindings across rounds 10, 11, 20 and 21; map cleanup passed. Socket/native tests cover partial refusal, lost acknowledgment, late preparation, original price, finite lifetime and no duplicate funding. | Separate financial-core and observer executable provenance is preserved below. |
| Bounded free capacity | Uniform authenticated ordinary constructors; physical-direction byte/cell accounting, paid/free separation, aggregate state/setup/work/queue limits, exact queue boundary/overflow/teardown regressions. Final native setup and process-byte pressure scenarios passed. | Queue was observed, not saturated live; no whole-host or production CPU guarantee. |
| Aggregate live-state overload and recovery | Four independent clients reached the lowered 64-circuit cap; cross-origin refusal, existing honest echo and fresh honest admission/echo passed. Process-v4 independently demonstrated both-direction credit exhaustion and honest recovery on the final native ELF. | The first live-state run used an earlier native ELF; final setup/traffic provenance is separate. Finite local workloads do not establish production tuning. |
| Native desktop and browser | Explicit Phoenixd auto-start populated balance/transactions on first visit. Native observer displayed Closed and Last verified active correctly; exact-owned-sidecar exit cleared status and enabled Activate without refresh. Firefox rendered proxied HTTPS. Ordinary restored configuration subsequently passed activation, bootstrap, transmission and rendered-browser checks. | Process readiness is distinct from authenticated paid-path status. Wallet inspection was read-only. |
| Local-only delivery and provenance | Seven foundation branches, repository-specific identities, blocked push URLs/rejecting hooks, relevant standalone/vendored alignment and source/running-executable/input digests audited. All ten configs and 68 native keys preserved. | LNI's preexisting Cargo.lock modification remains untouched. No push, PR, publication or real payment. |

The foundation account in the live test uses zero-priced policies and a loopback
wallet fixture that rejects payment APIs. Positive payment evidence in focused
native HTTP fixtures is separate from live transport. Neither establishes a real
Lightning settlement, a production deployment approval or an anonymity property.
A process indicator, bootstrap-complete dialog, round-1 budget or HTTP response
alone cannot establish all-hop paid activation.

The sustained supervisor waits for the daemon's authenticated minimum round
across all hops to reach round 2, then checks the native stream binding against
the exact approved scope and retains its identity throughout the transfer. It
makes no competing per-hop status reads against the daemon's shared query budget.
An expired or closed activation gate sends no HTTP request and never constructs
a replacement circuit. Failed attempts and financial histories remain retained.

## Final-source audit repairs and deployment

The sustained run is pinned to its captured build inputs. Subsequent source
changes must be deployed and validated separately; they do not retroactively
change that run's provenance. Read-only audit found two native integration gaps:
legacy/non-negotiated handshakes can bypass the foundation resource owner, and
the inherited EXTEND formatter appends a child payment proof outside that child
relay's encrypted handshake. Enabled-profile dispatch must enforce admission
while preserving bounded directory bootstrap, and foundation extension must
retain hop-private metadata only inside its intended handshake. Routine logs
must not expose these identifiers. The native and vendored fixes described below
are committed and built, with focused checks passing. Final-source sustained
transport acceptance remains separate from those checks.

The audit also identified two narrow daemon recovery gaps: renewal status GET
CAPACITY should defer without reconnecting, and direct schema-4/5 migrations
need the same transaction-local retained-history validation as schema 6 before
advancing to schema 7. Existing operation identities and financial exposure stay
retained. No real Lightning payment or paid onion-service implementation is
part of these checks.

## Retained renewal acceptance checkpoint

The scheduler build's sustained run completed successfully, with one native
stream and circuit binding retained across all 44 observations. The complete
1,351,680-byte body matched the expected SHA-256, all four required boundary
rounds (10, 11, 20, 21) were observed, and the temporary address mapping was
restored. Actual running app and receiving-controller executable hashes were
checked before shutdown. Firefox rendered a fresh HTTPS page through the app's
SOCKS proxy during this run. Private results and captures remain under the
workspace runtime directory.

This run predates the final audit repairs. Native commits `7792b6f904`,
`714ff93450` and `82b51919e4` constrain bootstrap, remove the plaintext child-proof
suffix and suppress legacy payment events in enabled bootstrap. Their vendored
mirrors preserve the preexisting stricter profile-off EXTEND behavior. Focused
native dispatch, crypto, framing and redaction tests passed. Daemon fixes cover
renewal read backpressure, atomic old-schema migration preflight and cancellation
of runtime-owned controllers. The leased, generation-owned status observation
and app adapter separate Tor readiness from paid-path verification. Focused
recovery/lifecycle checks, frontend/backend compilation and stopped-stack desktop
packaging passed. The native Tauri adapter also forwards the leased status field;
its later sidecar-aware UI verification is recorded below. All nine final native
nodes bootstrapped and Chutney transmission passed before the temporary resource
experiments. None of these later repairs is retroactively validated by the
retained transfer. The later financial-core and observer runs below provide
separate deployment evidence. The later process-byte-pressure checkpoint below supplies the separate traffic evidence.

## Final native setup-pressure checkpoint

The finite setup experiment used native executable SHA-256
`77135566706968a6bc710c89494fdc61f8e7d73841759245f669ec5ffcab3a09`.
Four independent origins submitted 12 stress requests: two built and ten were
refused or closed. Two crypto operations started within the measured 1,139 ms
window, within the configured one-per-second rate and four-attempt burst.
The waiting counter reached its lowered two-slot ceiling; the running jobs were
too short to observe, so this does not prove production CPU saturation.
All 133 samples stayed within the observed ceilings. The existing honest echo
completed during pressure, and a fresh circuit and checksum-verified echo
succeeded after ordinary refill. No wallet or payment commands were sent.

The separate initial traffic deployment failed its operator's full-bootstrap
gate before any experiment constructor was dispatched. Its private result is
retained and is not traffic-pressure acceptance. Temporary configurations and
fixture processes require exact restoration before returning to app acceptance.
These checkpoints change no protocol, wire, privacy or timing requirements;
they report finite local evidence and its limits.

## Final financial-core and native observer checkpoints

The second sustained run used the rebuilt financial core at `6f3184e`, including
ledger fix `bbab653` and receiver scheduling fix `8127748`, with the final native
ELF identified above. The actual native daemon sidecar, desktop and all receiving
controllers were pinned by running executable hashes. One stream retained its
exact native circuit/instance/handle for all 44 observations, the entire
1,351,680-byte body matched SHA-256, rounds 10/11 and 20/21 were observed, and the
routing map was restored. Private evidence is retained in
`.runtime/foundation-live/slowstream/run-20261029T071310Z/` under the workspace.

The later changes to daemon runtime source are confined to the status registry,
private observation bridge and module export. The financial coordinators,
ledger, RPC and native/wallet dependency revisions are unchanged from that run;
the source comparison and separate build/running-image manifests are retained
privately. The observer binary has its own acceptance: reopening the old scope
closed it without adding a constructor, operation, invoice, outgoing intent or
budget; SQLite integrity passed. A fresh zero-cost scope displayed **Last
verified active** without refresh/navigation, and Firefox rendered a fresh HTTPS
page through the app proxy. Terminating the exact verified sidecar through its
owned process handle left the desktop open, cleared its phase and re-enabled
Activate through polling. Frontend publication now avoids stale captured Redux
flags after an unexpected exit. The bridge's focused tests separately cover
leases, ownership, permissions, symlinks, bounded coalescing and stalled output.

The 22-minute result applies to its captured financial-core executable; the
subsequent observer changes have the separate UI/transport/lifecycle evidence
above. All live prices and spending caps were zero, and the fixture rejects
payment APIs. Existing wallet balance/transaction inspection was read-only.
These reporting and display changes affect no network message format, financial
authority or deadline. Broader independent review and real settlement remain
separate from this local acceptance.

Traffic profiles small-v1, startup-v2 and retry-paced-v3 have retained failures
before their mandatory baseline build. Original target configurations were
restored exactly and fixture origins stopped after each. They provide diagnosis,
not traffic-pressure acceptance. The subsequent process-v4 workload passed as
recorded below; failed claims and financial histories remain retained.


## Process-byte pressure and ordinary configuration restoration

The final-native `process-v4` scenario passed in
`.runtime/foundation-resource/process-v4-20261029/result.json`. Four independent
origins exercised six stress streams per direction under a shared 64 KiB/s
process bucket with a 512 KiB burst. Both upload and download drove observed
process credit to zero; native pressure circuits closed, and five honest
checksum echoes succeeded, including fresh admission after each pressure phase.
All 701 snapshots remained within the configured live/state/queue ceilings.
The largest observed queue was 6,336 of 65,536 bytes: this is observation, not
live queue saturation. Separate native queue boundary tests exercise exact-cap,
one-byte overflow, direction accounting and teardown. The setup experiment's
crypto-start and waiting limits similarly do not establish whole-host CPU
saturation. See [resource acceptance](foundation-resource-acceptance.md) for
exact workload limits, provenance, retained failures and interpretation.

After the fixture controllers and origins stopped, all nine node configurations
and the app configuration were restored byte-for-byte to their saved ordinary
free-testnet originals. The retained key-integrity comparison found all 68
native key files unchanged. A subsequent ordinary startup brought all nine
nodes to 100%, Chutney transmission passed, and the native app activated with
100% bootstrap and an established circuit. A fresh HTTPS request through app
SOCKS5 succeeded and Firefox rendered the page in the dedicated proxy profile.
The temporary loopback WebDriver was stopped afterward. No ledger, financial
history or failed claim was reset. The normal test launcher continues to disable
wallet auto-start; the separate explicit wallet-startup acceptance above remains
the evidence for that requested UI behavior.

These are local implementation and transport observations, not changes to the
protocol or evidence of real Lightning settlement. Paid onion-service features,
production tuning and independent cryptographic/privacy review remain separate.


## Requirement audit and implementation handoff

The original dependency order is retained in the implementation plan. The final
read-only audits checked source paths and actual assertions rather than treating
test counts or browsing as proof of financial correctness:

- Recipient chain: `src/wallet_adapters/cln.rs`,
  `src/relay/foundation.rs`, `src/relay/foundation/tests.rs`,
  `src/rpc/payment_consumption.rs`, and `src/payment_ledger.rs` tie native
  recipient evidence to a unique durable decision and matching consumption.
  Semantic corruption is rejected on read/reopen, while zero-price decisions
  remain explicitly separate from positive settlement claims.
- Outgoing/recovery: `src/payment_ledger/wallet.rs`,
  `src/client/foundation_tests.rs`, `src/payment_ledger/clock/tests.rs`, and the
  operation journal retain the original dispatch identity, invoice, limits and
  clock. Positive lost-payment/START fixtures recover without another native
  pay; relay lost-authorization fixtures reuse the saved decision.
- Renewal: `src/client/foundation_renewal_tests.rs` covers all-hop prerequisites,
  new period identifiers, partial/lost/refused admission and immutable prices;
  native payment profile/schedule/origin tests cover finite age and exact state
  transitions. The correlated same-stream transfer supplies live interoperability
  evidence that isolated state-machine tests cannot.
- Resource enforcement: native `free_capacity`, hook, worker, payment-resource
  and control tests plus the separate live-state/setup/process-v4 workloads
  cover enforced shared limits and honest recovery. Relevant standalone and
  vendored files align except the preserved profile-off testing-only extension
  exception; that exception is outside enabled foundation admission.
- Application: `eltor-app/docs/foundation-runtime.md` records owned-sidecar
  observation, original lease expiry, bounded writer coalescing and lifecycle
  behavior. Actual native UI/exit and browser captures validate those paths.

Private evidence lives under workspace `.runtime/foundation-live/`, including
`slowstream/run-20261029T071310Z/result.json`, `bridge-restart-result.json`,
`observer-owned-exit-result.json`, `firefox-final-observer-active.png`,
`final-restored-firefox-verify.log`, `final-restored-transmission.log`,
`firefox-final-restored.png`, config/key comparisons and source/running-image
manifests. Resource records live under `.runtime/foundation-resource/`; no raw
wallet records, keys or private paths are added to tracked documentation.

The enabled profile remains explicitly opted into on the local testing network.
The integrated capable wallet path is CLN BOLT12; ordinary wallet UI support and
generic pay-offer APIs do not establish that every adapter satisfies foundation
requirements. Broader ET items remain open where they include independent
cryptographic/privacy review, additional wallet methods, fuzzing, production
measurements or paid onion-service implementation. This handoff adds no network
message, identifier exposure, timing rule, payment authority or default enablement.
