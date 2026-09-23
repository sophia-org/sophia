---
id: 5pmnf6ie
date: 2026-09-22
kind: investigation
status: resolved
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

**Repaired 2026-09-23, on the narrow seam.** A first design fed XTEST pointer
events into the owner loop's physical routing so the Engine's hit-test would
pick their target. A read-only check before any code found that unsound as a
drop-in: `--no-input` skips the physical phase entirely; the FakeInput
completion ticket has only a blocking send and would have to be answered on
about ten routing paths or the client hangs; physical placement treats
positions as relative device motion; and the physical path would hand an XTEST
client shell chrome, launcher, WM-gesture and click-focus activation.

The seam taken keeps the Engine's hit-test as the one owner of "what is under
the pointer" and touches none of that. The owner loop publishes its input
layers into `LiveXTestPointerScene` (`live_session/x_frontend/xtest.rs`)
whenever `input_presentation_epoch()` moves, every pass, physical input or
not. `LiveXTestInjector` resolves each motion and button against the last
publication with the same `sophia_engine::hit_test_scene_surface_for_input`
the physical path uses, through `resolve_pointer_target`, and hands the event
on to `RoutedXTestInjector` unchanged -- so the completion barrier, the
registry, implicit grabs and delivery are exactly as they were. Over nothing,
or before a first publication, the plan's target stands. Keys never come here:
they belong to the focus.

The cost is staleness of at most one owner pass: a surface that has just
appeared is reachable on the next. A surface under the pointer with no client
route (shell chrome) is refused by the registry as today, which releases the
client's barrier rather than hanging it; XTEST gains no compositor privilege.

## Validation and remaining work

- [x] `a_synthetic_pointer_resolves_to_the_surface_under_it_not_the_focus`
      (`tests/support/application_lease_routing.rs`): with the plan naming
      surface 201, a point over 202 resolves to 202 at the position relative to
      it; over nothing, or with no published scene, the plan stands. Reverting
      the resolver to the focus target fails it (201 against 202).
- [x] End to end, t124's driver against the production session: drag in xterm
      A, middle-click in xterm B. Before, the paste half failed with
      `pointer_not_over_target` in every run; after, three of three pass with
      `owner_changes=1 conversions=2` -- the second conversion is xterm B's own
      ConvertSelection -- and `bounded_complete`. Logs in
      `.artifacts/t156-xtest-pointer-scene/`.
- [x] `sophia-x-authority` (1,923) and `sophia-session` (905) under the gate's
      isolation, the XTEST profile 44/44, clippy and fmt clean.

Not claimed: the X-root and Engine logical spaces are assumed to coincide,
which holds for the headless deterministic head here and in the QEMU guest; a
multi-head installed session is exercised by t147's hardware half.

## Connections

- [Primary selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  t124, whose paste half this blocks.
- [An XTEST button is delivered at the screen origin](csiz9c9x-an-xtest-button-is-delivered-at-the-screen-origin-not-where-the-pointer-is.md) --
  t155, the button-position half of XTEST's pointer defects.
- [Two production paths framed one window in two places](drutdyov-two-production-paths-framed-one-window-in-two-places.md) --
  t127, the geometry divergence that ruled out an X-tree walk.
