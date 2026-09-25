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

## Production routing follow-up

The follow-up to signed checkpoint `eaac9451` drives physical packets through
`route_input_events_with_launcher` with a completed modal publication. It checks
the exact keyboard zero-target and pointer region identities, swallows an
unbound modal key before application routing, and gives launcher capture and
the virtual-terminal chord precedence. Filling the session action queue then
issuing a routed action revokes locally; key and button releases remain consumed
after even the visible shield is removed. The retained application-grab control
now presents a policy tier over the application and verifies that motion and
release still reach the original client.

`t245-routing-session-lib.log` records 598 passed and 18 intentional ignores in
the device-hidden full native-session suite. The initial fixture omitted the
launcher's pointer placement and failed its activation assertion; that run is
retained as `t245-routing-session-initial.log`, followed by the corrected run.
`t245-routing-clippy.log` and `t245-routing-layout.log` retain strict session
all-target Clippy and the worktree layout audit. These controls use supplied
completion evidence to exercise routing; the separate mirrored backend control
owns actual retirement production. Independent Hagia transport is validated on
the director's joined branch and is not duplicated here. Final joined candidate
gates and physical acceptance remain separate.

## Review corrections before joined gates

Review found that the grab-continuity control used Overlay. A retired
ReplaceApplications frame removes application hit layers, which would make
the normal lease eligibility check cancel an existing capture. Session now
defers installing a changed replacement while application keys, leases or
keyboard/pointer handoffs remain. The regular presented-policy service retries
the committed candidate after settlement without a new WM proposal and queues
it through native composition. Until then it creates no completion receipt or
modal input authority. Installation revalidates registered actions and backend
source availability after the wait; loss revokes rather than installing stale
records. The conservative wait covers all outputs.

The session install-boundary control checks deferral, retry with unchanged
reducer commit serial, no fabricated receipt, and catalog loss during the wait.
A separate backend control retires a real application frame through the mirrored
target and evaluates `scene_contains_input_surface`, the lease eligibility
predicate, from its production input projection. Eligibility survives the
deferred interval and changes only when the replacement actually retires.
These are joined ownership controls, not a live hardware run.

A completed stamp-free projection now revokes any held output receipt before
keyboard routing. Enqueue also explicitly checks the current connection epoch.
The routing fixture now validates its publication shape, with a required passive
backdrop and an independent actionable region under ReplaceApplications.

Evidence: `t245-review-session-lib.log` passed 600 tests, 18 ignored;
`t245-review-deferred.log` includes the added catalog-loss branch;
`t245-review-retired-lease.log` passed the retired eligibility control.
`t245-review-clippy.log` and `t245-review-layout.log` retain final affected-crate
strict Clippy and worktree layout results. Final joined gates remain with the
director.

## Native-head geometry admission

The staged-publication preflight collects every current native head for every
covered output and calls the renderer owner's read-only
`validate_policy_presentation_on_heads`. Both initial staging and final pending
settlement perform this check. Deferred replacement installation collects and
checks current heads again when the capture settles. No additional wire record,
identity or cached topology is introduced.

Missing heads or a target with no clipped draw on any head refuse the whole
candidate before commit. This preserves the previous reducer publication and
backend presentation instead of accepting a permanently unpresentable target
and repeatedly revoking it. All-head retired membership remains an independent
completion defense against later topology changes.

The session control stages a tiny target against an existing publication,
accepts it on the primary, and refuses it when a Cover mirror crops it away.
It checks that the prior committed publication, backend getter and commit serial
remain unchanged, and that no receipt appears. The deferred-install control also
checks that losing all heads while waiting cannot install the candidate.

Renderer API `a529b2c4` is incorporated as signed `3f55da53`, retaining both the
session's `presentation_input` module and the renderer's shared fixture helper.
`t245-heads-session-lib.log` records 601 passed, 18 intentional ignores, in the
device-hidden native-session suite. `t245-heads-clippy.log` records strict session
all-target Clippy; `t245-heads-layout.log` records the worktree layout audit.
The director's exact joined candidate and main-tree gates are still separate.

## Connections

The [presentation contract](../../wm-presentation.md) defines the authority and
retirement rules. The [foundation plan](../plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md#t245-presented-input-and-session-lifecycle)
owns t245's exit. The [renderer inventory](egmb00jq-rendering-foundation-inventory-and-overview-ownership-correction.md)
separates existing mechanisms from the generic foundation, and
[t241 acceptance](ufhp04gq-workspace-overview-joins-policy-presentation-and-modal-input.md)
joins these mechanisms to the WM-owned feature.
