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

## The rerun hung, and why

With XTEST admitted and the warp repaired, every case that moved the
pointer read NORESULT: TET killed the client after its two-minute
allowance, each time right after "Generate ... event". A raw wire
client against the host reproduced it in one request: after a WarpPointer
(or a plain XTEST motion) the next QueryPointer never answered. The
conformance host's main loop accepted connections and reaped workers but
never called the broker's `route_pending`, nor shared the broker's input
authority with the runtime, which the production service and the routed
test fixture both do. An injection armed a completion nobody could
produce, and the injecting client waited on it for ever. The private
session host, which the XTEST probe profile exercises, drives routing;
the conformance host never had an injection completed until now. Fixed by
running the host on `run_x_server_frontend_routed_until_stopped`.

Unhung, the same client saw its pointer move (QueryPointer agreed) and
nothing delivered: an XTEST button or motion was planned against the
*focused* surface, and with none it was dropped, because which toplevel a
point falls in is the Engine's answer and the authority does not invent
one. On a host where clients place their own toplevels
(`with_client_toplevel_placement`, t189) the authority is the one that
placed them, so it now resolves the topmost viewable toplevel containing
the point (`client_placed_toplevel_at`) and plans pointer events against
it; a suite's plain, unfocused window is told of a press over it. In a
session nothing changes: there the Engine puts the focused surface under
the pointer.

## After the repairs

The rerun with the host unhung, pointer events resolved under the pointer
and keys under PointerRoot: 60 PASS, 64 FAIL, the rest the suite's own.
What stays red is device-event routing itself, not injection: a second
client selecting on the same window gets nothing, an unselected event
does not propagate to the ancestor that selected it, `subwindow` is not
filled, motion with a button held is not reported to Button<n>Motion
selectors, and a key carries no pointer coordinates (t220); crossing and
keymap events from hierarchy changes and visibility from occlusion
(t211); gravity (t199) and ColormapNotify (t210). Redirection (t198)
followed on the same branch: ConfigureRequest to the client managing the
parent, ResizeRequest to a ResizeRedirect selector, as MapRequest already
was.

## The windows scenario, in the authority's area

The pane ran Xlib4 and Xlib5 (408 purposes) the same way and handed over
the classes in this area. Read against Xvnc, the host-only rows were:
the redirect and border-width families above (t198, t221); a zero width
or height accepted where the reference answers BadValue, and a sibling
refusal that read BadWindow where the protocol says BadMatch (t222,
t223); ConfigureWindow on the root answered BadWindow instead of doing
nothing, an unknown window with a zero size answered BadValue before
BadWindow, an InputOutput child of an InputOnly parent was created, an
InputOnly window reported depth 24, and the attribute reply's
backing-planes read 0 where the default is all ones (t224); DeleteProperty,
SetSelectionOwner and ConvertSelection accepted atoms the table did not
know, ConvertSelection accepted a requestor that named no window, a
SetSelectionOwner earlier than the last change took the selection, and
RotateProperties moved values the other way round from Xorg (t225); and a
selection taken from one connection with another client's window
survived that connection's departure, because ownership was cleared by
window rather than by the client that took it (t226). After those the
host passes 214 of the scenario's purposes where Xvnc passes 277; what
remains in this area is border geometry (t227, coupled to the raster
side) and the pane's attribute, gravity and pixel work.

## Status

The repairs are on `xts-events/t196` with wire tests that were red on the
tree before them, and the scenario is declared: `xts_expected_events.json`
(60 passed, 135 declared) from `xts_reasons_events.json`, every
authority row naming its task, run under the gate with
`--xts-admit-xtest=yes`. Each seam that lands re-declares it.
