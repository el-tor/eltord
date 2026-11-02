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

## Upload-side follow-up

The 22:46 manual failure exposed a separate origin-side problem. Native logs
identified resource closure during DATA packing with a large amount of input
still buffered. The earlier 20-tab acceptance exercised downloads, so it did
not cover this upload backlog.

Paid edge streams now stop packaging before readiness expires or their packed
queue reaches 24 cells, leaving eight slots within the existing holding budget
for protocol traffic. They resume at 12 cells once paid readiness is verified.
Resume processes already-buffered input as well as re-enabling socket reads;
it does not depend on another SENDME or another socket becoming readable.
Relay resume follows activation notification generation, preserving cipher and
FIFO order. The original five-second held-cell deadline, pure observation
queries, authorization requirements and financial history remain unchanged.

The follow-up actual Firefox acceptance adds 2 MiB POST requests repeatedly,
with server-side pacing, while all 20 tabs also download. Across rounds 1, 2
and 3, all 40 streams remained connected and all downloads completed (375 MiB).
The browser confirmed 728 MiB of completed POSTs, with no failed uploads. The
strict final-completion check did not pass: five repeated upload loops still
had an in-flight final POST at the fixed cutoff, which cleanup canceled.

A separate finite completion probe on that same approved circuit then passed:
20 tabs each completed a 2 MiB POST and a 1.5 MiB download, with native samples
covering rounds 4 and 5. This verifies finite buffered uploads complete after
backpressure, beyond merely retaining live sockets. Combined native evidence
covers the same path through rounds 1–5. Both runs and the explicit combined
acceptance summary are retained locally. The existing held-cell age regression
also passed; no broad unit suite was run for this follow-up.

The test client was deactivated afterward. Cumulative accounting was 192 sats
spent and 2 sats still reserved, leaving 6 sats available under the unchanged
200-sat ceiling. No reservation was cleared to create more test budget.

For this follow-up, retained closed credit had filled the 20-sat concurrent
account ceiling. The explicit offline operator increased that ceiling to
32 sats (closed exposure plus room for current/next rounds), preserving every
non-budget row and the unchanged 200-sat cumulative ceiling. No historical
settlement or uncertain reservation was released.

## DNS recovery and fair buffered scheduling

The later manual browser investigation found destination DNS errors retiring
an otherwise active paid path. It also corrected buffered stream allocation to
use the small paid watermark and rotate service across streams. See the
[complete log review and live fault acceptance](browser-log-review-2026-11-01.md)
for coverage, limitations, retained failures and the final 20-tab completion.
