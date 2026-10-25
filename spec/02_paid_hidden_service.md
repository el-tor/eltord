# Paid Hidden Service Specification

**Design status:** proposed El Tor protocol, with implementation deferred. This document explains the architecture, decisions and tradeoffs. The [technical protocol specification](02_paid_hidden_service_implementation_details.md) contains exact messages, byte layouts, cryptography, key management, state transitions, timing/retry policies and validation criteria. Pending work is tracked in the [El Tor TODO](../TODO.md).

## Purpose and scope

An onion-service client funds both sides of a rendezvous connection while the service stays reachable without continuous relay charges for its mostly idle introduction circuits. Payments buy capacity one minute at a time; they do not guarantee delivery or require purchasing ten minutes upfront.

This is an El Tor protocol. Payment handling is always present, including zero-fee cases. Ordinary Tor client compatibility and silent protocol downgrades are not part of the design. Client and service agree on a supported El Tor version before paying.

## How an onion connection works

The service chooses introduction points, keeps circuits open to them, and publishes a descriptor to hidden-service directories (HSDirs). The descriptor tells a client how to find those introduction points. It is not an advertisement of a permanent rendezvous point.

The client chooses a rendezvous point for its connection. An introduction point passes the invitation to the service; the client and service then connect to the rendezvous over separate three-hop circuits. Website traffic uses those two circuits, not the introduction point.

Descriptor publication is a short operation. Its upload circuit and the continuously available introduction circuits are separate. Closing an upload circuit does not delete the published descriptor.

