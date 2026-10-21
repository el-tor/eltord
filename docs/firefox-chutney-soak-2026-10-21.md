# Firefox and desktop-app Chutney soak — 2026-10-21

The clean **20-minute traffic window completed with zero failed requests**.
Firefox used the native desktop app's SOCKS5 router at `127.0.0.1:18048`, then
the embedded client and the existing nine-node local Chutney network.

The measured window ran from **16:05:55.671 to 16:25:55.671 UTC**. Requests were
scheduled for exactly 1,200 seconds; the last browser request began at second
1,199.787 and completed 0.105 seconds after the scheduling deadline. Collection
and the final screenshot finished at second 1,204.216. No deliberate circuit
closure, relay pause, daemon restart, or binary replacement occurred in this
window.

## Results

| Traffic | Requests | Failures | Verified payload bytes | Median | p95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Firefox repeated downloads | 3,192 | 0 | 1,268,219,904 | 155 ms | 659 ms |
| Firefox paced 8 MiB transfers | 20 | 0 | 167,772,160 | 31.857 s | 31.889 s |
| Firefox public HTTPS page loads | 20 | 0 | Not included in payload totals | 351 ms | 431 ms |
| curl through app router | 1,600 | 0 | 1,048,576,000 | 131 ms | 503 ms |
| curl through embedded client | 1,600 | 0 | 1,048,576,000 | 121 ms | 477 ms |
| curl through standalone Chutney client | 1,600 | 0 | 1,048,576,000 | 181 ms | 330 ms |
| Direct origin control | 1,600 | 0 | 1,048,576,000 | 1.8 ms | 3.5 ms |

Firefox completed **3,212 verified downloads totaling 1,435,992,064 bytes
(1.34 GiB)**. Including curl through the same app router, that path carried
4,812 complete payload requests and 2,484,568,064 verified bytes, plus the 20
public page loads. Across all routes, 9,612 payload requests delivered
5,630,296,064 verified bytes. Direct-control traffic is included only in this
last all-routes total.

There were **zero request timeouts, incomplete payloads, HTTP failures, or hash
mismatches**. These are application-level request results; network packet loss
was not measured directly. The workload was deliberately paced, so delivered
bytes per second do not establish maximum bandwidth capacity.

## Comparison with the earlier runs

The comparable curl repeated-request phases used two workers per route, one
request per worker approximately every 1.5 seconds, with 256 KiB and 1 MiB
payloads. This run added the concurrent Firefox workload and lasted 20 minutes;
the earlier phases lasted eight minutes.

| App-router repeated requests | Requests | Failures | Median | p95 |
| --- | ---: | ---: | ---: | ---: |
| Earlier baseline | 630 | 0 | 650 ms | 1,657 ms |
| Earlier after-fix run | 633 | 0 | 593 ms | 1,567 ms |
| Current Firefox-parallel run | 1,600 | 0 | 131 ms | 503 ms |

The current router median was **79.8% lower, or 4.94 times faster**, than the
earlier baseline measurement. This is an observed whole-run difference:
circuit paths, background load, build settings, and the additional browser
workload differ. It does not isolate a causal speedup from one patch.

Both earlier valid soaks also had zero request failures. Consequently this
experiment cannot quantify a percentage reduction in intermittent failures.
It provides a longer clean observation with real Firefox traffic, repeated
public HTTPS loads, and three circuit-generation replacements. The earlier
fragmented-SOCKS reproduction remains separate evidence: nine router probes
failed before the framing fix, and all 36 multiport probes passed afterward.

## Method and log correlation

Firefox 158.0 ran visibly in a disposable profile with SOCKS v5, remote DNS,
no proxy exclusions, direct fallback disabled, local-address proxying enabled,
and both memory and disk caches disabled. Four workers repeatedly fetched
16 KiB, 256 KiB, 256 KiB, and 1 MiB payloads. A fifth worker fetched one paced
8 MiB stream per minute. A separate tab navigated to a unique HTTPS
`example.com` URL once per minute and checked the resulting page title.

