# Explicit foundation ledger maintenance

`eltor-foundation-ledger` is a separate operator command for the local foundation
fixture/regtest deployment. It never starts Tor, dispatches a wallet payment, or
changes runtime configuration. Daemon startup continues to reopen existing
storage only: it cannot provision, migrate, reset, or replace a ledger.

Use the existing private foundation JSON configuration. Its required fields are
`ledger_path`, `node_id`, `chain`, `cln_url`, `cln_rune`, and
`clock_uncertainty_ms`; see [the foundation plan](paid-circuit-foundation-plan.md).
A relative ledger path resolves beside the configuration file. On Unix the file
must be regular, not a symlink, and have no group/other permission bits. The
configuration is limited to 8 KiB and rejects unknown fields. Credentials stay
in this private file, never in command-line options or tracked examples.

Prepare the ledger's parent directory privately before creation. For example,
from the workspace root:

```sh
install -d -m 700 .runtime/foundation/ledger
```

Place the operator-reviewed JSON at `.runtime/foundation/relay.json`, with
`ledger_path` set to `ledger/account.sqlite`. Use the actual pinned native CLN
compressed node public key and chain for the isolated account. The read-only
rune must permit `getinfo`, `help` for `listinvoices`, and `listinvoices` with
`payment_hash`, `index`, `start`, and `limit` support. Maintenance probes one
bounded page of at most 16 incoming updates to check actual read permission.
The probe neither persists invoice data nor advances a durable discovery cursor.
Neither maintenance command needs `pay`, `fetchinvoice`, or outgoing lookup
permissions. Native transport still requires HTTPS or numeric loopback HTTP.

## Create an initial ledger

Provisioning is an explicit assertion that this is the initial authoritative
ledger for this account. A missing ledger after prior paid use requires restoring
its retained history; creating empty state for the same account cannot establish
replay safety. Native account verification does not establish that no previous
ledger existed.

```sh
./cargo-local.sh run --offline --manifest-path eltord/Cargo.toml \
  --bin eltor-foundation-ledger -- provision \
  --config .runtime/foundation/relay.json
```

The command checks that the target is absent, verifies the pinned CLN account
and read-only receiving capability, then uses exclusive `create_new` creation.
It refuses existing empty, corrupt, valid, directory, or symlink targets. Creation
uses mode 0600 on Unix and the existing immutable storage-budget profile. A
failed partial creation remains in place for inspection; another invocation does
not overwrite it. There is no force, reset, or delete option.

## Migrate retained storage

Stop the foundation daemon and other users of the ledger before maintenance.
Keep a private consistent copy of retained storage for recovery. The migration
command requires the existing authoritative file and the same pinned account:

```sh
./cargo-local.sh run --offline --manifest-path eltord/Cargo.toml \
  --bin eltor-foundation-ledger -- migrate \
  --config .runtime/foundation/relay.json
```

A read-only preflight checks the file, application ID, supported schema, account,
SQLite health, references, and rollback-journal mode before native verification.
The existing transactional migration dispatcher supports schema versions 1–6
to current schema 7, preserving records and uniqueness tombstones. An already
current ledger is explicitly verified without resetting anything. Unsupported,
missing, corrupt, wrong-account, or symlink storage is refused. Native identity
or receiving-capability failure also prevents migration.

Schema 7 retains the bounded origin-construction/accepted-plan journals
and exact native outgoing invoice artifacts for lookup-only recovery. Constructor
and renewal dispatch claims are single-use. The migration widens the database's
legacy round-counter constraint while the ledger still requires typed, immutable
all-hop admission before funding any renewed allocation. Existing unknown costs,
per-hop prices, aggregate financial caps, and hash/ID uniqueness remain intact.
It additionally retains preconstruction handshake-fee scopes, exact invoices and
one-shot invoice-request/payment dispatch claims. These upfront costs share the
account/path exposure limits with recurring round payments and remain reserved
when an outcome is uncertain. Binding the scope to the constructed native path
does not create another fee allowance. Payment preimages are never journaled.

Migration cannot invent historical payment evidence, discarded records, original
clock uncertainty, or missing invoice associations. Preserved legacy point-only
clock records therefore remain unsuitable for live interval-based authorization
until separately reconciled; migration never fabricates replacement clocks.

After explicit maintenance, configure the daemon with
`ELTOR_FOUNDATION_CONFIG=.runtime/foundation/relay.json` and the authenticated
Tor foundation profile. Startup still checks mode, existing ledger, account, and
capabilities independently. These commands do not enable foundation mode or
prove a paid circuit works.

## Attach to an existing relay process

`eltor-foundation-relay` runs only the foundation controller. Use it for an
already-running standalone Tor relay, including a generated local Chutney node.
It never starts embedded Tor or changes Tor configuration. Start only one
controller for each relay process; check existing controllers before launching.

