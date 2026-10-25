# Binding paid setup to the Tor introduction

**Proposed version-one protocol; not implemented.** This chapter supplies the
missing association between an accepted SCQ and the actual rendezvous cookie and
handshake input. It reuses the inherited Tor introduction and hs-ntor cryptography;
it does not derive new rendezvous traffic keys or treat payment as authentication.

## Client preparation and signed setup binding

Before requesting an invoice, the client establishes its own rendezvous circuit
at the RP named by the quote, using a fresh cookie and the inherited
[rendezvous protocol](https://spec.torproject.org/rend-spec/rendezvous-protocol.html).
It retains the actual local circuit object generation and hs-ntor client secret.
Normal client-side payment rules apply to that circuit independently. A missing
RENDEZVOUS_ESTABLISHED or dead client circuit stops setup; never assume the service
can make a nonexistent rendezvous circuit ready.

Prepare a complete v3 INTRODUCE1 payload using a selected authenticated descriptor
introduction key, that rendezvous cookie and the native handshake construction.
Do not send it through the ordinary introduction handler. Instead send common
service-control message 6 SETUP_BIND, round=0, with the usual signed SCQ, client,
circuit authorization, method and cap bindings. Its payload is:

```text
u8 setup_profile=1
intro_auth_key32
rendezvous_identity32
u16 introduce_length
introduce_payload[introduce_length]
```

`introduce_length` is positive and at most 2037 bytes, and also must satisfy the
inherited parser's actual length constraints. The outer payload is at most 2104
bytes and fits the existing 2800-byte control payload allowance. Encode exactly
the complete native INTRODUCE1 body, including its encrypted/MACed handshake
portion and native padding. No invented JSON, field reordering or re-encryption
of native handshake internals is permitted. The existing El Tor control HPKE is
an additional envelope, not a substitute for native handshake validation.

The [Tor introduction format](https://spec.torproject.org/rend-spec/introduction-protocol.html)
and the fork's `hs_service.c::service_handle_introduce2`/HS circuit parser are
implementation reference points. The implementation must separate parse/validate
from the old automatic rendezvous launch path: handling SETUP_BIND cannot call
a handler that builds first and checks funding afterward.

## Service validation and immutable association

SETUP_BIND is allowed for a still-valid offered quote before acceptance; it does
not accept the quote or create a payable invoice. The service verifies the outer
client signature, exact quote/client/core/circuit/RP bindings, original quote
validity and original inquiry limits. Resolve the intro key from the authenticated
metadata/key material retained for that inquiry. The embedded native auth key
must equal intro_auth_key. Decrypt and validate the native introduction using the
correct descriptor subcredential and retained introduction encryption key. Retain
no unbounded history of retired keys; unavailable/expired context is a refusal.

Resolve the native RP link specifiers and identity through verified directory
information and require that they identify the exact RP in both the quote and
SETUP_BIND. Reject mismatched, ambiguous or unsupported identity/link information;
do not repair an untrusted destination into a different relay. Preserve inherited
handshake key, MAC, replay and input-validity checks. A signature over malformed
Tor handshake bytes cannot bypass them. An introduction arriving through a
fresh control slot may refer to the retained original intro key: routing to the
same service does not silently replace the cryptographic context.

After validation, atomically store one immutable setup binding per
`(service,client,quote_core,circuit_authorization)`:

```text
setup_digest = SHA256(D("eltor/scq/v1/setup-binding") || service_identity32 ||
                     quote_core_digest32 || circuit_authorization_id32 || L(payload))
```

Store the native cookie/handshake context privately with this binding. Identical
logical retries recover it; changed cookie, native client key, intro key, RP or
bytes conflict even under a fresh operation ID. Retain native replay protection
across all entry paths. A duplicate allowed by El Tor idempotency returns existing
state without reparsing it as a new build or consuming a second authorization.
A replay of this native introduction through the ordinary handler must not launch
an additional or unpaid service path. Paid-service configurations require this
authenticated binding/funding gate on every rendezvous-launch entry path.

A successful response uses common status 9 SETUP_BOUND and payload
`setup_digest32`. Before acceptance agreement_revision is zero; after acceptance
it reports the existing positive revision. SETUP_BOUND means input validation and
association only, not funding, construction, joining or delivery. Semantic failure
uses the common REFUSED shape, with reason 3 for binding/handshake errors, 2 for
expiry, or 1 for capacity. Do not expose detailed native crypto errors to unauthenticated
parties. Conflicts never overwrite the original binding or authorize another build.

## Ordering and the single build

The required order is: quote verified, client RP established, SETUP_BIND validated,
quote acceptance persisted, round-one invoice issued, funds verified/reserved,
then one paid service build. ACCEPT_ROUND without the setup binding returns
REFUSED reason 3 and does not persist acceptance or issue an invoice. This is an
explicit prerequisite to the acceptance flow; no service wallet spending occurs
during quote or setup binding. The client may abandon an unaccepted binding;
its quote expiry bounds retention and no payment obligation is created.

At FUNDED, allocate the sole attempt ID and associate it with setup_digest before
any paid relay dispatch. Launch only to the retained RP with the retained native
cookie and hs-ntor context. The client later verifies the inherited rendezvous
handshake using its retained secret and expected service keys on its original
RP circuit. Service Tor binds the READY challenge to that attempt and actual
service-side circuit; client Tor correlates it with its locally verified original
RP circuit and accepted core. No signed status can override failed local handshake
verification. After completion, erase no-longer-needed ephemeral handshake secrets;
retain digest, attempt and financial/replay records required for reconciliation.

If either original path dies or native handshake context is lost, the attempt
fails under its existing deadlines and accounting policy. Do not substitute a new
cookie/client secret or rerun paid construction under the same acceptance. A new
connection needs a new quote/binding and explicit spending authorization, while
unknown old payment outcomes remain reserved. Internal Tor retry paths must be
constrained to the same authorized attempt and budget; absent a proven bounded
retry contract, fail rather than launch another paid circuit automatically.

Before acceptance, retain at most one setup binding per quote, 128 total per
service, each at most 4096 bytes including native state, until quote expiry.
Admit at most 32 new bindings/second with burst 64, sharing service crypto queues.
Accepted/funded records move into durable attempt accounting, not this disposable
cache. Full storage means refuse admission, not delete unresolved paid state.
These are candidate test ceilings, requiring memory/latency measurements.

## Local controller integration boundary

Tor must retain cryptographic secrets and circuit object generations internally.
Eltord supplies quote/funding authorization through authenticated local control;
it must not manufacture native handshake secrets from a ledger row. A recoverable
build operation needs the exact setup_digest, durable attempt ID and process-bound
local circuit handles. Existing EXTENDPAIDCIRCUIT alone does not implement this
service launch gate. The [service-build controller contract](../control/service-build.md) defines
registration, one path launch, query, activation-gated join and process loss.
Its implementation and recovery tests remain prerequisites to deployment.
No runtime implementation is authorized by this document.

## Privacy and validation

The native cookie, client handshake key and RP details are inside both the native
introduction protection and the encrypted signed control delivery. They never
become shared relay-payment tokens or public event fields. Retrying preserves
the native logical binding but refreshes outer HPKE and route tokens, preventing
direct comparison of its embedded ciphertext across introduction relays. This
does not prevent timing correlation or a malicious service from linking its own
requests. Resource limits apply before native parsing/decryption or paid work.

ET-13/16/23/28/29/33 must test RP/cookie/key substitution, exact duplicate after
lost SETUP_BOUND, stale descriptor keys, one native introduction through multiple
entry paths, unknown build dispatch after restart, client RP loss, service path
loss, and no paid launch before durable funding. Compare the inherited parser and
native cryptographic vectors with the implementation; no new handshake KDF is
specified here. Documentation/source inspection does not prove live interoperability.
