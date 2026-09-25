# One output's windows belong to that output

**Status:** built. Written after a live session showed a scrolling column
from DP-1 drawn on DP-2; kept as the record of why composition selects the
way it does.

## What happens

niltempus runs DP-1 (2560x1440 at x=0) and DP-2 (1920x1080 at x=2560). Three
windows sit on DP-1. The scroller lays them at strip positions 0, 1268 and
2536, which with the camera at -8 become screen x of 16, 1284 and 2552. The
third column is past DP-1's right edge, which in a scroller is ordinary: the
strip is longer than the display and the camera has not been scrolled to it
yet.

DP-2 draws it anyway, at its left edge. The window belongs to DP-1's
workspace and appears on the other monitor.

## Why

Placements are global coordinates and compositing selects by geometry, so
"past DP-1's right edge" and "inside DP-2's rectangle" are the same region.

- Hagia emits global coordinates: `bounds.x + outerGap + position - offset`
  (`policy/projection.nim:285`). So does Triad
  (`src/layouts/scroller.nim:282`) -- this is not where the two differ.
- The policy projection *is* per output: `PolicyOutputProjection { output,
  placements }`.
- That association is dropped when layers are built.
  `crates/sophia-session/src/live_session/wm/public_policy/proposal.rs:190`
  pushes a `LayerSnapshot` inside a loop that has `output.output` in scope and
  does not record it. `LayerSnapshot`
  (`crates/sophia-protocol/src/packets/surface/snapshot.rs:78`) has no output
  field.
- Composition then works from one global list. `production_cpu_cycle.rs:113`
  builds a single `presentation_order`, calls
  `surface_chrome_display_list(output.id, &presentation_order, ...)` with
  `output_descriptors.first()`, and produces frames for every output from that
  one scene. Each head draws whatever its rectangle overlaps.

So a surface is offered to every head, and each head keeps the part that falls
inside it. On DP-1 the third column is correctly clipped away at the edge. On
DP-2 the same surface is inside the rectangle, so it is drawn.

## How this was reached

The Engine used to clamp every placement into the work area
(`layout_epoch.rs`), so nothing was ever positioned outside an output and the
missing ownership never showed. Removing that clamp was correct -- it was
landing new columns on top of existing ones -- but it removed the thing that
had been standing in for ownership. The risk was noted when containment was
first relaxed ("whether the renderer handles partially-off-screen surfaces is
unverified") and not followed up.

## The rule

A surface is composited by the output whose projection placed it, and by no
other. Geometry decides what part of it that output shows; it does not decide
which output shows it.

This is also the answer to a question that keeps coming back in a different
form: there is no such thing as an overflowed column. A strip longer than its
display is the normal state of a scroller, and a column outside the camera is
not an error, it is simply not being looked at.

## What niri does

Ownership, categorically. `Niri::render` takes one output and immediately
resolves `layout.monitor_for_output(output)` (`src/niri.rs:4328`,
`src/layout/mod.rs:1788` -- an identity comparison on the `Output`, not
geometry), and everything below descends only into that monitor. There is no
geometric "which output does this window overlap" query in its render path at
all, and no clamp of a column into an output rectangle: off-view columns are
rendered at their off-screen position and discarded by the framebuffer.

niri also has no global window space -- strip coordinates are output-local,
column 0 at x=0 on every output, converted at render by subtracting the view
position. `global_space` "does not actually contain any windows"
(`src/niri.rs:233`).

**Sophia took the ownership half, not the coordinate half.** Ownership is what
prevents the bleed in niri too -- the monitor gate, not the coordinate space.
Output-local coordinates would change what Sophia puts on the wire and every
geometry consumer downstream; that is a protocol change, not a bug fix. The
divergence is deliberate and recorded here.

## The change

1. `LayerSnapshot` carries `output: Option<OutputId>`
   (`sophia-protocol/src/packets/surface/snapshot.rs`). `None` means no
   projection placed it, which is a real state rather than a default.
2. `public_live_proposal` sets it from the projection that placed the
   placement (`live_session/wm/public_policy/proposal.rs`), including when
   reusing a cached layer. First placement replaces `None`; a move replaces
   the previous output. Read-back sites pass `None` and say why.
3. The runtime retains a surface-to-output map from the layers it is already
   handed (`production_visual_runtime.rs`, `apply_presentation_layout`), so
   nothing new is plumbed.
4. `live_surfaces_owned_by_output`
   (`production_visual_runtime/compositor_graphics.rs`) selects a head's
   surfaces, and `display_list_for_output` builds from that instead of the
   global order.
5. A GPU Present gathers sources from the union of the applicable outputs'
   display lists, then lowers those same lists. It cannot use the primary
   output as a proxy for source discovery. A surface outside its owning
   viewport does not become presentable by overlapping a neighbour's viewport;
   such a Present follows the existing invisible-Present rejection path.

Clipping is untouched: a column half past the edge still shows its visible
half on its own output. Chrome needed nothing -- `IndicatorStrip` and `TabBar`
already carry their output -- and the cursor is already composed per output
from a global position.

## Evidence

`crates/sophia-backend-live/tests/per_output_ownership.rs` states the rule
directly, including the case that produced it: three columns owned by output
one, the third past its right edge, and output two composites none of them
whatever their coordinates. Ordering is preserved because it is stacking
order, and an unplaced surface belongs to no head.

`crates/sophia-engine/tests/layout_epoch.rs`
(`an_accepted_size_keeps_its_position_outside_the_bounds`) holds the other
half: a placement outside the bounds keeps the position it was given. The two
together are the whole rule -- position is not rewritten, and ownership rather
than position decides who draws it.

Still wanted: a conformance scenario with two outputs and a strip longer than
the first, asserting the second output's frame contains only its own surfaces.

The installed startup failure on `86b5fe1d20bc` exposed two missing links in
that rule. `sophia-session/tests/support/policy_output_ownership.rs` exercises
the actual proposal builder with an unassigned cached raster and with a raster
moving between outputs. `sophia-backend-live/tests/present_source_consistency.rs`
reproduces the missing-source error from using only the primary output's list,
checks a secondary-only Present, and lowers distinct CPU and DMA-BUF surfaces
on two outputs without cross-output leakage. These deterministic checks do not
replace installed physical acceptance.

## Not this change

- The scroller's own behaviour. Hagia places correctly; nothing in
  `policy/projection.nim` was implicated.
- Output-local strip coordinates (see above).
