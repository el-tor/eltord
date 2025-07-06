(01) El Tor - Paid Circuit Protocol 
==============================


Client Flow
==========
A client wants to build a paid circuit.

Steps:
1. Relay Descriptor Lookup
2. Handshake Fee
3. Circuit build
4. Test Bandwidth
5. Init Payments Ledger
6. Client Bandwidth Watcher
7. Circuit Kill. Repeat

<b>1. Relay Descriptor Lookup</b>

A client lookups the Relays by:

- `PAYMENT_RATE` - rate the relay charges in msats per payment interval (default=1000)
- `PAYMENT_INTERVAL` - seconds per each payment interval (default=60)
- `PAYMENT_INTERVAL_MAX_ROUNDS` - how many rounds of payments before the circuit is killed (default=10). max is 10 due to limits on data we can pass in a tor onion cell
- `HANDSHAKE_FEE` - a fee in msats the relay might charge to do bandwidth testing in the pre-build handshake step (default=0)
- `BANDWIDTH_QUOTA` - a quota set in KBytes on how much bandwidth a client can use per payment interval. *future work, not being used yet (default=0) unlimited

Select Relays:

Select 3 (or more) relays (entry,middle,exit) and lookup the BOLT 12 offer (or lnurl*) to pay. 
This info can be found locally in the `cached-microdesc-consensus` file that is downloaded from the Directory Authorities.
Relay selection is a topic for more research. For now, it's a random algo, excluding relays that charge exuberant fees. 
A client can set a `CIRCUIT_MAX_FEE` in msats to stay under. Maybe something like 20 sats for a 10 min circuit. 
It makes reasonable sense for a user to desire to keep monthly expenses to about what they are willing to pay for a centralized VPN service (average $10/month).
Maybe they will pay for privacy and splurge to $20-$30 a month. Since El Tor is based on usage, this amount can vary based on how long you use a circuit. A
future iteration could even include have a `BANDWIDTH_QUOTA` per circuit. But since El Tor is a free market, prices will vary.

<b>2. Handshake Fee</b>

During the handshake, in the circuit build step, a client can test the bandwidth. 
A relay can charge a fee for this step and there are incentives on why, or why not, the relay may want to charge a fee. 

Fee incentives:

A relay charging a handshake fee might make sense in a few senerios:
- if you are a mature relay that a lot of clients already trust you might want to charge a fee. The client will be willing to pay this fee
for the relays high service level and honest bandwidth.
- to prevent "free loader" clients. A "free loader" client is one that builds a circuit (without a handshake fee) then uses it for free until payment is 
required for the first round beyond the inital bandwidth test. Then the free loader prematurley kills the circuit before paying the first interval round. 
In Tor, relays cannot see the client IP (unless you are a guard), so there is no simple way to prevent a "free loader" except charging a fee. 
Free loading might be discouraged if the relay sets a small enough interval round, lets say 15 seconds. 
This could prevent the free loader from getting any meaningful bandwidth usage before he is disconnected. But the interval must be long enough to allow a lightning payment to go thru.


No Fee incentives:

Setting a fee probably does not make sense:
- if you are a noobie relay because nobody trusts your advertisted bandwdith yet. The relay might take the fee and run away
- to allow clients to do free bandwidth testing 
- if you want more clients connecting to you, leading to higher profits

<b>3. Circuit build</b>

Now that you know the handshake fee the next step is to build the circuit:

- a. Config 
    - a1. A typical relay might charge 1000 msats `PAYMENT_RATE` per interval of 60 seconds `PAYMENT_INTERVAL` up to 10 minutes `PAYMENT_INTERVAL_MAX_ROUNDS`. 
    Typically the `HANDSHAKE_FEE` is 0.
    - a2. A `HANDSHAKE_FEE` might be required by the relay
- b. Lightning Payment Setup
    - b1. For BOLT 12 Offer: Create 10 random `PAYMENT_ID` hashes (32 byte) for each relay to include later (see step 6) 
    in the encypted onion message of a BOLT 12 payment. Make sure each `PAYMENT_ID` is unique to avoid giving up privacy in correlation attempts. 
    This PAYMENT_ID is used as a lookup by the relay
    to verify payment. 
    - b2. For LNURL: create 10 invoices and concatinate the 10 payment hashes as the `PAYMENT_ID` in the chronological order that you are going 
    to pay them for each interval.
- c. Handshake fee
    - c1. If NO handshake fee is required the `handshake_fee_payment_hash` and `handshake_fee_preimage` is a random hash. 
    This is a dummy hash to pad for privacy reasons to prevent against a malicious relay that might contol two hops to prevent correlation and timing attacks.
    - c2. If a handshake fee is required, then the client pays the relay out of band and inserts a valid payment proof in the `handshake_fee_payment_hash` and `handshake_fee_preimage`
