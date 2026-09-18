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
The operator requested this follow-up on 2026-09-13 and authorized its execution
with the M3-to-master integration on 2026-09-18.

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

### Result recorded on 2026-09-18

After [M3 passed on local master](../milestones/nywg1vat-m3-integrated-acceptance-checkpoint-and-remaining-c-controls.md)
at `a4626b1b`, cleanup removed 58 obsolete local branch refs and 48 clean
worktrees, then pruned three registrations whose paths were already missing.
About 345 GiB of regenerable targets were removed, measured by apparent size.
Reports, source snapshots and evidence binaries were retained. No remote ref,
installed desktop or running test was removed.

Four local branches remain: master, the active Brave admission repair, Claude's
C acceptance worktree, and the coordinator's dirty worktree. The 29 remaining
worktrees comprise master, those three, 23 other dirty review experiments and
two installed comparison sources. Claude's language server still maps files
from the C target, so that checkout and target were retained. The Brave repair
`02548ed9` remains separate from the tested M3 source. These are explicit
retained exceptions, not failed removal attempts.

The inventory, process checks, owner responses, branch classifications and
removal records live under
`.artifacts/m3-master-integration-20260918/inventory/`. The archive at
`.artifacts/repository-retirement-20260918/` contains Git bundles, exact ref
identities, restore instructions, dirty-work snapshots and non-cache ignored
files from retired worktrees. Each bundle was fetched into an independent bare
repository and its recorded tips compared by full object ID before removal.
Temporary archive refs in the working repository were removed only after that
comparison; the bundles retain them.

Retirement decisions used ancestry, patch equivalence, an exact whole-tree
comparison for the old Lom branch, and a subject review of superseded M3 work.
Mutation tips remain archived and were not merged into production. P5's active
caches, Brave evidence and installed comparison sources stay protected. Future
cleanup of retained dirty or referenced work needs a new owner and process
check against that state.
