# Local upstream Tor 0.5.0 integration

This integration uses the `merge-upstream-0.5.0` branches of the sibling Tor,
`libeltor-sys`, Eltord, and El Tor App repositories. The Tor merge imports
upstream commit `4792124a79a10fd66ecfd4f16771396372d744a0` from the official
Tor GitLab repository. Its C version is **0.5.0.0-alpha-dev**, a development
snapshot. This does not implement the proposed paid-onion-service or renewal
specifications.

## Source graph

- Standalone Chutney relays use the merged `tor/src/app/tor` binary.
- Eltord embeds the separately built source under
  `libeltor-sys/libtor-src/tor-src`, through the existing `libeltor` wrapper.
- The native app launches its resource daemon `backend/bin/eltord`; rebuilding
  the app does not, by itself, rebuild that resource.
- LNI is pinned to `ac6e0eeb5b9103fdc2610c313535baf639145191`, which includes
  the Phoenixd transient-polling fix. Its current callback API uses `Arc`.
  Eltord's watcher and test adapter use that ownership model.
- The lockfile accommodates the current LNI graph, including already-locked
  transitive versions that cannot be freshly selected after being yanked.
  Keep the lockfile rather than regenerating it from an empty graph.

The sibling source overrides are opt-in workspace configuration. These branches
are local; their unpublished commits cannot be fetched by a normal remote-only
Cargo build. Use the workspace's `cargo-local.sh` and
`.cargo/local-sources.toml` to select the matching sibling sources. No changes
to global Cargo or Git settings are necessary.

## Build and deploy

From the workspace root:

```sh
# Native prerequisites include protobuf-compiler and libprotobuf-dev.
# Keep the normal Rust toolchain selection unchanged.
./cargo-local.sh build --manifest-path eltord/Cargo.toml
# Deactivate and stop the app before replacing its resource daemon.
install -m 755 target/debug/eltor eltor-app/backend/bin/eltord
(cd eltor-app/frontend && pnpm run build)
./desktop.sh build
```

The desktop's optimized development profile builds the C data plane separately
from the ordinary daemon development profile. Allow enough disk space for both.
Keep local wallet executables and configuration intact.

## Protocol and test boundaries

The fork retains 2048-byte cells and its payment control protocol. Upstream's
CGO mode assumes 509-byte cells and is disabled for this profile; TOR1 protects
the whole enlarged cell. The standalone testing-network unpaid extension
exception remains separate from the stricter vendored relay policy and from
any proposed free traffic allowance.

The Tor integration notes describe unit, framing, handshake, payment-control,
and full functional-suite checks. Runtime acceptance additionally requires
nine-node Chutney bootstrap and transmission, activation of the freshly rebuilt
native app, and an HTTPS page rendered in the dedicated Firefox SOCKS profile.
Use `test004r` for public DNS: other synthetic Chutney exits may intentionally
have DNS disabled. See the workspace `AGENTS.md` runbook for startup, listener
addresses, log locations, and shutdown. A free-mode Chutney smoke test does not
validate Lightning settlement or interoperability with the public Tor network.
