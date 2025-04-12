eltor
=====

`eltor` boots up the tor network fork. It also manages paid relays and communicates with your configured lightning node. 

Spec
----
- [(00) - El Tor Spec](./spec/00_spec.md)
- [(01) - Paid Circuit Protocol](./spec/01_paid_circuits.md)


eltor is very similar to how `tor` and `torrc` works. All of the same settings can be used in the `eltorrc` with these additional settings:

Config
------
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

# The DNS resolver that the exit node uses (useful to signal to clients if you use a specific DNS resolver, like family.dns.mullvad.net 194.242.2.6) *Optional
DNS_RESOLVER 1.1.1.1


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
cargo build -vv --features=vendored-openssl
cargo run --bin eltor -vv --features=vendored-openssl
```

.env
```
PHOENIXD_URL=http://localhost:9740
PHOENIXD_PASSWORD={{YOUR_PW}}
PHOENIXD_TEST_PAYMENT_HASH={{{{YOUR_TEST_PAYMENT_HASH}}}} 
PAYMENT_INTERVAL_ROUNDS=10 # Not being used, need to think more about this, hardcode to 10 now so we can pass in 10 payment id hashed during circuit build
```
