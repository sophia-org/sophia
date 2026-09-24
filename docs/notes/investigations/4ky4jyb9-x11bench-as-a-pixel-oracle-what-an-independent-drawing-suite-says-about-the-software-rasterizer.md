---
id: 4ky4jyb9
date: 2026-09-23
kind: investigation
status: investigating
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

Nothing here is a gate yet. t173 makes it one: references
generated on a pinned Xvnc at the host's screen geometry, retained in the
repository with their generator's identity; the run bounded and confined as
XTS5 is; and each non-PASS declared with a reason, as t164 made possible for
XTS5 manifests. Zero-width arcs are declared device-dependent and the three
restack tests declared as authority policy. AGENTS.md rule 11 asks for the
adapter in Rust/xtask; x11bench itself stays an independent C++ client.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md):
  the gate, confinement and declared dispositions this should reuse.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md):
  every drawing request is decoded; this note is about what they draw.
- `tools/probes/x11_conformance/drawing_cases.py`: Sophia's own pixel
  assertions, from the specification rather than from another server.
