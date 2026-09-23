---
id: urxcuj5s
date: 2026-09-23
kind: investigation
status: investigating
tags: [investigation, x11, input, xtest, selection]
---
# An implicit pointer grab delivers by position, not to the window that took the press

## Question

A drag that starts inside xterm's text widget and releases past the window's
edge never makes xterm claim PRIMARY. Where does the release go?

## Evidence

Found while running t124's drag-and-paste under Hagia (2026-09-23). Hagia had
tiled xterm B beside xterm A, and the driver's drag, aimed with A's stale
geometry, ran off A's right edge into B. Two layers were at fault, and only
the first is fixed here:

1. **The session's XTEST seam had no grab at all.** `LiveXTestPointerScene`
   hit-tested every motion and button, so once the pointer crossed into B
   the release was delivered to B's surface. Fixed in the same change as this
   note: the scene holds the pressed surface and its origin until the last
   button comes up (`route_button` / `route_motion`), pinned by
   `a_synthetic_drag_stays_on_the_pressed_surface_until_release`.
2. **The frontend still chooses the window inside the surface by position.**
   With the session grab in place, the no-WM gate was run with the release
   24 px past A's right edge (`xtest_selection_driver --overshoot`). The
   session delivered press, motions and release to A's surface, the log
   shows `injected_buttons=2`, and xterm still claimed nothing:
   `owner_changes=0`, `status=fail reason=primary_unowned`
   (`.artifacts/xtest-selection/2229c520-dirty-*/pass.log`, the run before
   the overshoot became opt-in). The only difference from the green run is
   the release position.

Reading `routing/registry/delivery.rs` (`InputEventKind::PointerButton`):
on a press with no grab active, `activate_button` opens an implicit grab
whose `window` is `surface_route.window` -- the surface's top-level -- with
`owner_events: true`. Every later event therefore sets `target_window` to the
top-level, and the writer (`connection/writers/input.rs`,
`pointer_event_target` / `selected_pointer_target`) descends from there to
the deepest mapped child **containing the event position**. Inside the text
widget that is the widget; past its edge there is no such child, the release
is delivered to the top-level shell window, which does not select
ButtonRelease, and xterm's `SelectEnd`, which is what calls
`XSetSelectionOwner`, never runs.

The core protocol says otherwise. An implicit grab's grab-window is the
window the press was *delivered to* -- the deepest window selecting
ButtonPress, xterm's VT100 widget -- with owner-events false, and until the
last button is released every pointer event is reported to that window,
positioned relative to it, wherever the pointer is. The authority's
`XActiveInputGrab` has the fields for this; the registry fills them with the
surface rather than the delivered window, which it cannot know at that
point, because the writer picks the delivered window later.

This is not XTEST-specific. The physical path reaches the same registry and
writer with the same surface-level target. A physical drag that ends past
the window edge -- the ordinary way to select to the end of a line -- would
lose its release the same way. It is a candidate for t124's original
report, and the one piece of that report the headless runs cannot settle.

## Finding and resolution

Open. The repair is in the frontend: the implicit grab must be opened on the
window the press is delivered to, with owner-events false, and while it is
active the writer must report to that window with coordinates relative to
it rather than descending by position. `input_authority.rs` already
distinguishes implicit from passive grabs (`pointer_implicit`) and releases
on the last core button, so the state exists; the delivered window has to
reach it. This changes X protocol delivery semantics and needs its own wire
red/green in `crates/sophia-x-authority/tests/x11_wire/`: press in a child,
release outside it, the release arrives at the child with its coordinates.

## Validation and remaining work

- [ ] Wire test: press inside a selecting child, release outside its bounds
      but inside the top-level; release delivered to the child. Red today.
- [ ] Same, release outside the top-level altogether (over the root or
      another surface); the session grab keeps it on the surface, the
      frontend must keep it on the child.
- [ ] `xtest_selection_driver --overshoot` goes green through the headless
      gate; consider making it the gate's default drag once it does.
- [ ] `xterm_pointer_oracle --overshoot` goes green: today xterm reports the
      drag to its last cell and never the release (`release_lost`), which is
      this defect seen from the client
      ([vm14kz5r](vm14kz5r-xterm-as-an-oracle-for-what-the-frontend-delivers-to-a-widget.md)).
- [ ] Re-read t124: if the operator's failing drags released past the edge,
      this is the physical answer.

## Connections

- [PRIMARY selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  the report this was found under.
- [XTEST pointer events target the focused window, not the window under the pointer](5pmnf6ie-in-a-session-with-no-window-manager-only-the-first-window-to-map-is-routable.md) --
  the seam whose grab is fixed alongside this note.
- [An XTEST button is delivered at the screen origin](csiz9c9x-an-xtest-button-is-delivered-at-the-screen-origin-not-where-the-pointer-is.md) --
  the same drag, one defect earlier.
