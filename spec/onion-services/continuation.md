# Continuation and finite circuit lifetime, version 1

**Proposed policy and wire contract; not implemented.** Keep the existing path,
streams, keys and independent hop clocks. An allocation contains ten identifiers;
it is not a ten-minute upfront purchase or an obligation to renew indefinitely.

## Lifetime and price policy

For this test profile, paid circuits have a default maximum age of 3600000 ms
and six ten-round allocations. A relay/operator may advertise a different finite
limit, up to 86400000 ms and 144 allocations. Require max_age_ms to equal
max_periods*600000 in this profile, with max_periods in 2–144. The accepted path
uses the most restrictive relevant limit; the SCQ additionally enforces client
age/round/spending caps. Lower user caps may end a connection earlier. A one-hour
default and 24-hour ceiling are design candidates for testing, not evidence that
long-lived circuits are safe or that transfers will remain reliable for that long.

Prices and originally accepted quotas stay fixed for the entire continuing
circuit, across every admitted allocation. No repricing, repeated setup/handshake
fee or renewal fee is allowed in version one. A relay can refuse the next period
rather than accept its old price; it must not shorten already-paid service.
A new price needs a new explicit circuit/SCQ, not an amended continuation.

At ten minutes, mark a paid circuit draining for new application streams. Existing
streams may continue through admitted and funded renewals until the accepted hard
age. Build separately authorized circuits for new streams. This retains active
downloads while avoiding indefinite attachment of unrelated new browsing activity
to one path. Draining does not migrate existing streams or make failure resumable.
At the hard age, stop even an active stream; application-level resume is separate.
Show this finite limit before acceptance and before predictable termination.

Free-only circuits do not renew in place in version one. Their lifetime is at
most 600000 ms from construction, and replacing one does not reset relay-wide
admission or traffic budgets. Services prepare replacement introduction paths
before expiry within the shared free-capacity caps. If reusing an introduction
authentication key at the same relay, wait for the old registration to close before
registering its replacement, respecting inherited duplicate-key rules; retain
other live introduction points for availability. New intro keys/points require
normal descriptor updates. Keeping a descriptor-upload circuit alive until its
limit does not itself require another descriptor upload. Paid circuits missing
a payment cannot fall back to this free-only lifetime.

## Service continuation records

Use the common signed/encrypted service-control envelope. Allocate messages
7 CONTINUE_PREPARE, 8 CONTINUE_ACCEPT, 9 CONTINUE_QUERY; common response status
10 CONTINUATION. These are endpoint record kinds, not new relay opcodes.
`round` in the common header is the next service allocation's first round.
The accepted core, client, method, issuer and caps remain unchanged.

PREPARE and QUERY payload is `attempt_id32 || u32 period`. Period begins at one;
first round must equal `10*period+1`, with checked arithmetic. PREPARE is allowed
only for the current live READY attempt, at most one pending service allocation,
under explicit local user continuation policy. It registers no payment obligation
by itself. It may cause bounded per-hop renewal preparation on the service's
existing path, but no wallet spending, new build or invoice issuance. QUERY only
reads state and cannot start preparation or reset retries.

The immutable extension core is exactly 131 bytes:

```text
u8 version=1 || attempt_id32 || extension_id32
u32 period || u32 first_round || u16 round_count=10
previous_extension_digest32
u32 max_rounds || u32 max_circuit_age_seconds
u64 total_cap_msat || u64 outstanding_cap_msat
```

For period one, previous_extension_digest is the original quote_core_digest;
otherwise it is the accepted prior extension digest. Extension IDs are fresh
random per-attempt 32-byte identifiers, never shared with relay payment IDs.
All four caps equal the original SCQ; they are not fresh spending limits per period.
Define extension_digest as SHA256 of
`D("eltor/scq/v1/continuation") || service_identity32 || quote_core_digest32 ||
circuit_authorization_id32 || L(extension_core)`.

