---
id: a3j8o6g0
date: 2026-09-25
kind: milestone
status: recorded
tags: [milestone, shell, acceptance]
---
# Desktop acceptance retargeted to Lom and Bemenu

## Retargeting, not completion

On September 25, niltempus agreed that Sophia t081 should accept the current
Lom + Bemenu desktop, superseding the requirement that Lom provide every shell
workflow. This records that authorization and reconciles its prerequisite t101.
The source baseline is `1fbaf4cbc81ef3629176aea30b758ce51018d887`.
No installation, reload, device access or attended run follows from this change.

The earlier [Lom critical path](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
required one combined content/descriptor shell. In particular, t101 depended on
Lom's combined descriptor launcher/switcher work, and t081 required those
workflows in the same shell. Those requirements remain historical scope and a
separate compatibility design; they no longer gate acceptance of the selected
two-component desktop. This does not remove an existing protocol or weaken its
negotiated-workflow tests.

## Selected desktop and ownership

| Function | Acceptance owner |
| --- | --- |
| Layout, focus, workspace policy, window navigation and overview | Hagia, through Sophia's generic policy/presentation contracts |
| Panel, workspace indicators/activation, clock and calendar popout | Lom, on its own protected component connection |
| Application launcher, catalog filtering, launch result and focus return | Bemenu, on its separate launcher connection |
| Physical input, presentation, supervision, grants and resource retirement | Sophia |

Narthex remains a separately selected descriptor reference and rollback session.
It does not run alongside the selected Lom + Bemenu pair. Missing tab-descriptor
service in the component owner is not declared implemented: t018 still uses its
explicit Hagia/Narthex reference matrix. No new descriptor switcher in Lom,
Provlita/dock, notification service, GPU bridge or extra provider is admitted.
The separate three-component t108 candidate is not promoted or completed.

## Acceptance consequences

[t101](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t101)
now verifies the exact Sophia/Hagia/Lom/Bemenu package and selected component
profile, each negotiated workflow, isolation through replacement, and separate
rollback. Its existing Sophia prerequisites remain. The `peer:lom/t020` link is
removed from its active row because that peer owns the superseded combined-shell
feature; this does not close or edit Lom t020. Lom package and attended-evidence
peers keep their own status and require reconciliation in their repository.

[t081](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t081)
retains exact installed identities, every admitted output, native presented
input, recovery, bounded retirement, measurable workload/resource/latency
budgets and a tested rollback. Its launcher observations apply to Bemenu and
window-navigation observations to Hagia. The original deterministic, simulated
and physical evidence keeps its own identity and limits; this change supplies
no missing observations.

The [critical-task audit](../investigations/4l5bntke-critical-desktop-exits-require-separate-device-tab-and-workload-evidence.md)
records the component/reference distinction and the narrower reusable teardown
evidence. The t069 normal-client device boundary, t097 GPU admission/retirement
proof and t020/t021 development-session exits remain unchanged. Task status,
priority and dependencies stay in [todo.md](../../../todo.md); no task closes
through this retargeting.
