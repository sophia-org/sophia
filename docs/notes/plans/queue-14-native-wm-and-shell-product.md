---
id: queue-14
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# Native WM and shell product

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-12-candidate-queue.md).



## t035

Before admitting a content prototype, characterize Quickshell's retained
popup-movement failure with an isolated display backend. After the CP-15
coherence gates, separately admit one panel/popout workflow and require an
independent C content client against the same public shell contract. The
[audit](../../shell-reference-client-audit.md) owns feasibility evidence and the
[proposal](../../content-shell.md) collects the behavioral requirements. Transport,
pixel semantics, numeric bounds, wire design, modeling, and a conformance
corpus remain prerequisites; documenting them admits no runtime implementation.


Previously completed evidence: [Implement the bounded session-owned control v1 endpoint and sophia msg: startup-only session.control "host-admin", disabled by default, socket-derived pidfd and user/mount/PID namespace admission,…](../sources/2026-09/todo-cutover-completed.md#legacy-done-019).


## t036

Optional short installed control smoke: discover, invoke a safe registered
action, restart WM, and observe continued input/rendering. Automated endpoint
and supervised-owner evidence is separate; do not restart the 36-row gate.


## t037

Verify owner recovery and complete control-endpoint settlement before
advertising or dispatching `reload-profile` through scripting.
Keep shell commands, delegated grants, parameters, queries, and subscriptions
behind separately specified contracts. Linux admission does not attest
arbitrary third-party sandboxes sharing the host namespaces.

**Scope clarified 2026-09-10.** The bounded shortcut-driven launch/profile
transaction landed in `17134504`; [t076](1agxbuuf-application-commands-in-the-desktop-profile.md)
owns its installed acceptance. Its 21 reload/registry regressions passed again
at `1f193b35`. This does not complete the scripting endpoint: its catalog
advertises only the Session `restart-wm` operation, and its dispatcher rejects
other Session names. Session and CLI tests explicitly require `reload-profile`
to remain absent. The [control contract](../../sophia-control-v1.md) now states
that distinction consistently.

The remaining gate is owner-semantic validation, checkpoint restoration and
rollback to a usable policy, followed by bounded endpoint dispatch and truthful
completion across the operation's own catalog/WM replacement. Reuse the
existing reload machinery; do not widen it into a general seven-authority
transaction. Shell, input, broker and non-launch Session settings retain their
documented deferrals. This remains candidate work requiring promotion before
implementation; t075/t076 physical acceptance is separate.

**Promoted and implemented 2026-09-26** (operator decision, for the t250 scripted
recovery rehearsal) as control revision 2: `reload-profile` and `logout` are
advertised to revision-2 connections and dispatched through the same owner-loop
requests as their bindings. A control reload settles on the reload owner's own
outcome (unchanged, declined, applied, or a required WM replacement that commits
or rolls back to the prior profile at a fresh epoch); a hand-off alone never
completes it. Revision-1 connections are unchanged.


## t038

Harden the implemented issuer-scoped action checks and reservation/work-area
coordination only against a named remaining lifecycle gap; preserve existing
offline conformance and distinguish it from signed physical acceptance.


## t039

Stabilize the minimum `sophia_shell_v1` lifecycle only after CP-15.1 and
CP-15.2; require signed installed Narthex evidence and preserve metadata
separation from the blind WM.


## t040

Add bounded target-resolved move, resize, drag, and scrolling interactions.


## t041

User promoted the named workflow on 2026-09-11: new child applications launched
from Kitty retain origin output/workspace while the user switches away, and
Triad-derived floating/dialog regressions cover geometry, stacking, and focus.
The approved implementation and evidence are recorded in the
[owning investigation](../investigations/bwffe5lv-triad-floating-regressions-and-child-launch-origins.md).
No swallowing, metadata rules, or existing-instance forwarding is admitted.
Exit requires both contributor gates and separate installed two-output acceptance.


## t042

**Result, reconciled 2026-09-10.** The proposed public per-head framebuffer
handles were superseded by `5efa438e4d0d340483ac410c85fd195243bd0e8a`.
The legacy CRTC framebuffer lookup remains appropriate for standalone use.
The redundant live-session apply was removed: the output-authority transaction
already quiesces presentation, prepares each head and rollback state, applies
the topology, and confirms presentation before publication. Profile reload now
submits candidates through that same authority. No new public framebuffer-handle
API was introduced.

The live diagnostic is now
`sophia_live_native_topology_apply schema=2 status=owned_by_output_authority`.
The old `status=declined reason=heads` line described the redundant path, not
proof that the authoritative transaction had failed. A command/keybinding-only
reload constructs no output candidate.

**Evidence and limits.** At `1f193b35`, 24 output-topology owner tests and 11
native-output topology tests passed with `--features native-session`. They
cover unchanged-output reloads, unsupported modes, exact mode selection,
publication identity and rollback ordering. The retained frame-fed archive
`0001` in [validation](../../validation.md) separately proves its historical
startup apply/rollback candidate. These checks do not establish this seat's
current DP-1 refresh rate. The original 120-Hz-request/60-Hz-observation is
unresolved historical evidence, not a continuing defect inferred from the old
diagnostic. Installed startup/output acceptance remains under t012 and t076;
this reconciliation retires the obsolete implementation proposal only.


## t043

Specify the remaining bounded redacted window/layout/focus feed and authorized
actions for native consumers. Workspace indicators already exist in shell r6;
their WM-owned tokens are not broker window metadata. The persistent app catalog
does not identify running windows. Reuse descriptor/tab issuer/recipient scope
instead of exposing raw XIDs, namespace/PID data or unrestricted inspection.

Exit: publication/disclosure policy, generation/revocation and action semantics
documented; independent minimal consumer; denied/redacted/stale/foreign action,
restart and bounded-update tests. No dock/window-switcher UI is required.

An advertised Session operation slot can carry an implemented action without
reopening the frozen WM layout, but does not itself implement a service or
authorize its disclosure. Lock remains t034, capture/prompts t046, background
t049, and external-service access t113. Their owning contracts must be settled
before advertising an operation; adding an enum/string alone is insufficient.