CONTINUATION payload is `extension_core131 || u8 state || u64 admit_remaining_ms ||
u64 activation_remaining_ms || u16 reason`, exactly 150 bytes. States:
1 PREPARING, 2 ADMITTED, 3 ACCEPTED, 4 ACTIVE, 5 REFUSED, 6 EXPIRED, 7 CLOSED.
Reason is 0 for nonterminal states; 1 policy/lifetime, 2 hop refusal,
3 deadline, 4 capacity, 5 protocol conflict or 6 closure otherwise.
Allocate the stable core before per-hop work so repeated PREPARE always returns
the same object. Only verified admission from every required service-side hop
permits ADMITTED. No private relay identity, per-hop digest or acknowledgment is
included; this is the service's authenticated claim, not independently provable
private-path performance or expenditure. The client still checks its own hops.

CONTINUE_ACCEPT payload is `attempt_id32 || extension_digest32`. The client signs
through the common request signature. Persist acceptance before returning ACCEPTED;
it must match the sole ADMITTED extension, exact caps, predecessor and next period.
Concurrent/conflicting operations fail without replacing the extension. The
client must already have required admission on its own path and permit continuation
under its retained local policy. An exact duplicate returns state without new
registration, invoice, payment or clock reset. ACTIVE means the admitted period's
first service round has actually activated; early funding alone is not ACTIVE.

REFUSED/EXPIRED/CLOSED never authorize additional rounds. Their core remains the
same for audit. No missing reply, changed operation ID or updated descriptor may
replace an unresolved extension or increase its ceilings. A new transport delivery
refreshes nonce/key/ciphertext only. The logical extension and original deadline
survive daemon restart as long as the same live circuit can be recovered.

## Independent clocks and funding gates

Begin preparation at each origin's round seven (then 17, 27, and so on). Service
PREPARE starts service-hop registration when that window is reached; an earlier
client request may reserve the one extension object but cannot pull all future
registrations or payments forward. One pending allocation per hop remains the
hard limit. The service must finish admission before the earliest applicable
hop admission deadline, and before funding lead time is needed on its SCQ clock.
Report admit_remaining_ms as the minimum of those conservative remaining budgets;
activation_remaining_ms is the remaining service-SCQ boundary budget. Unknown
bounds prevent admission/payment; zero means exhausted, never unlimited.

The client subtracts full control RTT and safety margin from returned budgets.
It separately evaluates its own hop deadlines; matching service period P to
client period P by number is insufficient because their clocks differ. Each
origin registers its own next allocation before its own deadline. Before every
payment for a new service period, the client requires accepted continuation,
service admission and a live/admitted own path whose remaining authorized lifetime
covers that next service-round boundary conservatively. Own-path payment remains
subject to its own round window and caps; service readiness cannot authorize it.

After acceptance, invoices remain one round at a time, no earlier than the
original 60-second advance window. Round 11 cannot be invoiced before continuation
acceptance and the next allocation's admission. The service does not pay its relays
until that round's client funds settle and are reserved. It must reconcile each
hop's funding window with this two-stage schedule. If differing clocks, wallet
latency or uncertainty leave insufficient time, stop further dispatch rather than
borrow operator money, extend grace or declare a later T0. The design deliberately
does not promise uninterrupted service for every clock/latency combination.

If either half refuses, expires or loses a required path, stop automatic new-period
payments and report its authenticated state where possible. Let still-viable
already-purchased service run to its original bounds. Settled/in-flight payments
retain the previously disclosed exposure and require reconciliation. There is no
atomic six-hop payment transaction, automatic refund or live-stream migration.

## Privacy and acceptance tests

Continuation messages retain the common fixed control shape and fresh delivery
keys. All extension IDs, caps and attempt bindings remain encrypted; per-hop
renewal IDs stay independent. Longer-lived paths and round-seven exchanges can
change correlation opportunities despite uniform headers. Draining and hard-age
behavior must be measured against normal browsing and long downloads. Free-only
replacement must be tested under intro-key registration and descriptor churn,
not assumed indistinguishable or continuously available.

ET-09/10/14/17/20/21 require independent-clock simulations, both 10→11 and 20→21
transitions, partial admission, stale extension predecessor, duplicate acceptance,
cap exhaustion, invoice-before-admission rejection, delayed two-stage funding,
refusal preserving existing streams, zero-fee renewal, free-only replacement,
new-stream draining and hard-age termination. All runtime, privacy and latency
tests remain deferred; these numeric policies are bounded test candidates.
