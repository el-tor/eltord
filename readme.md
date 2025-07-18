eltor
=====

`eltor` boots up the tor network fork. It also manages paid relays and communicates with your configured lightning node. 

Spec
----
- [(00) - El Tor Spec](./spec/00_spec.md)
- [(01) - Paid Circuit Protocol](./spec/01_paid_circuits.md)


eltor is very similar to how `tor` and `torrc` works. All of the same settings can be used in the `torrc` with these additional settings:

Config
------
`torrc`
```sh
### Client Settings ###

## Lightning node settings

PaymentLightningNodeConfig type=phoenixd url=http://url.com password=pass1234 default=true
PaymentLightningNodeConfig type=lnd url=http://lnd.com macaroon=mac1234

# Max amount in msats you are willing to pay for tor circuit
PaymentCircuitMaxFee 11000


### Relay Settings ###

# Static lightning offer code
PaymentBolt12Offer lno***

# BIP-353 name that uses DNS to map to a BOLT 12 offer
PaymentBolt12Bip353 name@domain.com

# Rate the relays charges in msats per payment interval (default=1000)
PaymentRateMsats 1000

# Seconds per each payment interval (default=60)
PaymentInterval 60

# How many rounds of payments before the circuit is killed (default=10). max is 10 due to limits on data we can pass in a tor onion cell.
PaymentInvervalRounds 10

# The DNS resolver that the exit node uses (useful to signal to clients if you use a specific DNS resolver, like family.dns.mullvad.net 194.242.2.6) *Optional
DnsResolver 1.1.1.1


# We recommend to set this 0 to allow the client to test the bandwidth. 
# Setting this might make your relay less desirable as a noobie relay, but can be useful if you are being spammed or are a mature relay
HandshakeFee 0 

# A quota set in KBytes on how much bandwidth a client can use per payment interval. *future work, not being implemented yet (default=0) unlimited
BandwidthQuota 0

# Eventually support BOLT 11 because some implementations support blinded paths!
PaymentBolt11Lnurl lnurl*** 
PaymentBolt11LightningAddress name@domain.com
```

Run
---
```sh
### Run just the relay with no settings
cargo build
cargo run

### Other ways to run
# 1. Run the Relay or Client
cargo run relay
cargo run client

# 2. Relay or Client with args*
# *(pw is ControlPort clear password (not hashed))
cargo run client -f torrc.client.dev -pw password1234_
cargo run relay -f torrc.relay.dev -pw password1234_

# 3. Relay or Client with env vars*
# *(nice to use with debugger if you set ARGS in .env file)
ARGS="eltrod relay -f torrc.relay.dev -pw password1234_" cargo run
ARGS="eltrod client -f torrc.client.dev -pw password1234_" cargo run
```

.env
```sh
PHOENIXD_URL=http://localhost:9740
PHOENIXD_PASSWORD={{YOUR_PW}}
PHOENIXD_TEST_PAYMENT_HASH={{{{YOUR_TEST_PAYMENT_HASH}}}} 
PAYMENT_INTERVAL_ROUNDS=10 # Not being used, need to think more about this, hardcode to 10 now so we can pass in 10 payment id hashed during circuit build
```
dev
```sh
ARGS="eltord client -f torrc.client.dev -pw password1234_"
```

torrc
======
`torrc.relay.dev`
```sh
# see the torrc sample file this project
```

`torrc.client.dev`
```sh
# same as the torrc sample file but minus theses settings
- OrPort
- PaymentBolt12Offer
- ExitRelay
```


TODO
====
1. simply read the payments-sent.json in as the same dir as the TOR DataDirectory
2. simplify the default command:
```
# from
./eltor client -f torrc -pw password1234_
# to 
./eltor
```
3. Should the same binary run both the client and relay? like always run client, but also run relay if bolt12 is configured?
4. rotate the payments-sent.json file after a few thousand rows