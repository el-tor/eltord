(00) El Tor Spec
================


## The Onion Pay Stream (TOPS)
`"Secure payments for onion-routed bandwidth"`


### Problem Case: 
Incentivizing Private and Trustless Bandwidth Payments in Tor

<b>Background</b>

Tor relays (especially exit relays) play a critical role in providing bandwidth for private communication. However, running a relay incurs operational costs (e.g., bandwidth, hardware, and energy). While donations and altruistic contributions sustain much of the Tor network, they are insufficient to incentivize widespread adoption of high-performance relays, particularly in regions with limited resources.

<b>Trustless Bandwidth Payment Challenges</b>

1. Trust Issues:
    - For Clients: Clients risk paying upfront for a service that a relay might not fully provide (e.g., poor bandwidth or early termination of the circuit).
    - For Relays: Relays risk routing traffic for clients who refuse to pay after receiving the service.
2. Anonymity and Privacy:
    - The protocol must ensure that neither the client's nor the relay's identity is exposed during the payment process.
    - Payments should not compromise the unlinkability of circuits, a fundamental property of Tor.
3. Asymmetric Accountability:
    - Relays cannot really blacklist misbehaving clients, as client IP addresses are hidden.
    - Clients, on the other hand, can blacklist poorly performing relays more directly by identifying their onion service or IP address.
4. Incremental Payments:
    - Large upfront payments discourage usage due to high trust requirements.
    - A pay-per-use model is more suitable for dynamic bandwidth usage but is difficult to enforce trustlessly.
5. Scalability:
    - The solution must scale across a decentralized network without adding excessive computational or bandwidth overhead.

### Proposed Solution: The Onion Pay Stream (tops)

"The Onion Pay Stream" (tops) addresses these challenges by introducing a trust-minimized, privacy-preserving protocol that integrates incremental payment streams into Tor circuits. 

<b>Use Case Example:</b>

A client wants to establish a Tor circuit for 10 minutes of browsing and is willing to pay 10 sats for this service.
Current Risks:
- Client Risk: If the client pays upfront, the relay could provide subpar bandwidth or terminate the connection early.
- Relay Risk: If the client pays only at the end, they may disconnect without paying.

Privacy Constraints:

- Payments must be conducted in a way that preserves Tor’s anonymity guarantees (blinded paths).
- Relays and clients cannot directly associate payments with real-world identities.

Key Requirements

1. Trustless Payment Mechanism:
    - Both parties should have limited risk exposure.
    - Payment should be incremental, matching the service provided.
2. Privacy-Preserving:
    - The payment process must not compromise the anonymity of either the client or the relay.
3. Dispute Minimization:
    - The protocol should inherently reduce the possibility of disputes by structuring payments in small increments tied to service performance.
4. Efficiency:
    - The payment process must not introduce significant latency or overhead into Tor’s operation.




## Detailed Breakdown of the "The Onion Pay Stream" Solution

"The Onion Pay Stream" or "TOPS" introduces a trust-minimized, incremental payment protocol for bandwidth usage in the Tor network. This solution ensures both clients and relays are incentivized to act honestly, while maintaining the privacy guarantees that Tor provides.

### (1) Key Features of Onion Pay Stream
1. Incremental Payments:
    - Payments are divided into small rounds (e.g., 1 sat per minute) to reduce risk for both parties.
    - The first round has an optional `handshake fee` but can be free to allow clients to evaluate performance without upfront risk. Noobie relays might want to make it free, while mature relays may want to charge a fee after they establish a reputation for providing high bandwidth. 
2. Privacy-Preserving:
    - Uses BOLT12 offers (blinded paths) to ensure that payments are unlinkable and do not compromise the anonymity of either the client or the relay.
3. Trust-Minimized:
    - Payments are made after each service round, ensuring clients pay only for the bandwidth they consume.
    - Relays are incentivized to perform well to receive subsequent payments.
