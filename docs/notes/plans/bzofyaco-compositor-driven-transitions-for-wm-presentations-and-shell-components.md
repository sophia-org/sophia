---
id: bzofyaco
date: 2026-10-02
kind: plan
status: proposed
tags: [plan, milestone]
---
# Compositor-driven transitions for WM presentations and shell components

## Scope and exit

niltempus noted that niri's overview (Super+o) zooms smoothly while Hagia's
jumps, and asked that both the WM and shell components be able to animate.
niri runs one critically damped spring (stiffness 800, damping ratio 1.0,
epsilon 1e-4) on a progress value. It draws the layout at that progress each
frame, at the display's rate.

The two kinds of client differ here.
- The WM stays blind and event-driven. It publishes one finished state per
  event and never drives per-frame rendering.
- Shell components already have a clock and per-frame work for their OWN
  content:
  - NativeInput's issued times are host `CLOCK_MONOTONIC`
    ([shell files](../../sophia-shell-files.md), "Input and actions");
  - FrameDemand reason 2 requests animation frames under the standing
    demand/permit lifecycle (the FrameDemand and permit rules there).
  That stays as it is.
What this plan adds is that Sophia owns the interpolation of what clients
cannot animate themselves: WM presentation targets, and shell PLACEMENT and
popout geometry. A client declares a target plus a transition, and Sophia
interpolates. Nothing here gives the WM timing or pixels.

Sophia already animates one thing. [Window translation](../../window-transitions.md)
(`translation_groups`, WM bit 12; `crates/sophia-engine/src/translation.rs`)
moves retained pixels between accepted placements:
- a critically damped, stiffness-800 position spring, sampled closed-form
  from `now`;
- native per-output scheduling and a settled final frame;
- the session override `SOPHIA_ENABLE_WINDOW_TRANSITIONS=0`.
This plan generalizes that mechanism and leaves it in place.

Exit for each task: its contract and SDK changes (where any), Engine tests
with injected timestamps, Session tests, and mutants. The matching product
work follows in its own repository. Live acceptance means comparing beside
niri, interrupting mid-animation, running at both refresh rates and checking
reduced motion. This plan does not claim it.

## Task details

<a id="t285"></a>
**t285: Engine transition primitive, candidate.** No contract change.
- `TransitionSpec` is bounded: a spring (`damping_ratio_milli`,
  `stiffness`, `epsilon`) or an ease (`curve`, `duration_ms` up to 1000).
  It animates rect (and so scale), clip and opacity.
- Elm architecture (TEA) in shape, with no framework or runtime crate:
  - The model is plain data: each node's target and active segment (start
    value, start velocity, start time, spec), plus the exit ghosts.
  - `update(model, event)` is pure. New targets retarget from the current
    value and velocity; nodes are added to enter and removed to exit.
  - `view(model, t)` is pure and closed-form in time. It samples the curve
    at each frame's presentation time and never integrates per tick, as
    niri's `Animation::value_at` does.
  - Consequences: heads at 60 and 120 Hz and mirror heads agree, nothing
    drifts, and the settle time is known in advance. The core reports "frame
    needed until T" as data, so there is no polling and no idle wakeup.
  - Effects stay in the backend and are reported as data: frame scheduling,
    damage (the union of the previous and current interpolated rects), GPU
    work and ghost disposal.
- Bounds: animating nodes per output, duration and ghosts. A change over
  budget snaps to its target; it never fails.
- A desktop-profile reduced-motion switch snaps every transition, including
  the existing window translation, and keeps honouring that translation's
  `SOPHIA_ENABLE_WINDOW_TRANSITIONS=0` session override.
- Reuse and compatibility, to decide BEFORE implementation:
  - The existing translation spring (translation.rs) is already a pure
    closed-form sampler. t285 generalizes it (rect, clip and opacity; easing;
    bounds), rather than adding a second animation path.
  - One behaviour differs. The existing retarget restarts from the current
    position with ZERO initial velocity, which matches the inspected niri
    movement path. The proposed primitive preserves velocity on retarget.
    The review must decide whether translation keeps zero-velocity retargets,
    as a per-use option, or adopts velocity carry-over.
  - Until that decision, existing translation behaviour is unchanged. This
    plan does not approve replacing it.
