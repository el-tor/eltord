# Local stability tests

Run from the workspace containing sibling `eltord`, `eltor-app`, `tor`, and
`chutney` checkouts. Python 3 and curl are required. Results are private local
artifacts under `.runtime/circuit-audit`; raw logs are not suitable for publishing.
Scripts discover the workspace from their location and app data from the home
directory. The app uses SOCKS router 18048, client 18058, control 9992, and the
Chutney client uses 9060. Override `APP_ELTOR_TOR_CONTROL_PASSWORD` if required.

These scripts exercise the existing free-mode Chutney setup; they are not a
production payment or anonymity validation. Use separate output directories for
each run. Do not run fault injection concurrently with a performance baseline.

```sh
python3 eltord/tools/stability/test_harness.py
python3 eltord/tools/stability/soak.py --seconds 480 --out .runtime/circuit-audit/new-run
python3 eltord/tools/stability/protocol_probes.py
python3 eltord/tools/stability/c_control_probes.py
python3 eltord/tools/stability/paid_format_probe.py
python3 eltord/tools/stability/fault_tests.py
```

`protocol_probes.py` requires the soak's HTTP origin on 18880 to be running.
It compares intact, split-greeting, split-request, and bytewise-request SOCKS
frames across the router, direct embedded Tor, and standalone Chutney client.

The soak verifies payload length and SHA256 for each request, records origin
request IDs, captures control events from the app client and all relay control
sockets, and tails client and relay logs with shared UTC epoch timestamps.
Its slow stream lasts about 32 seconds; it does not cross a full payment period.
The eight-minute repeated-request phase crosses multiple nominal periods, but
free mode does not exercise payment renewal.

The paid-format probe sends traffic for 100 seconds, closes one paid-format
circuit after 25 seconds, and requires the original monitor to survive across
renewal rounds. It starts an isolated daemon and an informational fake wallet
on 19741 that rejects all POSTs. It uses zero-fee local relays and does not access
the real wallet. The fault test closes an active circuit, then pauses exit 004r
for eight seconds and resumes it in a finally block. An intentionally interrupted
transfer is an expected result; subsequent requests must recover.

Portable malformed-command regressions also live in both Tor source trees:

```sh
python3 tor/src/test/test_eltor_control_errors.py tor/src/app/tor
python3 libeltor-sys/libtor-src/tor-src/src/test/test_eltor_control_errors.py eltor-app/backend/bin/eltord --eltord
```
