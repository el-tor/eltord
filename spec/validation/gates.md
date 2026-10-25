# Implementation and deployment gates

These gates consolidate unresolved work; they do not authorize runtime changes.
Each implementation task remains in the [backlog](../../TODO.md).

| Area | Defined in draft | Still required | Tasks |
| --- | --- | --- | --- |
| Shared paid construction | Uniform fields, independent per-hop IDs, free-capacity policy, initial commitment wire and durable authorization contracts | Implement and test authenticated commitments, capability/accounting schema and verified single-use settlement | ET-01–05 |
| Renewal | Per-hop wire layout, exporter proposal, retries and batch state | Independent crypto review, controller conformance, runtime lifetime/free-only conformance and runtime vectors | ET-06–10 |
| Discovery | Free HSDir/intro policy and aggregate bounds | Implement capability/resource/replacement profiles and measure their behavior | ET-11–12 |
| Quote bootstrap | Initial request/reply sizes, crypto profile, routing and privacy policy | Independent full-vector/crypto review and runtime interoperability for the specified commands/metadata | ET-13–14, ET-23–27 |
| Invoice and acceptance | Bound acceptance, per-round invoice association and durable idempotency | Implement the specified record framing and method-specific wallet bindings | ET-18, ET-28 |
| Funding and state | Reservation, setup, billing anchor, READY/recovery and exposure policy | Implement proposed controller/recovery and actual-path READY contracts; measure clock uncertainty | ET-15–17, ET-29–32 |
| Parameters | Candidate setup 60 s, recovery up to 15 s, three READY retries | Validate candidate delay/ACK ranges, resource ceilings, clock margins and startup success under load | ET-21, ET-31–32 |
| Release profile | Exact service version 1, no silent fallback | Use the pinned handoff/source baseline and capability set; independently review and test across implementations | ET-19–22 |

## Completion criteria

A draft can state open gates. An implementation-ready profile must define all
observable bytes, acceptance predicates, state transitions, error behavior and
limits so independent implementations agree. A deployable profile additionally
needs independent review and correlated paid-network tests with bounded resource
use and explicit privacy limitations. Do not fill measurement gaps with invented
production defaults or label undocumented runtime behavior as a standard.
