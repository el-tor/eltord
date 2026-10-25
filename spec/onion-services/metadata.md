# Descriptor metadata and catalog encoding, version 1

**Proposed encoding; not implemented or independently reviewed.** This chapter
makes the descriptor records referenced by the quote wire format concrete.
It does not change the fixed initial quote request or reply capacities.

## Descriptor placement and parsing

Add exactly one `eltor-service-v1 SP BASE64 NL` line to the service descriptor's
second-layer plaintext, before the first introduction-point section. BASE64 is
RFC 4648 standard padded base64 without embedded whitespace; decoding and
re-encoding must reproduce the exact string. Its decoded value is the metadata
record below, at most 16384 bytes. Reject duplicate lines, unknown versions,
trailing bytes and malformed lengths before any payment. An absent line means
this El Tor service profile is unavailable, not permission to use ordinary Tor.

Authenticate and decrypt the enclosing descriptor using the inherited
[Tor descriptor chain and encryption](https://spec.torproject.org/rend-spec/hsdesc-encrypt.html).
This extension is inside that protection, not an unsigned HSDir response or an
outer public wallet field. It needs no signature by the long-term onion identity
key beyond the inherited descriptor authority. Enforce Tor's total descriptor
size limit as well: the 16384-byte allowance does not reserve space independently
of introduction points, client authorization or other descriptor content. If the
complete descriptor does not fit, publish fewer offered payment methods/entries
or refuse configuration; never drop authentication or truncate entries.

All binary integers are unsigned network byte order. `L(x)` and `D(label)` use
the quote wire definitions (u32 byte length; ASCII plus NUL). Counts precede arrays;
there are no implicit optional fields, native structs, JSON or text numbers.

## Metadata container

Fields in exact order:

| Field | Encoding |
| --- | --- |
| schema | u16, exactly 1 |
| service_identity | 32 bytes, equal to the requested onion identity |
| metadata_revision | u64, positive, persistently increasing per service |
| published_at, valid_until | two u64 Unix-second timestamps |
| capability | L(canonical capability record) |
| signer_revoked_through | u64 generation floor, zero initially |
| request_revoked_through | u64 generation floor, zero initially |
| signer_count | u8, 1 or 2 |
| signers | signer_count delegation records |
| request_key_count | u8, 1 or 2 |
| request_keys | request_key_count delegation records |
| catalog_count | u8, 3 through 16 |
| catalog | catalog_count entries: u16 type followed by L(canonical entry) |

Metadata validity is at most 7200 seconds. Delegate validity can exceed the
current container lifetime; an expired container cannot authorize a new inquiry.
Metadata revisions survive restarts and descriptor time-period changes. Replicas
must share this authority sequence. A rollback cannot be fixed by resetting the
counter: stop new quotes until the retained revision floor is restored.
For a given service/revision all metadata bytes must be identical, even when the
same metadata is carried by multiple Tor descriptors. Different bytes at the
same revision fail closed. Clients persist the highest verified revision and
revocation floors they have observed; lower revisions never undo revocation.
First-time clients lack that history, so expiry only bounds replay exposure;
it does not provide instantaneous revocation or prevent selective suppression.

Clock validity requires the complete local UTC uncertainty interval to lie
inside `[published_at,valid_until)`. The proposed test profile allows at most
five seconds of clock uncertainty. Do not add five seconds to expiry. If a clock
cannot meet the bound, stop new authorizations and repair time outside the payment
protocol. This is a candidate operational requirement, not a measured guarantee.

## Capability snapshot

The canonical capability record is exactly:

```text
u16 service_version=1
u16 quote_wire_profile=1
u16 transport_profile=1
u16 crypto_profile=1
u16 resource_profile=1
u8 payment_methods
u64 supported_features
u32 max_circuit_age_seconds
u32 max_rounds
u8 selection_count
selection[selection_count]
```

Each selection is `u8 method || funding_policy_ref32 || issuer_ref32 || acquisition_ref32`
(97 bytes). `payment_methods` bit 0 is BOLT11, bit 1 BOLT12; other bits are zero.
It is nonzero and exactly matches selections; one selection per advertised method,
ascending method order, one or two selections. `supported_features` bit 0 is
in-place renewal; all other bits zero. Finite positive age and round ceilings
bound quotations. The resource profile names the [common version-one limits](../validation/resource-profile.md),
not arbitrary peer-selected parser sizes. Transport/crypto profile IDs refer to
the corresponding version-one chapters and cannot change under this version.

`capability_digest = SHA256(D("eltor/scq/v1/capability") || service_identity32 || L(capability))`.
This snapshot includes catalog references, not key generations or descriptor
revision. Routine key rotation can therefore refresh delivery authority without
repricing a logical inquiry. Changing prices, catalog selection or capabilities
changes the digest and requires a fresh explicitly approved inquiry. The service
may retain an old accepted snapshot but cannot substitute a new one in its reply.
The quote must use the three references selected for its payment method.

## Delegations and revocation

Each delegation is exactly 60 bytes:
`u8 purpose || u8 algorithm || u16 service_version || u64 generation ||
u64 not_before || u64 not_after || public_key32`.
Purpose 1 means service protocol signing, algorithm 1 Ed25519. Purpose 2 means
quote/control request decryption, algorithm 2 X25519 under the fixed HPKE suite.
Version is 1. A purpose-1 key may authenticate version-one quotes, invoice
bindings and service status under their distinct signature domains; it cannot
sign descriptors or authorize another service. Purpose-2 keys never sign.
The enclosing verified descriptor delegates authority; there is no self-signed
key certificate trusted in isolation. Validate keys with the fixed crypto profile.

Order each array by increasing positive generation, with no duplicates. Keys
must differ across purposes and across generations. No generation may be reused
for a different key or validity interval. Each interval is positive and at most
86400 seconds; two records may overlap for at most 7200 seconds. These are finite
test-profile ceilings requiring rotation/availability measurements. The service
publishes the replacement in advance, and uses the highest currently valid
non-revoked generation for new outbound signatures. Request senders similarly
select the highest valid request generation. Receivers try at most the two
currently valid request keys; generations remain encrypted inside the request.
No unbounded old-key search or public generation hint is added.

A revocation floor invalidates all generations less than or equal to it for that
purpose. Floors never decrease; compromise recovery publishes a replacement above
the floor through a fresh authenticated descriptor. Retire a compromised private
key immediately, including during normal overlap. Do not accept an old delegation
because a peer backdates a signature. Ordinary expiry does not rewrite an already
accepted quote, but new messages need current authorized signing/delivery keys.
A new delegated signer may attest retained accepted quote state under the exact
old core digest; it cannot change the receiver, amount or policy. Known compromise
suspends dependent payments until fresh authority and retained state are verified.
This mechanism does not revoke the onion identity or repair compromised descriptor
signing authority. Such compromise requires an external trust recovery decision.

## Canonical catalog entries

Use the existing service/type-scoped catalog digest, over exactly the entry bytes
specified here. Sort catalog entries lexicographically by `(type,digest)`; reject
duplicates, unknown types, missing selected references or method mismatches.
Each entry is at most 4096 bytes and the sum of all encoded catalog entries is at
most 12288 bytes, within the container ceiling. Lookup depth is at most two:
capability/quote to entry, acquisition to issuer. No URI fetch or recursive catalog.
Unused entries are allowed only when referenced by another advertised selection.

Every entry starts with `u16 schema=1 || u8 method`, where method is 1 or 2.
Following fields are ordered as below; no trailing bytes are permitted.

### Type 1: funding policy

Append:

```text
u8 fee_rule=1
u8 refund_rule
u8 start_policy=1
u16 round_seconds=60
u16 advance_seconds=60
u16 grace_seconds
u32 build_timeout_seconds=60
u16 recovery_seconds=15
u8 startup_round_cap=2
u8 ready_retransmissions=3
u64 max_relay_msat_per_round
u64 max_service_routing_msat_per_round
u64 max_setup_msat
u64 max_application_msat
u64 max_payer_routing_msat_per_payment
u64 max_total_msat
u64 max_outstanding_msat
u32 max_circuit_age_seconds
u32 max_rounds
```

`fee_rule=1` means integer millisatoshis, no rounding surcharge: setup and
application charges occur at most once, recurring charges only for authorized
rounds, and routing charges stay within their separately disclosed caps.
No minimum ten-round purchase, reset fee, retry charge or implicit top-up exists.
Amounts are ceilings; the immutable SCQ specifies exact charges within them.
Total/outstanding/age/round ceilings must be positive and finite; individual
charges may be zero. Grace is at most 60 seconds. SCQ timing fields must equal
this record's timing, not merely fit under a larger number. The two-round startup
ceiling includes unresolved dispatched payments, not just settled ones.

`refund_rule=0` discloses no guaranteed refunds; `1` requires returning verified
unspent service-side reserved principal after reconciliation, excluding disclosed
incurred setup/application/routing costs. Neither refunds unknown or spent money
as if it were unspent, nor authorizes a new circuit. Refund dispatch requires a
separate authenticated recipient instruction and idempotent outgoing intent;
its transport is part of the later accounting protocol. Unknown rules fail.
Rule 1 cannot be advertised until that refund mechanism is implemented and tested.
These policy codes do not guarantee delivery or recovery from a dishonest peer.

### Type 2: payment issuer

Append `u8 binding_profile=1 || authority_key32` (Ed25519). The key is a
service-scoped invoice-binding authority, not necessarily a public Lightning node
key. It signs the exact acquired invoice/payment-request association defined in
the acceptance protocol. Descriptor authentication establishes the delegation;
wallet-native invoice/offer validation is additionally mandatory. For BOLT11,
that authenticated endorsement identifies the invoice the service accepts for
this obligation; do not require a direct node connection from the client. For
BOLT12, validate the complete offer/invoice-request/invoice relationship as well.
Use a separate key from quote signing, request encryption and wallet keys.
Replacing this authority creates a different issuer reference; it does not
silently update accepted quotes. Known compromise requires suspended payment and
explicit reauthorization/reconciliation, not substitution of a recipient key.

### Type 3: BOLT11 acquisition

Method must be 1. Append `u8 mechanism=1 || issuer_ref32 || u16 invoice_profile=1`.
Mechanism 1 is the authenticated service invoice-request exchange, with exact
quote, client, circuit, round and amount bindings. There is no URL, reusable invoice
or BOLT11 offer in this record. Requests must recover existing unresolved state
before permitting a fresh invoice. The acceptance protocol defines the exchange.

### Type 4: BOLT12 acquisition

Method must be 2. Append `u8 mechanism=2 || issuer_ref32 || u16 invoice_profile=1 ||
u16 offer_length || offer_ascii[offer_length]`. Length is 1 through 2048 bytes.
Use the exact valid lowercase, whitespace-free, continuation-free BOLT12 offer string; uppercase, `+`,
continuation whitespace and alternate textual spellings are not canonical entry
bytes. Validate the offer under the wallet's supported BOLT12 profile before
acceptance. Do not merely hash a string that the wallet cannot interpret. Reject
oversized offers rather than truncate, dereference an external URL or expand the
initial quote. The offer is for the chosen chain and payment method; the wallet
must reject chain mismatch and incompatible/unknown required features.
Blinded paths remain recommended, optional receiver policy. Nothing here proves
that every installed wallet supports them or that sender privacy is unconditional.

## Acceptance, privacy and validation

Resolve all selected entries within the verified container before accepting an
SCQ. Pin their exact bytes with the quote and durable ledger. A new descriptor
cannot change already accepted semantics. Enforce ceilings before allocating or
verifying variable-size records, then authenticate and validate references before
wallet work. Valid metadata with an unsupported wallet profile still fails safely.

Keys/catalogs are inside the inherited encrypted layer, but descriptor readers
can learn their contents and correlate policies or reused issuer keys. Descriptor
size and update cadence remain observable; preserve inherited padding and test
publication through the bounded free allowance. Fixed quote envelopes hide method
length differences there, not the later wallet protocol or total descriptor size.
No extra public identity, request-key hint or new fetching host is introduced.

ET-13/14/19/24–27 require canonical positive/negative vectors, duplicate revision
and revocation rollback tests, bounded two-key decryption, clock-boundary cases,
wrong service/type/method/issuer references, complete descriptor size checks,
oversized offers and stale-key retries. Verify both wallet methods independently.
Only schema arithmetic and documentation checks accompany this proposal; wallet
interoperability, cryptographic review and timing/resource measurements remain
deferred implementation/deployment gates.
