# Manual browser failure review: 2026-11-01

## Scope and evidence

Reviewed 39 log sources covering 23:14:30–23:24:21 UTC, plus live native
snapshots captured before deactivation. Coverage includes all nine Chutney
nodes, client notice/info/debug logs, desktop/daemon and eight receiving
controller journals, the zero-price fixture, Firefox service and current/rotated
Phoenixd logs. Some rotated logs and journals have no events in the selected
window; the current Phoenixd log is included. The private extracted window is
approximately 267 MiB. Raw logs, wallet records, browser destinations and native identifiers
remain under `.runtime/paid-relay-e2e/`; they are not committed.

Kernel journal access yielded no usable records, so this review does not claim
to have ruled out every OS-level fault. The app, native client and selected
relays remained running and responsive to authenticated control requests.

## Primary finding

At **23:18:13 UTC**, the native client recorded a destination failure with
reason `resolve failed`. `connection_ap_process_end_not_open()` entered Tor's
normal retry path, which calls `mark_circuit_unusable_for_new_conns()`.
That function prohibits new streams and backdates the circuit's dirty time.
Ordinary Tor can choose another circuit; the foundation client intentionally
has one explicitly accepted paid path and cannot silently purchase another.

The original destination is scrubbed in the client log, and the retained exit
INFO log does not record the final DNS response code. Its timeout setup/removal
messages are bookkeeping, not proof of an actual DNS timeout. This review does
not claim to distinguish original NXDOMAIN from an upstream resolver failure.
The correctness problem is retiring the whole paid path for a destination error.

This explains the observed combination: old connections remained present,
new tabs stalled, and payment status still reported ACTIVE. Both the app SOCKS
router and a direct native SOCKS probe timed out. Thus the fault is below the
React view and is not specific to the SOCKS router. A page may make many
connections; the evidence does not establish an eleven-tab limit.

Before deactivation, all three native hop snapshots reported ACTIVE in round
10 with positive remaining enforcement windows. The ledger had ten consumed
round payments; every exact payment hash matched one recipient authorization.
Cumulative spending was 258 sats, with 2 sats reserved, under the approved
1,000-sat cap. There was no paid-circuit closure or budget exhaustion at the
reported failure. This incident differs from the earlier upload queue closure
and the separate 200-sat-cap exhaustion.

## Secondary observations

- Repeated background construction failures and circID-zero DESTROY warnings
  occurred across the network. They add noise and consume work, but were also
  present outside the incident; the accepted paid path continued authenticating
  rounds. They need a separate lifecycle/resource investigation.
- CircuitPriorityHalflife was repeatedly clamped from zero to its minimum.
  This is configuration noise, not evidence of the paid relay cutting the path.
- Two client QUERY observations became uncertain around 23:17. Subsequent
  authenticated snapshots and consumption demonstrate recovery; no duplicate
  payment or replacement paid build was observed.
- The current Phoenixd window had no warning/error entries. Payment and
  recipient records independently corroborate settlement and authorization.
- The UI's payment ACTIVE indicator is narrower than transport usability.
  Explicit identity retirement and genuine path failure still require a separate
  readiness/accounting presentation; payment freshness alone is insufficient.

## Recovery change and validation

For an already activated paid path, authenticated destination-specific failures
end only the affected stream with its original SOCKS error. They do not retire
all unrelated streams, reset clocks, change accepted rates, dispatch another
payment, or automatically build a replacement. Local BEGIN timeout similarly
bounds the failed stream. Ordinary Tor behavior remains unchanged. Explicit
identity retirement, stream-ID exhaustion, exit-policy checks and circuit-level
protocol/authentication failures keep their existing protections.

A concurrent-transfer follow-up also exposed unfair buffered packing after the
paid queue watermark was reduced. The old per-stream allocation still used the
larger ordinary watermark and always began at the head of the list. The fix
budgets against the actual paid watermark and rotates buffered packing using
the existing chosen stream. This preserves the 32-cell holding ceiling and
readiness checks while preventing one buffered stream from taking every batch.

Tor's existing DNS resolver behavior remains in place. A final unresolved name
can still fail that request; later requests must be able to use the same healthy
paid circuit. This does not promise that a nonexistent hostname can resolve.

The first DNS fault run correctly contained the error and loaded the public
page, but its content assertion expected text no longer present in the body;
inspection of the actual rendered page confirmed the title and content. A
corrected run passed three DNS-failure/public-page checks on the original paid
path through rounds 4–6, but its strict transfer check found two older readers
still behind. Those incomplete results remain retained; they are not reported
as full acceptance. The fairness correction and a bounded reader-drain check
are included in the final live validation below.

Final live validation passed on the rebuilt native app and both updated Tor
trees: 20 browser tabs, up to 40 simultaneous fixture streams, 375 MiB of fully
completed downloads and 40 MiB of fully completed uploads. All browser readers
finished by 153 seconds. At three separate times an intentional NXDOMAIN
returned SOCKS host-unreachable; a new public Firefox page rendered afterward.
The original construction scope and native circuit identity stayed unchanged,
with samples covering paid rounds 1, 2 and 3 and zero fixture stream losses.

The existing held-queue age regression also passed. The live fault injection
specifically covers DNS failures and normal data flow; other destination error
classes and explicit identity retirement still need their own fault cases.
Raw results, the public-page screenshot and running executable digests remain
local. No protocol wire layout, payment terms or cumulative budget was changed.

Privacy/timing: no wire fields, role markers, persistent identifiers or payment
headers change. The final stream error is returned without an implicit alternate
paid path or unbounded retry loop. New diagnostic text contains only a fixed
failure category, never a hostname, payment identifier or proof. Public browser
probes and captured raw records remain private. Explicit privacy/isolation
retirement must not be bypassed by the destination-error exception.
