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
| Durable accounting and recovery | Receiving/outgoing ledgers, explicit migrations, storage gates, original operation claims and uncertain-outcome recovery are implemented with focused fault tests. | Final requirement audit and live recovery checks; control backpressure must not strand or duplicate an operation. |
| Initial construction and activation | Cold BIND waiting and original construction deadlines are implemented; pending-COMMIT regression passed. | Current live attempt reached round 1, then failed on control CAPACITY handling before retained round-2 progress. No successful paid-path acceptance is claimed. |
| Renewal and active streams | Native codecs, admitted periods, fixed clocks and authenticated actual-activation notifications are implemented. | One exact approved stream through both 10→11 and 20→21, plus retained failure/recovery evidence. |
| Bounded free capacity | Uniform ordinary constructors and directional/aggregate traffic, state, setup, work and queue limits are implemented. | Final enforcement audit; live CPU/byte/queue saturation is distinct from the completed live-state scenario. |
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
