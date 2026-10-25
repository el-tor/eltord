> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# Revision requirements

These four requirements retain the descriptor, invoice, introduction, and rendezvous architecture while defining its authorization and economic boundaries. They are proposed requirements; exact encodings and interoperability rules must be specified before implementation.

### 1. Bind payment authorization to the client

The service MUST verify payment against its own Lightning node's records. It must confirm the expected invoice or transaction, service association, offer where applicable, amount, terms, and settled status. If a preimage is presented, its SHA-256 hash MUST match that invoice's payment hash. A pending payment, an invoice issued by another recipient, or a matching hash/preimage pair without a settled local invoice MUST NOT authorize access.

The Lightning node is authoritative for settlement; the service's durable ledger is authoritative for access redemption within the agreed window. An invoice can remain settled after its access allowance has been used, so checking settled status alone does not prevent replay.

A hash identifies an invoice, and a preimage demonstrates payment knowledge; neither identifies the connecting client. Before payment, the service MUST bind the invoice to a fresh client authorization public key through an authenticated invoice/quote exchange. The client proves possession of the corresponding private key when requesting access. This is a separate authorization key unless a reviewed construction explicitly defines safe reuse of a Tor or Lightning key. The protocol must specify how the binding is carried and authenticated; an arbitrary payer note is insufficient.

The signed authorization MUST cover the protocol version/domain, service identity, invoice identifier, opaque circuit authorization identifier, round number, agreed terms, expiry, unique attempt identifier, rendezvous relay and cookie, and client handshake material. This prevents moving an authorization to a different service, circuit, or round. The service MUST reject expired authorizations and atomically prevent duplicate redemption. A pre-build authorization identifier must be bound to the resulting circuit once it is created; its exact encoding remains to be specified. Retry and refund rules below do not grant reusable access across circuits.

Use a fresh authorization identity for each new paid circuit rather than a reusable wallet/node identity. Keep payment authorization inside the service-encrypted introduction payload. The final compact encoding MUST fit Tor's introduction payload constraints; do not insert an entire invoice or an unbounded proof. Advertise support and version requirements so unsupported clients do not pay for an extension they cannot use.

### 2. Define who funds each circuit

Paying the service for access and paying transport relays are distinct charges. The client funds the variable costs of its connection one circuit round at a time. The service maintains its introduction circuits within the bounded free allowance below; exceeding that allowance MUST NOT silently charge the operator. Descriptor publication and retrieval use the proposed bounded free discovery allowance below. Optional paid capacity beyond that baseline requires explicit payer approval and a spending cap.

| Activity | Payer and budget |
| --- | --- |
| Service-to-introduction-point circuits | Free within advertised limits at every hop, including bounded setup and replacement work. |
| Descriptor publication, storage, and client retrieval | Proposed bounded free discovery service, covering path traffic and HSDir resource limits; any optional paid extras require explicit approval and caps. |
| Client introduction circuit | May use the same purpose-independent free allowance where supported and sufficient. |
| Client-to-rendezvous circuit | Client pays its chosen relays in one-minute rounds. |
| Service-to-rendezvous circuit | Service pays its privately selected relays using this client's funding for the corresponding circuit and round. |
| Application access, setup fees, and wallet/routing fees | Explicitly disclosed in the quote, separately from transport allowance. |

The descriptor lists introduction points, not a permanent rendezvous point. The client selects the rendezvous relay for an attempt. The service retains private selection and payment of its own path; the client MUST NOT need a list of service-side relays to fund access. Onion-service traffic has no exit to the public Internet. Relay charging details remain governed by [the paid circuit proposal](../01_paid_circuits.md), with endpoint-specific rules still to be defined.

