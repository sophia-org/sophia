---
id: 5pmnf6ie
date: 2026-09-22
kind: investigation
status: investigating
tags: [investigation, input, session, placement]
---
# XTEST pointer events target the focused window, not the window under the pointer

(Filed first as "in a session with no window manager only the first window to
map is routable". That title described the symptom and guessed the wrong
cause; the path and id are kept so links hold. See *Corrected* below.)

## Question

In a headless no-WM session two xterms map, but an XTEST-driven pointer
resolves to only one of them anywhere on the screen. Is the second window
routable at all, and if not, why?

## Evidence

Found under t124 while driving two real xterms by XTEST
(`xtest_selection_driver`, run as the `--client` of
`sophia session run --display=:90 --no-input --admit-xtest`). The driver moves
the pointer with XTEST motion and reads `QueryPointer`'s `child`, which the
frontend takes from the pointer snapshot routed input sets
(`event_state.rs::query_pointer`), not from the X window tree. On failure it
sweeps the root and records where each child resolves.

- **Both spawned at once:** in 5 of 7 runs xterm A was reachable nowhere; only
  B resolved. The session's single `focus_applied` named whichever xterm mapped
  first, and only that one was ever under the pointer.
- **A spawned first, B after A is routable:** A resolves over its X frame; B,
  mapped at `664x188+40+320`, resolves nowhere, run after run.

Logs are retained under `.artifacts/t124-xtest-selection-gate/`.

## Corrected, 2026-09-22

The first reading -- that a no-WM session never routes a second window, which
would affect the native, standalone and kitty profiles -- is **withdrawn**. A
trace of the code shows the cause is XTEST's own targeting:

- `x11_socket/connection/xtest.rs::plan()` targets **motion and buttons** at
  the **focused** window's surface, and computes their local position relative
  to that window.
- The registry trusts the target it is given
  (`routing/registry/delivery.rs:261-264`: "Engine already selected the
  committed target surface"), and the frontend searches only inside it
  (`event_state.rs::pointer_event_target`, which descends from the target
  window and never considers its siblings). An XTEST pointer can therefore only
  land in the focused window's tree.
- In a no-WM session the focus is set once, to the first committed surface
  (`live_session/policy.rs::initial_session_focus_candidate`), and nothing moves
  it: WM focus needs a WM (`session_control.rs:493`), and click focus is off
  without one (`input.rs:458`, `physical_input_phase.rs:575-590`). Hence one
  `focus_applied`, and one reachable window.
- **Physical input is unaffected.** `route_physical_input` (`input.rs:382`)
  hit-tests the Engine scene with `hit_test_scene_surface_for_input`
  (`input.rs:1637`) and reaches whatever is under the pointer.

The reference delivers pointer events to the window under the sprite
(XYToWindow); the focus decides only where keys go. So `plan()`'s targeting is
right for keys and wrong for pointer events.

## Finding and resolution

**Established, not yet repaired.** The repair routes XTEST pointer events
through the same Engine hit-test as physical input, as a second, synthetic
source into the owner loop's routing, so "what is under the pointer" keeps one
owner and matches what is rendered. An X-tree walk in the frontend was
rejected because it would be a second notion of "under the pointer" that can
diverge from the Engine's committed geometry, which is the gap t127 records.

## Validation and remaining work

Open as t156, critical, in [todo.md](../../../todo.md). It blocks the paste half
of t124's and t147's selection smoke. Red/green: an owner-loop test in which a
synthetic pointer over surface B, with focus on surface A, routes to B; and the
real-client gate's paste half.

## Connections

- [Primary selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  t124, whose paste half this blocks.
- [An XTEST button is delivered at the screen origin](csiz9c9x-an-xtest-button-is-delivered-at-the-screen-origin-not-where-the-pointer-is.md) --
  t155, the button-position half of XTEST's pointer defects.
- [Two production paths framed one window in two places](drutdyov-two-production-paths-framed-one-window-in-two-places.md) --
  t127, the geometry divergence that ruled out an X-tree walk.
