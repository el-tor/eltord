# Paid circuit rollover under concurrent browser traffic

A busy socket callback can run before the payment timer at a minute boundary.
The permission getter correctly refuses to project a new ACTIVE round before
its authorization has been consumed. Previously, the forwarding/packing path
could consequently classify already-funded traffic as unpaid and fill the
small holding queue, closing the circuit during concurrent browsing.

The standalone and embedded Tor trees now service due funded transitions on
actual forwarding and application output, before applying holding limits. They
also service a due transition at the bulk queue flush boundary. Read-only
payment queries and permission getters remain observations. No new payment,
clock reset, free downgrade or additional spending allowance follows from
traffic. Unfunded rounds retain their original grace and closure deadlines.

Activation can generate a backward status cell. The forwarding/output hooks
run before backward encryption, so the notification and application cells enter
the queue in cipher order. Queue reservation itself does not advance the timer.
Already marked circuits are not marked for close twice after a queue failure.

## Validation

The focused native `payment_circuit/busy_round_transition` regression checks
read-only observation at an overdue funded boundary, repeated traffic-driven
advancement without resetting T0, and denial/closure of an unfunded round.

The live acceptance uses the actual desktop sidecar, experimental embedded
Phoenixd payer and one Core Lightning receiving relay. Twenty temporary Firefox
tabs each stream 128 KiB/s for 150 seconds through the app SOCKS5 router. Native
circuit binding and durable path scope are checked during the transfer; all
streams must remain on one approved paid circuit across rounds 1, 2 and 3.
Browser readers check complete byte counts. The original browser tabs remain
open and only fixture tabs/mappings are removed. Runtime evidence stays under
`.runtime/paid-relay-e2e/` and is not published.

Acceptance result (2026-11-01): passed. All 20 browser readers completed their
19,660,800-byte transfers (375 MiB total), with zero server disconnects. Native
samples retained the same paid circuit through rounds 1, 2 and 3. Recipient
ledger authorizations matched the test's outgoing round payments. The desktop
client was deactivated afterward to stop additional automatic spending.

The test retains all previous payment history and the approved cumulative
200-sat ceiling, with a maximum 5-sat routing fee per payment. Closed unused
credit remains outstanding. The explicit offline operator increased the separate
account concurrent ceiling from 10 to 20 sats, within that unchanged total, to
allow another test path without erasing old credit. A consistent ledger-copy
check verified identical non-budget rows, idempotent exact retry and rejection
of stale or above-total increases. Accepted path configuration must match the
operator-reviewed account limits before construction.
