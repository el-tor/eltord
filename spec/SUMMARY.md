# Summary

[About these specifications](index.md)

# Introduction

- [Introduction](00_spec.md)
  - [Inherited Tor behavior and El Tor changes](tor-relationship.md)

# Paid circuits

- [Paid circuit architecture](01_paid_circuits.md)
  - [Construction and free allowance](circuits/free-allowance.md)
  - [Initial construction and accounting](circuits/initial-construction.md)
  - [Initial commitment wire profile](circuits/initial-profile.md)
  - [Renewal semantics](circuits/renewal.md)
  - [Renewal wire protocol](01a_circuit_renewal_wire.md)

# Paid onion services

- [Paid hidden service architecture](02_paid_hidden_service.md)
  - [Inherited construction](onion-services/construction.md)
  - [Descriptor discovery](onion-services/discovery.md)
  - [Descriptor metadata encoding](onion-services/metadata.md)
  - [Quote transport commands](onion-services/transport.md)
  - [Acceptance and invoice wire](onion-services/payment-messages.md)
  - [Status and readiness wire](onion-services/readiness-wire.md)
  - [Paid introduction binding](onion-services/paid-introduction.md)
  - [Introduction availability](onion-services/introductions.md)
  - [Version agreement](onion-services/version.md)
  - [Service Circuit Quote and pricing](onion-services/pricing.md)
  - [Authorization and funding](onion-services/authorization.md)
  - [Bootstrap request and reply](onion-services/bootstrap.md)
  - [Payment-policy catalog](onion-services/catalog.md)
  - [Acceptance and invoices](onion-services/acceptance.md)
  - [Build status and readiness](onion-services/status.md)
  - [Rounds, setup deadlines and READY recovery](onion-services/timing.md)
  - [Renewing both halves](onion-services/renewal.md)
  - [Wallet methods and receiver privacy](onion-services/wallets.md)

# Protocol details

- [Implementation details index](02_paid_hidden_service_implementation_details.md)
  - [Cryptography and key lifecycle](onion-services/keys.md)
  - [Initial quote wire layouts](onion-services/wire.md)
  - [Transport, padding, timing and retries](onion-services/transport-policy.md)

# Control and RPC

- [Control protocol extensions](control/index.md)
  - [Commands](control/commands.md)
  - [Proposed renewal controller protocol](control/renewal.md)
  - [Events and errors](control/events.md)
  - [Initial operations and lifecycle](control/lifecycle.md)
  - [Durable authorization and recovery](control/recovery.md)
  - [Recoverable service build](control/service-build.md)
  - [Daemon helpers and proposed interfaces](control/daemon.md)

# Validation and open work

- [Validation](validation/index.md)
  - [Protocol design checklist](validation/design-checklist.md)
  - [Existing implementation comparison](validation/source-comparison.md)
  - [Implementation and deployment gates](validation/gates.md)
  - [Consistency review](validation/review.md)
  - [Implementation backlog](validation/backlog.md)

# Historical material

- [Historical material](history/index.md)
  - [Original TOPS proposal](history/tops-proposal.md)
  - [Original paid circuit flow](history/paid-circuit-flow.md)
  - [Original paid service flow](history/historical-flow.md)
  - [Earlier progress report](roadmap/00_milestone.md)
  - [Earlier architecture diagram](roadmap/diagram.md)
  - [Previous planning resume note](history/planning.md)

# Maintenance

- [Editing and local builds](maintenance.md)
  - [Specification workflow](AGENTS.md)
