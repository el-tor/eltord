# Circuit and bandwidth investigation — 2026-10-20

The investigation reproduced a TCP framing failure in eltor-app, an invalid
preferred-exit path in eltord, and a crash in the forked Tor payment controller.
It also found payment-renewal and wallet-polling logic that can tear down or stop
renewing otherwise usable paid circuits. Fixes and regressions are on the local
`intermitted-fix` branch in tor, libeltor-sys, lni, eltord, and eltor-app.

These are validated defects; they do not establish that every historical
interruption had the same cause. The local nine-node Chutney network is free
mode. Real Lightning settlement and a funded, production paid network were not
tested. The real wallet was not used to make payments.

## Reproduced defects and changes

| Component | Trigger and previous behavior | Resulting behavior and evidence |
| --- | --- | --- |
| App SOCKS router | TCP split a SOCKS greeting or CONNECT frame; a single read was treated as a whole message, producing EOF or broken pipe. | Exact frame reads handle fragmentation and preserve bytes following the upstream reply. Nine of nine fragmented router probes failed before; all 36 three-port comparison probes pass after. Five focused Rust tests cover greeting, address framing, truncation, and authentication negotiation. |
| Relay selection | EntryNodes/ExitNodes replaced a hop after unique hops had been selected; the preferred exit could equal the middle. | Preferences constrain eligible role pools before selection. Distinct fingerprints are required, reserving the exit before choosing the middle. A live paid-format attempt had a duplicated middle/exit and timed out; corrected paths built both primary and backup circuits and carried HTTP traffic. |
| Tor C controller, both source copies | A syntactically accepted EXTENDPAIDCIRCUIT referencing an unknown circuit jumped into cleanup before cleanup-pointer initialization; the process crashed. | Initialize pointers before every early exit. Also use one actual buffer capacity for hash serialization, reject overflow, and close newly allocated circuits on later construction failure. Each compiled binary survives 15 invalid requests and answers GETINFO afterward. This is an error-path reproduction, not evidence that ordinary valid paid circuits always hit it. |
| Dual circuit payment loop | Alternating circuit A/round 1, B/round 2, A/round 3 skipped half of each circuit's invoice sequence. | Every live circuit receives every round. A fake-controller/fake-wallet regression verifies A1, B1, A2, B2. A surviving circuit continues if its peer disappears. |
| Payment timing and health | Slow throughput probes ran on the renewal path; HTTP error status was treated as a dead transport; expiry rejected payments before their stated deadline. | Use bounded small transport checks with an independent fallback endpoint and two consecutive failures. Wallet/RPC time counts toward the monotonic renewal cadence. Expiry includes the configured grace. Tests cover HTTP 503 as working transport, endpoint fallback, payment order and expiry. |
| Phoenixd invoice polling | A transient HTTP/JSON error caused a panic or terminal invoice failure, allowing relay teardown despite a merely unavailable wallet API. | Return parse/status errors and keep temporary poll failures pending until the polling window ends. Mock HTTP regressions cover malformed JSON and 503 followed by a settled invoice. Relay watcher cancellation also interrupts an in-flight poll; the watcher window includes the existing grace period. |
| Stream attachment | Authentication success could hide command failure; monitor ownership and EOF cleanup could leave manual attachment enabled with stale circuit IDs. | Validate command replies separately, subscribe before enabling manual attachment, own one monitor per generation, retry a surviving circuit, and restore automatic attachment on exit. Ignore directory fetch/upload streams and keep closed-stream attachment races from terminating the monitor. Regression checks failed EXTEND/teardown and restoration on control EOF. |
| Configuration parsing | GETCONF protocol frames leaked into values; default wallet selection and full ControlPort endpoints were parsed incorrectly; consensus address/port/bandwidth indices were wrong. | Parse command data and select the actual default wallet; support bare, IPv4 and bracketed IPv6 control endpoints; parse the correct consensus fields. Focused controller fixtures verify these contracts. Raw wallet configuration and control credentials are no longer logged by the changed lookup path. |
| App startup and status | Linux pre_exec closed Rust's exec-status pipe, causing a child abort when the daemon executable was missing; UI showed connection success after a fixed timer. | Preserve the exec-status pipe and return a normal spawn error. Show the connection modal only after observed bootstrap completion, clear prior-generation progress and find progress within batched log events. A missing-executable regression passes; frontend/native builds verify integration. |
| App logs | Recent logs used the wrong Tauri directory; tailing an old inode stopped showing logs after replacement or truncation. | Read the app data directory and reopen replaced/truncated files. The native viewer and restart behavior are checked separately from transport results. |

