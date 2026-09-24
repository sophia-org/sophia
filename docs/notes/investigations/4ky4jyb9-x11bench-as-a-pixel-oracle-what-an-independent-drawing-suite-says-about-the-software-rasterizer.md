---
id: 4ky4jyb9
date: 2026-09-23
kind: investigation
status: resolved
tags: [investigation, x11, conformance, validation, drawing]
---
# x11bench as a pixel oracle: what an independent drawing suite says about the software rasterizer

## Question

XTS5 judges the authority by replies, errors and events; it does not look at
what a drawing request left in the drawable. `drawing_cases.py` does, but its
expectations are Sophia's own reading of the pixelization rules. x11bench
(`~/src/x11bench`, KarpelesLab, commit `7029758`) is an Xlib/XRender/Xft
program that draws 52 patterns, reads each back with GetImage and compares it
with a reference image, plus 8 stacking tests that read the root window. Is it
useful to Sophia, and what does it say about the software rasterizer today?

## Evidence

**Configuration.** `x11_conformance_host` built from `cbb5e34e` (sha256
`df27d0c4...`), started on a private socket inside bubblewrap with network
and IPC unshared, as `xts.py` starts it. x11bench built from `7029758` (sha256
`9b589879...`). No operator display was used. Evidence is under
`.artifacts/x11bench-oracle/`: the launcher, the references, the failing and
diff images of run 3, both run logs and the xscope traces.

**Upstream references are unusable here.** The PNGs in the x11bench checkout
are Git LFS pointers and git-lfs is not installed. They would also be the
wrong oracle: their server and fonts are unrecorded. References were
regenerated on TigerVNC 1.16.2 `Xvnc`, which draws through the same `fb`/`mi`
code as Xvfb (Xvfb is not installed). Xvnc passes 60 of 60 against its own
references, and does so again on a second run.

**References depend on the screen's millimetre size.** The first run used an
Xvnc screen of 1024x768 at 271x203 mm; the host's screen is 1280x720 at
339x191 mm. Xft derives its DPI from the screen height (96.09 against 95.75),
hints the glyphs differently, and uploads glyphs whose `xOff` differs by one
pixel: the traces show the client sending different AddGlyphs to each server.
Every text test failed for that reason alone. With Xvnc at 1280x720 all five
text tests and the Render tests pass. A reference set is valid only for the
screen geometry it was generated on.

**Run 3, host against the 1280x720 references: 46 of 60 pass.** Two runs of
the same candidate gave identical results. The fourteen failures:

| test | pixels | what differs | reading |
| --- | --- | --- | --- |
| clip_mask | 28 915 (44%) | the gradient covers the whole window, not the disc | **defect**: only `paint_damage` (fills) reads the clip pixmap; `draw_segments`, `draw_lines`, `draw_rectangles` and `draw_text` in `software.rs` never build an `XClipMask` |
| line_cap_styles | 471 | CapButt and CapRound ends drawn projecting | **defect**: `cap_style` and `join_style` are stored in the GC and read by nothing in `software/` |
| line_join_styles | 2 826 | every join drawn the same | same |
| line_widths | 144 | CapRound ends of wide lines | same |
| dashed_lines | 132 | one to two pixels at each dash end, width 2 and width 1 lines | same: wide dashes with CapButt, and CapRound at width 1 |
| fill_rule_evenodd, fill_rule_winding | 228 each | single pixels along the star's slanted edges | **defect**: FillPolygon's pixelization is exact in the protocol (a pixel is inside when its centre is, with the top-left rule on the boundary), and the host disagrees with `mi` on those pixels |
| circle, color_wheel | 226 each | single pixels along a zero-width arc | **not a defect**: the protocol leaves zero-width line and arc pixelization device-dependent |
| concentric_circles, arc_styles | 1 516, 274 | same, several arcs | same |
| win_raise, win_lower, win_restack_middle | -- | the raised window is never shown | **by design**: see below |

