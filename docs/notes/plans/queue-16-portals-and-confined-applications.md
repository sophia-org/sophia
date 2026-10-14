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

On October 14, after t319 acceptance, niltempus explicitly included the
remaining portal paths in the core architecture to finish before Engine
optimization. This promotes the existing scope to implementation and
acceptance; it does not waive the audits or per-slice gates below. Reuse the
accepted output-capture evidence from t319. Audit and deliver the remaining
clipboard/INCR, Xdnd, URI/file launch, prompt, notification and descriptor-handoff
paths through their existing owners, with minimal independent clients.

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

The [OSC 7501 investigation](../investigations/0photrwd-osc-7501-reports-program-status-through-admitted-notification-clients.md)
adds terminal-originated status notifications as a use case for the generic
notification route. Parsing, task records, progress aggregation and selection
of notification-worthy transitions stay in the terminal or multiplexer.
Sophia admits the sender and bounds delivery; program-supplied labels confer
no identity or permission. Authentication/approval status must not become
trusted credential entry, grant approval or automatic input. No OSC-specific
service, Engine parser or public wire format is introduced by this requirement.

Notification acceptance must cover denied/foreign grants, spoofed labels,
hostile text, explicit handling of text-size limits, rate and queue bounds,
sender replacement, revocation and stale status. Use independent generic
clients in Sophia tests. Terminal repositories own OSC parsing and prompt/reset
lifetimes; task clearing must not silently imply desktop dismissal. Keep rich
task trees outside the initial notification route. This adds no separate task
or change to the critical-path order.

The approved [Kitty/Ghostty interoperability cases](../investigations/u399qgtv-kitty-protocols-as-interoperability-evidence-for-sophia-portals.md)
also require exact binary/MIME clipboard delivery with explicit overflow
failure; trusted prompt attribution derived from admission rather than caller
names; sender- and generation-bound notification replacement, dismissal and
activation; and typed drag/file handoff that never interprets a remote path as
a local file. Test stale and foreign notification actions and failed-transfer
effects on retained clipboard data. Terminal protocols remain client-side;
Sophia's generic conformance tests must not depend on a named terminal.

The October 14 follow-up makes the broader transfer findings acceptance
requirements too. Preserve the complete MIME offer through executor custody
and prove selection of a non-first type. Bind user-initiated paste grants to
one recipient, selection generation and transfer; reject reuse after completion,
replacement or revocation. For file handoff, prove destination confinement,
directory/link handling, byte bounds and cleanup after cancellation or failure.
Cancellation must reject foreign and stale requests. These cases include
Ghostty's prompt-attribution lesson and the historical clipboard truncation
report; retain the OSC 7501 status cases above. Terminal passwords, labels and
request IDs supply no Sophia authority.

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

The [namespace prerequisite review](../investigations/id869143-x11-drawable-readback-and-an-operator-capture-path.md#namespace-prerequisites-for-capture-2026-10-09)
maps t133 admission, t142 live client groups and related work. It recommends
proving admission and confinement before shipping capture, while permitting
portal design and isolated tests alongside them. The accepted
[one-core delivery plan](jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order)
now places authenticated admission, group proof and recipe composition before
shipping the t319 output-capture bind. t315 supplies portal control and the
generic bind seam while preserving current X clipboard execution; it does not
wait for clipboard support through a future 9P application frontend. This
t046 row retains the later executor gaps and their per-recipient acceptance,
without making them prerequisites for the first screenshot.
