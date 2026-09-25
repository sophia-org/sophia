---
id: vxmhx2u4
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, input, rendering, policy]
---
# Presented policy input follows completed frames and independent revocation

## Question

How can the session join a complete WM publication to input without mistaking
requested records, a primary head, or transport progress for actual presentation?

## Implementation checkpoint

The t245 development branch is `rendering/input` in `sophia-borders`. Its base is
joined `e0e13d86`, with renderer review changes through `8329705b` incorporated as
`f983bfdc`. This checkpoint is buildable work toward t245, not its acceptance.

Engine retains admitted immutable records separately from completed per-output
identities. Keyboard targets are zero; pointer targets identify an exact region
or instance. No action name is interpreted. Source-only repaint preserves the
interaction epoch. Modal keys and pointer actions wait for every covered output.

Backend input projections carry three different facts: an all-head consensus
publication, whether that completion is established, and whether any retired
head still shows policy pixels. During primary-new/mirror-old or
primary-withdrawn/mirror-old transitions, consensus is absent but visible policy
pixels continue to shield input. A completed withdrawal requires every head to
leave the old identity; a missing frame is insufficient.

Session validates sources and pure action registration before layout staging and
pending settlement. It installs committed records at the production boundary,
without queueing an independent frame of the previous ordinary layout. Actual
completion, local revocation and completed withdrawal have separate receipts.
Bounded lifecycle storage is independent of action queue credit. Exhaustion
revokes input and schedules local withdrawal before refusing transport progress.

Pointer presses require a matching device, target and release. Consumed keys and
buttons retain release debt across close and reconnect. Protected handlers clear
terminal debt when they consume a release, and device removal clears that
device's debt. Stale actions are checked before issuance, on reply and during
settlement. Publication actions cover the entire scene even when their exact
target is on one output.

## Evidence and limits

Worktree evidence is retained under `.artifacts/`:

- `t245-engine-input.log`: completion, multi-output modality, source repaint,
  exact pointer identity, clipping, stale input and retained release controls:
  11 passed, including reused identities across reconnect.
- `t245-presented-frame.log`: the real production projection and mirrored target
  distinguish requested records, partially retired heads and completed removal.
- `t245-lifecycle-controls.log`: bounded notification overflow, catalog refusal,
  uncompleted stamp refusal, mixed-head shielding, one-shot withdrawal and
  reconnect through session action enqueue and receipt-epoch validation:
  six passed. Rewriting an old action's connection epoch still fails; the
  newly completed identity succeeds.
- `t245-session-lib.log`: the native-session library suite passed 594 tests
  with 18 intentional ignores before the additional reconnect-only control.
- `t245-clippy.log` and `t245-layout-audit.log`: strict affected-crate checks and
  the worktree source-layout audit. The exclusive main-tree gate is separate.

Backend and session execution uses `bwrap --bind / / --dev /dev --proc /proc
--unshare-pid --die-with-parent`, with live display variables and
`SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE` unset. Rust builds are niced and use the
disk-backed `sophia-t027/.artifacts/t026-target`; none use the main tree or a
live device. These are deterministic simulated-retirement observations, not
physical KMS or installed-session acceptance.

The transport capability ceiling from `829cba99` is incorporated as `8f3f332d`.
Startup and both restart paths remove presentation capabilities before worker
negotiation when native retirement is absent. Preflight refuses such candidates;
loss of native retirement revokes existing input locally. No software-only
completion owner is introduced. Full joined routing and independent Hagia acceptance,
including older-WM compatibility, remain required. The director owns task
closure and the gate window.

## Connections

The [presentation contract](../../wm-presentation.md) defines the authority and
retirement rules. The [foundation plan](../plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md#t245-presented-input-and-session-lifecycle)
owns t245's exit. The [renderer inventory](egmb00jq-rendering-foundation-inventory-and-overview-ownership-correction.md)
separates existing mechanisms from the generic foundation, and
[t241 acceptance](ufhp04gq-workspace-overview-joins-policy-presentation-and-modal-input.md)
joins these mechanisms to the WM-owned feature.