**The stacking tests meet the authority's policy.** The xscope trace of
`win_raise` shows the host answering ConfigureWindow stack-mode Above on an
unmanaged top-level with a ConfigureNotify marked sent by SendEvent,
`above-sibling: None`, and no Expose; Xvnc restacks, reports the new sibling
and exposes the raised window. `protocol_routing.rs` names that synthetic
ConfigureNotify "the protocol response to a managed ConfigureWindow request":
a client does not restack its own top-level. The tests that do not depend on a
client-driven restack (`win_stack_basic`, `win_three_stack`, `win_hide`,
`win_show_after_hide`, `win_destroy`) pass. The root GetImage they rely on
returns other clients' pixels; that is a separate question for confined
namespaces and is not answered here.

## Finding and resolution

x11bench is useful as a pixel oracle for the software fixture's raster, where
it complements rather than replaces `drawing_cases.py`: its expectations come
from another implementation, and it drives the real Xlib, XRender and Xft
client paths. It found three defects in `crates/sophia-x-authority/src/software`
that neither XTS5 nor the drawing cases report:

- a pixmap clip mask applies to fills only
  (t170);
- cap and join styles are not implemented for wide lines
  (t171);
- FillPolygon disagrees with the protocol's pixelization on slanted edges
  (t172).

It certifies the software fixture, not GPU composition or the Engine's scene,
and its text tests certify Render glyph composition only for a pinned screen
geometry and font set.

## Validation and remaining work

**t173, the gate (2026-09-23).** x11bench is the X11 profile gate's second
external suite, beside XTS5 (`crates/xtask/src/m3_acceptance/x11bench.rs`,
invocation in `docs/validation.md`). The gate re-enters itself inside
bubblewrap (`x11-profile --x11bench-contained`: private `/tmp`, no network,
no System V IPC, own PID namespace, cleared environment), starts the host and
Xvnc there, reads both screens from their setup replies with a parser of its
own and requires them equal, generates the references on Xvnc, runs Xvnc
against them as the negative control, then runs the host. No reference image
is committed; both servers share the machine's fonts and DPI.
`x11bench_expected.json` names all sixty tests and declares fourteen with
reasons: four zero-width arcs, three client restacks, clip_mask's rim, and
the t171 and t172 tests. The adapter is Rust, per AGENTS.md rule 11.

Evidence: on `195fa3b6`, `x11 profiles: PASS; xtest PASS (44/44); XTS5
BLOCKED; x11bench PASS (46 passed, 14 declared)`
(`.artifacts/x11-profile-195fa3b6-x11bench/`; oracle `Xvnc TigerVNC 1.16.2`,
screen 1280x720 at 339x191 mm, host sha256 `880fdaa6...`, x11bench
`9b589879...`). The same source with a mutated manifest outside the tree, one
arc left undeclared and a passing test declared FAIL, reads `x11bench FAIL
... circle: FAIL (expected PASS); solid_red: PASS but declared FAIL; the
manifest is stale` (`...-x11bench-mutation/`). The first gate run found that
the stacking tests print PASS rather than GENERATED while references are
drawn, because they verify themselves; generation now accepts either. Limits:
the unstable-oracle, geometry-mismatch and BLOCKED paths are covered by unit
tests (`tests/support/x11bench.rs`), not by a live mutation; the gate
certifies the software fixture only.

**t170, the clip pixmap (2026-09-23).** Every core path now honours it. The
helpers strokes, text, copies and images share apply the clip list pixel by
pixel but cannot reach the mask, which lives in the store beside the
destination; so each entry point in `software.rs` and `copy_plane.rs` holds
the destination's bytes as it found them (an `Arc` clone, copied only on the
first write) and `XClipMask::restore_withheld` puts back every pixel of the
request's damage the mask does not admit. Fills keep their per-pixel test.
Replay has no projection of a mask, so a command drawn through one now
poisons the journal with `unsupported_clip_mask` instead of replaying
unmasked -- which it did for fills as well as strokes.

