---
id: ljbbq5pw
date: 2026-09-23
kind: investigation
status: resolved
tags: [investigation, x11, input, xterm, selection]
---
# Motion during a held button is delivered only to windows selecting PointerMotion, never ButtonMotion

## Question

The operator, on release `30acf6af` under Hagia: copy and paste between two
xterms works, but "initially selecting the text when I press the left mouse
button does not highlight it (like it's invisible) but when I release the
mouse button it gets highlighted." Is that xterm?

## Evidence

It is not xterm. xterm's text widget follows a drag with the Xt translation
`~Meta <Btn1Motion>: select-extend()`, which selects **Button1Motion** on
the VT100 widget and nothing for plain motion. The highlight is redrawn on
every such event. It appearing only at release means no motion reached the
widget while the button was down.

The frontend chose the window a MotionNotify is reported on by
`XCoreEventSelectionState::selected_pointer_target`, which for motion
matched only `POINTER_MOTION_MASK` (bit 6). Button1Motion (bit 8) through
Button5Motion (bit 12) and ButtonMotion (bit 13) were never consulted. So
during a drag no window on the path from the pointer up to the surface
matched, the record fell back to the surface's top-level -- xterm's shell,
which has no such translation -- and xterm learned where the drag ended only
from the ButtonRelease, which the widget did select.

Two protocol traces of xterm A through xscope, driven by the headless
selection gate (`.artifacts/t124-xtest-selection-gate/xterm-a-xscope.trace`
from 2026-09-22 and `.artifacts/t162-button-motion/xterm-a-fixed.trace`):

| | before | after |
| --- | --- | --- |
| drag motions, `state: Button1` | 6, all `event: WIN 0040000c` (shell) | 6, all `event: WIN 00400016` (VT100) |
| aim motions, `state: 0` | 2, shell | 2, shell (unchanged: the widget asks for no plain motion) |
| ButtonPress / ButtonRelease | on the VT100 widget | on the VT100 widget |
| xterm redraws between press and release (`RenderFillRectangles` + `RenderCompositeGlyphs`) | 1 | 6 |

One redraw per drag motion is the highlight following the pointer.

## Finding and resolution

`motion_selection_mask(state)` now derives the masks a MotionNotify answers
to from the event's held-button bits: PointerMotion always, and with any
button down, ButtonMotion plus each held button's own ButtonNMotion (the
state bits Button1Mask..Button5Mask occupy the same positions as the masks
Button1Motion..Button5Motion, so the held bits are the masks). Both callers
in `writers/input.rs` pass the event's state; the explicit-grab filter
beside them, which compared a grab's event mask against PointerMotion alone,
applies the same rule.

Red/green:

- `motion_with_a_button_down_reaches_a_window_selecting_only_button_motion`
  (`x11_socket/tests.rs`): a Button1Motion-only child and a ButtonMotion-only
  child; plain motion reaches neither, button-1 motion reaches both,
  button-2 motion reaches only the ButtonMotion one; buttons ignore the
  motion masks.
- `motion_while_button_one_is_held_is_reported_on_the_child_selecting_button_one_motion`
  (`tests/x11_wire/xtest_admission_socket.rs`): over a real socket, a shell
  with a text child selecting Button1Motion and ButtonRelease; an XTEST
  press, then motion, is reported on the child with Button1Mask; after the
  release the same motion falls back to the shell.

Both fail with the mask reverted to PointerMotion alone.

Core motion is written to the surface's client whether or not any window
selected it (`write_core_record` defaults to true); only the window it is
reported on depends on the selection. That is why the wire fixture's plain
barrier could meet a queued MotionNotify, and why `XtestClient::settle`
reads past events to the reply. It is also a divergence from the core
protocol worth its own look, but not this row's.

## Validation and remaining work

- [x] Unit and wire tests above, red under the mutation.
- [x] xscope traces before and after, one redraw per drag motion after.
- [x] `cargo xtask check xtest-selection` stays green; the driver's own
      verdict does not depend on incremental highlighting, so the traces
      are the evidence for that.
- [ ] Operator: on the next installed release, the selection highlights as
      the pointer moves.

## Connections

- [PRIMARY selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  found on the operator's confirmation that t124's paste works.
- [An implicit pointer grab delivers by position](urxcuj5s-an-implicit-pointer-grab-delivers-by-position-not-to-the-window-that-took-the-press.md) --
  the neighbouring defect on the same delivery path: this one chose the
  wrong window inside the grab, t158 chooses by position at all.
