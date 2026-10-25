# Acceptance and invoice messages, version 1

**Proposed protocol, not implemented.** This chapter defines service-control
records separately from the fixed initial quote. It inherits the quote crypto
suite, canonical integer encoding, `D`/`L`, delegated keys and receiver-choice
privacy policy. All references below are inside authenticated encryption.

## Common protected control delivery

Use a separate service-control envelope of exactly 3472 bytes in each direction:
HPKE encapsulation 32 bytes, plaintext 3424 bytes, AEAD tag 16 bytes. Plaintext is
`u16 record_length || signed_record || zero_padding`, with exact zero padding to
capacity. The signed record includes one final 64-byte Ed25519 signature.
Each direction uses eight 434-byte slices and the existing 64-byte routing header,
Length 498. This equal-size profile applies to both methods and all success,
pending and refusal replies; it does not resize initial quotes or bulk traffic.

Allocate CONTROL_REQUEST=234, CONTROL_DELIVER=235, CONTROL_RETURN=236,
CONTROL_REPLY=237. Advertise `eltor-service-control 1 234 235 236 237` in the
introduction relay's signed descriptor. These are proposed El Tor assignments,
not upstream reservations. Require this capability in addition to quote transport
before offering a payable SCQ. The service metadata's transport_profile=1 includes
both sets of commands. No automatic paid path is needed for control delivery.

Use the quote slot's circuit-generation binding and rewritten leg-local routes,
but kind=3 request or 4 reply, count=8, indices 0 through 7 in order. The first
request fragment reserves the slot and absolute 30-second expiry. Reassemble the
entire request at the introduction relay before forwarding all eight DELIVER
fragments; do not extend expiry during assembly. For replies, validate and forward
ordered slices, with the client acting only after full authenticated reassembly.
Duplicate/conflict/wrong-circuit rules are identical to quote transport. Reserve
16384 bytes per live/terminal control slot, including retained fragment comparison
state. Control and quote slots share the same counts, admission buckets and
process budgets; slots do not each get an independent allowance. Later resource
profiles must account for this larger shared maximum. Unknown/expired tokens
never allocate reassembly. One complete exchange is 32 forwarded fragments over
all four legs, 15936 logical payload bytes, plus real fork-cell/path overhead.

Use new domains `eltor/scq/v1/control-request`, `.../control-reply` for client and
service signatures over `D(domain)||L(body)`. HPKE info is
`D("eltor/scq/v1/hpke-control-request")` or `.../hpke-control-reply`.
AAD is `D("eltor/scq/v1/control-envelope")||u8(kind)` with kind 3 or 4.
Each delivery uses fresh HPKE, nonce and reply key. No initial-quote signature or
ciphertext is valid under these domains. Authenticate before parsing variable
payloads; reject unknown message kinds, flags, enums and trailing bytes.

The service follows the quote profile's common 2–4 second response target. Wallet
work runs asynchronously; if an invoice is not available at the target, return
PENDING rather than waiting indefinitely or extending a slot. A subsequent
query recovers the same logical operation. No more than three transport attempts
per operation delivery group, forty seconds per attempt, shared backoff and
original expiry. Pending status polls require fresh delivery keys, at most one
poll per five seconds per agreement and six pending polls per round, within all
funding/setup deadlines. Exhaustion stops new dispatch; it does not declare a
wallet failure or erase an unresolved financial commitment.

## Request body

Encode these fields in order, without optional fields:

```text
u16 version=1
u8 message
u8 method
service_identity32 || quote_core_digest32 || quote_id32
circuit_authorization_id32 || client_auth_key32 || operation_id32
issuer_ref32 || acquisition_ref32
u32 round
u64 total_cap_msat || u64 outstanding_cap_msat || u64 payer_fee_cap_msat
u64 operation_expires_at
u64 request_key_generation
nonce32 || reply_encryption_key32
L(payload)
```

The fixed prefix is 368 bytes. Payload is at most 2800 bytes, additionally bounded
by its message schema. Append the client signature. Total signed length must fit
3422 bytes. `operation_id` is fresh random logical state scoped to service/client/
quote/circuit, never reused across distinct requests. It survives delivery retries.
The three caps must equal accepted SCQ caps; reducing a local user limit can stop
payment locally, but cannot silently amend signed terms. Method/issuer/acquisition
must equal the retained quote. The core digest commits to all other pricing and
policy fields, including funding_policy_ref; do not resolve a new catalog on retry.

