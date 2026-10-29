# Native bounded-free resource acceptance

`tools/foundation_resource_acceptance.py` prepares and runs one finite local
experiment against the actual native ordinary-circuit constructor. It never
starts/stops Tor, applies configuration, builds code, contacts a wallet,
provisions a ledger, sends payment commands, or deletes an earlier result.
Preparation creates fresh private client directories; existing Chutney identities,
wallets, configurations and financial histories are not copied or replaced.

The workload spaces requests by one second per stress origin so the shared
setup-rate limit does not prevent reaching the live-state ceiling.

The default `admission` scenario measures shared **live-circuit capacity**,
refusal across independent origin processes and honest recovery. It samples other resource
ceilings, but does not saturate every byte/CPU/queue ceiling or establish
production performance, timing privacy, paid settlement or Sybil resistance.

## Operator preparation

Coordinate with the operator before changing the running local network. Build
and deploy matching native code containing authenticated aggregate telemetry,
ordinary bounded-free construction and shared origin work admission. Record the
actual native executable SHA256; the runner checks `/proc/<pid>/exe` for every
participating process, not only a source revision or a file on disk.

Prepare a **new** runtime directory from the workspace root, using the existing
public loopback authority definitions:

```sh
python3 eltord/tools/foundation_resource_acceptance.py prepare \
  --nodes chutney/net/nodes --template chutney/net/nodes/005c/torrc \
  --directory .runtime/foundation-resource/RUN \
  --path 006m,007m,004r --target 006m \
  --origins 4 --per-origin 16 --max-circuits 24
```

`RUN` must be a new operator-selected run name. The command only writes private
files. Each generated origin has a new data directory, cookie-authenticated UNIX
control, an automatically allocated loopback SOCKS port, full descriptors,
predicted circuits disabled and application streams left unattached. These are
dedicated test clients; their controls must never be pointed at a personal
browser or wallet. The runner attaches each small echo explicitly to its owned
base circuit. It does not alter ordinary app or standalone-client behavior.

Before running, the operator must:

1. Back up the bottleneck relay configuration through the existing local stack
   workflow. Deploy the generated `target-overload.fragment` on that relay and
   restart it deliberately. The example lowers only its immutable live-circuit
   ceiling to 24; candidate production defaults and advertised 2/128/600000
   per-circuit profile remain unchanged. Leave the other hops with sufficient
   headroom. Do not apply this fragment merely to run ordinary transport checks.
2. Launch each generated `origin-N/torrc` with the pinned native binary through
   the existing supervised-process workflow, using one process per directory.
   No daemon/payment controller is required for these native base-only clients.
   Wait for descriptor-ready bootstrap (at least 75%) and full signed descriptors.
   With predicted circuits disabled, the first explicit circuit brings the
   honest client to 100%; the runner checks that transition before stress.
3. Ensure the approved exit permits the temporary loopback echo sink, as the
   existing free Chutney verification exit does. All listeners stay on loopback
   or private UNIX sockets. No public network endpoint is contacted.

Invoke the executable acceptance only after those explicit deployment steps:

```sh
python3 eltord/tools/foundation_resource_acceptance.py run \
  --directory .runtime/foundation-resource/RUN \
  --binary-sha256 EXPECTED_NATIVE_SHA256
```

The runner creates a one-time `run.claim` before dispatching any constructor.
A lost constructor reply is uncertain and is never retried. Do not remove the
claim to repeat a run. Keep the result and private native logs; an independently
approved later experiment uses another runtime directory.

## Required evidence

The run fails unless every selected hop publishes the exact signed free
capability, the origin actually builds ordinary native circuits, each sampled
owned circuit binds as base-only without financial authority, and the real
shared live-circuit counter reaches the configured lower cap. A new request from
the separate honest process must then fail while an already admitted honest
circuit still carries a checksum-verified tiny echo. The runner closes only its
own receipted circuits, waits for actual teardown/normal token refill and proves
a fresh honest circuit and echo succeed afterward. Neither a new identity nor
controller reconnection resets the shared owner.

`result.json` contains finite counts, binary provenance, sampled peaks and honest
transport times. It omits cookies, relay paths, circuit IDs/handles, payment
identifiers, keys and arbitrary exception/controller bodies. Files remain local
and private. The small sample is not a meaningful percentile distribution or a
network guarantee. A subsequent workload must separately exercise simultaneous
byte pressure, queues, attempted crypto limits, cancellation and honest recovery.