- Licensing: Sophia is BSD-3-Clause and niri is GPL-3.0, so no niri code is
  copied. The spring comes from the textbook critically damped and
  underdamped solutions with niri's parameters, and tests compare its curve
  with niri's.

<a id="t286"></a>
**t286: WM presentation transitions (next WM capability bit), candidate.**
Peers: hagia/h016, hagia/h017.
- A presentation-level default `TransitionSpec`, with per-instance and
  per-region overrides.
- An instance can enter from its source surface's placement, which gives
  niri's overview zoom-out.
- A withdrawal plays an exit transition on non-interactive ghosts.
- Lands through the usual sequence: contract and generated rows, C SDK
  minor, Rust doc import, Sophia pins, then the implementation.

<a id="t287"></a>
**t287: shell placement transitions (next shell capability bit),
candidate.**
- Content placement and popout enter, exit and move transitions: a panel
  slide or auto-hide, a launcher fade or slide, a popout growing from its
  anchor.
- Components keep drawing their own content under the existing rules:
  CLOCK_MONOTONIC issuance, and FrameDemand reason 2 with the demand/permit
  lifecycle ([shell files](../../sophia-shell-files.md)). This task adds only
  Sophia-interpolated placement geometry. It does not add a second content
  animation path.
- Product adoption (lom, bemenu) happens in those repositories. Sophia keeps
  only generic SDK conformance tests (rule 13).

<a id="t288"></a>
**t288: WM layout transitions, later candidate.** These are EXTENSIONS
beyond the current positional translation (bit 12), which already moves
retained pixels between placements for WM camera and column motion. The new
parts are:
- animated resizes, which need client resize synchronised with configure and
  commit;
- animated scale, clip and opacity of committed windows;
- workspace-switch effects that position alone cannot express.
They stay outside the first cut.

### Open semantics (unresolved; to settle in contract review)
These are proposals, not decisions. Capability bits stay unallocated until
contract review.
1. Input during a transition: proposed to hit-test the drawn, interpolated
   geometry (as niri computes positions with the current zoom), with exit
   ghosts never targets.
2. Receipts: proposed to issue a publication's receipt, and its modal
   readiness, at the first frame that shows it rather than at settle, so
   input does not wait for an animation.
3. Ghost keyboard scope: proposed that an exit ghost carry
   `PresentedKeyboardScope::None`, as in niri, where keys reach applications
   as soon as the overview closes. This would revise t279's "shielding
   follows the presented pixels" for withdrawing presentations, so it needs
   an explicit decision.
4. A source that disappears mid-animation: proposed to exit from its last
   value.
5. Cross-head instances depend on t284 custody.
6. Mirror groups project one logical progress onto each head.

## Connections

- Proposal and approval record:
  `development-evidence/transitions-proposal-01/PROPOSAL.md`; ID reservation
  in `ID-RESERVATION.txt` alongside it.
- [Held capture plan](kgo1ugnz-held-capture-and-blind-wm-capabilities-for-niri-parity.md)
  (t279) for presented keyboard scope.
- t284 cross-head preview custody
  ([investigation](../investigations/qrmqf4gy-alt-tab-exposes-missing-cross-head-retained-image-custody.md)).
- t276 notification-driven idle waits: transitions must not add idle
  wakeups.
- [Window translation contract](../../window-transitions.md) and
  `crates/sophia-engine/src/translation.rs`: the existing bit-12 position
  springs that t285 generalizes.
- [Shell files contract](../../sophia-shell-files.md): the shell clock and
  FrameDemand animation rules that t287 keeps.
- Hagia peers h016 (overview transition) and h017 (recent-windows strip
  fade), in Hagia's plan for this work.
