# Specification workflow

Follow the parent workspace's local-only Git and identity rules.

Every new or changed protocol requirement in this directory must add or update
a matching stable item in [the implementation TODO](../TODO.md) in the same
change. Include the source spec, affected projects, open decisions/dependencies,
and acceptance checks. Backfill omissions discovered during planning.

Distinguish draft design, implemented behavior, and executed validation. Leave
implementation deferred until requested; creating or updating a TODO does not
authorize starting it. Preserve the separate deferred replay-protection task.

## Privacy and timing review for every spec change

Every new specification and every edit to an existing specification must
consider privacy, identity exposure, message sizes, timing, and retries. Read
the relevant existing specs first, especially spec one's shared construction;
do not overlook protections already specified or invent a role-specific format.

For each change, review the following and record the applicable conclusions:

- **Observer and identity:** what the client, service, each individual relay,
  introduction point, HSDir, wallet/payment counterparty, and network observer
  can actually see. Distinguish existing knowledge from new disclosure; consider
  colluding observers. Keep persistent identities, private paths, wallet details,
  and stable quote/payment identifiers out of exposed routing fields and logs.
  Receiver privacy remains the receiver's choice; do not silently make blinded
  BOLT 12 mandatory or claim encryption makes all parties anonymous.
- **Linkability and retries:** independent hop identifiers; fresh, authenticated
  transport nonces/reply keys/encryption for each delivery attempt; no repeated
  ciphertext or shared cross-hop tokens that enable direct matching. Preserve
  logical idempotency and payment accounting without exposing stable references,
  resetting quote expiry, reusing AEAD nonces, or creating duplicate paid work.
- **Sizes and framing:** visible lengths, cell/fragment counts, last-fragment
  lengths, success/refusal shapes, and fragmentation/overhead. Apply each
  protocol's shared padded-envelope profile, including encrypted true lengths
  and authenticated padding where required. Preserve uniform initial payment
  fields and dummy zero-fee values. Do not assume outer relay padding hides
  lengths from the terminating introduction relay, or impose a control-message
  padding profile on unrelated bulk application data without design review.
- **Timing and lifecycle:** request/reply patterns, cached versus new replies,
  refusals, retries/failover, wallet latency, renewal/rotation, slot expiry, and
  deliberate delaying/dropping by a malicious relay. Review any proposed delay,
  batching, jitter, or padding against deadlines, resource budgets, and Tor's
  existing padding/isolation behavior. Do not treat random delay or fixed-size
  cells as proof against timing correlation or invent numeric defaults without
  explaining their basis and required measurements.
- **Abuse and validation:** aggregate bandwidth/CPU/state/queue limits, encrypted
  message authentication, replay protection, no automatic wallet spending or
  payment bypass, and safe failure under overload. Privacy measures must not
  require unlimited cover traffic or weaken payment/spending limits. Add
  concrete privacy/timing validation to the matching implementation TODO.

Include a brief **Privacy/timing assessment** in the edited spec, existing
review notes, or the local commit description: observers/signals affected,
protections retained or added, remaining exposure/open decisions, and checks
performed versus tests still deferred. For purely editorial changes, record
that wire/state/timing behavior is unaffected. This assessment is required for
every change; it is not a claim of an independent cryptographic/privacy audit.

Keep uncertain choices explicit and preserve the user's settled design
decisions. Documentation, design reasoning, synthetic vectors, or a free-mode
test do not establish live-protocol anonymity or successful implementation.
