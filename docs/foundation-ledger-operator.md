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
The existing transactional migration dispatcher supports schema versions 1–5
to current schema 6, preserving records and uniqueness tombstones. An already
current ledger is explicitly verified without resetting anything. Unsupported,
missing, corrupt, wrong-account, or symlink storage is refused. Native identity
or receiving-capability failure also prevents migration.

Schema 6 also retains private bounded origin-construction/accepted-plan journals
and exact native outgoing invoice artifacts for lookup-only recovery. Constructor
and renewal dispatch claims are single-use. The migration widens the database's
legacy round-counter constraint while the ledger still requires typed, immutable
all-hop admission before funding any renewed allocation. Existing unknown costs,
per-hop prices, aggregate financial caps, and hash/ID uniqueness remain intact.

Migration cannot invent historical payment evidence, discarded records, original
clock uncertainty, or missing invoice associations. Preserved legacy point-only
clock records therefore remain unsuitable for live interval-based authorization
until separately reconciled; migration never fabricates replacement clocks.

After explicit maintenance, configure the daemon with
`ELTOR_FOUNDATION_CONFIG=.runtime/foundation/relay.json` and the authenticated
Tor foundation profile. Startup still checks mode, existing ledger, account, and
capabilities independently. These commands do not enable foundation mode or
prove a paid circuit works.

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
