# Validation and open work

The architecture and detailed chapters form a proposed version-one specification.
They remain unimplemented and require independent review and conformance testing. Use the
[implementation gates](gates.md), [consistency review](review.md) and
[deferred task backlog](../../TODO.md) together.

Validation has separate levels: documentation integrity, synthetic byte/vector
checks, independent protocol/cryptographic review, implementation interoperability,
and measured runtime privacy/resource behavior. Passing one does not establish
the others. Existing free-mode Chutney results do not validate these paid features.

The [ordered protocol design checklist](design-checklist.md) is complete at the proposed-design level. Use the [implementation handoff](handoff.md) for dependency order and acceptance tests before starting separately authorized implementation.
