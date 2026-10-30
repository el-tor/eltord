# One-paid-relay end-to-end test plan

Status: receiving preflight passed; payer access and spending approval pending.
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
