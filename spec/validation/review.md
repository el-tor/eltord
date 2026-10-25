# Consistency review and remaining decisions

This review covers the reorganized Markdown and source-backed control inventory.
It is not an independent cryptographic audit or runtime conformance test.

## Resolved documentation conflicts

| Earlier wording | Resolution |
| --- | --- |
| TOPS post-round payment and stronger privacy guarantees | Retained only as historical material; current rounds, exposure and privacy requirements are authoritative. |
| Watcher uses callback time and deletes ledger rows on closure | Historical sketch; current accounting requires actual settlement, durable single-use binding and reconciliation. Runtime remediation remains deferred. |
| Kill every circuit after ten IDs or immediately on final payment | Allocation-based renewal is explicit; receipt of a payment does not end its round. |
| Original paid service diagram bypasses SCQ/authenticated return path | Moved to historical material; current bootstrap/acceptance/readiness chapters define the sequence. |
| `EXTEND_PAID_CIRCUIT` event in prose | Control inventory records the observed subscription and `EVENT_PAYMENT_ID_HASH_RECEIVED` event token separately. |
| Every setup timeout is terminal | No-READY timeout may enter bounded WAIT_READY_RECOVERY with new spending stopped. Explicit failure/closure and recovery expiry are terminal. |
| Retry the “exact READY” | Same logical signed authorization, fresh encrypted delivery as required; no repeated ciphertext or reused AEAD nonce. |
| Fixed ten-minute path versus ongoing downloads | Ten-round allocations can renew only with negotiated support and accepted finite lifetime limits. Backup circuits do not migrate streams. |

## Remaining interoperability decisions

The [gate matrix](gates.md) owns the outstanding list. Particularly significant:
initial relay term/start authentication; exact catalog/delegation and acceptance/
invoice/status/READY/continuation schemas; control RPC recovery; relay command
allocation; finite lifetime and free-only renewal; measured free traffic, retry,
clock and resource budgets. No chapter may label these complete because an
architecture decision or initial quote layout exists.

## Readiness and spending invariant

FUNDED precedes paid construction. Billing starts at first paid relay activation.
Client connectivity requires its local Tor handshake. READY accepted on the same
live rendezvous path confirms service setup state. At the setup deadline without
READY, new spending stops; recovery uses only existing funded time. Valid READY
can complete a live recovering attempt but never reset billing or revive closure.
READY does not authorize new spending beyond the accepted round and budget policy.

## Privacy/timing assessment

Reorganization introduces no runtime wire behavior. Stable links replace positional
cross-references; proposed commands remain distinct from source-observed APIs.
Historical privacy claims are isolated. Existing requirements for independent hop
IDs, encrypted authorization, uniform padding, bounded retries and optional receiver
privacy remain. Timing correlation and privileged logging remain explicit risks.
Documentation coverage/link/render checks and their limitations are recorded in
the maintenance workflow; runtime, crypto and correlated paid tests stay deferred.

## Documentation verification for this rewrite

On 2026-10-25, the Markdown checker covered all 43 listed book chapters plus
repository entry points and the deferred backlog. mdBook 0.4.43 built locally;
the rendered checker covered 46 HTML pages with no broken local file or fragment
references. The sidebar was inspected in the local browser, including independent
El Tor numbering and the Control/RPC hierarchy.

The initial quote wire tables and fenced encoding examples were compared with
the pre-reorganization source and retained unchanged. The renewal wire document
and synthetic vector file were unchanged. The former technical companion's
sections were checked against their new destinations; obsolete walkthroughs are
retained as historical content. The readiness table now explicitly includes the
already-agreed nonterminal recovery state. Source-backed RPC documentation was
checked against the revisions named in its introduction.

These checks establish document structure, retained layouts and local navigation.
They do not establish runtime payment correctness, independently reviewed crypto,
current wallet interoperability or measured privacy. Those gates remain explicit.