The runner closes owned probe circuits and its temporary echo listener. The
operator remains responsible for stopping only the generated fixture processes
and restoring the backed-up relay configuration through the normal launcher.
It never resets a ledger or erases histories to regain capacity.


## Separate traffic and setup scenarios

The additional scenario definitions are executable acceptance candidates. The
local record below distinguishes passed and failed executions. Run them only
after the sustained app/paid-stream experiment finishes
and the matching final native binaries are deployed. Each needs a new private
runtime directory, its own one-time claim, explicit lower-cap relay deployment,
and dedicated origins. Do not reuse an earlier claimed directory, change an app
mapping, replace identities, or restart a running app to obtain a result. The
same cookie, process-digest, signed-capability, base-only binding and owned-stream
checks apply. Old admission manifests remain supported.

The following preparation commands are alternatives, performed separately:

```sh
python3 eltord/tools/foundation_resource_acceptance.py prepare \
  --scenario traffic --traffic-profile startup-v2 --nodes chutney/net/nodes \
  --template chutney/net/nodes/005c/torrc \
  --directory .runtime/foundation-resource/TRAFFIC-RUN \
  --path 006m,007m,004r --target 004r \
  --origins 4 --per-origin 8 --max-circuits 64

python3 eltord/tools/foundation_resource_acceptance.py prepare \
  --scenario setup --nodes chutney/net/nodes \
  --template chutney/net/nodes/005c/torrc \
  --directory .runtime/foundation-resource/SETUP-RUN \
  --path 006m,007m,004r --target 006m \
  --origins 4 --per-origin 8 --max-circuits 64
```

Deploy only that scenario's generated fragment on its target, preserving the
configuration backup, then launch its dedicated clients using the normal
supervisor workflow. The runner requires every scenario option to match exactly
through authenticated GETCONF. Invoke `run --directory ... --binary-sha256 ...`
as above. The runner has a 120-second experiment deadline, up to four origins,
three pressure workers and 1200 observations at a requested 100 ms interval.
Cleanup gets a separate bounded control deadline. It does not start processes,
apply the fragment, reset counters, consult wallets, or send payment commands.

| Scenario | Additional immutable target limits | Maximum constructor attempts with four origins |
| --- | --- | ---: |
| Traffic `small-v1` at exit `004r` | byte rate 2053 B/s, burst 16424 B; connection rate 8192 B/s, burst 32768 B; shared outgoing queue 65536 B | 9 |
| Traffic `startup-v2` at exit `004r` | byte rate 16384 B/s, burst 65536 B; connection rate 8192 B/s, burst 32768 B; shared outgoing queue 65536 B | 9 |
| Traffic `retry-paced-v3` at exit `004r` | exactly the `startup-v2` resource limits, with the separately recorded retry/prediction options below | 9 |
| Setup at first hop `006m` | attempted work rate 1/s, burst 4; running 1, waiting 2 | 26 |

The `traffic_profile` manifest field names the reviewed traffic limits. Older
manifests without it retain the exact `small-v1` interpretation. The later
`startup-v2` candidate increases only the process byte rate/burst to cover native
startup/constructor overhead. It keeps the 48 KiB payloads, connection/queue
limits and every acceptance assertion unchanged. The three stress paths share
one predecessor-to-exit OR link, whose 8 KiB/s and 32 KiB burst are tighter than
the process limits in `startup-v2`: results must identify shared-link/queue
pressure and must not claim isolated process-bucket saturation.

### Reviewed retry isolation candidate

`retry-paced-v3` retains every `startup-v2` byte, connection, live-state and queue
limit, the same 48 KiB payloads, and all baseline, pressure, native-closure,
checksum and fresh-recovery assertions. It adds only these target fixture options:

```text
TestingServerConsensusDownloadInitialDelay 10
TestingClientConsensusDownloadInitialDelay 10
__DisablePredictedCircuits 1
```

New dedicated origins additionally set
`TestingClientConsensusDownloadInitialDelay 10`; their existing
`__DisablePredictedCircuits 1` remains unchanged. Preparation records exact
`target_isolation_options` and `origin_isolation_options` separately from
`resource_limits`. The generated target fragment includes both sets of target
options, and authenticated preflight checks their exact values on each relevant
process. An operator deployment helper that applies only `resource_limits` must
be updated to apply the complete reviewed fragment before this profile can run.
Older manifests/profile names acquire no implicit isolation options.

