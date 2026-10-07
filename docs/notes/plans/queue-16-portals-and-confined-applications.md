---
id: queue-16
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# Portals and confined applications

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-12-candidate-queue.md).



## t045

Promote a confined daily-driver group only after Kitty and Firefox pass their
grant and recovery gates.


## t046

Add evidence-driven X11 `INCR`, Xdnd, URI/file launch, prompts,
notifications, and capture/FD handoff through portals.

Audit each existing reducer, frontend adapter and production service separately
before extending it. X clipboard delivery does not establish native clipboard
history access; a NotificationPortal command does not establish a native
notification-provider path. Scope each promoted slice to its missing join,
including prompt/authentication disclosure where required.

Exit per slice: explicit source/recipient grants, bounded payload/action/data
transfer, cancellation, revocation and owner replacement; a minimal independent
provider proves the complete wire/owner route, including denied/stale/foreign
requests and backpressure. Use t109 presentation admission where needed. No
notification center, clipboard manager or prompt UI product is required.

The capture slice also carries the session lock's screen-capture gate, which
t292 left here because no portal executor existed: while the session is
locked, a capture or frame handoff is refused and none in flight completes
after the lock is applied
([lock plan](8jcykhdc-secure-session-lock-authority-and-lock-provider-role.md#t292-session-lock-state-and-input)).
