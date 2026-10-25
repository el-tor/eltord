# Initial operations, registration and lifecycle events

**Proposed version-one local control protocol; not implemented.** Use the
[renewal controller grammar, authority and errors](renewal.md). All commands
require the same privileged local daemon authority. Wallet verification remains
in the daemon; Tor owns live circuits, authenticated hop observations and clocks.
Neither controller success nor an event proves service delivery.

## Origin initial operations

```text
ELTOR_INITIAL_PREPARE 1 instance handle hop operation nonce terms
ELTOR_INITIAL_START 1 instance handle hop operation nonce
ELTOR_INITIAL_GET 1 instance handle hop operation
ELTOR_INITIAL_REFRESH 1 instance handle hop operation nonce
```

`instance`, `handle`, `operation` and `nonce` are 16-byte hex; `hop` is one-based
u16; `terms` is the exact 55-byte policy as 110 lowercase hex characters.
PREPARE derives A0 from retained construction IDs and creates PAYMENT_COMMIT.
START requires a verified COMMITTED result at that hop and creates the one
immutable PAYMENT_START_STATUS START. The daemon MUST collect all required hop
commitments before intentionally dispatching round-one payments or START.
REFRESH creates a PAYMENT_START_STATUS QUERY, not a new START. It is explicitly
wire-active; GET is strictly local and read-only. REFRESH requires committed
terms and uses the original lifetime/deadline, with one dispatch and no automatic
retry. Follow the initial wire QUERY rate limits. Origin-only handles are required.

For each operation store exact arguments, kind and immutable wire request before
dispatch. Same identity and bytes recover state without an immediate resend;
changed arguments conflict. Allow one PREPARE and one START per hop, with distinct
operation IDs, and at most one pending REFRESH. A different operation cannot
replace the existing PREPARE/START or reset its retry counter. Tor owns the
initial wire's four-transmission retry schedule. REFRESH nonce reuse across new
operations is rejected. Local acceptance replies are:

```text
250 ELTOR_INITIAL_QUEUED 1 instance handle hop operation revision
```

GET permits `operation=-` for only the current hop snapshot. Success:

```text
250-ELTOR_INITIAL 1 instance handle hop revision
250-PROFILE terms_digest initial_allocation state relay_revision
250-CLOCK current_round elapsed_upper_ms round_remaining_ms enforcement_remaining_ms setup_remaining_ms
250-OP operation kind phase request_digest transmissions
250 OK
```

`terms_digest`, `initial_allocation`, `request_digest` are 32-byte hex. `state`
is UNCONFIRMED or the exact initial wire state enum name. Before a committed
profile, digests, relay_revision and all CLOCK fields are `-`; do not manufacture
zero timestamps. Otherwise CLOCK values are u64 decimal, conservatively derived
from authenticated observations. `elapsed_upper_ms` includes the full elapsed
local time since the earliest dispatch of the corresponding request; remaining
budgets subtract that interval. Nonapplicable fields use `-`. The local revision
and relay_revision are separate counters; the latter is unavailable before an
authenticated status reply. For `operation=-`, OP has five `-` values.

`kind` is COMMIT, START or QUERY. `phase` is QUEUED, SENT, VERIFIED, REFUSED,
UNCERTAIN or CLOSED. VERIFIED means a matching authenticated reply was processed,
not necessarily ACTIVE; inspect PROFILE. A verified WAIT_PAYMENT response cannot
be treated as settlement. UNCERTAIN never permits additional spending by itself.
Local errors use the renewal table; missing commit returns
`551 INITIAL_STATE_UNCONFIRMED`. GET unknown operations returns
`552 UNKNOWN_OPERATION`. Refreshes cannot extend an absolute deadline inferred
from an earlier trustworthy observation. Contradictory authenticated observations
stop new funding and are reported as protocol failure.

## Relay registration snapshot

```text
ELTOR_PAYMENT_REGISTRATION_GET 1 instance handle period
```

This read-only query accepts relay-side handles only. `period` is u64, zero for
the initial ten rounds. Keep the active and admitted-next registrations queryable.
Once an older period is retired, return `552 UNKNOWN_PERIOD`; the durable daemon
ledger retains its accounting. No query recreates a retired allocation.

```text
250-ELTOR_REGISTRATION 1 instance handle revision period
250-POLICY terms initial_allocation allocation_digest first_round
250-STATE state current_round elapsed_ms setup_remaining_ms enforcement_remaining_ms
250-FEE amount_msat payment_hash decision authorization
250-ROUND round payment_id amount_msat opens_in_ms closes_in_ms decision payment_hash authorization
250 OK
```

Emit exactly ten ROUND lines in ascending order. `terms` is 55-byte hex; allocation
digests, payment IDs and hashes are 32-byte hex; decision is 16-byte hex or `-`.
`authorization` is NONE, AUTHORIZED or CONSUMED. All amounts, rounds, durations and
revisions are u64 decimal. `state` uses the initial wire state names. The fee line
repeats the same one-time fee state for each period; it does not create a new fee.
For a zero fee its hash is zero; the original dummy proof stays inside Tor.
Unclaimed positive round hashes are `-`, since a generated round ID need not be
the actual invoice hash. An authorized round reports the verifier's recorded hash.

