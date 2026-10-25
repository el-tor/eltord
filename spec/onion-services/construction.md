> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# Circuit construction inherited from spec one

All circuits in this proposal MUST be built the same way as [spec one](../01_paid_circuits.md#shared-circuit-construction-and-bounded-free-allowance): descriptor upload/fetch circuits, both introduction paths, and both rendezvous paths inherit the ordinary El Tor per-hop payment construction. Retain the handshake hash/preimage fields, ten unique round identifiers per hop, per-hop encryption, and equal-size padding. Use fresh dummy handshake values where no fee applies; do not omit fields or introduce shared constants for free circuits.

The small free allowance is part of spec one's shared circuit/resource policy, not an onion-service-only circuit header or a shorter payment payload. No guard/middle payment field needs to label its origin as an onion service, and construction size or payment-field presence MUST NOT vary merely because a circuit is using the free allowance. Normal Tor introduction, directory, and rendezvous commands still operate at their intended endpoints. The client-to-service access authorization in this document is carried separately inside the service-encrypted introduction message; it does not replace or change the shared relay-payment construction.

One-minute paid rounds, ten-round payment allocations, early closure, and idle periods are ordinary El Tor lifecycle behavior. Paid renewal inherits spec one; without compatible renewal, expiration still requires replacement. This section preserves the same construction and resource policy across roles; it is not a claim that every traffic pattern is indistinguishable or that these proposed extensions are implemented.

**Service setup:** Before a client arrives, the service establishes circuits to its introduction points and publishes a descriptor listing them to hidden-service directories. The service keeps introduction circuits available using the bounded free allowance described below. Descriptor upload is a separate operation; publishing a descriptor does not require retaining its upload circuit. Introduction circuits carry introduction messages, while the later rendezvous circuits carry application traffic.

For the current sequence, follow the architecture overview and the bootstrap, acceptance and readiness chapters. The original payment-first walkthrough is retained only in the historical material.
