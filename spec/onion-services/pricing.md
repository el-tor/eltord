> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# Service Circuit Quote (SCQ)

### Definition and calculation

A **Service Circuit Quote (SCQ)** is the onion service's authenticated quote to the client for funding its **service-to-rendezvous circuit**. The client separately selects and pays the relays on its own circuit to the same rendezvous point. An initial SCQ covers one service-side circuit for at most ten one-minute rounds, paid incrementally rather than all ten rounds upfront. Continuing that same circuit requires an authenticated, explicitly authorized SCQ extension under the renewal rules below. It is not a reusable payment session or a balance across circuits.

Relay operators set their own prices in their relay descriptors, as in spec one. The service reads those descriptors, selects a compatible candidate path, and totals its cost. It does not override relay rates or assume they are equal. Payment intervals and capabilities must be compatible with the quoted schedule. Candidate selection and quoting do not authorize paid construction or relay payments.

The fixed per-minute amount includes the combined selected relay rates and an explicitly disclosed service-side Lightning routing-fee budget. One-time relay handshake/setup charges are itemized separately and MUST NOT recur every round. Specify fee limits, integer units such as millisatoshis, any rounding, and how unused reserved fee amounts are treated. A separate application/access charge, if any, must be identified and accepted separately from the transport total rather than hidden in relay costs.

The client can display its own selected-path costs alongside the SCQ to show the complete connection budget. Its own wallet routing fees remain subject to its own fee limits; do not silently count them again as service-side charges. The service need not reveal its relay identities or full path. An SCQ authenticates the service's price commitment, not an independent proof of its private relay costs, path selection, or delivered bandwidth.

### Descriptor advertisement and required terms

The onion-service descriptor advertises accepted payment methods, SCQ capability, and how to obtain an authenticated quote. Generate an SCQ for the intended connection from current candidate relay prices. It need not be uploaded to HSDirs or trigger descriptor republication for every connection or round. Discovery metadata and cached descriptors are not substitutes for a current accepted quote.

The exact wire format remains open. Authenticated terms MUST include:

| Field or term | Purpose |
| --- | --- |
| Protocol version and unique quote identifier | Identify this agreement and avoid confusing quotes or versions. |
| Version-agreement binding | Confirm the selected El Tor service version, fresh client nonce, authenticated descriptor capability-block digest, and client-key/payment-method selection before quote acceptance or payment. |
| Service identity and client authorization binding | Authenticate the intended service and bind acceptance to a fresh client authorization key without requiring its public wallet identity. |
| Opaque circuit authorization identifier and rendezvous relay | Bind the quote to the intended service-side circuit/destination without disclosing its private path. |
| Fixed per-round amount, unit, and breakdown | State the selected relay-rate total and disclosed service-side routing-fee budget for each 60-second round. |
| One-time charges and wallet fee limits | Itemize setup/handshake costs and bound all authorized extra spending. |
| Authorized period, maximum rounds, total cost, and outstanding commitments | Initially at most ten rounds/ten minutes; any continuation needs an accepted extension. Include setup/padding costs and bound cumulative spending and how much may be reserved, prepaid, or in flight at once. |
| Acceptance expiry and round/build schedule | Distinguish the deadline to accept from the lifetime of the accepted fixed price; specify lead time, grace, build timeout, and start conditions. |
| Payment method and invoice association | Bind each payable invoice/payment identifier to this accepted SCQ, circuit, and specific round. |
| Failure, retry, and unused-funding rules | Disclose permitted incurred costs and reconciliation/refund handling without automatic cross-circuit credit or unlimited rebuilds. |

Authenticate the SCQ as belonging to the onion service through its descriptor-authenticated identity or delegated signing key. The exact signing/delegation scheme and domain-separated serialization remain to be defined. Requests, replies, and acceptance need a bounded authenticated bootstrap exchange available before the paid service-side circuit exists. A quote receipt, a client acceptance, an issued invoice, and settlement are separate states.

### Fixed price and acceptance sequence

Once accepted within its validity window, the per-minute price is fixed for that circuit's agreed lifetime. Paying incrementally does not permit repricing each minute. Changed relay descriptors or service metadata cannot alter an accepted SCQ. The renewal draft preserves the accepted per-minute price for the continuing circuit; the version-one continuation profile forbids repricing at renewal boundaries. The client may reject the quote, or stop renewing an accepted circuit under the existing performance/payment rules; acceptance does not commit the client to purchasing all ten rounds.

The proposed sequence is: request SCQ, select candidate relays and calculate cost, authenticate and deliver the quote, client accepts or declines, obtain and settle initial funding, then construct/pay the service-side circuit. The client may prepare its own rendezvous circuit independently. The service MUST NOT incur paid work until the agreed initial funding is settled and reserved. Before requesting payment, decline if no workable candidate path fits the quoted price.

Each underlying relay MUST honor the rate accepted when its circuit is established for that circuit's lifetime. This commitment requires relay negotiation and enforcement; publishing a descriptor alone does not reserve a price or capacity. If a cached rate is stale or a relay refuses the terms, use an equivalent path within the accepted budget/terms or follow the disclosed failure/requote policy. Never silently raise an accepted price or spend beyond the circuit's settled funding. A quote is not a guarantee that capacity has been reserved or that a build will succeed.