Paid circuits retain spec one's one-minute payment rounds and ten-round allocations, with explicitly bounded timing padding for Lightning routing and settlement. Extending a paid circuit past the initial allocation requires the renewal procedure below. The client pays its own side and funds the service's side one round ahead. Each funding payment MUST be bound to one circuit and round and MUST NOT authorize another circuit or round. Before payment, the authenticated Service Circuit Quote (SCQ) MUST specify the fixed round price, setup or handshake charges, acceptance expiry, padding/deadline rules, fee limits, and failure/refund policy. The service MUST reserve settled round funding before relay payments, including any costs of the agreed padding. A pending or held Lightning payment is not settled funding. This is circuit-specific payment accounting, not a reusable payment session or shared balance across circuits. Without successful renewal, expiration requires a replacement circuit with a new authorization and payment sequence. An admitted continuation stays bound to the existing circuit and does not transfer its funding elsewhere.

### 3. Specify failures, retries, and round accounting

Payment settlement and Tor rendezvous completion are separate events. A successful Lightning payment MUST NOT be represented as a successful connection. Maintain a durable circuit/round ledger recording invoice settlement, bound client key, circuit authorization identifier, round number, terms, expiry, reservations, actual charges, and consumed payment identifiers. Each attempt has a unique identifier, bounded reservation, deadline, and lifecycle: reserved, building, active, then closed or failed.

Concurrent introductions, duplicate wallet notifications, retries, and restarts MUST NOT create duplicate funding, reservations, circuits, or payments. Persist reservations and outgoing-payment identifiers before starting paid work. Duplicate requests must refer to the existing attempt rather than launch another one. After a timeout or restart, reconcile uncertain outgoing payments and circuit state before releasing a reservation or retrying a payment.

If rendezvous fails, record setup or relay costs already incurred under the agreed terms and reconcile the unused part of that attempt's reserved funding. An exact retransmission is idempotent and MUST NOT create another paid circuit or charge. A fresh attempt for an unestablished circuit requires an authenticated retry decision, fresh attempt details, and atomic rebinding of only demonstrably unspent funding; the old authorization must become unusable. Replacement of an established or expired circuit starts a new payment sequence. Do not automatically carry unused rounds to another circuit or grant unlimited free rebuilds. A failed attempt may have consumed real relay fees.

The readiness wire and refund chapters define authenticated status/retry, successful connection establishment and unused-funding policy. Refunding unused funding is a separate Lightning operation requiring authenticated destination handling and its own fee policy; it does not establish a reusable payment session. Do not interpret introduction delivery acknowledgment as payment acceptance, completed rendezvous, or delivered application data.

### 4. Cap spending and keep wallet work off the handshake path

An unpaid, malformed, expired, or replayed request MUST NOT cause the service to initiate a paid rendezvous circuit, relay payment, or unlimited automatic top-up. Before any outgoing payment, enforce both the circuit round's remaining reserved budget and the operator's wallet-wide limits, including routing fees, outstanding payment commitments, and concurrent attempts. Renewal or rebuild costs must also fit these limits. Missing round funding or exhausted limits stop further paid work. When a required payment misses its bounded deadline/grace period, close the paid circuit; it MUST NOT silently fall back to the free allowance.

Standing infrastructure needs separate protection: the bounded free introduction allowance below MUST NOT trigger operator wallet spending when it is exhausted. Any separately paid descriptor publication or hosting needs explicit spending caps; unsolicited introductions or descriptor retrievals MUST NOT create unbounded charges. Apply introduction rate limits, bounded invoice issuance and queues, and bounded retention of unsettled requests. Standard Tor introduction DoS controls can help limit forwarding; they do not themselves enforce a financial budget. Hitting a cap may reduce availability but MUST NOT silently increase spending.

A background wallet component synchronizes settled invoices and reconciles outgoing payments into the durable ledger. The Tor handshake reads local authorization state and reserves circuit/round funding without waiting on wallet network RPCs. If settlement is not yet synchronized, defer or reject the attempt without spending; allow a later authenticated retry. If wallet state is stale or unavailable, do not authorize new spending until it can be reconciled. Payment, replay, and reservation state required to prevent overspending must survive process restarts.

These measures bound financial exposure; they do not eliminate CPU, bandwidth, liquidity, or availability attacks. Tor encryption and Lightning blinded paths also do not eliminate payment timing/amount correlation. The final protocol needs a privacy review as well as spending-limit validation.