Browser payloads used an origin on port 18881, separate from curl on 18880.
Captured app-client STREAM events identify six successful SOCKS5 connections
to the browser origin; Firefox reused these HTTP connections. Each payload
carried a unique request ID and was checked for HTTP status, length, and SHA256.
An independent Python check revalidated every recorded browser hash. All 3,212
browser IDs and all 6,400 curl IDs match unique, fully sent origin records with
no origin write errors.

Control events and notice/info logs were captured from the app client and all
nine Chutney nodes on the same UTC clock. Every node's event capture continued
through the end of the window. There were no monitor errors, app warning lines,
app error lines, or client STREAM FAILED events during the clean run.

The controller completed three generations and began replacement monitors at
16:09:02, 16:15:48, and 16:22:34 UTC. The last pair was circuits 53 and 54.
Traffic continued successfully across all three transitions.

The app client recorded 24 internal one-hop circuit closures and four unused
CONFLUX_UNLINKED build failures with NOPATH around 16:22:33–16:22:36 UTC. None
corresponded to a failed measured request. Circuit IDs are process-local and
must not be equated numerically across relay and client logs.

## Additional defects found during preparation

1. **Nickname exit preferences were ignored.** The app configured
   `ExitNodes test004r`, while the Rust selector only matched fingerprints.
   A preflight HTTPS request timed out after 20 seconds, and logs showed other
   exits being selected. The selector now recognizes case-insensitive nicknames
   and comma-separated nickname/fingerprint lists. Two selection regressions
   pass. This change was compiled and deployed before the clean measurement.

2. **A stopped embedded Tor left its controller process alive.** SIGTERM stopped
   the old Tor event loop, but a Rust blocking task then slept forever and client
   tasks kept reconnecting. After replacement, the old controller attached to
   the new control socket and competed over stream assignment. The partial run
   with both controllers is preserved separately and excluded from all tables.
   The old process was fully terminated before the clean window, with one app
   client daemon verified throughout it.

The shutdown fix removes the post-Tor infinite sleep, cancels controller flows
when Tor returns, handles shutdown during initialization, and preserves nonzero
Tor exit status. It was built and tested **after** the 20-minute window so the
measured daemon binary remained constant. Disposable network-disabled instances
exercise startup SIGTERM, active SIGTERM, and active controller shutdown. The
previous binary survived all three checks and had to be killed by the test.
The corrected binary exited cleanly in all three cases in **31–67 ms**. An
invalid-configuration check also exited promptly with nonzero status. Six
focused unit regressions and six controller integration tests pass, and the
final daemon build succeeds.
The final deployment again built both circuits, Firefox loaded a fresh public
HTTPS page through the proxy, and Chutney transmission verification and all nine
node status checks passed.

The measurement used daemon SHA256
`7108a3dc3c3f6e6b876796e078b9d80aa385311bf39c0527b9e4e906970a9ddc`.
The post-measurement shutdown fix changes lifecycle behavior; it is not included
in the latency comparison above.

## Artifacts and limits

Raw results remain local under `.runtime/firefox-soak-2026-10-21/`: `run/`
contains requests, origin records, control events, logs, comparisons, correlation
checks, and the final Firefox screenshot. `excluded-mixed-controllers/` is
explicitly excluded. `lifecycle-before/` and `lifecycle-after/` contain isolated
shutdown regressions. These private artifacts are not committed.

Reusable harnesses live in `tools/stability/firefox_soak.py` and
`tools/stability/daemon_lifecycle.py`; see `tools/stability/README.md`.

This is a local zero-fee Chutney result. It does not exercise funded Lightning
settlement, WAN packet loss, or every cause of historical intermittent outages.
All changes and commits remain local on `intermitted-fix` under the repository
identity `Eltor Dev <eltordev@proton.me>`.
