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
| Initial construction and activation | Cold BIND waiting and original construction deadlines are implemented; pending-COMMIT regression passed. | The prior live attempt failed on control CAPACITY handling before retained round-2 progress. Commit `e1df067` fixes bounded QUERY retry and read pacing; its TCP regression passed. The rebuilt warm-controller attempt also timed out before all-hop activation. Commit `a718169` adds bounded actionable scheduling, fair background probes and durable closed-work filtering; its 50-candidate socket regression passed. The rebuilt attempt reached authenticated all-hop round 2 and started a strictly bound same-stream transfer; the full renewal result is still pending. |
| Renewal and active streams | Native codecs, admitted periods, fixed clocks and authenticated actual-activation notifications are implemented. | The pinned pre-audit build passed one 22-minute transfer: 1,351,680 bytes with matching SHA-256 and 44 unchanged native stream/circuit samples including rounds 10, 11, 20 and 21. Final-source deployment and confirmation remain pending. |
| Bounded free capacity | Uniform ordinary constructors and directional/aggregate traffic, state, setup, work and queue limits are implemented. The final native setup-pressure scenario passed with four origins, 12 stress requests, 10 refusals/closures, waiting peak 2/2 and both existing and fresh honest echoes. | Final enforcement audit and traffic-pressure experiment; these are distinct from the completed live-state and setup scenarios. |
| Aggregate live-state overload and recovery | Four independent native clients reached the lowered 64-circuit limit; cross-origin refusal, an existing honest echo and subsequent honest admission/echo passed. Original relay configuration was restored. | This finite experiment does not establish every resource ceiling or production latency. |
| Native desktop and browser | Desktop builds and Phoenixd automatic startup/data refresh passed. HTTP succeeded through the app, including ordinary free circuits. | Final rendered Firefox capture and transport evidence tied to the approved foundation path after activation/renewal succeeds. |
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
its actual UI behavior still needs final live verification. All nine final native
nodes bootstrapped and Chutney transmission passed before the temporary resource
experiments. None of these later repairs is retroactively validated by the
retained transfer. Matched final app deployment, traffic-pressure acceptance and
the final app/browser run remain required.

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
