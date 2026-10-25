> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# Bounded free allowance for introduction availability

### Purpose and scope

The service needs open circuits to its introduction points so those relays can forward client introductions. These circuits are shared availability infrastructure across many visitors, not a new service-side circuit built for every client. Charging ordinary paid-circuit rates continuously could make staying reachable prohibitively expensive.

This proposal uses spec one's shared, purpose-independent free traffic allowance at **every hop** of an eligible ordinary El Tor circuit, including its guard, middle, and introduction endpoint. Waiving only the introduction endpoint's fee would leave the other hops charging the operator. A service MUST choose a path whose relays support the allowance and currently admit free circuits. Availability remains subject to advertised resource limits; free capacity is not guaranteed under overload.

The allowance is based on enforceable resource use, not a claim that a circuit is for an onion service. Earlier relays MUST NOT need to learn the onion-service identity, full path, or introduction role to apply it. Any origin may use this limited capacity, including for other low-volume communication. Such use is an intentional subsidy; the design does not claim that a tiny allowance is useless to a freeloader.

### Relay policy and enforcement

A relay's normal advertised paid rate remains under the operator's control. Supporting relays MUST also advertise the free allowance and admission limits explicitly; clients MUST NOT infer support from a zero handshake fee or from the presence of an introduction endpoint. The allowance is a proposed protocol feature, not a behavior already guaranteed by the implementation.

Free capacity MUST have enforceable limits for:

- Sustained traffic per circuit, with a bounded burst allowance sufficient for legitimate introduction messages and maintenance.
- Traffic in both directions, including padding, with clearly defined cell/byte accounting and wire overhead treatment.
- Circuit creation rate, handshake computation, pending circuit builds, and concurrent established circuits.
- Total free bandwidth, circuit state/memory, and setup work across the relay. Per-circuit limits alone are insufficient because an origin can open many circuits and combine them.
- Queued data and queued work. Excess traffic must be bounded, throttled, or rejected without allowing unlimited buffering or silently increasing fees.

Enforcement must account for rebuilds, overlapping replacements, and attempts spread across multiple connections or claimed identities. Anonymous origins can create new identities, so identity-based limits alone are insufficient; the relay-wide resource ceiling remains mandatory. Exact limits, overload/fairness behavior, capability signaling, and accounting formats remain open until measured under local tests.

At the introduction endpoint, enforce the introduction protocol's permitted behavior and rate-limit forwarded requests. Do not permit an established introduction circuit to become an unrestricted application-data path. These endpoint checks complement resource limits at earlier hops; they do not prove the circuit's purpose to the guard or middle relay. Payload semantics at earlier hops remain encrypted.

### Circuit lifetime and payment behavior

Free introduction circuits follow the proposed ten-minute maximum circuit lifetime. The service establishes replacements before retiring old circuits and updates its descriptor when introduction points or their published keys change. Replacement overlap, setup, acknowledgments, and normal maintenance must fit the bounded free allowance. The descriptor upload circuit is separate and need not stay open just to keep the descriptor published. Descriptor distribution uses the proposed bounded free discovery policy above; exact capacity limits remain design work.

Eligible free setup and maintenance MUST NOT require the service to pay a normal paid-circuit handshake fee merely to remain reachable. The existing paid-circuit handshake-fee logic remains: relay operators set that fee, and new relays may advertise zero sats. This proposal does not introduce a new paid-activation fee or use the free allowance as a promise of full-bandwidth testing.

Rendezvous/application-data circuits continue to use one-minute payment rounds with bounded settlement padding and ten-round allocations. Continuing past an allocation requires negotiated paid renewal of the existing circuit; free-only circuit renewal is still undecided. Failure to make a required payment closes the paid circuit. A closed or unpaid paid circuit MUST NOT continue by switching into free mode. A separately admitted free circuit is still subject to the same small allowance and all aggregate limits.

The service MUST NOT automatically spend wallet funds when free capacity is exhausted. It may retry within bounded setup limits, select another supporting path, or temporarily lose introduction availability. Any choice to purchase additional capacity requires an explicit operator budget; unsolicited traffic cannot authorize that purchase.

### Required design validation

Before choosing numeric limits or claiming this is affordable, measure introduction message sizes, bursts, maintenance/padding, circuit setup, and descriptor-update work under the ten-minute rotation policy. Validate that a legitimate service remains reachable through repeated rotations without continuous relay payments. Separately test combined free circuits, rebuild storms, queue exhaustion, and attempts to carry bulk traffic. Verify that each hop enforces its local limits, aggregate resource use stays bounded, and exhausted free capacity cannot drain either wallet. This is future validation work, not a report of completed tests.