4. Scalable and Efficient:
    - Requires minimal changes to Tor’s circuit-building process.
    - Payment operations are lightweight, leveraging the existing Lightning Network infrastructure.

### (2) How It Works
Phase 1: Circuit Establishment
1. Relay Shares Payment Terms:
    - During the Tor circuit negotiation, the relay advertises its payment requirements (e.g., 1 sat per minute for 10 minutes, first minute free) using a static BOLT12 offer or a similar descriptor.
2. Client Sends Payment Intent:
    - The client acknowledges the relay's terms by sending a unique payment id hash (generated locally) agreeing to use the relay’s static offer.
3. First Round Starts Free:
    - The relay begins providing service immediately for the first round without requiring payment (if no handshake fee is required).

Phase 2: Incremental Payments
1. At the End of Each Round:
    - The relay requests payment for the previous round *(out of band via eltord).
    - The client generates a Lightning payment for the agreed amount (e.g., 1 sat) and sends it out of band using the Lightning Network.
2. Payment Verification:
    - The relay queries its own Lightning node to verify the payment.
    - If the payment is valid, the relay continues providing service.
3. Final Round Payment:
    - After the last round payment then kill the circuit.
Phase 3: Circuit Termination
1. If Payment Fails:
    - If the client fails to pay, the relay terminates the circuit.
2. If Service Fails:
    - If the relay fails to provide service, the client can disconnect and stop payments.

### (3) Key Protocol Components

a. Preimages and Payment Hashes

- The client pre-generates a unique payment id hash.
- This hash is sent to the relay during circuit establishment. This is the key the relay uses to verify the client has paid.

b. Payment Rounds

- The relay generates invoices for each payment hash (automatically using BOLT12 offers) and uses the Lightning Network to receive payments.

c. Privacy Guarantees

- BOLT12 ensures payments are unlinkable to specific circuits or clients.
- The preimages and hashes and hashed payment id do not reveal client identities.

### (4) Implementation Details
Client-Side Changes

1. Generate Payment ID Hash:
- Before building the circuit.
- Example:
```
const preimage = crypto.randomBytes(32);
const paymentIdHash = crypto.createHash('sha256').update(preimage).digest('hex');
```
2. Send Payment ID Hash:
    - Add the payment id hash to the CREATE2 or EXTEND2 cells during the circuit setup.
3. Make Incremental Payments:
    - Use the Lightning Network to pay invoices incrementally.

Relay-Side Changes

1. Advertise Payment Terms:
    - Include payment terms in the EXTENDED2 cell response during circuit setup.
    - Example Terms:
        First minute: Free.
        Subsequent rounds: 1 sat per minute.
2. Verify Payments:
    - Use the paymentHashId to match that the payment for each round are received via the Lightning Network in the message field of the BOLT12 offer.

### (5) Example Payment Schedule
```
Round	Cost (sats)	    Action
1	    Free	     Test bandwidth.
2	     1	         First payment.
3	     1	         Second payment.
...	   ..........   ..............
10	     1	         Final payment.
```

### (6) Benefits of Onion Pay Stream
For Clients

1. No Upfront Risk:
    - The free first round ensures clients don’t pay for subpar service.
2. Incremental Payments:
    - Clients only pay for the bandwidth they consume.
3. Privacy Protection:
    - Payments are unlinkable and preserve anonymity.

For Relays

1. Incentivized Bandwidth:
    - Relays earn incremental payments for good service.
2. Minimal Risk:
    - Relays can terminate service if payments are not received.


### (7) Model 

Here is a model of TOPS rounds. Any given round can be terminated if either party cheats.
```
1    ->  Free (or handshake fee)
2    ->  1 sat
3    ->  1 sat
4    ->  1 sat
5    ->  1 sat
6    ->  1 sat
7    ->  1 sat
8    ->  1 sat
9    ->  1 sat
10   ->  1 sats
```