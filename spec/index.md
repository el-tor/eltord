# El Tor specifications

**Status: proposed version-one specification.** This book describes El Tor's
payment extensions to Tor. It is a design draft, not a claim that every feature
is implemented or interoperable. Runtime implementation remains deferred.

Start with [Introduction](00_spec.md), then read the architecture chapters for
[paid circuits](01_paid_circuits.md) and
[paid onion services](02_paid_hidden_service.md). Their technical chapters define
protocol details. [Control and RPC](control/index.md) distinguishes source-observed
interfaces from proposed commands. [Validation and open work](validation/index.md)
records what must be resolved before implementation or deployment.

The organization follows the topical chapters and nested navigation of the
[Tor specifications](https://spec.torproject.org/toc.html), with independent
El Tor chapter numbering. Links and protocol identifiers, not chapter numbers,
are stable references. Tor's specifications remain authoritative for inherited
Tor mechanisms; this book documents El Tor additions and explicit differences.

The [table of contents](SUMMARY.md) works directly in Markdown. A local mdBook
build provides the sidebar, search and chapter navigation. Nothing in the build
publishes this book or connects it to a remote repository.

## Reading status labels

- **Proposed:** an agreed design direction; implementation is not implied.
- **Source-observed:** found in the named local source revision; not a runtime test.
- **Candidate parameter:** a value to measure before deployment.
- **Implementation gate:** missing definition, review or evidence that prevents
  claiming complete interoperable support.
- **Historical:** retained context, not current requirements.

See [editing and local build instructions](maintenance.md).
