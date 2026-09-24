---
id: 1kdl1945
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, core-protocol, windows, placement, xts]
---
# The conformance host kept a mapped toplevel where a policy would have put it

## Question

Found by the x11bench pane while finishing t181: the root halves of XTS's
arcs purposes (/Xlib9/XFillArc 28, XFillArcs 30, XDrawArc 81, XDrawArcs 91)
failed with `Pixel mismatch at (50, 20)` and `Drawing on root window with
IncludeInferiors gave incorrect results`. Each test moves its toplevel to
(0, 0) before drawing on the root, and the wire repro showed the move
refused: a toplevel created at (100, 50), mapped, then
`ConfigureWindow x=0 y=0` was answered with a synthetic ConfigureNotify at
(100, 50), and GetGeometry still read (100, 50). Whose placement was that?

## Evidence

`client_controls_window_geometry` (`runtime/windows.rs`) answers false for
any viewable window whose presentation role is PolicyManaged -- every
non-override-redirect child of the root -- and the ConfigureWindow arm then
keeps the geometry and reports it with `synthetic: !client_controls`,
which is what a redirecting window manager answers a client with. That is
right in the live session: in its native mode (`LivePolicyMapMode::Direct`)
the Engine owns placement and the policy map is not deferred, so this
refusal is the only thing that keeps a client from moving a toplevel the
layout placed. But the refusal did not depend on a policy being present.
The conformance host runs no window manager at all, and the suites it
serves assume the reference server without one, where a client's move of
its own toplevel simply takes effect.

## Finding and resolution

The frontend config gains `with_client_toplevel_placement`, off by
default: the state passes it to the runtime as the other placement modes
are passed, and `client_controls_window_geometry` answers true for every
window when it is on. The conformance host turns it on, with the reason in
place. Nothing changes for a session with a policy: the option is never set
there, and the wire test keeps the refusal on record.

`tests/x11_wire/toplevel_placement.rs`, both byte orders: with the option
a mapped toplevel created at (100, 50) moved to (0, 0) reports a real
ConfigureNotify at (0, 0) and GetGeometry agrees; without it the same
request answers a synthetic ConfigureNotify at (100, 50) and the placement
is kept. Red before (the no-manager case answered the synthetic notice),
green after.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] The core profile reads PASS, 154 of 154.
- [ ] The gate on the committed candidate, both scenarios.
- [ ] The arcs scenario's root halves, rerun by the x11bench pane on the
      merged master.

## Connections

- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the coverage the arcs purposes belong to.
- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  what the suite assumes of its server.