- d. Next, call the <b>EXTENDPAIDCIRCUIT</b> RPC
    ```
    EXTENDPAIDCIRCUIT 0
    fingerprint_entry_guard handshake_fee_payment_hash handshake_fee_preimage 10_payment_ids_concatinated
    fingerprint_middle_relay handshake_fee_payment_hash handshake_fee_preimage 10_payment_ids_concatinated
    fingerprint_exit_relay handshake_fee_payment_hash handshake_fee_preimage 10_payment_ids_concatinated
    ```
The `EXTENDPAIDCIRCUIT` RPC command builds an onion layer for each of the relays. 

- Relay 1 (entry) gets the payment hashes and preimages wrapped up into the onion message for hop 1. 
- Relay 2 (middle) gets his payment hashes and preimage wrapped up in hop 2 of the onion message. 
- Repeat for N middle relays. 
- Finally the exit relay gets his payment hashes and preimage wrapped up in hop 3 (or N). 
- *This works a little bit different for Hidden Services because there is no exit relay as they build a circuit to the introductory and rendevous points.

This is important for privacy because the client is never directly communicating with the middle or exit relays to preserve privacy. 
The relays cannot correlate any data because the data is all unique hashes. Even if two relays in a circuit are the same, they 
would not be able to correlate data because all communication is done thru onion messages and all data is padded to be equal size. 
Each relay can only decrypt the data that belongs to them in their decrypted onion.  

<b>4. Test Bandwidth</b>

After the circuit it built, the client can test that the circuit's bandwidth is as advertised for the first 10 seconds or so 
(make sure to pay before 1 min (or interval due) , *remember that it might take some time to find a lightning route).

<b>5. Init Payments Ledger</b>

Add the newly built circuit data to the `payments-ledger` to track each round by `CIRC_ID` and `PAYMENT_ID`. 
Extract each of the 10 `PAYMENT_ID`'s from the RPC call and assisgn a round number to each in order. 
Record 0 for each `UPDATED_AT` to signal that the round has not been paid yet.
See diagram below:

`payments-ledger`
```
PAYMENT_ID    CIRC_ID          ROUND      RELAY_FINGERPRINT    UPDATED_AT  
------------- -----------  ------------   -----------------   ------------  
   111           456             1             ENTRY_N             0
   222           456             1             MIDDLE_N            0
   333           456             1             Exit_N              0
   444           456             2             ENTRY_N             0
   555           456             2             MIDDLE_N            0
   777           456             2             Exit_N              0
   ...           ...             .                .                .
   999           456             10                                0
```

This kicks off the "Client Bandwidth Watcher".

<b>6. Client Bandwidth Watcher </b>

The client watcher is responsible for testing bandwidth every interval and handles payment for the next round of bandwidth.

```
LOOP every interval (1 min default) up until MAX rounds (10 default), then kill circuit
    - if good bandwidth 
        - then pay each relay with their respective PAYMENT_ID for the ROUND 
    - else if bad bandwidth - kill circuit
LOOP
```

<b>6. Repeat</b>

After the circuit is expired, build a new one and repeat. 

Relay Flow
=========
A relay wants to start sharing his bandwidth. Configure payment preferences and rate.  

Steps:
1. Set Torrc Config
2. Handshake
3. Watch emitted Event `PAYMENT_ID_HASH_RECEIVED`
4. Start Relay Payment Watcher
5. Init Payments Ledger
6. Start Lightning payment watcher
7. Payment Ledger Cron (Auditor Loop)

<b>1. Config</b>

torrc
```
# Static lightning offer code
PAYMENT_BOLT12_OFFER lno***

# BIP-353 name that uses DNS to map to a BOLT 12 offer
PAYMENT_BOLT12_BIP353 name@domain.com

# Rate the relays charges in msats per payment interval (default=1000)
PAYMENT_RATE 1000

# Seconds per each payment interval (default=60)
PAYMENT_INTERVAL 60

# How many rounds of payments before the circuit is killed (default=10). max is 10 due to limits on data we can pass in a tor onion cell.
PAYMENT_INTERVAL_MAX_ROUNDS 10

# We recommend to set this 0 to allow the client to test the bandwidth. 
# Setting this might make your relay less desirable as a noobie relay, but can be useful if you are being spammed or are a mature relay
HANDSHAKE_FEE 0 

# A quota set in KBytes on how much bandwidth a client can use per payment interval. *future work, not being implemented yet (default=0) unlimited
BANDWIDTH_QUOTA 0
```

Eventually support BOLT 11 because some implementations support blinded paths!
```
PAYMENT_BOLT11_LNURL lnurl*** 
PAYMENT_BOLT11_LIGHTNING_ADDRESS name@domain.com
```

<b>2. Handshake</b>

A relay receives a handshake cell to extend/create a circuit from a client willing to pay for bandwidth.

Handshake Fee check: A relay receives an onion message to `EXTENDPAIDCIRCUIT`. Verify the `handshake_fee_payment_hash` and `handshake_fee_preimage`. 
If no fee is required you can ignore the padded data.

<b>3. Watch emitted Event PAYMENT_ID_HASH_RECEIVED</b>

The tor daemon will emit the event `EXTEND_PAID_CIRCUIT` for the relay watcher to verify against the lighting database.

<b>4. Relay Payment Watcher</b> 

