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

The [2026-10-09 screenshot report](../investigations/id869143-x11-drawable-readback-and-an-operator-capture-path.md)
adds an operator and agent use case: one authorized image of a selected window
or composed output, including accelerated content. A minimal CLI/provider
path should return bounded image data with target, dimensions and frame
identity, and a distinct refusal or unavailable result when no capture can be
produced. Audit the existing CPU-backed GetImage path separately; a successful
read of zero-filled backing is not proof of the visible frame.

For this slice, prove the selected window/output using a generic known-pattern
client, including accelerated presentation, and declare crop, occlusion and
cursor semantics. Cover resize or output replacement while pending, bounded
completion, recipient isolation, cancellation and the lock gate above. A CLI
caller and an agent use the same explicit grant; no new input permission is
implied. Live desktop capture does not depend on completing the separate
[t303 VKMS investigation](../investigations/jweorh0z-headless-sophia-validation-and-capture-with-vkms-writeback.md#t303).