The embedded C dependency was compiled with `-O0` in the debug desktop build,
while standalone Tor used `-O2`. Both app Cargo manifests now compile libtor-sys
with optimization even in a debug build. Measurements below describe whole-run
results; circuit paths, relay versions, libraries and concurrent local work also
vary, so they do not isolate an optimization-only causal effect.

## Test method and correlation

The harness compares direct HTTP, embedded Tor SOCKS 18058, app router 18048,
and standalone Chutney SOCKS 9060 against one deterministic local HTTP origin.
Every request records UTC epoch time, curl timings, HTTP status, size and SHA256.
Origin request IDs connect outcomes to origin delivery. Control streams capture
CIRC, STREAM, ORCONN and status events from the app client and nine Chutney nodes;
client/relay logs and periodic control snapshots share the same clock.

Each main run contains serial 16 KiB/1 MiB/8 MiB requests; 4, 16 and 48 concurrent
256 KiB requests; one approximately 32-second paced stream per route; and an
eight-minute repeated-request phase with two workers per route. The paced stream
alone does not span a payment interval. Free-mode soak time does not validate
funded renewal; targeted fake-wallet round tests and the separate paid-format
probe cover different parts of that path.

The baseline completed 2,863 requests and 1,911,816,192 verified bytes with zero
failures; all request IDs matched the origin. Twelve observed client circuit
closures were ONEHOP_TUNNEL,IS_INTERNAL directory circuits with FINISHED reasons.
They were not user download circuit failures. No STREAM FAILED event appeared
in that baseline. Background Conflux warnings without a matching failed request
are insufficient evidence to blame Conflux for the reported historical drops.

Raw artifacts remain local under `.runtime/circuit-audit/`. An early post-build
run named `after-fix-startup-failure` is excluded from performance comparisons:
resource deployment had removed the daemon executable and exposed the launcher
error path. It was a deployment failure, not a valid steady-state measurement.

The reusable control-capture helper was also corrected to parse data blocks:
a circuit ID beginning with 5 is data, not a 5xx status. Earlier GETINFO snapshots
could therefore be truncated, while continuously captured control events, request
results, origin IDs and log files were unaffected. Two parser regressions cover
that distinction. Final paid-format evidence uses the corrected helper.

## Completed traffic and fault results

The rebuilt free-mode run completed **2,870 requests, zero failures**, with
1,919,156,224 verified bytes and all 2,870 request IDs matched at the origin.
Together the two valid runs completed **5,733 requests and 3,830,972,416 bytes**.

For 48 simultaneous 256 KiB requests, median completion time changed as follows:

| Route | Before | After |
| --- | ---: | ---: |
| Embedded client | 6.253 s | 1.208 s |
| App router | 6.037 s | 0.755 s |
| Standalone Chutney client | 0.356 s | 1.846 s |

These are measurements from two local runs, not a controlled benchmark proving
that optimization accounts for all differences. The standalone comparison became
slower, underscoring the effect of path choice and local load.

Controlled faults ran after the performance soak, at 23:33 UTC:

- CLOSECIRCUIT returned 250 OK for the circuit carrying the paced transfer. The
  client emitted CIRC CLOSED REASON=REQUESTED within one millisecond of the command.
  The active download ended early with curl error 18. All five subsequent requests
  succeeded; the first completed in 0.123 seconds.
- Exit 004r was SIGSTOP-paused for 8.091 seconds, then SIGCONT-resumed in finally.
  Direct origin access succeeded in 0.000661 seconds during the stall, while all
  three two-second SOCKS probes timed out. The existing 16 MiB transfer ultimately
  completed, taking 19.325 seconds. All five requests after resume succeeded, with
  the first taking 0.083 seconds.

