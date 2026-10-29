# Native bounded-free resource acceptance

`tools/foundation_resource_acceptance.py` prepares and runs one finite local
experiment against the actual native ordinary-circuit constructor. It never
starts/stops Tor, applies configuration, builds code, contacts a wallet,
provisions a ledger, sends payment commands, or deletes an earlier result.
Preparation creates fresh private client directories; existing Chutney identities,
wallets, configurations and financial histories are not copied or replaced.

The current scenario measures shared **live-circuit capacity**, refusal across
independent origin processes and honest recovery. It samples other resource
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
