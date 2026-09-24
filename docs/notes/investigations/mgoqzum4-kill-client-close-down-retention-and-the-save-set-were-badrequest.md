---
id: mgoqzum4
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, core-protocol, lifetime, xts]
---
# KillClient, SetCloseDownMode and ChangeSaveSet were BadRequest, and a departing client always took its windows with it

## Question

Of the twenty-one core opcodes XTS5 named as BadRequest (t166), three act
on the connection itself rather than on a resource: KillClient (113),
SetCloseDownMode (112) and ChangeSaveSet (6). Teardown here was one path:
the departing client's range was released, deepest window first, whatever
the client had asked for. What did each owe, and what did serving them
require of the socket layer, which owns leases and disconnects, rather
than of the runtime?

## Evidence

The teardown in `x11_socket/connection/dispatch.rs` released the range
through `release_client_resource_range` unconditionally, then retired
selections, the device bundle and pointer state. No lease recorded a
close-down mode; nothing remembered a range after its client left, and a
range is never reused, so nothing could name one later. No save-set
existed: a window manager's frame took the client window down with it when
the manager died, which is the failure the save-set was designed against.
ReparentNotify (21) did not exist as an event; ReparentWindow emits none
either, and the routing registry's parent map was not updated by it.
Disconnecting another client existed for input recovery
(`input_recovery.disconnect`), which shuts the target's socket so its own
thread runs its own teardown.

In the last run before this seam (`.artifacts/xts-xproto/run-t166b/`):
KillClient 1 `wanted NOTHING, got ERROR - BadRequest`, and the same for
SetCloseDownMode 1 and ChangeSaveSet 1; the purpose-2 halves read the
BadRequest where BadLength was wanted.

## Finding and resolution

- SetCloseDownMode: the mode is recorded on the requester's lease
  (Destroy, RetainPermanent, RetainTemporary; anything else BadValue). At
  teardown Destroy is the path as it was. A Retain mode skips the range
  release and records the range as retained, permanent or temporary
  (`retain_client_range`); selections, the device bundle and pointer state
  still end with the connection, as the reference ends them. Client atoms
  are forgotten only when no client remains and no range is retained.
- KillClient: the resource's owner, if connected, is disconnected through
  the input-recovery path and its own teardown frees its range under its
  own close-down mode; a resource in a retained range frees that range now,
  through the same release the teardown uses, with its DestroyNotify and
  selection clearing routed as a departure's are; AllTemporary (0) frees
  every retained temporary range; a resource nobody holds is BadValue
  carrying it.
- ChangeSaveSet: the window must exist (BadWindow) and belong to another
  client (BadMatch for the requester's own). The set lives on the
  requester's lease. When the requester departs, before its range is
  destroyed, each saved window still alive and outside the range is given
  to its nearest ancestor outside the range (the root when none), and
  re-mapped if it was mapped: UnmapNotify, ReparentNotify (21, new) and
  MapNotify go to whoever selected on it (`apply_save_set`,
  `route_x11_save_set_reparents`); the routing registry's parent map
  follows (`update_window_parent`), so later hierarchy events find the
  right parent.

`tests/x11_wire/client_lifetime.rs`, both byte orders: a peer killed by
its window id reads EOF and its window answers BadWindow to a third
client while the killer's round trip completes, and an id nobody holds is
BadValue; a RetainPermanent client's window outlives its connection,
answering GetWindowAttributes, until a KillClient on it, and a
RetainTemporary one until KillClient(AllTemporary); a manager-like client
reparents a peer's window under its frame, saves it and departs, and the
peer's window is back under the root and mapped. Red on master (three
BadRequests, and the window gone with the manager); green after. Three
probe cases (`kill_client`, `set_close_down_mode`, `change_save_set`)
enter the core profile and the three opcodes enter the inventory.

XTS (`.artifacts/xts-xproto/run-t166c/`): KillClient 1 and 2,
SetCloseDownMode 1, ChangeSaveSet 1 and 2 retire; SetCloseDownMode 2 joins
the suite's own UNTESTED (a four-byte request cannot be made shorter under
BIG-REQUESTS); nothing else moves: 335 passed and 54 declared.

Left open as t184: ReparentWindow itself emits no ReparentNotify and does
not update the registry's parent map, so a window manager that reparents
and then selects on the parent sees the wrong ancestry until this seam's
`update_window_parent` and the new event are wired into that request too.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` and `sophia-session` suites and clippy under
      the gate's isolation.
- [x] The core profile reads PASS, 144 of 144, the three cases in both
      orders.
- [x] XTS: five rows retire, one joins the suite's own UNTESTED.
- [ ] The gate on the committed candidate, both scenarios.
- [ ] The rest of t166: the input maps.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the rows this retires.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the decisions, opcode by opcode.
- [Four hierarchy requests were BadRequest](1lyul4om-four-hierarchy-requests-were-badrequest-and-querytree-listed-children-in-creation-order.md) --
  the previous group of t166.
