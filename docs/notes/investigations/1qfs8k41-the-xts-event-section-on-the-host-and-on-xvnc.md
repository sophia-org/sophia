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

## Selected by direction

The next rerun after the fan-out stood still at 78: the purposes that
wanted an event withheld (ButtonPress 4 and 6, ButtonRelease 3, KeyPress 3,
KeyRelease 3, each reporting `Got 1 unexpected events`) were still fed.
Two rules in the connection's selection table looked at one combined mask
where the protocol has two: `keyboard_delivery` asked for KeyPress or
KeyRelease together, so a client that had selected releases alone was
written the press, and `selected_pointer_target` did the same for buttons.
And the implicit grab a press activates was recorded for the surface's
owner with a mask of every event, so the input writer took it for the
owner's own grab and let the press through on the grab's terms instead of
the window's; only an explicit grab's mask decides delivery now. The
readiness wait that holds a key for a client still installing its masks
asks whether any keyboard selection decides the path, not the direction's
own, so a client that chose one direction is not held five seconds on the
other. The rerun read 83 passed; the five moved and nothing else did. Red
before the fix: `a_press_the_owner_did_not_select_is_not_written_to_it`
and `a_key_press_reaches_only_the_clients_that_selected_presses` in
`tests/x11_wire/xtest_admission_socket.rs`.

What the implicit grab is still not is the reference's: it belongs there
to the client the press was delivered to, with that client's selection as
its mask, and other clients hear nothing while it lasts (t230).

## KeymapNotify, and a crash the stray motion had hidden

The rerun without the stray motion read KeymapNotify 1 UNRESOLVED where it
had read FAIL: the suite's binary died of a SIGSEGV. Its check walks the
events after each warp expecting an EnterNotify then a KeymapNotify, and
when the EnterNotify is the last event it reports `Missing %s event` with
the event's type number as the string, which is the crash. Before, a
MotionNotify nobody had selected followed every EnterNotify and the branch
was never reached. A KeymapNotify is owed after every EnterNotify and
FocusIn to the clients that selected KeymapState on the window (t211's
item), and it is written now on both paths: the input writer's crossing,
and the FocusIn of the focus records and of the protocol routing pass,
which is the path a client's own SetInputFocus takes. It carries the keys
down as QueryKeymap reports them, less the bitmap's first byte. Red before
the fix: `a_keymap_notify_follows_an_enter_notify_and_a_focus_in`, which
crosses between two windows of one client, because a motion onto the root
reaches no writer of the client the pointer left and the return then
crosses nothing this layer can see; that gap stays with t211. The rerun,
made beside the full test suites, read 84 passed: KeymapNotify 2 moved and
KeymapNotify 1 read FAIL instead of a crash, `No events received` on four
of its five warps. The gate's own run on a quiet machine then passed it,
every warp answered with its EnterNotify and KeymapNotify before the XSync
reply: the load-only miss is the ordering race of t229, and the scenario is
declared from the gate's journal at 85 passed.

## The barrier that ended too early

KeymapNotify 1 then read PASS under one gate and FAIL under the next, on
one host build, `No events received` on some or all of its five warps. A
stderr trace in the host showed the order: the connection's FakeInput
wait printed `settled` before the input writer printed its write of the
EnterNotify. The barrier a FakeInput (and a WarpPointer from a client that
may inject, which becomes one) waits on is raised by the broker when the
routing is done, with the event queued for the writers and not yet
written; the next request, the XSync's GetInputFocus, was then read and
answered, and the suite found nothing pending after the reply. Loaded or
not decides only how often. This is t229's race with a face: the
injecting connection now takes a mark of what the registry has queued for
it once the routing is done and does not read on until its own writer has
drained to the mark (a watermark shared by the registry's senders and the
connection's writer, bounded at one second because the writer may be
parked on the keyboard readiness wait). A reply on any other connection,
and any physical event, is still unordered against the writer; that is
the general seam and stays with t229. Red before the fix, on a fraction
of its rounds: `an_injections_events_precede_the_reply_to_the_next_request`.

## The child field

