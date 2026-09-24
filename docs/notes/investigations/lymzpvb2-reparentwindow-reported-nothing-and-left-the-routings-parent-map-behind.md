---
id: lymzpvb2
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, core-protocol, windows, events]
---
# ReparentWindow reported nothing and left the routing's parent map behind

## Question

t184, left open by the save-set seam (t166-c): ReparentWindow moved a
window in the runtime but emitted no ReparentNotify, and the routing
registry's parent map -- what names the SubstructureNotify recipients of a
window's later map, unmap, circulate and destroy notices -- kept the parent
the window was created under. What did a client selecting on the window or
on either parent miss, and what did the stale map misroute?

## Evidence

The dispatch arm (`dispatch/core/windows.rs`) called `set_window_parent`
and placed the window, and returned no outputs. The save-set path built
the event (`ReparentNotify`, 21) and `update_window_parent` for its own
use, so a window given back to the root by a departing manager was
reported and re-mapped correctly, while the same move made by a request
was silent. The wire test's first run: a watcher selecting StructureNotify
on the window and SubstructureNotify on both parents read nothing for five
seconds after the reparent, and a destroy that followed was reported with
the root, not the frame, as the parent.

## Finding and resolution

- The runtime gains `reparent_window`, which performs the request as the
  protocol orders it: a mapped window is unmapped, given its new parent and
  position, and mapped again; the outcome carries the old parent, whether
  it was mapped, its override-redirect flag and the surfaces the Engine
  applies, in that order.
- The dispatcher reports it with one record per window the notice belongs
  to: the ReparentNotify addressed to the window (StructureNotify), to the
  old parent and to the new parent (SubstructureNotify on each), and for a
  mapped window the UnmapNotify addressed to the window and the old parent
  before, and the MapNotify addressed to the window and the new parent
  after -- the parent-addressed form a destroy already used.
- The routing pass and the requester's local filter deliver a map, unmap or
  reparent by the window it names: StructureNotify when it names the window,
  SubstructureNotify when it names a parent; and for a window reparented in
  the batch they derive no parent copy of their own, since only one parent
  could be named there. Once the notices are out, the registry's parent map
  follows the reparent.

`tests/x11_wire/reparent_notify.rs`, both byte orders: a watcher selecting
on the window, the root and a frame reads three ReparentNotify copies, one
per window it selected on, with the new parent and position; QueryTree
agrees; a destroy afterwards reaches the frame's watcher and not the root's.
A mapped window reparented reads UnmapNotify, ReparentNotify, MapNotify in
that order. Red before, green after.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] The core profile reads PASS, 154 of 154.
- [x] The gate on the committed candidate
      (`.artifacts/x11-profile-4b67af5e-{selected-core,xproto}/`): both
      scenarios PASS, xproto with 339 passed and 50 declared, selected-core
      with 76 passed and 23 declared.

## Connections

- [KillClient, SetCloseDownMode and ChangeSaveSet were BadRequest](mgoqzum4-kill-client-close-down-retention-and-the-save-set-were-badrequest.md) --
  where the event and the map update were first built, and the row filed.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the coverage this completes for ReparentWindow.
