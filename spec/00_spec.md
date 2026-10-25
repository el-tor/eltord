# Introduction

El Tor adds incremental Lightning payments to onion-routed bandwidth. Clients
buy short rounds, can stop renewing, and accept bounded exposure when a peer
fails after payment. Payments do not guarantee delivery or refunds.

## Riga presentation

Watch the [El Tor presentation in Riga](https://www.youtube.com/watch?v=96n34yZAO1U&t=825s)
for background on the project. The link starts at **13:45**. The chapters in this
book define the current proposed protocol; the presentation provides context.

## Architecture

An origin selects relays and their advertised terms, establishes a circuit with
independent payment information for each hop, and funds one-minute rounds.
The initial ten identifiers are an allocation, not a ten-minute purchase.
Negotiated renewal replenishes identifiers while preserving the circuit and its
streams. A bounded free allowance supports small control and discovery traffic.

For onion services, the client pays its own rendezvous path and funds the
service's private path through a Service Circuit Quote. The service verifies and
reserves funds before paid work. Introduction paths and descriptor discovery use
bounded free capacity; application capacity follows the paid-round policy.

## Relationship to Tor

El Tor inherits Tor mechanisms where this book explicitly references them; it
does not claim compatibility with ordinary Tor clients or arbitrary future Tor
versions. The [inheritance map](tor-relationship.md) identifies the relevant
upstream chapters and El Tor differences. A release must pin the tested Tor
baseline and El Tor capability profile before claiming interoperability.

## Terminology

| Term | Meaning |
| --- | --- |
| Channel | Link between adjacent Tor participants. |
| Circuit | Onion-routed path; each rendezvous half is its own circuit. |
| Stream | Application traffic attached to a circuit; a backup circuit does not migrate an established stream. |
| Round | One interval of paid capacity, normally 60 seconds in this draft. |
| Allocation | A batch of ten per-hop round identifiers; registration is not payment. |
| SCQ | Service Circuit Quote for the service-to-rendezvous path. |
| Setup attempt | One explicitly authorized construction attempt under an accepted quote. |
| Payment ID | Protocol lookup identifier; not necessarily a Lightning invoice payment hash. |

Requirements using MUST/SHOULD apply to the proposed profile, not to a claim
about existing code. Byte encodings and domain separators are protocol-specific:
never substitute a renewal encoding for a quote encoding because symbols match.

## Scope and limitations

Payment handling is mandatory even with zero fees. Receiver privacy is a choice;
blinded BOLT 12 is preferred where supported, and BOLT 11 invoices are supported
by the design. Uniform fields and padding do not prove timing anonymity.
All free allowances, startup work, retries and payments require finite budgets.

The [original TOPS proposal](history/tops-proposal.md) is historical. Its payment
schedule and stronger privacy language do not override the current chapters.
