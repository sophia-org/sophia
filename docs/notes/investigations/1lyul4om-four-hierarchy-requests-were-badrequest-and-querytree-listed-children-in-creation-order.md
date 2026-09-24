---
id: 1lyul4om
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, core-protocol, windows, xts]
---
# Four hierarchy requests were BadRequest, and QueryTree listed children in creation order

## Question

Of the twenty-one core opcodes XTS5 named as BadRequest (t166), four act on
the window tree or the active grab: UnmapSubwindows (11), CirculateWindow
(13), RotateProperties (114) and ChangeActivePointerGrab (30). The state
they act on exists -- a stacking rank per window, a property table, an
active grab record -- so what did each owe, and what did serving them show?

## Evidence

MapSubwindows was served (`map_direct_subwindows`) and UnmapWindow was,
with UnmapNotify; nothing unmapped every child at once. `XWindowTable::restack`
existed for ConfigureWindow's stacking modes, and `direct_children_bottom_to_top`
read the ranks, but QueryTree reported `direct_children`, which is creation
order, so a restack was invisible to the one request that lists siblings.
The property table had change, get and remove but no rotate. The active
grab record kept its event mask, and nothing rewrote it. No CirculateNotify
or CirculateRequest existed.

## Finding and resolution

- UnmapSubwindows: every mapped child, top to bottom in stacking order as
  the protocol orders it, each through the path UnmapWindow takes, with
  its UnmapNotify; a child already unmapped is skipped.
- CirculateWindow: the lowest child occluded by a mapped sibling above it
  goes to the top (RaiseLowest), the highest occluding one to the bottom
  (LowerHighest), occlusion being the siblings' rectangles meeting;
  CirculateNotify (26, new) to the child and to its parent's
  SubstructureNotify selectors, filtered by selection like its siblings. A
  client selecting SubstructureRedirect on the parent is asked instead,
  with a CirculateRequest (27, new) naming the child, at the socket layer
  where MapRequest is decided; nothing moves. No Expose: windows are
  retained surfaces here, and raising one uncovers nothing that was lost;
  the suite's purpose passed without it.
- QueryTree now lists children bottom to top in stacking order, which the
  protocol requires and a circulate or restack makes differ from creation
  order.
- RotateProperties: the named properties' values move delta places along
  the list (`XPropertyTable::rotate`), a PropertyNotify per moved value; a
  property missing or named twice is BadMatch and nothing moves, an atom
  nobody interned BadAtom, an Engine-owned property BadAccess.
- ChangeActivePointerGrab: the event mask of an active grab the requester
  holds is rewritten (`change_active_pointer_grab`); without one the request
  has no effect, as the protocol says; a mask bit outside the pointer
  events is BadValue carrying the mask; the cursor is validated and not
  applied, cursor display being config-driven here.

`tests/x11_wire/hierarchy_requests.rs`, both byte orders: two overlapping
children, the raise, QueryTree agreeing, a second raise, the unmap top
first and nothing to unmap twice; three properties rotated by one with
three notices and the values moved, a missing property refused; a grab
change without a grab answering nothing and a bad mask BadValue. An
in-process test shows a peer's request leaves the holder's mask alone. Red
on master, green after. Four probe cases enter the core profile, the
circulate one exercising the redirect with a second client, and the four
opcodes enter the inventory.

XTS (`.artifacts/xts-xproto/run-t166b/`): all eight rows of these cases
retire and nothing else moves, 330 passed and 59 declared.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] The core profile reads PASS, 138 of 138, the four cases in both
      orders; the gate on the committed candidate
      (`.artifacts/x11-profile-c7997078-{selected-core,xproto}/`) reads
      PASS on both scenarios, xproto with 330 passed and 59 declared.
- [ ] The rest of t166: connection lifetime and the input maps.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the rows this retires.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the decisions, opcode by opcode.