Messages: 1 ACCEPT_ROUND, 2 ROUND_INVOICE, 3 ROUND_QUERY. Other values are reserved.
ACCEPT_ROUND requires round 1 and durably accepts the exact offered core before
invoice creation. ROUND_INVOICE requires existing acceptance and a currently
admissible round. ROUND_QUERY is read-only financial observation: no invoice
creation/replacement, acceptance, wallet dispatch or paid build. It uses its own
query operation ID but looks up the unique agreement/round record. QUERY payload
is empty. ACCEPT/INVOICE payload is the acquisition payload below.

The stable request digest is SHA256 of
`D("eltor/scq/v1/control-logical") || L(body[0:296]) || L(payload)`.
Bytes 0–295 include the immutable expiry and exclude request generation/nonce/key.
The service records that digest for mutations before invoking invoice creation.
Different stable bytes under one operation ID conflict; a new operation for an
already registered round recovers that round only if its acquisition matches.
A new operation ID never creates a second invoice or resets the round deadline.
The full delivery digest is SHA256 of
`D("eltor/scq/v1/control-delivery") || L(body||client_signature)`.

## Response body

```text
u16 version=1 || u8 message || u8 status || u8 method
service_identity32 || quote_core_digest32 || quote_id32
circuit_authorization_id32 || client_auth_key32 || operation_id32
issuer_ref32 || acquisition_ref32
u32 round || u64 agreement_revision
request_delivery_digest32 || u64 signer_generation
nonce32 || reply_encryption_key32
L(payload)
```

The prefix is 377 bytes; payload at most 2800; append service signature, then pad
and encrypt. Echo message, bindings, nonce/reply key and exact request delivery
digest. Revision is a positive durable agreement counter after acceptance; zero
only for refusals before any agreement exists. Counters never wrap. Clients retain
a high-water revision but an event is never proof of their own wallet outcome.

Statuses: 1 INVOICE, 2 PENDING, 3 SETTLED_RESERVED, 4 ZERO_AUTHORIZED,
5 REFUSED, 6 CLOSED_UNRESOLVED, 7 FAILED_FINAL. INVOICE alone permits considering
a payment, after all gates below. SETTLED_RESERVED reports service node/ledger
state, not another request to pay or proof of rendezvous. ZERO_AUTHORIZED is only
for a quoted amount of zero, with no fabricated invoice. PENDING/FAILED_FINAL/
REFUSED/CLOSED_UNRESOLVED never authorize wallet dispatch. Missing replies change
no financial state. New valid observations can refine UNKNOWN but cannot reopen
closure, reset expiry or duplicate a purchase.

For non-INVOICE replies, payload is exactly
`u16 reason || u64 expected_amount_msat || payment_hash32` (42 bytes).
Reason 0 means no error; 1 capacity, 2 expired, 3 bad binding/terms, 4 unsupported
method, 5 wallet pending, 6 uncertain outcome, 7 closed, 8 terminal wallet failure,
9 round not admitted. Refusal requires nonzero reason; otherwise use 0 except
PENDING (5/6), CLOSED_UNRESOLVED (6/7) and FAILED_FINAL (8). Unknown enums fail.
Payment hash is zero if unavailable or zero-priced, never invented. Expected
amount is the retained calculation, zero for pre-acceptance refusal. An unsigned
transport error cannot become one of these authenticated service outcomes.

## Acquisition payload and invoice endorsement

For BOLT11, ACCEPT/INVOICE payload is empty. The service creates a fresh, exact
amount invoice for the unique round obligation through its configured wallet.
For BOLT12, payload is `u16 invoice_request_length || invoice_request_ascii`, at
most 2048 ASCII bytes of valid lowercase, whitespace-free, continuation-free canonical BOLT12 invoice
request. The client's wallet generates and retains this exact native request,
including its payer key, offer relation and amount. Retries must use those same
native request bytes while refreshing El Tor delivery encryption. The service
passes it to the receiving wallet's native invoice-request handler, not pay_offer.
This tunneled acquisition is a required adapter capability: do not claim support
from an API that merely pays an offer and hides the invoice before verification.