Every core device event was written with `child` None. The protocol fills
it with the child of the event window on the way to the source: the source
itself when it is a child of the event window, the ancestor of the source
that is a child of the event window when the source is deeper, and None
when the source is the event window or not an inferior of it (as under a
grab elsewhere). The encoders cannot know it; the input writer can, from
the source window's ancestry it already walks to choose the event window,
and for a key from the pointer window's. It patches the record's bytes 16
to 20 after encoding, as it does the sequence. Ten purposes moved
(ButtonPress 8 and 9, ButtonRelease 5 and 6, KeyPress 5 and 6, KeyRelease
5 and 6, MotionNotify 15 and 16), the scenario reads 95 passed, and nothing
else changed. Red before the fix:
`a_core_events_child_is_the_event_windows_child_toward_the_source`.

## Propagation past the surface window

The owner's walk from the source window up stopped at its own toplevel,
the surface window, so a client that had selected a press on the root and
nothing on its windows heard nothing from a press in them, while a peer
that selected on the root did (the fan-out walks the registry's ancestry
to the root). The walk now goes on to the root through the connection's
own table, stopped by a do-not-propagate mask or a selector, as the
protocol has it; ButtonPress 7 and ButtonRelease 4 pass and the scenario
reads 97. What stays open on t220 is that the owner's walk and the
fan-out decide separately: a peer selecting on the toplevel should stop
the owner's own delivery on the root, and does not yet. Red before the
fix: `a_press_propagates_to_the_root_and_stops_at_do_not_propagate`.

## Wheel buttons are buttons

XTEST dropped a FakeInput press of button 4 to 7 as a wheel step the
injector had no way to carry, so a motion with button 4 held never carried
Button4Mask and never reached a Button4Motion selector (MotionNotify 6
and 7). To the core protocol a wheel button is a button: pressed and held
until released, with its own state bit and motion mask. The XTEST plan now
gives buttons 4 to 7 evdev codes of their own above the device space, and
the pointer mapper holds them like any button; a device's wheel is an axis
and never arrives that way. The scenario reads 99. MotionNotify 13 is the
suite's own: purposes 10 and 11 end unresolved with Button1 pressed and
never released, so 13 finds Button1Mask where it expects nothing held, on
Xvnc as on the host. Red before the fix:
`an_injected_wheel_button_is_held_like_any_button`.

## The protocol's crossings

The crossing model was one EnterNotify of detail Nonlinear on the window a
motion was reported on, and nothing at all when the pointer left a
client's windows: the input writer compared the window it had last
reported on with the one it was reporting on now, and a motion over the
bare root went to the focus or nowhere. The writer now tracks the window
the pointer is in, this table's view (the deepest window under the
pointer, the root when the point is outside the surface window), and on a
change generates the protocol's crossings from the two windows' ancestries:
`to` an inferior of `from` (leave `from` as Inferior, enter the windows
between as Virtual, enter `to` as Ancestor), `to` an ancestor of `from`
(leave `from` as Ancestor, leave the windows between as Virtual, enter `to`
as Inferior), or through the nearest common ancestor, which hears nothing
(Nonlinear at the ends, NonlinearVirtual between); every leave precedes
every enter, each carries the child toward the pointer and coordinates in
its own window, and a KeymapNotify follows each written EnterNotify. A
pointer injection over no toplevel goes to the last toplevel's client, so
that client sees the pointer leave to the root. Eleven purposes moved
(EnterNotify 4, 7 to 9, 13; LeaveNotify 4, 5, 8 to 10, 15) and the
scenario reads 110, then 112 with three more: a window destroyed under
the pointer leaves it in the root for the next crossing (EnterNotify 3), a
peer that selected only crossings on the owner's window is told of the
motion so its writer generates them (KeymapNotify 3), and the crossing's
focus flag follows the focus projection, which the suite's EnterNotify 12
and LeaveNotify 14 still read as set after the focus was moved to another
window. What stays with t211: those two, a move between two clients'
windows, where the registry routes nothing to the client the pointer left;
the crossings a map, unmap or reparent under the pointer owes; and the
visibility purposes. Red before the fix:
`a_pointer_move_generates_the_protocols_crossings`.

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
(112 passed, 83 declared after the selection-by-direction, KeymapNotify,
subwindow, propagation, wheel-button and crossing reruns; 60 and 135 at
the section's first declaration) from `xts_reasons_events.json`,
every authority row naming its task, run under the gate with
`--xts-admit-xtest=yes`. Each seam that lands re-declares it.
