---
id: vm14kz5r
date: 2026-09-23
kind: investigation
status: resolved
tags: [investigation, x11, input, xterm, validation]
---
# xterm as an oracle for what the frontend delivers to a widget

## Question

Four pointer-delivery defects in a week -- buttons at the origin (t155),
events to the focus (t156), no motion during a drag (t162), a release past
the widget's edge lost (t158) -- were each found by hand, by pixel, or by a
protocol trace. Is there a real client that will simply say what it
received, so a gate can ask it?

## Evidence

xterm will. With mouse tracking on it writes an SGR escape sequence to its
pty for every pointer event its text widget receives: `CSI < Cb ; col ; row
M` for a press or a motion, `m` for a release, Cb 0/1/2 for buttons 1/2/3,
+32 for motion with that button held, 35 for motion with none. The cell
names the position, the code names the button state, and the report exists
only if the event reached the widget -- the window, the coordinates and the
state all in one line of text, from the client the report was about.

`crates/sophia-session/examples/xterm_pointer_oracle.rs` starts a real
xterm whose command puts the pty in raw mode, turns tracking on and copies
the pty's input to a file; injects XTEST motion, press, drag and release at
cells computed from xterm's own size hints; and matches the reports in
order. `cargo xtask check xterm-pointer-oracle` runs it as the headless
gate's client (`crates/xtask/src/xterm_pointer_oracle.rs`, on the shared
`headless_client_gate.rs` the selection gate now uses too) and reads the
session's XTEST record alongside.

**Which tracking mode matters.** The first version used any-event tracking
(`?1003h`) and passed with the t162 mask reverted:

```
\e[<35;1;1M\e[<35;11;3M\e[<0;11;3M\e[<32;15;3M\e[<32;19;3M\e[<32;23;3M\e[<0;23;3m…
```

In that mode xterm selects PointerMotion on the widget itself, so it hears
drag motion even from a frontend that only delivers to PointerMotion
selectors -- the defect was invisible. Button-event tracking (`?1002h`)
reports motion only with a button held and leaves xterm relying on its
`<Btn1Motion>` translation, the Button1Motion selection an ordinary xterm
drags a selection with. That is the default, and with it:

| tree | reports | verdict |
| --- | --- | --- |
| fixed (`4eacfcfb`+) | `2;41;3M 2;41;3m 0;11;3M 32;15;3M 32;19;3M 32;23;3M 0;23;3m 1;31;3M 1;31;3m 2;31;3M 2;31;3m` | pass |
| t162 mask reverted | `2;41;3M 2;41;3m 0;11;3M` | `no_drag_report` |
| fixed, `--overshoot` | `… 32;23;3M` and no `m` | `release_lost` (t158, as expected) |
| `--self-test`: no `--admit-xtest` | none (XTEST absent) | `connection` |
| `--self-test`: `--no-tracking` | none | `tracking_never_reported` |

The first pair is the red/green for the gate; the overshoot row is t158's
second red probe. Logs under `.artifacts/xterm-pointer-oracle/`.

A detail worth keeping: an XTEST button carries no position. The first
overshoot probe "released past the edge" without moving there and passed,
xterm reporting the release at the last drag cell; the probe now moves
first, as a hand does.

## Finding and resolution

The gate is in place and green; its self-test fails both mutations; the
source mutation that matters was made once by hand and recorded above. It
reads cells, not pixels: it says what xterm was told, not what it drew, and
it runs headless with no window manager. `--any-event` remains for the
plain-motion path (code 35), with the caveat above written into the driver.

## Validation and remaining work

- [x] `cargo xtask check xterm-pointer-oracle` green; `--self-test` red twice.
- [x] t162 mask reverted: `no_drag_report`.
- [x] `--overshoot`: `release_lost`, red until t158.
- [ ] A keyboard sibling: xterm with `cat` as its command, XTEST keys, the
      typed bytes read back -- key delivery, XKB mapping and modifiers
      through the same oracle. Not this row.
- [ ] The QEMU guest could run the oracle beside the selection scenario; it
      needs `stty` in the image. Not this row.

## Connections

- [PRIMARY selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  the selection gate this one sits beside.
- [Motion during a held button is delivered only to windows selecting PointerMotion](ljbbq5pw-motion-during-a-held-button-is-delivered-only-to-windows-selecting-pointermotion-never-buttonmotion.md) --
  the defect the oracle's red is taken from.
- [An implicit pointer grab delivers by position](urxcuj5s-an-implicit-pointer-grab-delivers-by-position-not-to-the-window-that-took-the-press.md) --
  `--overshoot` is its probe here.
