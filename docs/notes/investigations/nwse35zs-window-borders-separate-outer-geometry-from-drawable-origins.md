---
id: nwse35zs
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, x11, raster, input, conformance]
---
# Window borders separate outer geometry from drawable origins

## Question

X11 geometry locates a window's outer border, while drawable and event
coordinates start inside it. Can t214's raster placement and t227's model
origins change together without moving an Xt application's pointer one pixel
away from its content?

Acceptance requires accumulated interior origins through ancestors, matching
GetImage/presentation/IncludeInferiors placement and ancestor clipping, an
Engine surface at the top-level interior, and matching socket event coordinates.
Changing border width must update existing pixels and coordinates together.
The windows and events XTS scenarios and the core probe are the integration
gates. No physical session or installed binary changes are part of this task.

## Finding and resolution

Border width was stored separately from the window model. Coordinate walks
and raster walks used raw geometry, and GetImage's descendant composition did
not clip a grandchild to its parent. The connection's input selection table
also accumulated raw coordinates independently of the runtime.

The candidate keeps border width in the passive window record and distinguishes
outer geometry from interior geometry. Root coordinates accumulate every
window's border; presentation offsets stop at the top-level interior, whose
border is instead applied once to its Engine surface. Engine admission and
configure translate the interior back to the outer corner for X replies.
The connection retains outer rectangles plus widths and uses interior origins
for input coordinates. A border change preserves the pointer's root position
and republishes the top-level raster from its existing drawable buffers.

Borders reserve space but do not paint pixels. Border pixel/pixmap painting
remains outside Sophia's compatibility contract; this seam does not claim it.

## Validation and remaining work

Worktree: `sophia-borders`, branch `raster/t214-t227-borders`, based on
`baae1419`. Initial targeted regressions cover nested coordinates, the Engine
round trip, readback versus presentation, IncludeInferiors clipping, changed
border width, and real-socket pointer delivery. Worktree logs are under
`.artifacts/borders/`; final candidate identity and retained gate evidence will
be recorded after the complete gate chain. Task status stays in
[todo.md](../../../todo.md).

## Connections

The [XTS event investigation](1qfs8k41-the-xts-event-section-on-the-host-and-on-xvnc.md)
identified the model half and why it cannot land separately from the raster
half. The [compatibility matrix](../../x11-compatibility-matrix.md) owns the
decision not to paint X window borders.
