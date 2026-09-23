---
id: csiz9c9x
date: 2026-09-22
kind: investigation
status: investigating
tags: [investigation, x11, xtest, input]
---
# An XTEST button is delivered at the screen origin, not where the pointer is

## Question

An XTEST drag into a real xterm, with the pointer confirmed over it, never
makes xterm take PRIMARY. The events are delivered. What is wrong with them?

## Evidence

Found under t124, on a headless production session
(`sophia session run --display=:90 --no-input --admit-xtest
--client=xtest_selection_driver`). The driver spawns xterm A, waits until
`QueryPointer` resolves to it, then drives press, six motions and release by
XTEST `FakeInput`. The session records `injected_buttons=2
injected_motions=7 refused=0` and `sophia_live_selection owner_changes=0`.

xterm A was run behind `xscope` (a protocol proxy on display 91) so every
event it received is decoded. Trace retained at
`.artifacts/t124-xtest-selection-gate/xterm-a-xscope.trace`:

| event xterm received | root position | state |
| --- | --- | --- |
| MotionNotify (aim) | 48,51 | 0 |
| **LeaveNotify** | **0,0** | 0 |
| **ButtonPress 1** | **0,0** | 0 |
| EnterNotify, 6× MotionNotify | 154…688, y 51 | Button1 |
| **LeaveNotify, ButtonRelease 1** | **0,0** | Button1 |

Motion is correct, including the Button1 state bit. Every button event is
preceded by a crossing to the origin and delivered there, so xterm sees a
press and release at the same point: a zero-length selection, which it rightly
never claims.

## Finding and resolution

**Established.** `RoutedXTestInjector::submit_button`
(`x11_socket/routing/routed_xtest_injector.rs`) submits
`sophia_protocol::Point::default()` as both the global and the surface-local
position; `submit_motion` passes real points. The routed button therefore
carries (0,0), the frontend moves the pointer there to deliver it, and the
crossings follow.

The XTEST contract (plan `7xqjn8rp`, M5) says the root and coordinates of a
key or button `FakeInput` are ignored: the button happens wherever the pointer
already is. The repair gives the button the current pointer position, global
and surface-local, the way motion carries its own. The XTEST profile's
`xtest_button_pair` case checks that a button arrives, not where, which is why
no gate saw this.

Consequence: every XTEST click or drag against a live session lands at the
origin -- xdotool clicks, XTS purposes that press, any automation.

This does **not** explain t124's original report, which was a physical mouse
drag; physical buttons do not pass through this injector.

## Validation and remaining work

Open as t155 in [todo.md](../../../todo.md). A red/green test must pin the
delivered button's position to the pointer's; then t124's gate is re-run with
the corrected injector.

Also observed, not this task's: in a no-WM session only the first window to
map was reliably routable by the pointer; the driver now spawns one xterm and
waits for it to be routable before the next.

## Connections

- [Primary selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  t124, whose gate this blocks.
- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  the M5 contract that says button coordinates are ignored.
