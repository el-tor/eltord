> **Proposed version-one protocol.** Implementation and independent review remain deferred. See the [implementation gates](../validation/gates.md).

# Payment methods and receiver privacy

### Receiver choice and negotiation

Receiver-hiding payment paths are RECOMMENDED, not mandatory. BOLT 12 offers with receiver-hiding blinded paths are the preferred option when both wallets support them. BOLT 11 invoices, including blinded-path invoices where supported by the relevant implementations, are also permitted. An operator may explicitly choose a method that exposes its Lightning node identity or other receiver information.

The receiver advertises its accepted methods and their relevant capabilities in authenticated service metadata. The client selects a mutually supported method according to its wallet capabilities and configured privacy preferences before paying. A payment method MUST NOT be inferred to provide receiver privacy merely from its BOLT number; the actual fields, path configuration, and wallet behavior determine what is exposed. A public relay and a location-hidden onion-service operator may make different choices.

This is an extensible method policy, not a claim that every wallet supports every method. Additional methods require an explicit integration, an authenticated payment-request mechanism, and reliable recipient-side settlement verification. Unsupported methods MUST be rejected before funds are sent. Any fallback must respect the receiver's advertised choices and the client's configured policy; the protocol does not impose a universal requirement to hide receiver identity.

### Shared verification requirements

Regardless of method or privacy choice, the receiver MUST verify the intended recipient, agreed amount, settled status, circuit/round binding, and single redemption. A matching preimage/hash pair alone is insufficient without the receiver's own settled-payment record. Privacy preferences do not relax payment authorization, spending limits, or replay protection.

Each circuit round needs a distinct payment identifier and authorization; duplicate delivery or retry must follow the existing idempotency rules rather than reuse an already consumed payment. A BOLT 12 offer may be reused to request fresh invoices, but an already paid invoice MUST NOT fund another round. Methods based on BOLT 11 or an external invoice-acquisition service must define how fresh invoices are obtained and authenticated before the paid rendezvous exists. That bootstrap exchange and its bounded resource use remain protocol design work.

### Privacy properties and implementation scope

When the receiver chooses BOLT 12 blinded paths, account for both invoice-request delivery and payment delivery, and inspect identity fields in the offer and invoice for unintended disclosure. Blinding does not hide the receiver from all of its direct Lightning counterparties or remove timing/amount correlation. Network-address privacy also depends on wallet transport and deployment choices. The operator decides which of these properties it requires.

LND documents blinded invoice creation through its `is_blinded` option and an ephemeral signing key. The local LNI LND adapter exposes this option; that source support is not an end-to-end interoperability test. Wallet-specific support must be checked and tested before advertising a method as usable. No new wallet implementation is claimed by this specification update.