Prepare this candidate in a new short private directory, without deployment:

```sh
python3 eltord/tools/foundation_resource_acceptance.py prepare \
  --scenario traffic --traffic-profile retry-paced-v3 --nodes chutney/net/nodes \
  --template chutney/net/nodes/005c/torrc \
  --directory .runtime/foundation-resource/TRAFFIC-V3 \
  --path 006m,007m,004r --target 004r \
  --origins 4 --per-origin 1 --max-circuits 64
```

Source scope matters. `src/app/config/config.c` and `testnet.inc` in the native
Tor tree default both consensus delay options to zero, including the
`TestingTorNetwork` defaults. `src/feature/dirclient/dlstatus.c` uses the server
option for a public relay's consensus schedule reset and failure backoff.
`src/feature/dirclient/dirclient.c` treats HTTP 304 as a failed consensus fetch;
`src/feature/nodelist/networkstatus.c` checks that schedule before retrying.
Thus the server option addresses a concrete observed retry path: the retained
`startup-v2` logs counted 48 HTTP 304 replies among 81 consensus-download
launches. The matching client option affects a client/bridge with a reasonably
live consensus; bootstrapping clients still use their distinct
`ClientBootstrapConsensus*DownloadInitialDelay` schedules. These settings do not
cap all successful fresh-consensus fetches, descriptor/certificate downloads,
or aggregate directory traffic.

`src/core/or/circuituse.c` checks `__DisablePredictedCircuits` only before
prediction. It does not suppress relay reachability/bandwidth-test construction
from `src/feature/relay/relay_periodic.c`, `src/core/mainloop/mainloop.c` and
`src/feature/relay/selftest.c`. Ten such construction launches were observed in
the failed `startup-v2` window, but no bandwidth-test DROP dump was observed;
there is no evidence to attribute its byte consumption to such a dump. No
`ExcludeNodes`, `AssumeReachable`, directory-cache, forwarding, advertised-profile
or enforcement changes are part of this candidate. It is a limited isolation
hypothesis, not a guarantee that the remaining background work fits the caps.
If its unchanged honest baseline or later gates fail, retain that failure.

The private `retry-v3-20261029` directory was prepared without a run claim,
result, process launch or deployment. This candidate has no live acceptance
result. Do not deploy
or dispatch it during the sustained paid-stream run. Descriptor-ready target
status of at least 75% permits dispatch after operator release, but does not
establish full target reachability or substitute for a real honest echo.

The earlier `small-v1` deployment reached only target 95%, and a separate fresh
unchanged runner failed its baseline build before pressure. Those private
failures are retained. Target descriptor-ready status of at least 75% permits a
new experiment; it does not prove full target reachability. The mandatory actual
honest circuit/echo proves forwarding separately. A named profile or readiness
observation never replaces that baseline or the later recovery checks.

Both scenario fragments set the explicitly selected live-circuit cap, 64 in these
examples. The signed per-circuit 2-cell/s, 128-cell burst and 600000 ms lifetime
remain unchanged. Other relay limits remain in force. These deliberately small
local caps are experiment controls, not proposed production defaults. Traffic
uses the approved loopback exit; setup targets the first hop so the independent
origins reach it over independent links rather than only a shared downstream
connection. Setup preflight also requires enough live-state headroom for every
requested constructor plus the honest baseline, so a preoccupied circuit cap
cannot stand in for crypto pressure. If the dedicated isolated target lacks
headroom, do not delete state: choose a newly prepared smaller storm (for
example `--per-origin 4`) or wait for ordinary teardown. Four origins with four
requests each remain above the four-attempt burst.

### Traffic evidence

The runner first proves a tiny honest echo. It then builds at most one pressure
circuit per stress origin for each of two phases, with setup spacing and normal
byte refill before loading them. Upload sends a fixed 48 KiB payload and checks
its returned SHA-256 if delivery completes. Download requests the same bounded
48 KiB from a temporary loopback-only source and checks its content if complete.
Every stream is explicitly attached to a receipted ordinary base circuit; no
routing map, public endpoint or replacement path is used. Across both phases,
four origins offer at most 288 KiB of pressure payload.

