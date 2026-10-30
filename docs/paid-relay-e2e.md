# One-paid-relay end-to-end test plan

Status: real native payment, receiving authorization, app paid activation and
Firefox rendering passed. Same-stream period renewal and stop-payment closure
are in progress.
This is a distinct real-settlement goal following the zero-price foundation
acceptance. Keep all work local on `paid-circuit-foundation` and preserve the
repository-specific identities and push protections.

## Scope and gates

1. Verify the receiver's native account and bounded receiving capabilities using
   its private rune. Use only the explicitly approved instance-local TLS bypass
   over the operator's Tailscale route. Never export or log the rune.
2. Confirm the supplied offer is an active offer owned by that receiver. Do not
   create another offer without authorization. A public offer is not payer access.
3. Create a new authoritative receiving ledger exclusively after native account
   verification. Preserve every previous ledger, claim and failed attempt.
4. Obtain a separate payer account/configuration and explicit relay rate,
   handshake, per-payment routing and total-spend limits before any pay request.
   Current integrated outgoing foundation support requires capable CLN. Do not
   infer Phoenixd support from its separate working wallet UI.
5. Choose one paid relay in an explicit three-hop client plan; keep the other
   hops zero-priced. Back up current node/app configs and record actual source,
   lockfile and running-executable provenance before deploying. Never regenerate
   node keys. Bootstrap and descriptor/control access remain loopback-only.
6. Check signed offer/rate/terms, immutable accepted path and finite exposure
   before activation. Ensure budget arithmetic includes routing and uncertain
   in-flight outcomes. No payment while approval is missing.
7. Correlate native recipient settlement to the exact durable authorization and
   authenticated all-hop activation. Bind actual Firefox app-proxy traffic to
   that exact approved circuit; unrelated browser success is insufficient.
8. Keep one checksum-verified stream across a payment-period boundary, retain
   native stream/circuit identity and per-hop round evidence, and prove renewal
   is funded incrementally. Query uncertain operations; never repay or rebuild
   merely because a reply was lost.
9. Stop renewal/payment without killing the circuit prematurely. Observe the
   original deadline/grace and closed paid path, with no free downgrade. Do not
   infer refunds or erase financial history from closure.
10. Restore exact ordinary free-testnet configs, stop only owned fixture processes,
    and repeat Chutney transmission/app/Firefox checks. Retain the bounded local
    evidence, all settlement/unknown records and exact total-spend report.

## Approved payer and limits

The user selected the app's existing embedded Phoenixd wallet, and approved
1 sat per minute for the paid relay, zero handshake fee, at most 1 sat routing
fee per payment, and 100 sats total including routing. The current foundation
client's CLN-only outgoing integration must therefore be extended; selecting
another payer or bypassing invoice-before-pay validation would not meet this test.

The actual embedded binary reports version `0.9.0-b072567`. Inspection of that
pinned official source shows `/payoffer` fetches and pays together, `/payinvoice`
accepts BOLT11, and neither endpoint accepts a per-payment routing fee cap.
Read-only outgoing lookup by payment hash exists. A safe adapter cannot infer
an unknown BOLT12 invoice/hash before dispatch or enforce the approved routing
cap merely by checking fees afterward. These missing wallet capabilities must
be resolved before real dispatch. No payment endpoint has been called.

## Executed receiving preflight

The receiver is on Bitcoin mainnet. Native `getinfo`, receiving command help and
one bounded invoice read succeeded. The rebuilt explicit ledger operator verified
that account and created a fresh schema-7 ledger with SQLite integrity passing.
The supplied offer matched exactly one active offer owned by that node. No offer
was created and no payment was sent. The local clock reports NTP synchronization;
the candidate 1000 ms uncertainty bound still requires validation during timed
payment observations rather than inference from synchronization alone.

Private credentials, account identity, offer and ledger live outside tracked
repositories under the user's El Tor configuration directory. Bounded evidence
is in workspace `.runtime/paid-relay-e2e/`. The receiving configuration is not an
outgoing account and provisioning grants no spending authority.

## Privacy and timing

This plan introduces no wire fields or timing policy. Preserve independent hop
identifiers, encrypted construction, finite retries and original deadlines.
Opted-in TLS bypass relies on actual private-network peer authentication rather
than the server certificate; it is not a claim of public-network TLS validation.
Do not publish wallet identifiers, offer contents, private paths or native payment
records in tracked reports. Real receiving evidence must remain distinct from
simulated wallets, zero-price activation and ordinary free transport.


## Local Phoenixd capability implementation checkpoint

The installed wallet binary and its data remain untouched. Protected local
source checkouts are under workspace `.runtime/paid-relay-e2e/phoenixd-source`
and `lightning-kmp-source`, both on `paid-circuit-foundation` with rejecting
push hooks and disabled push URLs. Phoenixd is based on the exact installed
`b072567` revision; Lightning KMP is based on version 1.13.0.

KMP local commits `3fd0aa7` and `737edff` add an explicit fetch-only offer mode,
redact its invoice/identifier diagnostics, and test actual onion-message invoice
acquisition without a financial command, duplicate-response suppression and an
actual capped HTLC with no fee escalation. Both focused JVM regressions passed.
A task-local checksummed JDK 21 was required; the system Java configuration is
unchanged. The initial Java 17 test failure is retained as a toolchain failure.

Phoenixd local commit `8f9d5d7` adds separate authenticated experimental invoice
fetch, capped payment and native operation lookup endpoints. JVM compilation
passed against the pinned local KMP source. The fee schedule has one attempt,
zero proportional fee and a base bounded by the caller's whole-sat fee cap.
Invoice/chain/signature/payer/amount validation remains distinct from dispatch.
A native build is in progress; compilation is not native runtime acceptance.

