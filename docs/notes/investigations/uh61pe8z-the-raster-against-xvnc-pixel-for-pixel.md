---
title: The raster against Xvnc, pixel for pixel
date: 2026-09-24
tags: [investigation]
status: resolved
---

# The raster against Xvnc, pixel for pixel

## Question

Where does the CPU raster still draw pixels the reference server does not,
across every GC function, plane masks, the four fill styles with their
origins, line styles with caps, joins and dash offsets at zero and wide
widths, clip rectangles in each ordering and clip masks with origins,
subwindow modes, and every pixmap depth both servers advertise?

## Evidence

A differential checker (an investigation script, kept out of the tree)
drives the fixture host and TigerVNC's Xvnc with the same requests over
their private sockets and compares GetImage results pixel for pixel. Each
case draws one primitive (PolyFillRectangle, PolyLine, PolySegment,
PolyRectangle, PolyArc, FillPoly, PolyFillArc, PolyPoint, PolyText8 and
ImageText8) on a fresh 64x64 pixmap pre-filled with the same noise on both
servers, so functions and plane masks show. Groups: all 16 functions x two
plane masks x widths 0 and 3; fill styles 0-3 x three tile and stipple
origins x copy and xor x widths 0 and 5; line styles x widths 0, 1, 2, 5,
9 x four caps x three joins x two dash offsets; clip rectangles in each
ordering and at two origins, and a clip mask at three origins; the 16
functions on a depth-1 pixmap; depths 8, 16 and 32; and a window with a
mapped child and grandchild under both subwindow modes, read back whole.

A depth-24 pixel travels in 32 bits whose top byte the protocol leaves
undefined, and the two servers disagree there (Xorg leaves an inverting
function's high byte set). Only the drawable's depth is compared.

## Finding and resolution

Before the fixes: 153 mismatches in the functions group, 47 in fills and
64 in lines; clips, depth 1, depths 8, 16 and 32, and windows matched
exactly. Three classes:

**t202, PolyText ignored the fill style (fixed).** Glyphs were painted
solid in the foreground; Xorg paints PolyText through the tile or stipple
like any other request. Under a non-solid fill style the glyph ink is now
gathered as spans and painted through the stroke path's pattern.
ImageText stays solid, as the protocol fixes it. Density replay held no
pattern pixels either, so a tiled or stippled fill, line or PolyText
replayed solid at other densities; such a command now poisons the journal
with `UnsupportedFillPattern`, as a clip mask does.

**t203, dashed zero-width lines and arcs (fixed).** They were drawn as the
wide dash at width one. Xvnc draws with `fb`: `fbZeroLine`/`fbSegment`/
`fbBresDash` for lines, which is `miZeroLine`'s Bresenham with the dash
stepped per pixel, the offset carried across a polyline's joints and only
the last segment drawing its end point; and `miZeroArcDashPts` for arcs,
which `fbPolyArc` hands every dashed thin arc. Both are ported in
`software/geometry/zero_line/dash.rs`. A zero-width PolyRectangle now keeps
each pixel's last paint rather than its first: the protocol only says a
pixel is painted once, and the last is the colour Xorg leaves where a
dashed outline closes over its start.

**t204, a zero-width outline painted twice by Xorg (declared).** Where a
thin PolyRectangle retraces pixels -- the degenerate rectangle of zero
width, and under a fill pattern the corner a closed outline returns to --
Xorg's `mi` path paints them twice, so under a non-idempotent function
(xor, andReverse, nor and the rest) its result differs from painting once.
The protocol says "for any given rectangle, no pixel is drawn more than
once", and Sophia keeps it; Xorg's own solid fast path paints once too.

After the fixes every group matches Xvnc except t204's 25 cases.

## Validation

Unit tests pin `fb`'s dash walk (phase across a joint, the background of a
double dash, an offset, an odd list); wire tests pin PolyText through a
tile and solid text under FillSolid; the raster fallback test pins a tiled
paint refusing replay. The core profile, x11bench 60/0 and the XTS lines,
rectangles, arcs, points, fills and images scenarios pass on the fixes.
