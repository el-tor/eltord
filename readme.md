eltor
=====

`eltor` boots up the tor network fork. It also manages paid relays and communicates with your configured lightning node. eltor is very 
similar to how `tor` and `torrc` works. All of the same settings can be used in the `eltorrc1 with these additional settings:

`eltorrc`
```
### Client Settings ###

## Lightning node settings

# LNDK
LND_MACAROON xxxxx
LND_REST_URL https://xxx

# Core Lightning
CORE_LIGHTNING_RUNE xxxx
CORE_LIGHTNING_REST_URL https://xxxx

# Phoenixd
PHOENIXD_API_KEY xxx
PHOENIXD_REST_URL https://xxxx

# Strike (send only bolt12)
STRIKE_API_KEY xxx

# Max amount in msats you are willing to pay for tor circuit
CIRCUIT_MAX_FEE 20000


### Relay Settings ###

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

# Eventually support BOLT 11 because some implementations support blinded paths!
PAYMENT_BOLT11_LNURL lnurl*** 
PAYMENT_BOLT11_LIGHTNING_ADDRESS name@domain.com
```

Run
---
```
cargo run --bin eltor -vv --features=vendored-openssl -f eltorrc
```