Prepare the explicit account JSON and provision or migrate its ledger using the
commands above. Configure the existing Tor process with a matching hashed
control password through the local operator procedure. Cookie authentication
may remain enabled. Store the plaintext password in a separate private regular
file owned by the current user, with mode 0600 or 0400 on Unix. The file accepts
one printable ASCII password, optionally followed by one newline, with at most
1024 bytes total. Empty, oversized, multiline, symlink and group/world-readable
files are refused. Do not put the password in argv, environment variables,
tracked Tor configs or logs.

From the workspace root, after the combined daemon build:

```sh
target/debug/eltor-foundation-relay \
  --control 127.0.0.1:8061 \
  --password-file .runtime/foundation/relay-control-password \
  --foundation-config .runtime/foundation/relay.json
```

Replace the endpoint and private paths with those of the existing relay. The
endpoint must be a numeric loopback address with a nonzero port. Authentication
and an exact `ElTorFoundation=1` response are required before opening the existing
ledger and checking the pinned receiving wallet. Explicit configuration prevents
legacy fallback even when Tor reports foundation mode disabled. Startup failure
returns a nonzero exit status with a redacted diagnostic. Once attached, the
existing coordinator recovers its original process/handle state after control
connection failures and never interprets a lost response as permission to pay.
Stopping this foreground controller leaves the existing Tor process running.

Keep temporary Chutney configuration backups and restore them after the live
fixture run. This attachment command does not provision missing ledgers, launch
another wallet, or prove settlement using a real funded account.

## Attach to an existing standalone client

`eltor-foundation-client` attaches the same origin coordinator used by the app
to an existing local Tor client, such as Chutney node `005c`. Supply its own
explicit account JSON, previously provisioned ledger and private client plan.
The plan carries the original stable scope, ordered relay fingerprints and
finite monetary caps described in the foundation plan. Do not reuse the app's
ownership scope or change an accepted scope to recover a lost construction.

```sh
target/debug/eltor-foundation-client \
  --control 127.0.0.1:8060 \
  --password-file .runtime/foundation/client-control-password \
  --foundation-config .runtime/foundation/client.json \
  --client-plan .runtime/foundation/client-plan.json
```

The password-file and numeric-loopback restrictions are identical to the relay
controller. Authenticated foundation mode is required before wallet access and
is independently checked before live construction. Existing origin plans,
dispatch claims and uncertain native payments use the same recovery path as
the app. This foreground command never starts Tor, changes environment globals,
creates ledgers, enables legacy payment handling or allocates a replacement
scope after a failed attempt. The app continues using its existing explicit
environment paths. Start one origin controller for this standalone client and
stop it before restoring the temporary testnet configuration.

## Executed validation

Local HTTP fixtures verified native account/capability checks with only `getinfo`,
`help`, and a bounded receiving-only `listinvoices` probe; any other native method
fails the fixture. Tests cover
exclusive creation, private permissions, missing/corrupt/wrong-account and
unsupported storage, symlink/config refusal, native account/capability mismatch,
and explicit v4 migration retaining claims, consumed authorizations, batches,
and closed-instance tombstones. Existing ledger migration tests cover the older
schema routes. No real wallet was contacted or funded for this validation.

Privacy/timing assessment: maintenance reveals the existing pinned account and
read-only capability checks only to its configured native endpoint. It sends no
Tor relay messages, creates no cross-hop identifier, and performs no wallet
payment or retry. Native response bodies, credentials, ledger paths and payment
associations are excluded from command diagnostics. Real deployment timing and
full paid-path/regtest acceptance remain separate checks.


## Explicit payer account ceiling extension

Normal runtime configuration still requires an exact replay of the accepted
account budget. An operator may separately raise only its lifetime ceiling
after explicit spending approval, without replacing the authoritative ledger:

```sh
eltor-foundation-payer-ledger --config "$PAYER_CONFIG" \
  --expected-lifetime-msat 100000 --lifetime-limit-msat 200000
```

The command verifies the configured native account and capabilities with read-only
requests. A compare-and-swap transaction rejects a stale old ceiling, reductions,
or a blocked account. Repeating the exact successful extension is idempotent.
It changes neither outstanding/concurrency limits nor accepted path budgets,
spent amounts, reservations, operations, or payment evidence. Startup cannot
perform this extension implicitly. A new explicitly accepted path may use the
new account ceiling; existing paths keep their original limits.

For the manual paid-relay session, the user approved a 200-sat cumulative ceiling,
keeping the 1-sat relay round, zero handshake and at most 5 sats routing per payment.
A consistent private ledger snapshot and approval record were retained first.
The operator was exercised on a private copy: all non-budget rows and other budget
fields stayed identical, an exact retry succeeded, and a stale extension failed.
The approved transaction was then applied to the existing ledger: 96 sats spent
and 2 sats reserved were unchanged. This operation sent no payment.
