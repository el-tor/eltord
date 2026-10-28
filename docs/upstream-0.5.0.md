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

Both daemon and desktop development profiles keep the embedded C data plane at
optimization level 2 while the Rust application remains debuggable. This avoids
deploying an unoptimized Tor daemon from a debug source build. Different Cargo
feature graphs can still require separate artifacts; allow enough disk space.
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

## Acceptance results, 2026-10-28

- Standalone Tor: upstream ancestry verified; complete unit binary passes;
  functional suite has 32 passing groups, two environment skips, zero failures.
- Payment controls: 15 malformed commands rejected while Tor stays responsive.
- Eltord: source build succeeds; all 45 library tests pass, including prior
  payment-loop and bandwidth-test reliability regressions.
- LNI: both Phoenixd HTTP polling regressions pass. Its test fixture now
  supplies the current optional transaction date fields; production wallet
  behavior is unchanged by that fixture adjustment.
- Source parity: all 2256 tracked Tor files are present in the vendor tree;
  only the intended build helper and standalone testing-policy differences
  remain. Existing style diagnostics decrease from 361 to 343, with no new
  diagnostic content. The aggregate style gate remains an inherited backlog.
- Native desktop: frontend and Tauri builds succeed; app starts and Activate
  reaches a green Client indicator and Connected dialog on port 18048.
- Chutney: all nine running nodes report 0.5.0.0-alpha-dev and 100% bootstrap;
  transmission succeeds. Select the DNS-capable exit for external requests.
- Embedded Tor: control reports 0.5.0.0-alpha-dev, 100% bootstrap, and an
  established circuit. Running daemon bytes match the optimized build and both
  source and copied native resources; C compile flags contain `-O2`.
- Browser and data: Firefox renders the Tor Project website; socket inspection
  confirms its connections to the app's SOCKS router. Six repeated HTTPS page
  requests plus one 337736-byte transfer have zero failures over approximately
  76 seconds. The larger file's SHA-256 equals the direct reference. This
  measures completed HTTP requests, not packet loss or long-term reliability.

Raw local results, source parity checks, style comparison, build logs, and the
Firefox screenshot are under the workspace's `.runtime/upstream-0.5.0/` and
adjacent `.runtime/upstream-*` files. Keep raw logs private. The app, Firefox,
and testnet are left running for inspection. All work remains local; no changes
are pushed or published. Before using a remote-only source build, explicitly
publish the paired branches and update dependent source pins in a separate,
authorized step.