Each phase requires nonzero actual serialized-link counters in both directions,
at least one pressure transfer failure and an independently observed native
circuit closure. At least one phase must show positive native queued bytes,
with all snapshots within their configured ceilings. The sent-byte delta must
fit `burst + rate * elapsed`, using a monotonic bracket that includes both
snapshot exchanges. Received excess is already on the wire and cannot be
undone: the test requires bounded failure and recovery rather than falsely
claiming that an ingress counter cannot exceed available credit.

The runner closes only its owned stress receipts, waits eight seconds for
ordinary refill, and requires a newly admitted checksum-verified honest echo
after **each** pressure phase. Total byte exhaustion can close an existing
honest circuit too; this fixture does not claim a protected traffic lane or
priority under complete exhaustion. The setup scenario separately requires an
already admitted honest circuit to progress under crypto pressure. Report
phase counts, payload limits, serialized-link-byte deltas, observed closures, queue
peaks, elapsed windows and post-recovery counters. A timeout without observed
native closure, an unobserved queue, or a failed recovery makes the scenario
fail; it must not be relabeled as successful pressure acceptance.

### Setup evidence

After a baseline honest circuit and tiny echo, three independent origins each
submit at most eight ordinary constructors, without the admission scenario's
one-second inter-request spacing. Every constructor is dispatched once. A lost
receipt remains uncertain and is never retried. The target's existing shared
owner must bound all attempted work across those origins.

The result requires actual attempted setup work, at least one refusal/closure,
and an attempt delta within `burst + rate * elapsed`. Every observed running,
waiting, slot and arena counter must remain below its configured ceiling. The
same previously admitted honest circuit must carry another tiny echo after
the finite storm. After owned teardown and normal refill, a new honest circuit
and echo must also succeed. Fast native crypto jobs can begin and finish
between samples: record observed running/waiting peaks, but do not require a
positive sampled peak or claim that this proves 32-way production CPU
saturation. No timer, token, identity or lifecycle reset is used for recovery.

These scenarios do not prove the full configured host-memory footprint, TLS/IP
or kernel buffering limits, every transient peak, HSDir/quote-slot abuse,
production latency/privacy, or real Lightning settlement. The protocol arena
and sampled packed-cell queue are specific native counters. Preserve failed
results and logs. A candidate that cannot produce the required observations or
honest recovery needs an explicit reviewed fixture/profile change and a new
run directory; weakening a check or removing a claim is not recovery.


## Local execution record, 2026-10-29

The setup fixture `final-setup-20261029T0646` passed on the final native image
SHA-256 `77135566706968a6bc710c89494fdc61f8e7d73841759245f669ec5ffcab3a09`.
Twelve stress requests from three independent processes produced two built
circuits and ten refusals/closures. The target started two setup operations in a
1139 ms measured window, within its 1/s and burst-4 envelope. Across 133 samples,
waiting work reached 2/2, slot usage reached two and arena usage reached
1621944 bytes. Running work finished between observations, so its sampled peak
was zero. Three honest echoes passed: baseline, the same existing circuit under
setup pressure, and a new circuit after normal refill. The target was initially
95% descriptor-ready; actual forwarding acceptance is distinct from its own
reachability status. All fixture origins were stopped and its exact original
configuration was restored, after which it reached 100% bootstrap.

Traffic acceptance remains **failed/incomplete**. The original `small-v1`
deployment did not meet an operator-added target-100% gate; it did not run the
workload. A fresh `traffic-fwd-20261029T0702` fixture used descriptor-ready target
status and the unchanged runner, but genuinely failed its mandatory baseline
build. The named `startup-v2` fixture `traffic-v2-20261029T0715` also failed that
baseline before pressure. Neither result proves a traffic-pressure/recovery
pass. The final target snapshot already had zero process byte credit before
the planned workload, and safe log counts showed native directory and self-test
activity during the lowered-cap interval. A ten-second observation with the
original target limits and no fixture origins had zero serialized-link-byte
delta, so that quiet baseline must not be confused with the startup/rebuild
interval. Investigate and explicitly isolate unrelated fixture work before
another experiment; do not keep enlarging limits or weakening acceptance gates.

All claims, results, operator receipts and private logs remain under the
workspace's `.runtime/foundation-resource/` directories. Both affected relay
configurations were restored byte-for-byte and reached 100%; all twelve origins
from the setup and two executed traffic fixtures were stopped. No wallet or
financial payment commands were used. This local evidence is not production,
whole-host memory, privacy, or Lightning-settlement acceptance.