[Detailed Tor flow and construction](02_paid_hidden_service_implementation_details.md#circuit-construction-inherited-from-spec-one).

## Free discovery and introduction availability

A tiny, bounded, purpose-independent free allowance supports descriptor publication/retrieval, introduction availability, and the control messages needed to agree on payment. Every selected relay must support and admit it. Ordinary clients can use that same allowance; relays do not need to identify an onion-service role in a guard or middle-hop payment header.

The allowance is not unrestricted bandwidth. Both per-circuit and aggregate traffic, setup, memory, CPU and queue limits are required, including protection against many fake services or circuits. Exhaustion cannot silently spend the service's wallet funds. Standing introduction costs remain within these limits; application traffic uses paid capacity.

All circuits inherit [spec one's shared construction](01_paid_circuits.md#shared-circuit-construction-and-bounded-free-allowance), including uniform encrypted payment fields, padding and dummy values when appropriate. New relays may keep their existing zero-sat paid handshake policy; the free allowance does not introduce a new trial fee.

[Discovery and resource rules](02_paid_hidden_service_implementation_details.md#descriptor-publication-retrieval-and-free-discovery) · [Introduction availability](02_paid_hidden_service_implementation_details.md#bounded-free-allowance-for-introduction-availability).

## Who pays and how pricing works

The client pays its own relays to the rendezvous directly. It also pays the service, which privately pays the relays on its own path to that rendezvous. The service must receive and reserve the required funding before incurring connection-specific paid work.

The service sends a **Service Circuit Quote (SCQ)** for its side. Relay operators set their own rates; the service totals the selected rates and discloses routing allowances, setup charges and any application fee. The client may reject the quote. The accepted price stays fixed under the agreed circuit/continuation terms, while payment remains incremental.

The service's private relay identities are not disclosed. Its authenticated quote is a price commitment, not independent proof of its relay costs, capacity or performance. Payments are bound to one client-authorized circuit and round, not a reusable session balance.

[SCQ terms and accepted exposure](02_paid_hidden_service_implementation_details.md#service-circuit-quote-scq).

## Agreeing before spending

1. The client verifies the service descriptor, supported version, payment method and terms.
2. Over bounded free introduction paths, it requests a quote. The service returns an authenticated encrypted quote without building a new paid circuit for that inquiry.
3. The client verifies the quote and its referenced payment-policy catalog. Unknown, missing or substituted terms stop payment.
4. The client signs acceptance of the exact quote, circuit, limits and initial round, then obtains that round's invoice through the agreed method.
5. Before paying, it verifies the invoice's receiver, amount, validity and association with the accepted quote and round.
6. The service checks settlement through its own node, reserves funds for that circuit/round, and only then authorizes paid construction.
7. Later minutes repeat the round-specific invoice/payment process within the accepted terms and budgets. The client can stop renewing.

Retries recover the same logical acceptance and payment state with fresh encrypted deliveries. A timeout is not evidence a payment failed and cannot authorize duplicate charges or builds. Descriptor and key updates do not silently change accepted prices, recipients or policies.

[Quote bootstrap](02_paid_hidden_service_implementation_details.md#quote-bootstrap-through-the-introduction-point) · [Payment-policy catalog](02_paid_hidden_service_implementation_details.md#authenticated-payment-policy-catalog-and-reference-resolution) · [Acceptance and invoices](02_paid_hidden_service_implementation_details.md#quote-acceptance-and-round-specific-invoice-acquisition).

## Payment rounds, renewal and failures

Payments cover one-minute rounds. The initial allocation contains ten rounds; it does not force a ten-round purchase. Pay early enough for the service to receive funding and pay its relays, with advance notice separate from late-payment grace.

A healthy paid circuit can continue through explicitly agreed renewal, preserving the relays and existing streams. Both rendezvous halves must be prepared and funded. Without compatible renewal or timely payment, closure follows the agreed deadlines. A backup circuit alone cannot migrate an existing download; application/server resumption may be needed after failure. Free-only renewal remains undecided.

The design limits exposure but cannot eliminate loss. A peer may fail after payment. Exposure can include the unused current round, the prepaid next round and disclosed setup/routing fees. Refunds are not automatic, uncertain payments must be reconciled, and funds cannot silently be moved to another circuit or round. Standing free circuits retain their bounded replacement policy.

[Timing and funding](02_paid_hidden_service_implementation_details.md#payment-timing-and-circuit-rounds) · [Two-half renewal](02_paid_hidden_service_implementation_details.md#renewing-both-rendezvous-circuit-halves) · [Failures and accounting](02_paid_hidden_service_implementation_details.md#3-specify-failures-retries-and-round-accounting).

## Wallet support and privacy

Support the protocol's BOLT 11 invoice and BOLT 12 offer-based flows through actual wallet integrations. Each receiver advertises what it supports, and the client chooses a mutually supported method. Receiver privacy is the receiver's choice; blinded BOLT 12 paths are preferred where supported, not mandatory.

Keep stable payment/client references and service-path details encrypted, authenticate both sides' authorization, and avoid shared identifiers across hops or unrelated circuits. Fixed-size padded control messages, fresh retry encryption, bounded timing policies and short-lived delegated keys reduce specific disclosures. They do not prove anonymity or prevent an observer from correlating traffic.

Routine key rotation preserves accepted terms; known compromise requires suspending dependent new authorizations/payments until trusted recovery. The technical document distinguishes signer and decryption keys, cached-descriptor risks and the limits of recipient-key compromise protection.

[Method/privacy policy](02_paid_hidden_service_implementation_details.md#payment-methods-and-receiver-privacy) · [Technical privacy and crypto rules](02_paid_hidden_service_implementation_details.md#quote-reply-authentication-and-encryption).

## Open work and where to resume

Next discussion: authenticated funded-build/start/readiness signaling and failure after funding. Exact acceptance/invoice/status/continuation wire, descriptor/delegation/catalog schemas, concrete timing/resource budgets, relay price commitments, lifetime policies and independent security review remain open in the technical specification and TODO. Runtime implementation, interoperability and correlated paid-network/privacy measurements remain deferred.

The separate paid-circuit replay-protection investigation stays deferred. Existing source-review findings, synthetic sizing checks and earlier free-mode tests do not establish support for this proposed protocol.

**Privacy/timing assessment:** this document split changes presentation and requirement locations, not agreed wire, accounting or timing behavior. The newly recorded acceptance/invoice requirements and their remaining validation are preserved in the technical companion under ET-28. Follow the [spec workflow](AGENTS.md) for both document layers; documentation checks are not runtime or anonymity evidence.
