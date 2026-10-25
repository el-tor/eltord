> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# 1. Common representation

Offsets are zero-based, ranges half-open. A numeric field of 1/2/4/8 bytes is an unsigned u8/u16/u32/u64 respectively, multibyte values in network byte order. Identity/key/digest/nonce/reference fields are raw bytes, not integers or hex text. Service identity is the 32-byte v3 onion identity public key. Rendezvous identity is the 32-byte Ed25519 relay identity resolved through verified directory information; the RP's other required link/handshake information is obtained from that verified record, not an arbitrary untrusted address supplied by this request.

`service_version=1`; inner `record_kind=1` is initial quote request and `2` is quote reply. `payment_method=1` is BOLT11_INVOICE and `2` is BOLT12_OFFER. Reply `status=1` is an offered quote and `2` is a service refusal. No other values or implicit defaults are accepted in these initial records. These draft assignments are El Tor-local and are not upstream Tor or Lightning registrations. The [acceptance and round-invoice flow](acceptance.md#quote-acceptance-and-round-specific-invoice-acquisition) is now recorded in spec two. Later acceptance, invoice/status, and continuation messages still need separate schemas, not reinterpretation of these two records.

All costs/caps are integer millisatoshis. Per-round service relay cost and service-side outgoing routing allowance are separate; setup and application charges are initial one-time maxima, not recurring fees. The payer-routing cap bounds the client's fee for each payment to the service. Total and outstanding caps cover this service-side purchase, including its applicable payer fees; the client separately budgets payments to its own relays. Checked arithmetic must prevent overflow or rounding-driven excess. Zero cost is permitted when explicitly quoted, but never turns a refusal into a payable quote. Age/round/spending ceilings must be explicit and finite; zero does not mean unlimited. A quote's costs, ceilings, and features must fit the request and local policy.

Timestamps are unsigned Unix seconds; durations are seconds. Preserve the lifecycle requirements for freshness and bounded clock tolerance; a sender timestamp alone cannot prove timely admission. The absolute inquiry deadline is stable across retries, and local monotonic delivery/slot deadlines remain separate. `capability_digest` references the authenticated canonical capability/terms snapshot selected for this inquiry; its canonical descriptor schema is defined in [metadata encoding](metadata.md). New request-key generations may come from a newer authenticated descriptor without silently replacing the inquiry's original price/capability snapshot. If current authority no longer supports that snapshot, refuse/expire the inquiry rather than reset it or automatically buy again.

## 2. Request body (316 bytes)

The fields below form the complete canonical initial request body, excluding its signature and encrypted length/padding:

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 2 | `service_version` |
| 2 | 1 | `record_kind` |
| 3 | 1 | `payment_method` |
| 4 | 32 | `service_identity` |
| 36 | 32 | `capability_digest` |
| 68 | 32 | `logical_inquiry_id` |
| 100 | 32 | `client_auth_key` |
| 132 | 32 | `rendezvous_identity` |
| 164 | 8 | `max_relay_msat_per_round` |
| 172 | 8 | `max_service_routing_msat_per_round` |
| 180 | 8 | `max_setup_msat` |
| 188 | 8 | `max_application_msat` |
| 196 | 8 | `max_payer_routing_msat_per_payment` |
| 204 | 8 | `max_total_msat` |
| 212 | 8 | `max_outstanding_msat` |
| 220 | 4 | `max_circuit_age_seconds` |
| 224 | 4 | `max_rounds` |
| 228 | 8 | `required_features` |
| 236 | 8 | `inquiry_expires_at` |
| 244 | 8 | `request_key_generation` |
| 252 | 32 | `delivery_nonce` |
| 284 | 32 | `reply_encryption_key` |

Bytes `[0:244]` are the stable logical request projection. Preserve them, including client authorization and deadline, on all delivery retries. Bytes `[244:316]` select the authorized request-key generation and fresh delivery binding. Changing a logical field requires a new explicit inquiry, not resetting the retry/spending budget or abandoning uncertain payments. The full body is signed by the `client_auth_key`; the 64-byte Ed25519 signature follows at offset 316, producing a 380-byte signed record. Public client key validity and signature validation do not prove uniqueness, settlement, or entitlement to paid work.

`required_features` bit 0 requires in-place paid circuit renewal; other bits are reserved and must be zero in this draft. A successful quote must include every required feature; it may not silently downgrade renewal to forced disconnection. `max_rounds` and `max_circuit_age_seconds` bound approved continuation across allocations, not a requirement to buy those rounds. Requests outside local finite ceilings fail before payment.

## 3. Stable reply core (456 bytes)

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 2 | `service_version` |
| 2 | 1 | `status` |
| 3 | 1 | `payment_method` |
| 4 | 32 | `service_identity` |
| 36 | 32 | `capability_digest` |
| 68 | 32 | `logical_inquiry_id` |
| 100 | 32 | `logical_request_digest` |
| 132 | 32 | `client_auth_key` |
| 164 | 32 | `rendezvous_identity` |
| 196 | 32 | `quote_id` |
| 228 | 32 | `circuit_authorization_id` |
| 260 | 8 | `relay_msat_per_round` |
| 268 | 8 | `service_routing_msat_per_round` |
| 276 | 8 | `setup_msat` |
| 284 | 8 | `application_msat` |
| 292 | 8 | `payer_routing_cap_msat_per_payment` |
| 300 | 8 | `total_cap_msat` |
| 308 | 8 | `outstanding_cap_msat` |
| 316 | 8 | `accept_before` |
| 324 | 4 | `first_round` |
| 328 | 2 | `round_count` |
| 330 | 2 | `round_seconds` |
| 332 | 2 | `advance_seconds` |
| 334 | 2 | `grace_seconds` |
| 336 | 4 | `build_timeout_seconds` |
| 340 | 4 | `max_circuit_age_seconds` |
| 344 | 4 | `max_rounds` |
| 348 | 8 | `features` |
| 356 | 1 | `start_policy` |
| 357 | 32 | `funding_policy_ref` |
| 389 | 32 | `payment_issuer_ref` |
| 421 | 1 | `acquisition_kind` |
| 422 | 32 | `acquisition_ref` |
| 454 | 2 | `refusal_reason` |

The complete 456-byte core is immutable for a logical quoted result. `quote_id` and `circuit_authorization_id` are independent fresh 32-byte service-generated opaque identifiers, protected inside encryption. Generation identifiers are service-scoped u64 counters authenticated by their descriptor delegations; persistence/recovery must prevent ambiguous reuse. They are not public cross-service tracking tokens.

For `status=1`, `refusal_reason=0`, `first_round=1`, `round_count=10`, and `round_seconds=60`. Ten rounds describe the initial allocation only; clients still buy one minute at a time and can stop. `advance_seconds`, `grace_seconds`, build timeout and maximum age/round ceilings come from understood authenticated policy and must fit client limits, key/quote deadlines, and the timing rules in spec two. Encoding these fields does not establish a measured universal payment lead/grace or override operator policy. Start policy `1` means the initial setup/first round is settled and reserved before paid construction, with a bounded build deadline; the exact start/readiness event exchange must be specified before deployment. Other start policies are rejected.

Reply `features` uses the request bit assignments and must satisfy the required set. `funding_policy_ref`, `payment_issuer_ref`, and `acquisition_ref` are 32-byte authenticated references, never URL redirects or proof of settlement. The funding policy must resolve to a fully understood authenticated record specifying fee treatment, start/deadline rules, failure/retry/refund policy and limits before acceptance; a hash of unknown terms cannot authorize payment. The payment issuer reference binds the intended receiver under the authenticated method policy. Acquisition kind `1` denotes the bounded service-issued BOLT 11 invoice mechanism; kind `2` denotes a BOLT 12 offer selected from authenticated metadata. The acquisition reference identifies that mechanism/offer without embedding the variable invoice or offer. Bind the later invoice, receiver, exact amount and round to this SCQ under the separate acquisition protocol. [Spec two's catalog section](catalog.md#authenticated-payment-policy-catalog-and-reference-resolution) now defines service/type-scoped content references and bounded authenticated resolution. Canonical descriptor/catalog/offer/policy schemas are defined in [metadata encoding](metadata.md); unsupported/unresolved references fail without payment. Accepted snapshots are retained rather than silently replaced by a newer descriptor.

For `status=2`, keep version/method/service/capability/inquiry/logical digest/client/RP confirmation fields valid. Zero every subsequent core field from `quote_id` through `acquisition_ref`; set only `refusal_reason` to a recognized nonzero code. Initial codes: `1` unavailable capacity/path; `2` requested budget/terms unsupported; `3` authenticated capability snapshot stale; `4` method unavailable. Unknown codes are terminal non-payable failures; authenticated stale metadata still follows the existing bounded refresh policy. Missing/invalid version negotiation is not an exception permitting a payable refusal. The wrapper and service signature remain valid, and the refusal uses the same reply size/scheduler as success. Zero fields must never be interpreted as a zero-price offer.

## 4. Fresh reply wrapper and signatures

Append the following to the core; offsets are relative to the full reply body:

| Offset | Bytes | Field |
| --- | --- | --- |
| 456 | 1 | `record_kind` |
| 457 | 8 | `signer_generation` |
| 465 | 8 | `request_key_generation` |
| 473 | 32 | `delivery_nonce` |
| 505 | 32 | `reply_encryption_key` |
| 537 | 32 | `delivery_request_digest` |

The signed reply body is 569 bytes. `record_kind=2`; request-key generation, nonce, and reply key echo the active authenticated delivery. `signer_generation` selects a currently authorized quote signer under the independently verified descriptor chain; an in-message generation alone delegates nothing. Append the 64-byte service signature at offset 569 for a 633-byte signed reply record. Cached replies may reuse the exact core but must create and authenticate the new wrapper and fresh HPKE delivery. Normal signer rotation may therefore preserve the core without reissuing purchases or repricing.

Define `D(label) = ASCII(label) || 0x00` (exact ASCII without whitespace, followed by one zero byte), and `L(x)=u32be(length(x)) || x`. Use plain Ed25519 over the complete following bytes, not Ed25519ph or an unsigned reconstruction:

- Client proof: `D("eltor/scq/v1/client-request") || L(request_body)`.
- Service signature: `D("eltor/scq/v1/service-reply") || L(reply_body)`; `status` is inside that signed body, so a refusal cannot be recast as success.
- Logical request digest: `SHA256(D("eltor/scq/v1/logical-request") || L(request_body[0:244]))`.
- Delivery request digest: `SHA256(D("eltor/scq/v1/delivery-request") || L(request_body || client_signature))`.
- Stable core digest when referenced by a future acceptance: `SHA256(D("eltor/scq/v1/quote-core") || L(reply_body[0:456]))`. This does not itself define or authorize that later acceptance protocol.

Validate exact original bytes and lengths; reject alternate serialization, integer overflow, unknown fields and unallocated trailing content. Logical idempotency is scoped by service + client authorization + logical inquiry and checked against the stable digest across key generations. A changed request proof or delivery nonce must not create a new purchase under the same inquiry. Retain durable payment/replay state separately from ciphertext caches and reply slots.

## 5. Protected envelope and fixed capacities

The HPKE plaintext is `u16be(record_length) || signed_record || zero_padding`. The request record length MUST be 380; reply/refusal record length MUST be 633. Padding fills the fixed plaintext capacity and MUST be zero after successful AEAD verification. Padding and true length are inside authenticated encryption; do not parse or allocate from unauthenticated decrypted bytes. Reject inconsistent lengths, extra records, nonzero inner padding and truncated ciphertexts.

Use the fixed suite from spec two. HPKE `info` is exactly `D("eltor/scq/v1/hpke-request")` for requests and `D("eltor/scq/v1/hpke-reply")` for replies. AAD is exactly `D("eltor/scq/v1/envelope") || u8(kind)`, with kind 1/2 matching direction and the inner signed record. AAD carries no identity, key generation, price or logical inquiry; those are bound inside. Each delivery creates a fresh standard single-shot base-mode HPKE context, with no separate invented nonce field. Wire ciphertext is `enc[32] || ciphertext_with_tag`; receiver selection is bounded as specified in spec two.

| Component | Request | Reply or service refusal |
| --- | --- | --- |
| Canonical body | 316 | 569 = 456 core + 113 wrapper |
| Ed25519 signature | 64 | 64 |
| Encrypted true-length prefix | 2 | 2 |
| Authenticated zero padding | 4 | 185 |
| Fixed HPKE plaintext | **386** | **820** |
| Encapsulation + AEAD tag | 32 + 16 | 32 + 16 |
| Fixed encrypted envelope | **434** | **868** |
| Fragment data capacity | 434 | 434 |
| Fragments per transport leg | **1** | **2** |
| Per-fragment header | 64 | 64 |
| Relay Length per fragment | **498** | **498** |

The request is the maximum initial field inventory with one signature and no embedded invoice, offer, delegation certificate, service-side relay list, or free-form text. Reply references use the same fixed widths for both methods; additional artifacts need their own bounded authenticated mechanism. Never grow this record or add method-specific fragments. The 386-byte request plaintext is the largest plaintext fitting one 434-byte fragment after HPKE overhead; the 820-byte reply plaintext similarly fits two fragments. These are derived layout proposals, not measured free-bandwidth budgets or latency guarantees. A future schema exceeding them requires explicit profile/version work.

## 6. Fragment/routing header (64 bytes)

Every new quote-transport command uses ordinary RELAY cells, StreamID zero, with the same cell sizing and normal per-hop encryption/padding as the rest of this fork; it must not consume RELAY_EARLY. Relay command opcode allocation and capability negotiation remain open. All four legs (client→intro, intro→service, service→intro, intro→client) use the same 498-byte per-fragment logical Length:

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 1 | framing version, 1 |
| 1 | 1 | request 1 or reply 2 |
| 2 | 1 | zero-based fragment index |
| 3 | 1 | fragment count, request 1 or reply 2 |
| 4 | 32 | role-specific routing value |
| 36 | 16 | fresh leg-local delivery token |
| 52 | 12 | reserved, zero |
| 64 | 434 | envelope slice (outside the 64-byte header) |

Client→intro routing value is the existing 32-byte Ed25519 introduction authentication public key identifying the registered service (the key used by the relay-side introduction circuit map). Intro→service and service→intro routing value is the relay-allocated 16-byte reply-slot handle followed by 16 zero bytes. Intro→client routing value is all zero. The client generates a fresh unpredictable 16-byte token for its admitted attempt; the intro allocates a separate fresh token for the service-facing leg, rewriting routing/token fields when forwarding. Replies echo only the token for their own leg. Neither token is a payment ID, an end-to-end client identity, or shared across different introduction points. Do not use a raw/recycled circuit number as a token or forwarding destination.

Each relay-owned slot maps the actual circuit instances, service registration, both leg tokens, handle, expected kinds/counts, and admitted byte/deadline budgets. Forward only to the mapped destination and validate service replies against the actual registered service circuit and slot. The headers are hop-local routing metadata, not the end-to-end quote signature or a payment proof; rewriting them is permitted only by the corresponding relay and cannot override the signed delivery binding. Encrypt and authenticate all sensitive content independently; a malicious intro may still delay/drop/replay and knows its own routing association.

Admit provisional reassembly/slot work under strict aggregate caps from the first fragment. The absolute admission expiry is not reset by fragments; an incomplete message cannot refresh it. Require index 0 then index 1 for a reply on each ordered leg; request index is always 0. Reject wrong count/kind/token/route/reserved fields, conflicting duplicates, out-of-order/extra fragments, wrong circuit instances, and late fragments. Accept at most the exact envelope length, verify the complete encrypted record before acting, and release bounded transport state under the existing slot rules. A replay cannot reserve unbounded replacement slots. Retransmission uses the existing new-delivery/backoff policy rather than adding an ACK-driven fragment retry loop. Authenticated refusal still requires both reply fragments; malformed/unadmitted/overload failures are not owed cover traffic.

The 498-byte logical limit is a conservative profile budget, not a claim that this fork sends upstream-sized cells: local `tor/src/core/or/or.h` defines CELL_PAYLOAD_SIZE=2048 and an 11-byte relay header, leaving 2037 bytes of relay data. These messages still use Length=498 and the remaining normal fork padding; this draft does not resize cells or assume older clients interoperate. Normal queue, CPU, free-capacity and padding accounting apply to every forwarded copy. A complete successful request/reply consumes six quote fragments across all four legs (one + one + two + two), or 2988 bytes of logical relay payload; actual link traffic, three-hop forwarding in each direction, maintenance and outer padding add overhead.

## 7. Privacy, validation, and remaining gates

**Privacy/timing assessment:** both methods have identical request/reply envelope sizes and fragment counts, including refusals. True lengths, stable IDs, prices, method, client proof and signing/key-generation references remain encrypted. Outer intro registration and leg-local routing state are visible to the terminating introduction relay, which already knows its circuit association; no new service-role marker is added to guard/middle payment headers. Fixed sizes do not hide the existence of the quote exchange, inter-fragment timing, invoice acquisition, traffic correlation, stale-cache behavior or overload. The shared response scheduler must budget completion of both reply fragments before expiry, with the same rule for fresh/cached/refused results; retry/failover remains bounded and sequential. Separate invoice/offer acquisition can still expose method-related timing/size and wallet counterparty information. Do not claim anonymity or full-method indistinguishability from this layout.

Before implementation: independently check offsets and maximum sizes, RFC primitive vectors and El Tor known-answer vectors; compare C/Rust canonical bytes/signatures/HPKE domains/digests; fuzz parsing, lengths, flags, overflow, nonzero padding, wrong domains/keys, altered terms/proofs and unknown enums. Test refusal-as-free-quote confusion; late/duplicate/reordered fragment/slot handling; malicious routing/header rewriting; replay across key generations, new slots and restart; immutable cached cores with fresh wrappers; current/retiring key limits; no duplicate invoice/payment/paid build. Measure endpoint-visible Length/count/timing across both methods and refusal/cache paths, all four transport legs, aggregate free resources and honest success under load. Only documentation/offset/arithmetic checks accompany this draft; no live payment, packet trace, independent crypto audit or runtime implementation has run for it.

Remaining gates include canonical descriptor/delegation/policy/issuer/offer-reference records and freshness, relay opcode allocation/feature negotiation, invoice/acceptance/status/continuation wire, concrete lifecycle/timing/resource budgets, verified directory identity mapping, secure key storage/replay persistence, strict library verification behavior, independent crypto review, and runtime interoperability/privacy tests. Keep [ET-13/14/18/19/21/23/24/25/26](../../TODO.md) deferred. This draft does not start the separate paid-circuit replay-protection implementation.
