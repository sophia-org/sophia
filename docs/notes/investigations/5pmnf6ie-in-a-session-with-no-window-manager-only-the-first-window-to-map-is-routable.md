---
id: 5pmnf6ie
date: 2026-09-22
kind: investigation
status: investigating
tags: [investigation, input, session, placement]
---
# In a session with no window manager only the first window to map is routable

## Question

In a headless no-WM session two xterms map, but the pointer resolves to only
one of them anywhere on the screen. Is the second window routable at all, and
if not, why?

## Evidence

Found under t124 while driving two real xterms by XTEST
(`xtest_selection_driver`, run as the `--client` of
`sophia session run --display=:90 --no-input --admit-xtest`). The driver
moves the pointer with XTEST motion, which is correct (t155 concerned buttons
only), and reads `QueryPointer`'s `child` -- which the frontend takes from the
pointer snapshot the Engine's routing sets (`event_state.rs::query_pointer`),
not from the X window tree. On failure it sweeps the root in 24-pixel steps
and records the box in which each child resolves.

- **Both spawned at once:** in 5 of 7 runs xterm A was routable nowhere; the
  sweep resolved only B. The session's `focus_applied` named whichever xterm
  mapped first, and only that one was ever under the pointer.
- **A spawned first, B after A is routable:** A resolves exactly over its X
  frame (`x 52..700, y 52..220` for `664x188+40+40`); B, mapped at
  `664x188+40+320`, resolves nowhere, run after run, while A still does.

Logs and driver builds are retained under `.artifacts/t124-xtest-selection-gate/`
(`run4`..`run10`, and the t155 re-runs in the scratchpad record of that task).

## Finding and resolution

**Not diagnosed.** The observation is that a second policy-managed window in a
Direct-mode session maps but is never hit-testable. Candidates to separate,
not yet tested:

- The Engine routes by surface admission, and in Direct mode only the surface
  that receives the initial focus is admitted or activated; the direct-map
  test pins a new no-WM window's admission as `Inactive`.
- The surface is admitted but never enters the routing set the pointer is
  resolved against -- the same family as t119, where a no-WM window reached no
  output because it was in neither routing arm.

This matters beyond the test: the `native`, `standalone` and `kitty` profiles
all run without a window manager (`sophia-conformance/src/profile.rs:31-55`),
so in any of them a second window may never receive pointer input.

## Validation and remaining work

Open as t156, a candidate, in [todo.md](../../../todo.md). It blocks the paste
half of t124's real-client smoke, which needs the pointer over a second xterm.
Reproduce with the driver's own sweep; decide between the candidates above
before any repair.

## Connections

- [Primary selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  t124, whose paste half this blocks.
- [A no-WM session never routes its window to an output](z2pghbxr-a-no-wm-session-never-routes-its-window-to-an-output.md) --
  t119, the neighbouring no-WM routing gap.
