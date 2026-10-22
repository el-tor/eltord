# Specification workflow

Follow the parent workspace's local-only Git and identity rules.

Every new or changed protocol requirement in this directory must add or update
a matching stable item in [the implementation TODO](../TODO.md) in the same
change. Include the source spec, affected projects, open decisions/dependencies,
and acceptance checks. Backfill omissions discovered during planning.

Distinguish draft design, implemented behavior, and executed validation. Leave
implementation deferred until requested; creating or updating a TODO does not
authorize starting it. Preserve the separate deferred replay-protection task.
