---
id: nwse35zs
date: 2026-09-25
kind: investigation
status: closed
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

The first draft XTS windows run moved XTranslateCoordinates 1 to PASS. Purpose
3 exposed a separate stub: the reply always named no child. It now resolves
the supplied point against mapped direct children, including their borders,
without reading or moving the pointer. XQueryPointer 3 exposed an unanchored
root warp in the conformance host: only its client-placed mode may resolve a
top-level without an Engine target. The desktop mode still refuses to invent
Engine hit testing, and a regression exercises both modes.

Pixel-reference failures remain distinct. XCreateSimpleWindow 4 creates a
three-pixel border and compares the clipped result with a painted-border image;
the draft reports 111 differing pixels. The win-gravity purposes explicitly
set contrasting child borders before their pixel checks. SetWindowBorder,
SetWindowBorderPixmap and SetWindowBorderWidth likewise check painted borders.
Their declarations must describe this retained limit instead of claiming that
interiors still use outer-corner coordinates.

## Validation and remaining work

Accepted candidate: signed `73392063f2c53424be13d87e9a023bc049d6836e`, on
`01828630`, branch `raster/t214-t227-borders`. Four regressions cover nested
coordinates, the Engine round trip and raster transaction origin, readback
versus presentation, IncludeInferiors clipping, changed border width,
real-socket pointer delivery, and the client-placed root-warp query guard.

The main-tree gates all passed on that exact clean candidate, with unchanged
source attested after every run:

| Gate | Passed | Declared exceptions |
| --- | ---: | ---: |
| Windows, all profiles | 244 | 60 |
| Selected-core, all profiles | 76 | 23 |
| Xproto, XTEST profile | 339 | 50 |
| Events, XTEST admitted | 123 | 72 |

Every XTEST profile passed 44/44; both native-input profiles passed 40/40.
The separate core probe passed 162/162. The authority and session all-feature
suites passed 2,969 tests across 74 binary/doc groups. Workspace/all-target
Clippy, formatting, layout and diff checks passed. An initial session run
inherited the operator's configuration and failed the optional Hagia policy
test during argument parsing; the full rerun under a fresh private
`XDG_CONFIG_HOME` passed without a source change. That failed run is retained
separately, not counted as acceptance.

Three previously declared purposes became mandatory passes:
XTranslateCoordinates 1 and 3, and XQueryPointer 3. No mandatory purpose was
weakened. Painted-border pixel references remain declared for the explicit
compatibility limit above; passing coordinate checks is not evidence of
border painting or physical desktop acceptance. No live install or reload
was performed.

Checksummed reports, journals, source attestations and suite logs are retained
at `~/.local/state/sophia/development-evidence/t214-t227-borders-73392063/`
(`SHA256SUMS`, `summary.json`). The original main-tree gate outputs are
`.artifacts/x11-profile-73392063-xts-{windows,selected-core,xproto,events}`;
the core output is `.artifacts/conformance-core-73392063`. Task status stays
in [todo.md](../../../todo.md) and its monthly completion file.

## Connections

The [XTS event investigation](1qfs8k41-the-xts-event-section-on-the-host-and-on-xvnc.md)
identified the model half and why it cannot land separately from the raster
half. The [compatibility matrix](../../x11-compatibility-matrix.md) owns the
decision not to paint X window borders.
