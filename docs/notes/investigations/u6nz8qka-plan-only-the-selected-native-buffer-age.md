---
id: u6nz8qka
date: 2026-11-03
kind: investigation
status: candidate
tags: [rendering, performance, damage]
---
# Plan only the selected native buffer age

## Question

Can the worker avoid constructing damage plans for buffer ages EGL does not use?

## Change

The worker previously reduced every retained age (up to four), before EGL selected
one. A render-local borrowed repaint source now reduces only the actual age,
after the context is current and before drawing. The existing table implements
the same interface and remains the equivalence oracle. Engine owns reduction;
EGL has no Engine dependency. The borrow stays on the renderer worker and is
released before history settlement. No cross-frame cache or pixel change.

Fallback precedence, rectangles, damage causes and geometry evidence match the
eager table. Failed writes and target replacement still invalidate history;
only completed exports record it. Planner metrics count actual reductions, so
unused ages no longer increment them. Successful-render counters keep their
existing meaning. NativeCompositionFrame and mixed export APIs now accept a
borrowed NativeCompositionRepaintSource instead of only a precomputed table.
The provider is synchronous and render-local, not transferred between workers.

## Evidence

Device-free comparison covers disabled/missing/history-depth/age/failure
combinations, plus the existing geometry, transition and history fixtures.
A usable age causes exactly one reduction; immediate fallbacks cause zero.
Backend library tests: 363 passed, one ignored. Backend integration tests:
317 passed, two ignored (including the explicit stage benchmark).
Real-GPU full/partial pixel equality and lying-table negative control pass on
renderD128; the committed-damage journal equivalence test passes on renderD129.
The rendering test also asserts one provider selection per successful render.

An alternating-order release stage benchmark (four rounds, 10,000 selections
per variant per round) measured eager 2.29–2.33 us versus selected 0.583–0.588 us.
This synthetic four-surface fixture measures planning only, not native us/frame.
The eager side borrows its selected table entry as the old renderer did.

Strict all-target clippy passes for backend-live and renderer-native-egl.
Including renderer-live in strict clippy encounters its existing
unnecessary_filter_map in owned_mixed_export.rs; no unrelated fix is included.
Layout audit and changed-file rustfmt pass.
Evidence: ~/.local/state/sophia-selected-age-damage-01.

## Remaining qualification

No native ABBA, saving/adoption claim, merge, push or installation yet.
Retained-layer and changing-strip comparisons must use current adopted master
8359816f0 as control, identical native Void builds and W2 off. Keep the existing
cadence, identity and accounting gates; no repeat-until-pass policy.

## Connections

- [Protected retry optimization](../milestones/r7q2m9xp-skip-protected-retained-projection-retries.md)