Evidence: `pixmap_core_drawing.rs` checks fill, segments (at two clip
origins), polyline, rectangle outline, CopyArea and PutImage through a
depth-1 mask, five of the six red before the change; `raster_fallback.rs`
checks the replay refusal; `drawing_cases.py` `gc_dashes_clip` checks the same
five requests from the independent client, 126 of 126 required cases PASS on
the fixed host. x11bench `clip_mask` falls from 28 915 differing pixels to
342, all on the disc's rim: the zero-width arc drawn over it and FillArc's
edge in the mask, not the mask. Host sha256 `880fdaa6...`, run in
`.artifacts/x11bench-oracle/run-t170/`. Text under a mask has no dedicated
case; it takes the same restore as the paths that do.

**t171, wide lines (2026-09-23).** A line of width one or more is now drawn
by a port of the X server's `mi/miwideline.c`
(`software/geometry/wide_line.rs`, with `miStepDash` and the wide branches
of `miPolySegment` and `miPolyRectangle`; licence in
`THIRD-PARTY-NOTICES.md`). yserver's stroker was the first candidate and was
set aside: it rounds every offset corner to a whole pixel and approximates
round caps with sixteen chords, so it could not reproduce `mi`'s sub-pixel
edges, which are what the references are. The port keeps `mi`'s integer edge
walkers, its floating-point expressions in their order, its span groups for
the raster functions that must touch a pixel once, and two things that look
like slips -- `miLineProjectingCap` passing `xorgi` for `yorgi`, and the
fixed span array `miLineArcI` fills from both ends -- because agreeing with
the reference server is the point. Zero-width lines keep the store's own
path; an arc's chords stay on the brush (`XSegmentStroke::ArcChords`) until
wide arcs are `miarc.c`; spans are painted solid in the pixel `mi` chose, so
a wide line with a tile or stipple still ignores its fill style, as it did.
Density replay still strokes wide lines with the brush.

Evidence: x11bench `dashed_lines`, `line_cap_styles`, `line_join_styles` and
`line_widths` match Xvnc's `fb`/`mi` output exactly; their declarations are
gone and the gate reads `x11bench PASS (50 passed, 10 declared)` on
`d851c8a7` (`.artifacts/x11-profile-d851c8a7-x11bench/`). `drawing_cases.py`
`poly_primitives` gained butt, projecting, miter and bevel assertions from
the protocol's geometry: FAIL on master's host (`a4ee182a`), PASS here. The
port's own tests (`wide_line/tests.rs`) work the same shapes by hand, plus a
dash and a GXxor polyline whose spans must not overlap. The same core run
showed `xfixes_selection_stalled` TIMEOUT on master and here alike -- a
stalled subscriber no longer disconnected since t165's output spill --
reported to that task's owner, not a drawing matter.

**t172, polygon edges (2026-09-23).** The cause: the filler (derived from
yserver) sampled each row at `y + 0.5` in floating point and rounded x up,
while the protocol puts pixel centres on integral coordinates -- a pixel is
inside when its centre is, and a centre on an edge only with the interior to
its right. `mi` meets that with integer Bresenham walkers sampled at each
integral y and spans half open on the right. `software/geometry/polygon.rs`
is now a port of `mi/mipoly.c` with the macros of `mi/miscanfill.h`:
`miFillGeneralPoly` for Complex and Nonconvex polygons under both rules, its
winding chain kept as a per-edge flag recomputed exactly when `mi`
recomputes the chain, and `miFillConvexPoly` when the client declares its
polygon Convex, bounded so a false claim cannot spin. PolyFillArc still
filled the polygon `arc.rs` approximates; this paragraph first said that was
what remained of `clip_mask`'s 342-pixel rim, which t176 disproved (below).

Evidence: x11bench `fill_rule_evenodd` and `fill_rule_winding` match Xvnc
exactly; the gate reads `x11bench PASS (52 passed, 8 declared)` on
`100cfc9f`. `drawing_cases.py` `fill_primitives` gained a triangle (0, 0),
(9, 0), (0, 3) whose slanted edge passes through centres (6, 1) and (3, 2):
on master's host it filled rows 0-7, 0-4 and 0-2; the rule and this host
give 0-8, 0-5 and 0-2. `polygon/tests.rs` works two edge cases of the rule
by hand and checks the convex filler against the general one. The core run
is otherwise PASS but for t165's `xfixes_selection_stalled`, as before.

