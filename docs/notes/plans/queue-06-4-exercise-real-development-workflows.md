---
id: queue-06
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# 4. Exercise real development workflows

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md).



## t017

Use terminal editing/building and Firefox for real work; verify clipboard
transfer in both directions, keyboard shortcuts, focus, resize/move, workspace
navigation, and both monitors.


## t018

Complete the [physical tab acceptance](../../tabbed-layouts.md#verification-and-operator-acceptance)
for frame-tree/Notion and i3/split-tree: empty/nested groups, hidden-member
activation, shell recovery, title changes, fullscreen, and floating occlusion.
Implementation and offline verification are already complete.
The [September 25 evidence review](../investigations/4l5bntke-critical-desktop-exits-require-separate-device-tab-and-workload-evidence.md#tabs-reference-shell-and-component-desktop)
maps the separate Narthex reference run and the current component-desktop
limitation; it does not claim the physical matrix was executed.


## t019

Observe idle, VT resume, and normal logout during actual use. Turn any
blocking failure into the next concrete repair task within this stage; retain
its evidence, add a focused regression where feasible, and recheck the affected
workflow after correction. The [status-1 logout investigation](../investigations/64o6l37g-normal-logout-reports-failure-after-x-protocol-errors.md)
retains physical acceptance of the logout repair on installed `8921174c`: exit
status zero with ten protocol refusals preserved and successful TTY recovery.
The subsequent
[runtime crash investigation](../investigations/fltuldiq-runtime-session-crash-retains-no-specific-cause.md)
tracks a separate owner-loop failure whose specific cause is not yet known.


Exit: every listed workflow has retained observations, with no unresolved
failures preventing those tasks or safe recovery. Unrecoverable sessions, lost
input, application-blocking failures, visible corruption, undrained work, and
unbounded resource growth are blockers. Longer sessions are useful evidence,
not mandatory hour/day counters that restart after every fix.