Native semantics follow [BOLT12](https://github.com/lightning/bolts/blob/master/12-offer-encoding.md).
The El Tor catalog/control representation is stricter than its display format:
normalize valid wallet input before signing by using lowercase and removing
allowed continuation separators; on the wire reject alternate spellings, including
`+`, rather than silently changing signed bytes.

The invoice must match the exact accepted offer, request, chain and amount under
native BOLT12 validation. Preserve the optional blinded receiver policy; service
endorsement does not replace native authentication. The service does not need
the client's payer private key. El Tor does not add a cross-service wallet ID.
Adapters unable to generate/validate this bounded request/response fail capability
checks before acceptance; no silent fallback to BOLT11 or direct external URL.

An INVOICE payload is:

```text
u8 endorsement_version=1
issuance_id32 || native_request_digest32 || payment_hash32
u64 amount_msat || u64 pay_before
u16 invoice_length || invoice_ascii[invoice_length]
issuer_signature64
```

Invoice length is 1 through 2048. Use exact valid lowercase whitespace-free, continuation-free native
BOLT11 invoice or BOLT12 invoice text; reject invalid encodings, chain/amount/hash
mismatches and unsupported required features. BOLT11 native_request_digest is
zero32; BOLT12 uses SHA256 over the exact ASCII request bytes. `issuance_id` is a
fresh random 32-byte identifier retained durably for this round's one invoice.
The issuer signs:

```text
D("eltor/scq/v1/invoice-binding") || service_identity32 || quote_core_digest32 ||
quote_id32 || circuit_authorization_id32 || client_auth_key32 ||
issuer_ref32 || acquisition_ref32 || u8(method) || u32(round) ||
L(invoice_payload_without_issuer_signature)
```

Verify using the accepted issuer entry's authority key, then validate the native
invoice. This stable endorsement survives status-query delivery wrappers; the
service's fresh outer signature binds it to the current delivery. Issuer key
compromise suspends dependent payments, not just display of warnings.

Compute round amount with checked u64 arithmetic as relay_msat_per_round plus
service_routing_msat_per_round, plus setup_msat and application_msat only for
round one. Invoice native amount must exactly equal that result. Payer routing
fees are not added to the invoice; reserve their permitted maximum in the client's
spending/exposure ledger before dispatch. Unused allowance treatment follows the
accepted fee/refund policy. `pay_before` is the earliest applicable invoice,
round-funding or setup deadline; it never extends accepted clocks. Native expiry
must be no earlier than pay_before. For the initial invoice before FUNDED, expiry
is bounded by the immutable operation expiry and the offer's acceptance validity;
reservation later starts the separate setup deadline. Subsequent invoice requests
cannot rely on the now-expired initial quote acceptance window to change terms.

## Idempotency, expiry and settlement

Before invoice creation, persist one round obligation and wallet creation intent.
If creation times out, query by durable wallet operation/issuance association.
Never create a second invoice just because no reply arrived. An adapter lacking
recoverable creation must mark the round uncertain and stop. Returning the same
invoice after a retry is safe only while all original limits still hold.

Version one does not replace an invoice within the same agreement/round. If it
expires or definitively fails, report terminal status and reconcile. A fresh
circuit/SCQ needs explicit authorization; unresolved exposure stays reserved.
This deliberately avoids double-charge races from simultaneous old/new invoices.
Native wallet payment dispatch likewise persists and queries the exact invoice
hash before retry; obtaining a repeated INVOICE does not tell the client to pay
twice. The service verifies recipient-node settlement, amount and window, then
atomically reserves that payment for the unique obligation before paid work.
Late settlement is reconciled under original terms; no reopening or automatic
credit transfer. Zero amount follows mandatory zero-charge authorization records.

A request may authorize only a currently permitted round: initial round one,
then at most current plus next, bounded by startup cap, all-hop admission and
SCQ continuation. Queries never grant missing admission. Accepting a quote does
not preauthorize ten invoice payments or a replacement paid circuit. Ledger and
replay retention follow the durable recovery chapter, including process loss.

## Privacy and validation

Control requests/replies use equal fixed capacities for both methods and refusal,
with fresh delivery encryption and independent route tokens. The introduction
relay can distinguish this larger control profile from the initial quote, and
wallet latency/status polling remains observable. No anonymity guarantee follows
from padding. Payload parsing and wallet work are bounded and outside Tor's cell
handler; malformed inputs cannot dispatch payments or reserve unbounded state.

ET-14/16/18/23/27/28 require cross-language signature/domain/length vectors, strict
native invoice/request validation for each wallet, duplicate creation under lost
replies, wrong-round/amount/issuer substitution, pending creation and payment,
zero-charge paths, expiry races and ledger crash boundaries. Test all 32 forwarded
fragments and resource contention. Independent review and runtime interoperability
remain required; drafting these records does not implement wallet support.