Cross-log correlation supports the relay-stall diagnosis: the 004r info log's
last line before the pause was at 23:33:57.274, and its next burst began at
23:34:07.766 when SIGCONT was sent. Meanwhile the app client's info log recorded
SOCKS connection EOFs at 23:34:01, 23:34:03 and 23:34:07, aligned with the timed-out
probes, while the direct origin request succeeded. Captured events include the
app client and all nine Chutney nodes; relay and client circuit identifiers are
local to their respective processes and must not be equated numerically.

The deliberate failures have distinct timing and control evidence. They are
excluded from the zero-failure steady-state counts above. The first longer
paid-format probe also caught an attachment-monitor regression: a directory or
already-closed stream produced a 555 response and stopped the monitor. Its source
fix and a fake-controller regression were added before the final live repetition.

## Final paid-format and desktop verification

On 2026-10-21 at approximately 14:18–14:20 UTC, the corrected live probe built
both paid-format circuits using zero-fee Chutney descriptors and an informational
mock wallet. It completed **49 HTTP requests with zero failures over 101.34
seconds**. At roughly 25 seconds it closed the primary circuit with CLOSECIRCUIT
(250 OK). The original monitor remained active throughout (one monitor start),
traffic used the surviving circuit, and the renewal loop continued across the
45- and 90-second boundaries. The mock rejected every payment POST and no real
wallet was used by this probe. This verifies paid-format transport and lifecycle,
not real Lightning settlement.

The final Linux Tauri build succeeded. Resource binary hashes match the compiled
daemon and selected Phoenixd executables. Activation produced fresh log lines in
the existing native viewer after the log file was replaced, then showed the
connection modal at observed bootstrap completion. HTTPS through app SOCKS 18048
returned 200. Firefox Connection Settings showed manual SOCKS v5 at
127.0.0.1:18048, and a newly navigated HTTPS Example Domain page rendered
successfully. The nine-node Chutney transmission verification also passed after
the extended idle interval. An older client log ended with `Interrupt: exiting
cleanly`; that idle interval is not counted as a continuous traffic soak.

## Reproduction and operational limits

See `tools/stability/README.md` for commands and endpoint assumptions. The harness
uses private local outputs and discovers home/workspace paths at runtime. Never
publish raw daemon or wallet logs without review. Test wallets are informational
mocks that reject every payment request, with real wallet entries removed from
the disposable configuration.

The malformed-command tests run against disposable network-disabled Tor instances.
Fault injection is separate from performance runs and resumes the paused exit in
a finally block. Deliberately closing a circuit can interrupt an active stream;
recovery means new requests work, not that TCP transparently resumes lost bytes.

The deployed app remains connected to free-mode Chutney. Public El Tor authority
endpoints returned stale-consensus errors during setup, so production paid-network
validation remains outside the evidence here. This was a focused correctness and
reliability audit of circuit/payment/stream paths, not an exhaustive audit of every
line in the Tor fork or an anonymity/security certification.

All changes remain local. Commit author and committer are
`Eltor Dev <eltordev@proton.me>`. All 12 checkouts have repository-local identity,
empty credential helper overrides, disabled push URLs and rejecting pre-push hooks.

## Local change and validation index

| Repository | Local implementation commit | Validation |
| --- | --- | --- |
| tor | ba9b593 | Standalone build; 15 rejected control commands without process exit |
| libeltor-sys | 543241c | Embedded rebuild; same 15 rejection probes; actual paid-format traffic |
| lni | 90dd176 | Two mock Phoenixd HTTP polling regressions |
| eltord | a433ba4 | Six controller integration tests; five payment/heartbeat/consensus/selection unit tests; two harness parser tests; live 101-second failover probe |
| eltor-app | cf206d5 | Five SOCKS unit tests; one missing-executable regression; frontend and Linux Tauri builds; native activation, logging and Firefox HTTPS |

The test artifacts and source are local. Existing free-relay C changes, local
wallet binary replacements, Cargo.lock changes and the app's embedded-wallet
launch setting were preserved outside these implementation commits.
