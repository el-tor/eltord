# Inherited Tor behavior and El Tor changes

This is an extension map, not a copy of Tor's specification or an upstream
allocation. Upstream documentation is a living reference. Pin an exact upstream
source baseline and tested El Tor revision in a release profile before claiming
wire compatibility; this draft does not select a new baseline by implication.

| Mechanism | Upstream reference | El Tor extension or qualification |
| --- | --- | --- |
| Channels, circuit construction and relay cells | [Core Tor](https://spec.torproject.org/tor-spec/index.html) | Per-hop payment construction and proposed renewal commands. The inspected fork uses larger cells; logical payload budgets do not resize them. |
| Path selection, guards and stream isolation | [Path specification](https://spec.torproject.org/path-spec/index.html) | Price/capability admission must preserve guard/isolation constraints; backups do not migrate streams. |
| Relay descriptors | [Directory protocol](https://spec.torproject.org/dir-spec/index.html) | Advertised prices, payment methods and capabilities need authenticated encodings and initial term commitments. |
| Onion descriptors, introductions and rendezvous | [Onion services](https://spec.torproject.org/rend-spec/index.html) | Free bounded discovery, SCQ return path, authorization and funding. INTRODUCE_ACK alone is not an end-to-end quote reply. |
| Padding | [Padding specification](https://spec.torproject.org/padding-spec/index.html) | Preserve ordinary padding; additionally pad specified control envelopes. Neither hides all timing correlations. |
| Controller authentication, command framing and replies | [Control protocol](https://spec.torproject.org/control-spec/index.html) | Document El Tor commands separately from relay-cell commands and Rust helper APIs. |

Wire commands proposed here are not upstream Tor assignments. El Tor version
agreement is mandatory; unsupported peers fail without silent downgrade or
ordinary-Tor fallback. Method support and per-hop renewal admission are separate
from end-to-end service version selection.
