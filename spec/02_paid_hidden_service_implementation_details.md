# Paid Hidden Service Specification — Implementation Details

**Status: proposed version-one specification; implementation deferred.**

Read the [architecture specification](02_paid_hidden_service.md) first. Detailed requirements now have focused chapters in the book sidebar. Existing section links are preserved below.

- [1. Common representation](onion-services/wire.md)
- [Detailed architecture and protocol requirements](history/historical-flow.md)
- [Circuit construction inherited from spec one](onion-services/construction.md)
- [Quote bootstrap through the introduction point](onion-services/bootstrap.md)
- [Quote reply authentication and encryption](onion-services/keys.md)
- [Authenticated payment-policy catalog and reference resolution](onion-services/catalog.md)
- [Quote acceptance and round-specific invoice acquisition](onion-services/acceptance.md)
- [Funded build status, client readiness, and setup failure](onion-services/status.md)
- [Slot lifetime, abuse bounds, and failures](onion-services/transport-policy.md)
- [El Tor service version agreement](onion-services/version.md)
- [Revision requirements](onion-services/authorization.md)
- [Service Circuit Quote (SCQ)](onion-services/pricing.md)
- [Payment timing and circuit rounds](onion-services/timing.md)
- [Renewing both rendezvous circuit halves](onion-services/renewal.md)
- [Payment methods and receiver privacy](onion-services/wallets.md)
- [Descriptor publication, retrieval, and free discovery](onion-services/discovery.md)
- [Bounded free allowance for introduction availability](onion-services/introductions.md)
- [Planning resume point](history/planning.md)

<span id="document-map"></span>

## Previous section locations

