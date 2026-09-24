---
id: 0kkfrvlt
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, input, shape, projection]
---
# A SHAPE input region never reached the live pointer projections

## Question

t064: the t061 popup audit found both live pointer-projection constructors
in `production_visual_runtime/projection.rs` setting `input_region` to
`None`, so a shaped window's input region could not take effect on the
path the session routes with, whatever the authority and the Engine's hit
test supported. What was missing, and what proves a shaped panel is
click-through?

## Evidence

The X authority computes the effective SHAPE input region at every commit
(`runtime.rs`, `effective_shape(window, X_SHAPE_KIND_INPUT)`) and puts it on
the surface transaction; the Engine's layer templates carry it
(`engine.rs`, `layer_templates_from_surface_transactions`), and its hit
test skips a layer outside its region (`input/hit_test.rs`). But the live
runtime rebuilds its input projections from the committed record
(`CommittedSurfaceState`) and from the retired frame
(`OutputFrameSurfaceState`), and neither carries a shape: both are records
of placed pixels. Both constructors therefore wrote `None`, and the session's
`hit_test_scene_surface_for_input` (`live_session/input.rs`) never saw a
region. The comment left on the presented constructor said as much: the
region needed a source, and the retired frame was not it.

## Finding and resolution

The source is the surface's own metadata, which the runtime already keeps
per surface for the namespace (`LiveSurfaceProjectionMetadata`, written at
every authority transaction by `observe_surface_metadata`). It now carries
`input_region` from the transaction, so it is as fresh as the last commit,
and both constructors -- `layer_snapshot` for the committed path and
`presented_input_layer_snapshots` for the retired frame -- read it. Nothing
in the committed or retired records changed; a shape is not a pixel, and
the record that names the surface is where it belongs.

Proof, `tests/support/presented_projection.rs`
(`a_shaped_input_region_reaches_both_input_projections_and_punches_through`):
a panel over a window, the panel's input region its left half only. Both
projections carry the region on the panel and none on the window, and
through the hit test the session routes with, a pointer inside the region
answers to the panel and one outside it falls through to the window. Red
before the constructors read the metadata (the projections carried `None`
and the panel took both), green after. This is the headless proof of panel
click-through; the physical panel acceptance stays with the installed
session's acceptance rows.

## Validation and remaining work

- [x] Red then green in the projection tests.
- [x] `sophia-backend-live` suite and clippy.
- [x] The transport gates: `xterm-pointer-oracle` pass (injected_motions=7,
      injected_buttons=8); `xtest-selection` pass on the rerun
      (owner_changes=1, conversions=2, injected_buttons=4). Its first run
      ended with the session declaring `session control failure: TimedOut`
      after the driver had already finished, a teardown race unrelated to
      the region (`.artifacts/xtest-selection/f499cbe2-1790247697/pass.log`),
      filed as t187.
- [ ] Physical click-through on the installed session (operator; rows t060
      and t061 cover the installed popup and menu acceptance).

## Connections

- [Parallel production readiness](../plans/queue-11-parallel-production-readiness.md#t064) --
  the row and the audit that found the gap.
- [Blank Thunar menus and frozen Brave](ce2b55uy-blank-thunar-menus-and-frozen-brave-need-separate-pixel-and-delivery-evidence.md) --
  the t061 audit.