The relay has an event watcher running that tracks payments and verifies against the remote lightning database. It also listens for events emitted
from the tor daemon `EXTEND_PAID_CIRCUIT`. The relay payment wacher can write to the `payments-ledger` 

On `EXTEND_PAID_CIRCUIT` event received:
- If the relay requires a handshake fee then check that the payment `handshake_fee_payment_hash` is valid and belongs to you by checking the lightning database. 
    - If not valid, then kill the circuit. 
    - If good, then add the newly built circuit to the `payments-ledger` to track each round (in step 5)

<b>5. Init Payments Ledger</b>

Add the newly built circuit data to the `payments-ledger` to track each round by `CIRC_ID` and `PAYMENT_ID`. 
Extract each of the 10 (or `PAYMENT_INTERVAL_MAX_ROUNDS`) `PAYMENT_ID` from event and assisgn a round number to each in order. 
Record 0 for each `UPDATED_AT` to note that the round has not been paid yet. Mark the `RELAY_FINGERPRINT` as `ME` since you are the one getting paid.
See diagram below:

`payments-leger`
```
PAYMENT_ID    CIRC_ID          ROUND      RELAY_FINGERPRINT    UPDATED_AT  
------------- -----------  ------------   -----------------   ------------   
   111           456             1               ME                0
   222           456             2               ME                0
   ...           ...             .               ME                0
   999           456             10              ME                0
```

<b>6. Lightning Payment Watcher</b>

After you add each `PAYMENT_ID` to the `payments-ledger` kick off a "lightning watcher".

Watch your lightning node for incoming payments that includes a 32 byte `PAYMENT_ID` hash inside a BOLT 12 message (payer_note).
Also watch BOLT 11 invoices, if used, for that same `PAYMENT_ID`, but this value is actually the payment_hash of the invoice.

Payment Proof:

When a lightning payment id (hash) matches then call the `Payment Ledger Cron` in step 7.


<b>7. Payment Ledger Cron (Auditor Loop)</b>

A loop is running every minute (or configured interval) to audit the payments ledger to make sure the pay streams are coming in for active circuits.
```
Loop each `CIRC_ID` and find the smallest round that has an `UPDATED_AT` field that is not 0 (if none then its the first ROUND, no payments received yet)
    ROUND=N
    check in the lightning database if the invoice with that `PAYMENT_ID` was paid:
    if yes paid
        - check if the client missed the payment window (NOW - UPDATED_AT > 1 min (or PAYMENT_INTERVAL) )
            - if yes, missed payment window, then kill the circuit and remove the rows from the ledger for the CIRC_ID
            - else 
                Update the UPDATED_AT field with the current unix timestamp for the ROUND. thus incrementing the ROUND
                - if round is greater than or equal to 10 (or MAX_ROUNDS) 
                    - if yes, then kill the circuit and remove the rows from the ledger for the CIRC_ID
                    - if no, return
    if no 
        - check if the client is outside the payment window (NOW - UPDATED_AT > 1 min (or PAYMENT_INTERVAL) )
            - if no, return
            - if yes, then kill the circuit and remove the rows from the ledger for the CIRC_ID
LOOP
```

BOLT 12 Tests between implementations
-------------------------------------
Can we send a message with 32 byte PAYMENT_ID in each of the implementations? (test using BOLT12 playground)
```
CLN
    - CREATE: lightning-cli --network regtest offer 0 clndesc
    - LIST: lightning-cli --network regtest listinvoices
    - PAY: lightning-cli fetchinvoice -k "offer"="lno***" "amount_msat"="2000" "payer_note"="<PAYMENT_ID>"

Phoenixd 
    - CREATE: Autogenerated or with elcair `./bin/eclair-cli eclair1 tipjarshowoffer`
    - LIST: Subscribe to webhook: `websocat --basic-auth :<phoenixd_api_password> ws://127.0.0.1:9740/websocket`
    - PAY: curl -X POST http://localhost:9740/payoffer \
            -u :<phoenixd_api_password> \
            -d amountSat=2 \
            -d offer=lno
            -d message='<PAYMENT_ID>'
LNDK
    - CREATE: ./bin/ldknode-cli lndk1 offer
    - LIST: ??
    - PAY: ??

Strike (only works with LNURL currently)
    TODO TEST LNURL + Blinded Paths
    TEST Paying to BOLT12

```

- CLN -> Phoenixd
    - [X] BOLT12 message works
    - [X] Message Field Name=payer_note
 
- CLN -> LNDK
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Phoenixd -> CLN
    - [X] BOLT12 message works
    - [X] Message Field Name=payerNote

- Phoenixd -> LNDK
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- LNDK -> CLN
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- LNDK -> Phoenixd
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Strike -> CLN
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Strike -> Phoenixd
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?

- Strike -> LNDK
    - [ ] BOLT12 message works
    - [ ] Message Field Name=?


### Compatiabilty issue notes

- CLN (current Umbrel version) cannot pay a lno without a description. Phoenixd does not include an offer description when they automatically create the BOLT 12 offer.
