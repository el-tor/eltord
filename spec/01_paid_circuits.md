# Paid circuits

**Status: proposed version-one architecture.** A client selects a path using
relay prices and capabilities, pays any agreed handshake charge, and buys
capacity in one-minute rounds. Each relay receives independent payment fields
inside its own encrypted layer. Zero-fee construction retains fresh dummy fields
and equal-size padding; it does not waive a required positive payment.

## Circuit lifecycle

1. Read authenticated relay terms and select a path within explicit budgets.
2. Establish the ordinary El Tor per-hop construction and retain accepted terms.
3. Verify required payment against the receiving Lightning node and reserve its
   use for the authorized circuit and round. Registering an ID is not settlement.
4. Fund upcoming rounds within their agreed deadlines. Missing required payment
   closes the circuit; it cannot fall back to free capacity.
5. Prepare renewal at round seven of each allocation if every hop supports it.
   Acknowledgment registers the next ten identifiers; payments remain incremental.
6. Stop at accepted lifetime/resource limits or failure. Preserve uncertain
   payment records for reconciliation. A replacement circuit does not migrate
   existing application streams or silently inherit funds.

## Technical chapters

- [Uniform construction and bounded free allowance](circuits/free-allowance.md).
- [Renewal semantics](circuits/renewal.md).
- [Renewal wire protocol](01a_circuit_renewal_wire.md).
- [Initial construction and payment accounting](circuits/initial-construction.md).
- [Control commands and events](control/index.md).

The initial allocation has ten identifiers per hop, not a mandatory ten-round
purchase. Free-only renewal remains open; the baseline free lifetime policy
remains bounded. Initial price/start commitment and precise paid-construction
encoding are implementation gates; this overview does not claim they are solved.

## Previous section locations

<span id="shared-circuit-construction-and-bounded-free-allowance"></span> [Shared circuit construction and bounded free allowance](circuits/free-allowance.md#shared-circuit-construction-and-bounded-free-allowance)

<span id="uniform-per-hop-payment-payload"></span> [Uniform per-hop payment payload](circuits/free-allowance.md#uniform-per-hop-payment-payload)

<span id="small-free-capacity-on-ordinary-circuits"></span> [Small free capacity on ordinary circuits](circuits/free-allowance.md#small-free-capacity-on-ordinary-circuits)

<span id="use-cases-and-rationale"></span> [Use cases and rationale](circuits/free-allowance.md#use-cases-and-rationale)

<span id="shared-lifetime-and-specification-relationship"></span> [Shared lifetime and specification relationship](circuits/free-allowance.md#shared-lifetime-and-specification-relationship)

<span id="circuit-renewal-draft"></span> [Circuit renewal draft](circuits/renewal.md#circuit-renewal-draft)

<span id="purpose-and-scope"></span> [Purpose and scope](circuits/renewal.md#purpose-and-scope)

<span id="proposed-renewal-sequence"></span> [Proposed renewal sequence](circuits/renewal.md#proposed-renewal-sequence)

<span id="payload-size-and-private-continuity"></span> [Payload size and private continuity](circuits/renewal.md#payload-size-and-private-continuity)

<span id="price-consent-and-resource-protection"></span> [Price, consent, and resource protection](circuits/renewal.md#price-consent-and-resource-protection)

<span id="open-decisions-and-validation"></span> [Open decisions and validation](circuits/renewal.md#open-decisions-and-validation)

<span id="client-flow"></span> [Original client flow](history/paid-circuit-flow.md#client-flow) is historical.

<span id="relay-flow"></span> [Original relay flow](history/paid-circuit-flow.md#relay-flow) is historical.
