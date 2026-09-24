---
id: 1qfs8k41
date: 2026-09-24
kind: investigation
status: open
tags: [investigation, x11, xts, events, input, conformance]
---
# The XTS event section on the host and on Xvnc

## Question

XTS5's Xlib11 section is the core protocol's events: thirty-three cases,
195 purposes, one per event type and per rule about who is told. The
adapter had run only the wire tests (Xproto), nine core cases and the
drawing sections. What does the event section say about the authority,
and which of its verdicts are the suite's own?

## Method

`xts_select.py --scenario events` over every case of `xts5/Xlib11`, run
through `xts.py` twice with the same manifest: on the conformance host,
and on TigerVNC's Xvnc 1.16.2 through a wrapper that starts it on the
adapter's display (`.artifacts/xts-events/`). Xvnc is the oracle for what
the suite can pass at all; a purpose both fail is the suite's, one only
the host fails is the authority's.

The first host run read every XTEST-driven purpose UNTESTED: the suite
probes the extension with `XQueryExtension` (`src/lib/extenavail.c`), and
the host advertises XTEST only when started with `--admit-xtest`, which
`xts.py` never passed. It now takes `--admit-xtest` and starts the host
with it; the profile gate's scenarios (all wire and drawing tests) do not
need it.

## Evidence

Xvnc: 121 PASS, 43 NOTINUSE, 22 UNSUPPORTED, 4 UNTESTED, 3 FAIL
(ButtonPress 10, and two MotionNotify purposes the suite itself reports
as "Path check error"), 2 UNRESOLVED. The host, XTEST admitted, before any
repair: 47 PASS and 76 FAIL. Every host-only failure fell into one of
these:

1. **WarpPointer moved nothing but QueryPointer's answer.** Every input
   purpose warps the pointer into its window before injecting a button, a
   key or motion; the injected event landed at the origin, on no window.
   ButtonPress 1-9, ButtonRelease 1-7, KeyPress 1-7, KeyRelease 1-7,
   MotionNotify 1 and 3-9, KeymapNotify 1 all read "Expected event, got
   none". Repaired: an accepted WarpPointer from a client that holds an
   XTEST injector is submitted through that injector as an absolute
   motion, so the routed pointer follows and the motion and crossing
   events a warp owes are the ones any motion owes. A client without an
   injector still gets what it got: the pointer is the Engine's, and a
   client that may not inject may not move it. A warp that lands where
   the pointer already is, or that the protocol makes a no-op (source
   rectangle not containing the pointer), reports nothing.
2. **InputOnly windows were exposed.** Mapping one reported
   VisibilityNotify and an Expose of its whole extent (Expose 1,
   VisibilityNotify 1). Repaired in the map paths: an InputOnly window
   changes map state and shows nothing.
3. **A saved window that was unmapped stayed unmapped.** The save-set walk
   re-mapped only what had been mapped, and skipped a window it had no
   reason to reparent (MapNotify 1). The protocol maps every unmapped
   save-set window, "even if it was not an inferior of a window created
   by the client". Repaired: the walk leaves every saved window mapped,
   and the router reports MapNotify (with VisibilityNotify and Expose
   when it became viewable) after the reparent events, if any.
4. **Events the authority does not generate yet**, each its own task:
   ColormapNotify on XSetWindowColormap (ColormapNotify 1);
   ConfigureNotify for a stacking change alone (ConfigureNotify 1-2);
   ConfigureRequest and ResizeRequest redirection (ConfigureRequest 1, 3,
   6; ResizeRequest 1); GravityNotify from window gravity on a parent's
   resize (GravityNotify 1-2); VisibilityNotify state transitions from
   occlusion (VisibilityNotify 2-3, 7-9); EnterNotify and LeaveNotify from
   a hierarchy change under the pointer, with their subwindow, detail and
   focus fields (EnterNotify 1, 3-4, 7-9, 12-13; LeaveNotify 1, 4-5,
   8-10, 14-15); KeymapNotify after EnterNotify and FocusIn (KeymapNotify
   1-3). Which of the crossing purposes stay red after the warp repair is
   read from the rerun below.

## Status

The three repairs are on `xts-events/t196` with wire tests that were red
on the tree before them. The scenario is not declared yet: the rerun
after the repairs decides the reasons file, and the remaining families
are filed as tasks with their purposes named.