<span id="1-common-representation"></span> [1. Common representation](onion-services/wire.md#1-common-representation)

<span id="2-request-body-316-bytes"></span> [2. Request body (316 bytes)](onion-services/wire.md#2-request-body-316-bytes)

<span id="3-stable-reply-core-456-bytes"></span> [3. Stable reply core (456 bytes)](onion-services/wire.md#3-stable-reply-core-456-bytes)

<span id="4-fresh-reply-wrapper-and-signatures"></span> [4. Fresh reply wrapper and signatures](onion-services/wire.md#4-fresh-reply-wrapper-and-signatures)

<span id="5-protected-envelope-and-fixed-capacities"></span> [5. Protected envelope and fixed capacities](onion-services/wire.md#5-protected-envelope-and-fixed-capacities)

<span id="6-fragmentrouting-header-64-bytes"></span> [6. Fragment/routing header (64 bytes)](onion-services/wire.md#6-fragmentrouting-header-64-bytes)

<span id="7-privacy-validation-and-remaining-gates"></span> [7. Privacy, validation, and remaining gates](onion-services/wire.md#7-privacy-validation-and-remaining-gates)

<span id="detailed-architecture-and-protocol-requirements"></span> [Detailed architecture and protocol requirements](history/historical-flow.md#detailed-architecture-and-protocol-requirements)

<span id="how-to-pay-for-a-rendezvous-circuit-tor-hidden-service--lightning-network"></span> [How to pay for a Rendezvous Circuit (Tor Hidden Service + Lightning Network)](history/historical-flow.md#how-to-pay-for-a-rendezvous-circuit-tor-hidden-service--lightning-network)

<span id="circuit-construction-inherited-from-spec-one"></span> [Circuit construction inherited from spec one](onion-services/construction.md#circuit-construction-inherited-from-spec-one)

<span id="quote-bootstrap-through-the-introduction-point"></span> [Quote bootstrap through the introduction point](onion-services/bootstrap.md#quote-bootstrap-through-the-introduction-point)

<span id="chosen-architecture"></span> [Chosen architecture](onion-services/bootstrap.md#chosen-architecture)

<span id="request-and-reply-flow"></span> [Request and reply flow](onion-services/bootstrap.md#request-and-reply-flow)

<span id="quote-request-field-inventory"></span> [Quote request field inventory](onion-services/bootstrap.md#quote-request-field-inventory)

<span id="quote-reply-field-inventory"></span> [Quote reply field inventory](onion-services/bootstrap.md#quote-reply-field-inventory)

<span id="quote-reply-authentication-and-encryption"></span> [Quote reply authentication and encryption](onion-services/keys.md#quote-reply-authentication-and-encryption)

<span id="quote-signing-key-expiry-rotation-and-compromise"></span> [Quote-signing key expiry, rotation, and compromise](onion-services/keys.md#quote-signing-key-expiry-rotation-and-compromise)

<span id="request-encryption-and-service-decryption-key-lifecycle"></span> [Request encryption and service decryption-key lifecycle](onion-services/keys.md#request-encryption-and-service-decryption-key-lifecycle)

<span id="version-one-cryptographic-profile-and-binary-encoding-rules"></span> [Version-one cryptographic profile and binary encoding rules](onion-services/keys.md#version-one-cryptographic-profile-and-binary-encoding-rules)

<span id="initial-requestreply-byte-layout"></span> [Initial request/reply byte layout](onion-services/keys.md#initial-requestreply-byte-layout)

<span id="authenticated-payment-policy-catalog-and-reference-resolution"></span> [Authenticated payment-policy catalog and reference resolution](onion-services/catalog.md#authenticated-payment-policy-catalog-and-reference-resolution)

<span id="quote-acceptance-and-round-specific-invoice-acquisition"></span> [Quote acceptance and round-specific invoice acquisition](onion-services/acceptance.md#quote-acceptance-and-round-specific-invoice-acquisition)

<span id="funded-build-status-client-readiness-and-setup-failure"></span> [Funded build status, client readiness, and setup failure](onion-services/status.md#funded-build-status-client-readiness-and-setup-failure)

<span id="slot-lifetime-abuse-bounds-and-failures"></span> [Slot lifetime, abuse bounds, and failures](onion-services/transport-policy.md#slot-lifetime-abuse-bounds-and-failures)

<span id="fixed-size-quote-envelopes"></span> [Fixed-size quote envelopes](onion-services/transport-policy.md#fixed-size-quote-envelopes)

<span id="shared-response-timing"></span> [Shared response timing](onion-services/transport-policy.md#shared-response-timing)

<span id="bounded-client-retry-and-failover-timing"></span> [Bounded client retry and failover timing](onion-services/transport-policy.md#bounded-client-retry-and-failover-timing)

<span id="privacy-remaining-wire-work-and-validation"></span> [Privacy, remaining wire work, and validation](onion-services/transport-policy.md#privacy-remaining-wire-work-and-validation)

<span id="el-tor-service-version-agreement"></span> [El Tor service version agreement](onion-services/version.md#el-tor-service-version-agreement)

<span id="required-payment-protocol-and-strict-version-matching"></span> [Required payment protocol and strict version matching](onion-services/version.md#required-payment-protocol-and-strict-version-matching)

<span id="agreement-before-a-payable-quote"></span> [Agreement before a payable quote](onion-services/version.md#agreement-before-a-payable-quote)

<span id="required-features-and-hop-level-protocols"></span> [Required features and hop-level protocols](onion-services/version.md#required-features-and-hop-level-protocols)

<span id="mismatch-stale-metadata-and-errors"></span> [Mismatch, stale metadata, and errors](onion-services/version.md#mismatch-stale-metadata-and-errors)

<span id="validation-and-implementation-status"></span> [Validation and implementation status](onion-services/version.md#validation-and-implementation-status)

<span id="revision-requirements"></span> [Revision requirements](onion-services/authorization.md#revision-requirements)

<span id="1-bind-payment-authorization-to-the-client"></span> [1. Bind payment authorization to the client](onion-services/authorization.md#1-bind-payment-authorization-to-the-client)

<span id="2-define-who-funds-each-circuit"></span> [2. Define who funds each circuit](onion-services/authorization.md#2-define-who-funds-each-circuit)

<span id="3-specify-failures-retries-and-round-accounting"></span> [3. Specify failures, retries, and round accounting](onion-services/authorization.md#3-specify-failures-retries-and-round-accounting)

<span id="4-cap-spending-and-keep-wallet-work-off-the-handshake-path"></span> [4. Cap spending and keep wallet work off the handshake path](onion-services/authorization.md#4-cap-spending-and-keep-wallet-work-off-the-handshake-path)

<span id="service-circuit-quote-scq"></span> [Service Circuit Quote (SCQ)](onion-services/pricing.md#service-circuit-quote-scq)

<span id="definition-and-calculation"></span> [Definition and calculation](onion-services/pricing.md#definition-and-calculation)

<span id="descriptor-advertisement-and-required-terms"></span> [Descriptor advertisement and required terms](onion-services/pricing.md#descriptor-advertisement-and-required-terms)

<span id="fixed-price-and-acceptance-sequence"></span> [Fixed price and acceptance sequence](onion-services/pricing.md#fixed-price-and-acceptance-sequence)

<span id="incremental-payments-stopping-and-accepted-loss-exposure"></span> [Incremental payments, stopping, and accepted loss exposure](onion-services/pricing.md#incremental-payments-stopping-and-accepted-loss-exposure)

<span id="security-and-exposure-limits"></span> [Security and exposure limits](onion-services/pricing.md#security-and-exposure-limits)

<span id="shared-construction-and-validation"></span> [Shared construction and validation](onion-services/pricing.md#shared-construction-and-validation)

<span id="payment-timing-and-circuit-rounds"></span> [Payment timing and circuit rounds](onion-services/timing.md#payment-timing-and-circuit-rounds)

<span id="draft-schedule"></span> [Draft schedule](onion-services/timing.md#draft-schedule)

<span id="first-paid-minute-start-and-independent-circuit-schedules"></span> [First paid-minute start and independent circuit schedules](onion-services/timing.md#first-paid-minute-start-and-independent-circuit-schedules)

<span id="setup-deadline-and-startup-funding-cap"></span> [Setup deadline and startup funding cap](onion-services/timing.md#setup-deadline-and-startup-funding-cap)

<span id="authenticated-ready-and-bounded-recovery"></span> [Authenticated READY and bounded recovery](onion-services/timing.md#authenticated-ready-and-bounded-recovery)

<span id="candidate-recovery-and-ready-retry-limits"></span> [Candidate recovery and READY retry limits](onion-services/timing.md#candidate-recovery-and-ready-retry-limits)

<span id="initial-round-and-setup"></span> [Initial round and setup](onion-services/timing.md#initial-round-and-setup)

<span id="round-identifiers-acceptance-windows-and-deadlines"></span> [Round identifiers, acceptance windows, and deadlines](onion-services/timing.md#round-identifiers-acceptance-windows-and-deadlines)

<span id="round-payments-and-failure-behavior"></span> [Round payments and failure behavior](onion-services/timing.md#round-payments-and-failure-behavior)

<span id="allocation-boundary-and-validation"></span> [Allocation boundary and validation](onion-services/timing.md#allocation-boundary-and-validation)

<span id="renewing-both-rendezvous-circuit-halves"></span> [Renewing both rendezvous circuit halves](onion-services/renewal.md#renewing-both-rendezvous-circuit-halves)

<span id="scq-continuation-and-funding"></span> [SCQ continuation and funding](onion-services/renewal.md#scq-continuation-and-funding)

<span id="coordinated-preparation-and-activation"></span> [Coordinated preparation and activation](onion-services/renewal.md#coordinated-preparation-and-activation)

<span id="remaining-decisions-and-tests"></span> [Remaining decisions and tests](onion-services/renewal.md#remaining-decisions-and-tests)

<span id="payment-methods-and-receiver-privacy"></span> [Payment methods and receiver privacy](onion-services/wallets.md#payment-methods-and-receiver-privacy)

<span id="receiver-choice-and-negotiation"></span> [Receiver choice and negotiation](onion-services/wallets.md#receiver-choice-and-negotiation)

<span id="shared-verification-requirements"></span> [Shared verification requirements](onion-services/wallets.md#shared-verification-requirements)

<span id="privacy-properties-and-implementation-scope"></span> [Privacy properties and implementation scope](onion-services/wallets.md#privacy-properties-and-implementation-scope)

<span id="descriptor-publication-retrieval-and-free-discovery"></span> [Descriptor publication, retrieval, and free discovery](onion-services/discovery.md#descriptor-publication-retrieval-and-free-discovery)

<span id="ordinary-onion-service-publication-flow"></span> [Ordinary onion-service publication flow](onion-services/discovery.md#ordinary-onion-service-publication-flow)

<span id="el-tor-upload-circuit-lifetime"></span> [El Tor upload-circuit lifetime](onion-services/discovery.md#el-tor-upload-circuit-lifetime)

<span id="proposed-pricing-bounded-free-discovery"></span> [Proposed pricing: bounded free discovery](onion-services/discovery.md#proposed-pricing-bounded-free-discovery)

<span id="spam-and-resource-exhaustion-controls"></span> [Spam and resource-exhaustion controls](onion-services/discovery.md#spam-and-resource-exhaustion-controls)

<span id="validation-and-open-decisions"></span> [Validation and open decisions](onion-services/discovery.md#validation-and-open-decisions)

<span id="bounded-free-allowance-for-introduction-availability"></span> [Bounded free allowance for introduction availability](onion-services/introductions.md#bounded-free-allowance-for-introduction-availability)

<span id="purpose-and-scope"></span> [Purpose and scope](onion-services/introductions.md#purpose-and-scope)

<span id="relay-policy-and-enforcement"></span> [Relay policy and enforcement](onion-services/introductions.md#relay-policy-and-enforcement)

<span id="circuit-lifetime-and-payment-behavior"></span> [Circuit lifetime and payment behavior](onion-services/introductions.md#circuit-lifetime-and-payment-behavior)

<span id="required-design-validation"></span> [Required design validation](onion-services/introductions.md#required-design-validation)

<span id="planning-resume-point"></span> [Planning resume point](history/planning.md#planning-resume-point)
