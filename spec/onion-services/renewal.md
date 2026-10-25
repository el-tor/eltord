> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# Renewing both rendezvous circuit halves

This proposal inherits [spec one's circuit renewal draft](../01_paid_circuits.md#circuit-renewal-draft) and the proposed [`PAYMENT_RENEW`/`PAYMENT_RENEWED` wire protocol](../01a_circuit_renewal_wire.md). Renewal is a shared transport feature, not an onion-service-specific way to move streams. Each circuit origin separately sends the same per-hop messages to its own relays, with independent private bindings and identifiers. Preserve the two existing circuits joined at the same rendezvous point, their Tor stream/cryptographic state, and the client/service authorization binding. Neither the client nor the service can keep a connection alive if the other half disappears. Renewal does not require a new introduction or descriptor publication solely because another payment period begins.

### SCQ continuation and funding

Before the existing authorization runs out, the service offers an authenticated extension referencing the accepted SCQ, circuit authorization identifier, fresh extension identifier, next period and round range, unchanged per-minute price, and the agreed activation boundary. The extension must retain the client-key/recipient binding and disclose cumulative cost, outstanding-funding limits, deadlines, and failure accounting. It authorizes further individual round purchases on the same circuit, not a reusable session or cross-circuit balance. No ten-round lump payment or repeated build/testing handshake fee is introduced. Version one forbids price changes on the continuing circuit.

The client may decline or approve continuation within an explicit renewal/spending policy. Approval of the original SCQ alone does not authorize unlimited extensions. Preparing an extension and registering fresh relay identifiers must fit bounded control-plane resources and cannot incur unfunded paid work for the service. Each hop may refuse future admission; existing paid service still follows its accepted schedule.

### Coordinated preparation and activation

1. Use spec one's draft trigger at the start of round seven in each ten-round allocation (global rounds 7, 17, 27, and so on). When continuation is wanted and authorized, the client prepares renewal with every hop on its existing path. In parallel, the service prepares renewal with every hop on its private existing path, under the proposed SCQ extension. Use independent per-hop identifiers and acknowledgments. Each half follows its agreed relative schedule; specify how preparation is coordinated when their boundaries differ.
2. The client authenticates the service's extension/readiness response, bound to the existing connection and upcoming round range. The service need not reveal its private relay identities or acknowledgments. Its readiness statement is a service commitment, not independently verifiable proof of its path, payment, or future delivery.
3. Before initiating funding for the new period, require accepted extension terms, admission on the client's own path, and the service's authenticated readiness statement. Preparation beginning at round seven must finish before the existing 60-second payment lead is needed. It does not move round-eleven payment into round seven: keep funding incremental on the agreed schedule and within the same outstanding-payment cap. Exact completion deadlines must be specified and measured; negotiation is not assumed to be instantaneous or covered by another free grace period.
4. Continue the existing two-stage funding schedule: the client pays its own relays and settles the service's next-round funding; the service reserves that settlement before paying its own relays. An admission/readiness acknowledgment does not authorize borrowing from the operator's wallet or another client's funds. Pay only the next authorized round within existing caps.
5. On success, both halves continue through their agreed schedules without deliberately closing active streams at round ten. If either side cannot renew or fund its next period, stop further renewal payments, report failure through the authenticated control exchange where possible, and expire under the existing deadlines. Preserve uncertain payment records and reconcile unused funding. Do not cut short already purchased service merely because a future extension was refused.

This is coordinated preparation, not atomic settlement across the client, service, and relays. A half can fail after readiness or after some payments settle. Accepted loss exposure still includes unused current-round service, the prepaid next round, and disclosed fees across both halves; the SCQ's onward relay payments are not another client charge. No automatic refund or live-stream migration is promised. A separately authorized replacement can serve new connections; resuming an interrupted download still depends on the application and remote server.

### Remaining decisions and tests

The per-hop wire draft specifies exact batch acknowledgment, replay/conflict handling, conservative timing, and bounded pending-state rules. The continuation and readiness wire chapters define those encodings and independent-clock policies. SCQ extensions cannot override relay limits or send a common SCQ identifier to transport relays as a renewal token. Version one uses bounded-free introduction-circuit replacement, and do not indefinitely renew an idle descriptor-upload circuit under this paid extension. All design/implementation/validation work is tracked in the [El Tor TODO](../../TODO.md), including separate deferred replay protection.

Test continuous transfers across multiple allocation boundaries without changing either rendezvous circuit or stream. Test each side declining, one hop refusing, stale/duplicate extension acceptance, unequal boundary times, partial settlement, missing service readiness, payment delay, budget exhaustion, and a relay actually dropping. Verify no unfunded service spending, no unauthorized eleventh round, no duplicate setup fee or quota reset, bounded pending state, and correct closure/reconciliation. This section defines draft acceptance criteria; no renewal implementation or executed end-to-end test is claimed.

## Version-one continuation profile

The [continuation/lifetime contract](continuation.md) resolves the draft policy choices
above: fixed prices throughout, finite candidate one-hour default/24-hour ceiling,
ten-minute new-stream draining, free-only replacement and exact SCQ continuation
records. These are proposed test policies, not measured runtime support.