Tor returns these snapshots only after the initial commit reserves its IDs.
Before that return `551 INITIAL_STATE_UNCONFIRMED`. The initial allocation is A0;
`allocation_digest` is A0 for period zero or the admitted renewal digest otherwise.
The first round is exactly `10*period+1`. Policy amounts determine expected fees.
A caller cannot choose another amount through registration.

Before T0, only round one is payable: its opening is zero and closing is the
remaining setup budget; later windows are `- -`. After T0, round r has nominal
start `T0+(r-1)*60000`, opens at `max(T0,start-lead_ms)` and closes at
`start+grace_ms`. Report remaining times clamped at zero, preserving the
fact of a passed deadline in the retained registration/authorization state.
The ledger records the original windows, not merely these remaining counters.
Renewal admission is separately required before any next-allocation payment.
Deadlines expire even if the controller is disconnected. Old settlements never
become valid because a new query returns zero remaining time.

For initial round one, the registration window starts at accepted COMMIT, before
T0 exists, and remains that original window after activation. Later rounds use
T0-based windows. The daemon anchors registration observations to monotonic and
UTC clocks with measured uncertainty, as required by [recovery](recovery.md).
A callback timestamp is never substituted for node settlement time.

## Inventory and event recovery

Tor assigns relay handles when initial payment-bearing construction is accepted.
The payment inventory contains all live paid-profile origin and relay handles,
including registrations not yet committed; roles are local ORIGIN or RELAY, not
an onion-service role. New handles produce an event but do not authorize payment.

```text
ELTOR_PAYMENT_LIST 1 instance cursor
```

The first page uses `instance=- cursor=-`; replies reveal the current instance.
Subsequent pages use that instance and the opaque cursor (16-byte hex):

```text
250-ELTOR_PAYMENT_LIST 1 instance snapshot_sequence next_cursor
250-CIRCUIT handle role revision
250 OK
```

Zero to 128 CIRCUIT rows per page, ordered by binary handle. A first request takes
an immutable inventory snapshot; `snapshot_sequence` is the current event sequence
at that capture. `next_cursor=-` ends it. A cursor belongs to the authenticated
control connection, snapshot and next page, expires 10 seconds after creation
and never extends on reads. Exact page retries return the same page. Allow one
snapshot per connection and at most four per process; maximum 4096 rows per
snapshot. Return `451 CAPACITY` rather than silently truncate. A stale cursor
returns `552 UNKNOWN_CURSOR`; an obsolete explicit instance returns STALE_INSTANCE.
Snapshot allocation is bounded query bookkeeping, never circuit/payment mutation.

Subscribe with `SETEVENTS ELTOR_PAYMENT_STATE` on an authenticated persistent
connection. The proposed event is:

```text
650 ELTOR_PAYMENT_STATE 1 instance sequence handle role revision kind
```

`sequence` is a process-wide u64 starting at one; increment for each event;
`revision` is the local circuit revision. `kind` is REGISTERED, COMMITTED,
START_REQUESTED, AUTHORIZED, ROUND_ACTIVE, RENEW_ADMITTED, RENEW_REFUSED,
GRACE, EXPIRED or CLOSED. Emit after the associated state transition is visible
to queries. Multiple transitions may occur before a query; event handlers fetch
a snapshot rather than reconstructing financial truth from events. Queries and
replayed commands do not create events. No payment material appears in events.

To resynchronize, subscribe first, buffer subsequent events, obtain all inventory
pages, then apply event hints after snapshot_sequence. Duplicates and lower
revisions are ignored; sequence gaps, overflow, disconnect or expired inventory
require another inventory/query pass. Missing handles must be reconciled, never
replaced automatically. Circuit closure racing the snapshot may make a subsequent
GET return UNKNOWN_CIRCUIT; that is loss of live state, not proof money is unspent.
A changed instance closes all old live bindings. Do not infer a completed wallet
transaction from Tor's absence of an old handle.

There is no durable event replay guarantee. Bound each connection's queued events
to 1024; disconnect a subscriber before exceeding it, forcing resynchronization.
All initial/renewal/registration queries share the 64 requests/second authority
budget. Initial operations share the 4096 pending operation ceiling. Keep at most
16 recent QUERY operations per hop; evict completed oldest QUERY results only.
Never evict active/pending COMMIT, START or renewal operations to make room.
Counters must not wrap: stop admitting work before exhaustion; a process restart
uses a new instance. Financial records are not evicted with control caches.

## Privacy/timing and acceptance checks

Controller handles, role and timing are privileged local metadata. Events omit
preimages, wallet identities, invoice strings, private paths and payment IDs.
Wire refresh deliberately produces encrypted traffic, unlike local GET; the
caller must not poll more frequently under uncertainty or imitate a heartbeat
without the resource budget. Initial exchanges keep uniform fixed sizes and
independent per-hop nonces, but do not hide temporal correlation from colluding
observers. Inventory limits must be tested at expected relay scale before use.

ET-08/09/30/33 require parser vectors for every reply and error, GET with no wire
or wallet effects, lost initial replies, event gaps/duplicates/reordering,
subscribe/snapshot races, relay versus origin role misuse, all-hop commitment,
clock-boundary queries, stale cursors and process restart. These are deferred
implementation tests. Documentation defines the contract, not runtime support.