These features still need native build verification, fault tests and integration
with Eltord's durable payer intent, exact invoice validation, original clock and
current-plus-next exposure accounting before the approved wallet can spend.
Do not call the existing pay-offer shortcut or treat callback/HTTP success as
settlement. A lost reply retains UNKNOWN and never licenses another pay command.
No new API has been deployed and no real payment has been sent.

The fetch path retains Phoenixd's existing default offer payer key, so it does
not claim unlinkability between receivers. No new Tor circuit identifier or
payment preimage is exported into routine logs; sensitive native records remain
inside authenticated local wallet/controller communication and private evidence.


## Native wallet and payer adapter checkpoint

The native wallet distribution built successfully and its executable reports
`0.9.0-8f9d5d7`. Its digest and archive digest are retained in the private runtime
record; the installed wallet binary is unchanged. The local source/dependency
checkouts retain their push blocks. This build is not a real settlement result.

LNI now shares pure signed-invoice validation across wallet adapters; the 26
existing CLN foundation regressions passed after extraction. The Phoenixd native
adapter uses only numeric loopback HTTP/verified HTTPS, refuses proxies/redirects
and certificate bypass, checks the pinned account and explicit capability
profile, validates the exact signed offer/note/invoice and retains native msat,
operation and timestamp evidence. Four socket tests passed, including a lost pay
reply followed by read-only native recovery, mismatched evidence rejection and
preflight latency consuming the original dispatch window. No pay-offer fallback
or automatic transport retry is added.

Eltord has explicit native payer selection through the same durable intent,
reservation and dispatch machinery. Legacy CLN account files still select CLN;
Phoenixd requires `wallet: "phoenixd"`, pinned node/chain, loopback endpoint and a
separate private password file. Runtime opens an existing matching ledger before
wallet access, and explicit `eltor-foundation-payer-ledger --config ...` creation
verifies native identity/capabilities without paying. Two private-config/startup
regressions passed, and all daemon binaries pass compilation.

The broader coordinator socket run currently has seven passes and eight clock
failures with its frozen zero-uncertainty fixture clocks. A serial representative
positive-payment test fails with the identical clock-discontinuity error on the
unchanged pre-payer daemon baseline. Do not weaken production clock checks or
claim that this run passed. The remaining composed Phoenixd/ledger crash checks,
fixture clock correction, native read-only wallet startup/provenance, deployment
and actual paid circuit/Firefox/renewal/stop checks remain open. No real payment
has been dispatched and ordinary Chutney configuration remains unchanged.


## Actual UI attempt and response fix

The operator approved real circuit payments and requested real app/Firefox UI
verification and a video, prioritizing fixes/builds/live retries over further
unit suites. Native wallet startup against the existing mainnet account passed
read-only capability/account checks. The original wallet database and executables
were backed up before the explicitly scoped test runtime was deployed.

The first app activation was recorded, but the paid path closed. The actual
Phoenixd StatusPages handler converts empty payment lookups (204) to an exact
404 `Not found` body. The adapter had treated that as a transport uncertainty,
so it stopped during native preflight before calling pay. The retained intent
keeps its two-sat reservation; neither absence nor this diagnosis is used to
release/replay the historical operation. The response fix recognizes only that
exact bounded body on the authenticated operation-lookup route; unknown endpoint
404s and other responses remain uncertain. This fix is being deployed for a new
circuit scope on the same authoritative payer ledger and unchanged account-wide
100-sat cap. No tests were rerun after the user's UI-only instruction.


## Live integration fixes after first dispatch

The actual UI run exposed two further wallet interoperability problems. CLN can
return a cached invoice when the payer identity is reused across different
requests. The experimental fetch flow now derives a distinct, recoverable payer
key for each retained logical payment note using the wallet library's existing
deterministic blinded-offer key derivation. A subsequent real run obtained a
fresh valid invoice and sent a capped HTLC. The native wallet reports a final
`NotEnoughFees`/`RetryExhausted` failure with zero fee spent.

Native UUID deserialization normalizes version and variant bits. The experimental
API now reports the exact operation primary key used by its native lookup,
rather than the altered decoded display ID. Exact invoice/hash/amount/account
checks remain required, and historical uncertainty is reconciled without a
replacement pay request. The native financial database is never edited.

The operator explicitly increased the routing cap to five sats per payment,
keeping the one-sat relay round, zero setup fee and 100-sat account-wide ceiling.
New path quotes count routing exposure only for positive-price round payments;
zero-price hops never dispatch a wallet payment. Existing immutable account
budgets and all historical intents remain unchanged. Builds and actual native
app attempts are the current validation path; no further unit suites were run.


## Successful native paid-path activation

The rebuilt native app obtained a new validated invoice, sent the approved
1-sat principal with a 5-sat routing cap, and recorded the native 6-sat total.
The receiver's authoritative ledger independently claimed the 1-sat settlement,
and the payer recorded consumption only after authenticated activation. The
Connect screen displayed `Paid path: Last verified active`. Subsequent real
round payments progressed on the same owned circuit.

Firefox rendered HTTPS content through the app SOCKS router. Native successful
stream bindings matched the exact accepted private path, not merely any circuit.
A recorded native app-window clip and actual Firefox rendered-frame clip are
retained privately. Firefox used its ordinary Wayland backend; an X11 attempt
failed to open the display under Snap confinement, independently of transport.
The stale dedicated test-profile lock was retained before retrying.

A server-paced, checksum-targeted single TCP stream is now running across the
first ten-round payment-period boundary. This is actual live data-plane work,
not a mocked wallet or unit test. Renewal continuity, intentional payment-stop
closure, final accounting and restoration are not yet accepted.