**t176, filled arcs (2026-09-23).** PolyFillArc is now a port of
`mi/mifillarc.c` (`software/geometry/fill_arc.rs`): the ellipse walker,
written once over `i64` and `f64` for `mi`'s integer and double copies and
chosen as `miCanFillArc` chooses, and the chord and pie-slice edges that clip
each row. The chord-stepped polygon path for filled arcs is gone. In x11bench
`arc_styles` the filled red arc differed from Xvnc on 48 pixels and now on
none. `clip_mask` did not move: all 342 of its differing pixels are black on
one side, the zero-width outline drawn over the disc, which covered the
disc's own rim all along. Its manifest reason is corrected a second time;
both earlier readings (the polygon edge rule, then the filled arc) were
wrong. Evidence: the gate reads `x11bench PASS (52 passed, 8 declared)` on
`4c7cf37b`, the core profile passes 126 of 126, and `fill_arc/tests.rs`
checks every centre strictly inside an ellipse is drawn at both walkers'
sizes, a pie slice against a chord, full turns, empty arcs, and that
opposite sweeps over one span fill alike. No wire case distinguishes the
old rim from the new; x11bench is the red and green.

What x11bench declared after t176: the zero-width arcs of four tests and
of `clip_mask`'s outline, whose pixels the protocol leaves to the server,
and three client restacks (authority policy).

**t178, zero-width lines and arcs (2026-09-23).** Fidelity rather than a
fix, since the protocol leaves thin pixels to the server, but the most used
drawing path left: xterm draws its box characters as zero-width
PolySegment. `software/geometry/zero_line.rs` ports `miZeroLine` with the
default zero-line bias, and `miZeroArcSetup` and `miZeroArcPts` for solid
thin arcs; `fb`'s fast paths share that bias and setup, so they are the
pixels Xvnc draws. A dashed thin line is `mi`'s `miZeroDashLine`: the wide
dash at width one. `mizerclip.c` is not ported, because the store clips
pixel by pixel and so keeps the unclipped pixels it exists to preserve.
Every line of any width now goes through `mi`-derived code; the brush is
left to arc chords (dashed thin arcs, wide arcs, arcs too large for the
walker -- t179) and to density replay.

One deliberate departure: the protocol says of PolyRectangle that no pixel
of a rectangle is drawn more than once, and a zero-width rectangle with no
width or height, drawn as `mi`'s closed polyline, runs down a line and back
-- under GXxor, `mi` erases it. The pixels here are `mi`'s, each painted
once; `output_and_draw.rs` had pinned the protocol's answer, and still
passes.