Record the accepted SCQ in the circuit/round ledger and bind every distinct subsequent invoice/payment to it. Duplicate quote requests or acceptance messages MUST NOT create extra circuits, duplicate reservations, or duplicate charges. Quote changes require fresh identifiers and explicit acceptance; old and replacement authorizations must not claim the same funds concurrently. Circuit replacement starts a new SCQ/payment sequence. Continuing the existing circuit past round ten requires an accepted SCQ extension and relay renewal; neither quote acceptance nor elapsed time automatically buys an eleventh round.

### Incremental payments, stopping, and accepted loss exposure

From the client's perspective, there are two separately funded circuit halves. The client pays its own selected relays to the rendezvous point, and separately pays the service's SCQ through Lightning. The service spends that SCQ funding on its own selected relays. These payments are incremental purchases of circuit rounds, not a single upfront purchase of the entire ten-minute connection. Initial service-side funding covers the first round and explicitly agreed setup fees.

The client may stop buying further rounds at any time, including because performance is inadequate or the connection is no longer needed. Accepting an SCQ fixes the price of any rounds purchased within its lifetime; it does not obligate the client to buy all ten. If the client purchases rounds one through three and does not fund round four, required-payment enforcement closes the affected circuit at the applicable deadline/grace. If round four was already prepaid, choosing to stop does not undo that settled payment or automatically entitle the client to its refund.

A relay or service may fail, disconnect, or cease delivering traffic at any time, even after payment. The protocol cannot force an uncooperative peer to remain online or deliver bandwidth, and a receipt/accepted SCQ is not an uptime or delivery guarantee. Nonpayment closure follows the agreed deadline/grace; unrelated failure or disconnection can happen earlier. The client/service can stop further renewal and reconcile outstanding work when either circuit half fails. This availability risk does not waive the price, authorization, spending-limit, or agreed failure-accounting rules.

**Accepted tradeoff:** incremental payment bounds prepayment exposure but does not make it zero or guarantee a refund. The client may lose amounts already paid for service that is not subsequently delivered. With the candidate 60-second lead time, the current round can still have unused time while the next round is prepaid or in flight. Exposure can therefore include the unused portion of the current round plus the prepaid next round, agreed setup charges, and routing fees. It is not strictly limited to one minute's fees.

The client-side relay payments and SCQ payments each contribute to the client's total budget and exposure. Service-to-relay payments funded by the SCQ must not be counted again as a second client charge. Enforce the disclosed outstanding-funding cap across both current/next-round commitments and unreconciled attempts; retries, overlapping builds, or restarts MUST NOT multiply that cap. Do not release uncertain in-flight funds merely because the client closed the connection. The existing reconciliation/refund policy governs confirmed remaining funds and actual agreed costs; it does not promise recovery from an uncooperative recipient.

### Security and exposure limits

Protect quote, recipient, and invoice binding together: a valid service quote must not be usable with a substituted invoice, amount, destination, circuit, or round. Invoice/payment-method verification must authenticate the intended receiver without imposing a universally public wallet identity. Reject expired or incompatible quotes before payment, and reconcile payments that nevertheless settle late under the existing failure rules. Exact signing and binding formats need validation before implementation.

Rate-limit quote requests, candidate selection, invoice issuance, and outstanding quote state using bounded per-request and aggregate budgets. An unpaid requester must not trigger paid circuits or unbounded memory/CPU work. Many fresh client keys must not evade the service's global limits. Client-supplied rendezvous details still require normal Tor eligibility/path checks and authenticated binding to the accepted quote.

A client receiving a quote for a private path cannot independently verify its component relay prices without additional disclosure or a separate reviewed proof mechanism. Treat the total as a service-authenticated price offer which the client may reject. Do not claim that the quote proves honest pricing, relay performance, successful delivery, or automatic fair exchange.

Keep uncertain in-flight payments reserved until reconciled, including after circuit closure or a request to re-quote. Refund/unused-funding claims require client authorization bound to the original quote, confirmed remaining funds, and bounded refund fees; a retry or refund request must not redirect someone else's funds. The [refund profile](refunds.md) specifies the bounded authenticated mechanism, and the SCQ does not promise an unconditional refund of already incurred agreed costs.

### Shared construction and validation

Carry SCQ negotiation and compact references in the authenticated client/service exchange. Preserve spec one's unchanged per-hop construction, unique per-hop round identifiers, dummy zero-fee handshake fields, encryption, and equal-size padding. Do not add an onion-service purpose marker to intermediate relays' payment payloads. A full SCQ need not fit in one introduction cell; define bounded delivery and appropriately sized authenticated references before choosing an encoding.

Validate unequal relay prices, fee caps/rounding, price changes, stale descriptors, quote rejection/expiry, substituted invoices, destination/client binding, duplicate acceptances, replay, bounded quote floods, and loss of connectivity during the exchange. Check that existing circuits retain accepted rates, failures cannot overspend settled circuit funds, all simultaneous commitments stay within the cap, and clients can approve a complete budget without learning the service's path. Include stopping after round three, stopping after the next round was already prepaid, peer disconnection immediately after payment, and reconciling both circuit halves without double-counting the SCQ's onward relay payments.

SCQ encoding/authentication, bootstrap delivery, relay rate commitment, exact build/start events, and unused-fee/refund mechanics remain implementation-level design work. This section records the agreed architecture and its open security checks, not a completed implementation or executed test suite.
