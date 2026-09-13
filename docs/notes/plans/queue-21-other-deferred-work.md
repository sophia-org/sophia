---
id: queue-21
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# Other deferred work

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-19-deferred.md).



## t054

XLibre provider integration until a measured native-X gap justifies its
authority and maintenance cost.


## t055

Any new application protocol or compatibility frontend without a
specification amendment backed by named product evidence.


## t056

VRR until physical hardware reports `vrr_capable=1`.


## t057

On 2026-09-12 the operator explicitly admitted the broader independent X11
conformance gate and placed it ahead of the individual protocol repairs. Its
selected mandatory profile, evidence and remaining coverage are owned by the
[socket investigation](../investigations/wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md#gate-and-coverage).
Task priority and status remain in `todo.md`; this historical deferred heading
does not restrict that explicit assignment.


## t058

Runtime effect plug-ins or a sandboxed effect host until the private
build-linked provider proves a need and a safe lifecycle.

## t096

Clean up obsolete branches, worktrees and build caches after the active Lom and
native-input integrations have reached a stable accepted checkpoint on master.
The operator requested this follow-up on 2026-09-13; cleanup is deferred until
then, not part of current runtime implementation.

Coordinate with the owners before removing anything. Inventory active processes,
dirty worktrees, unique commits and review evidence; verify cherry-picked changes
by content rather than relying only on Git's merged-branch status. Preserve
unintegrated work and the source identities, fixture changes and logs required to
reproduce retained findings. Remove only confirmed obsolete branches, disposable
worktrees, stale registrations and unused regenerable targets. Keep future large
build targets off the /tmp tmpfs.

Completion requires a recorded inventory of what was removed and retained, with
active worktrees still usable and retained evidence still locatable. This task
does not require merging superseded experiments into master.