Evidence: x11bench reads 57 of 60 -- `circle`, `color_wheel`,
`concentric_circles`, `arc_styles` and `clip_mask` match Xvnc exactly and
left the manifest (they failed on master's host, as the t176 run records);
the gate reads `x11bench PASS (57 passed, 3 declared)` on `9a5ff4b9`, and
the core profile passes 126 of 126. `zero_line/tests.rs` checks that a thin
line covers the same pixels in either direction at several slopes, a joint
once, NotLast, closed paths and points, and that arcs stay on their box and
in their quadrant.

What x11bench still declares: three client restacks, answered by authority
policy. Every drawing test matches the reference server.

**t179, wide and dashed arcs (2026-09-23).** Promoted on XTS5's red rather
than x11bench's, which has no wide arc: XTS's `makegc` draws at line width
one, so its Xlib9 arc cases are all `miarc.c`'s. On master `83708031`
(scenario `arcs`: XDrawArc, XDrawArcs, XFillArc, XFillArcs) XDrawArc failed
38 purposes and XDrawArcs 45. `software/geometry/wide_arc.rs` with
`wide_arc/spans.rs` and `wide_arc/faces.rs` now port `miarc.c`: the offset
ellipse's quartic, the per-quadrant spans, caps, joins, the dash walk along
an arc, and `miWideArc`'s grouping -- each rendered group one union painted
once, which is what `mi`'s scratch bitmap gives a raster function that reads
the destination and what painting twice gives one that does not, clipped for
the first kind to that bitmap's extent. PolyArc at every width goes through
a runtime `apply_arc_draw`; the chord stroke and the yserver dash walker had
no callers left and are gone. Density replay is still handed each arc's
chords. Two things found on the way: `mi` reads phase 1's arc count for an
on/off dash, which has no phase 1 (past the end of its array; harmless there
because every use is guarded, and kept harmless here, with a test), and
every stroke -- wide lines, thin lines, arcs -- ignored the GC's fill style.
Strokes are now painted through it, each batch's pixel standing in for the
foreground as `mi` swaps it in for a double dash's off dashes.

Evidence, from the XTS journal (the adapter cannot yet parse this scenario's
journal; reported to its owner): XDrawArc 38 failures to 4, XDrawArcs 45 to
4, and XFillArc and XFillArcs unchanged at 4 each. Every remaining failure is
filed elsewhere: IncludeInferiors on the root (t181), and a GC clip mask,
tile or stipple whose pixmap the client freed after setting it (t180) -- the
clip-origin, clip-mask and tile/stipple-origin purposes all free theirs.
x11bench still reads 57 of 60, the core profile 126 of 126, and
`wide_arc/tests.rs` checks a wide circle against the ring it sweeps, a
quarter arc's quadrant, double-dash phase order, once-per-pixel groups, and
the on/off phase read. Dashed thin arcs are drawn as `miWideArc` draws the
ones its thin walker refuses, since `miZeroArcDashPts` is not ported.

**t180, pixmaps a GC holds (2026-09-24).** The store looked a GC's clip
mask, tile and stipple up by pixmap XID at draw time, so after
`XSetClipMask` then `XFreePixmap` -- legal, and what XTS's clip-origin,
clip-mask and tile/stipple-origin purposes all do -- a draw found nothing
and went unclipped or solid. The runtime already kept one freed pixmap alive
for its referents: a RENDER picture or GLX pixmap moves it to a private key
above the 32-bit XID range (`render_picture_lifetime.rs`, from the kitty
cursor fix). Graphics contexts are now a third kind of referent: on
FreePixmap a context holding the pixmap as mask, tile or stipple is
repointed at the private key, and the backing is dropped once nothing holds
it -- re-checked after ChangeGC, CopyGC, SetClipRectangles and FreeGC,
which is every way a context lets go, client teardown included. The GC's
count is read from the contexts rather than kept, so it cannot drift. A
reused XID names the new pixmap and leaves the context's mask alone, as the
protocol has it.

Evidence: `pixmap_core_drawing.rs` checks a freed clip mask under a reused
XID, a freed tile, and release when the last of two contexts lets go (red
before the change); XTS's arcs scenario on `ef25a282` passes 234 purposes
(222 before) and fails only the four IncludeInferiors purposes of t181; the
core profile passes 152 of 152 and x11bench 57 of 60.

**t181, the root (2026-09-24, in part).** Decided with the operator: a draw on
the root lands in a root private to the drawing client's namespace -- a
screen-sized CPU buffer under a key above every XID and retained-backing key,
created on the first draw and kept for the namespace's life, as a root's
contents outlive any one client; GetImage on the root reads it with that
namespace's own windows over it (t185 made the readback namespace-scoped);
it is never presented, because the Engine and the shell own the desktop.
Every core draw now resolves its target through \`draw_target\` and
\`draw_key\`, which map the root to that key. A wire test pins that the drawer
reads its root back, another namespace reads zero, and the draw presents
nothing. Still open under t181: IncludeInferiors, which draws through a
window's viewable descendants. It needs t188 first.

**t188, found on the way.** \`present_window_damage\` copies only the drawing
window's own pixels into its toplevel's presentation. A probe on \`55e50b1e\`: a
child filled green, then its parent filled blue across it, presents blue
where the child is. GetImage composites inferiors and reads green, so the
default ClipByChildren holds for a readback and not for the screen. Any
client that fills a parent across its children hides them until they redraw.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md):
  the gate, confinement and declared dispositions this should reuse.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md):
  every drawing request is decoded; this note is about what they draw.
- `tools/probes/x11_conformance/drawing_cases.py`: Sophia's own pixel
  assertions, from the specification rather than from another server.
