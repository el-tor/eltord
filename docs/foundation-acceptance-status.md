# Paid circuit foundation acceptance status

Status: incomplete local implementation acceptance. All work remains on
`paid-circuit-foundation`; paid onion-service implementation is outside this
phase. The [implementation plan](paid-circuit-foundation-plan.md) retains the
source baseline, dependency order and detailed historical evidence. The
[implementation TODO](../TODO.md) and [handoff](../spec/validation/handoff.md)
remain the requirement references.

| Requirement | Current evidence | Remaining acceptance |
| --- | --- | --- |
| Recipient-backed validation and replay protection | Exact native recipient lookup, settlement/preimage/hash/amount/time checks and transactional unique redemption are implemented; focused adapter and ledger tests are recorded in the plan. | Final trace from wallet evidence through native authorization and consumption, including restart and duplicate outcomes on the final deployed sources. |
| Durable accounting and recovery | Receiving/outgoing ledgers, explicit migrations, storage gates, original operation claims and uncertain-outcome recovery are implemented with focused fault tests. Final-source live restart recovery closed the previous instance's scope without adding construction, operation, invoice, outgoing-intent or budget records; SQLite integrity passed. | Final requirement audit; control backpressure must not strand or duplicate an operation. |
| Initial construction and activation | Original construction/COMMIT/START identities, fixed clocks, pending-BIND recovery and paced control retries are implemented. The final financial-core build reached authenticated all-hop round 2 and completed the sustained bound transfer. A 200-candidate/two-page socket regression confirms financial progress under ordinary inventory load. | Final requirement evidence review; retain the earlier failed attempts and their fixes in the implementation plan. |
| Renewal and active streams | Two pinned 22-minute transfers passed. The later build includes the native privacy/bootstrap repairs, ledger integrity checks and 4096-candidate receiver scheduling. It transferred 1,351,680 bytes with matching SHA-256 and 44 unchanged native bindings across rounds 10, 11, 20 and 21; mapping cleanup passed. | Source/provenance audit and separate observer-only deployment checks; see the final financial-core checkpoint below. |
| Bounded free capacity | Uniform ordinary constructors and directional/aggregate traffic, state, setup, work and queue limits are implemented. The final native setup-pressure scenario passed with four origins, 12 stress requests, 10 refusals/closures, waiting peak 2/2 and both existing and fresh honest echoes. | Final enforcement audit and traffic-pressure experiment; these are distinct from the completed live-state and setup scenarios. |
| Aggregate live-state overload and recovery | Four independent native clients reached the lowered 64-circuit limit; cross-origin refusal, an existing honest echo and subsequent honest admission/echo passed. Original relay configuration was restored. | This finite experiment does not establish every resource ceiling or production latency. |
| Native desktop and browser | Phoenixd auto-started and its balance/transaction table populated without refresh. The actual native sidecar bridge showed Closed on recovery and Last verified active on a fresh scope. After exact-owned-sidecar termination, the open desktop cleared the phase and enabled Activate without refresh. Firefox rendered a fresh proxied HTTPS page. | Preserve proof captures and restore ordinary free-testnet settings after the remaining resource experiment. |
| Local-only delivery and provenance | Repository-specific identities and push protections retained; native/vendored mirrors and executable/input digests recorded locally. | Final clean-tree, source/mirror, deployed-binary and requirement-by-requirement audit. |

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
separate deployment evidence. Traffic-pressure acceptance remains outstanding.

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
not traffic-pressure acceptance. A source-derived workload revision remains in
progress; resource acceptance and final ordinary-configuration restoration keep
the goal open.
