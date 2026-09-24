---
id: xshiuuuo
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, core-protocol, xts]
---
# Eight core requests were BadRequest for want of a place to keep what a client sets

## Question

XTS5 declared forty-six purposes against t166, twenty-one core opcodes the
authority did not decode. Nine of the opcodes share one shape: a client sets
something the reference server keeps -- pointer acceleration, the screen
saver, the keyboard's bell and click and LEDs and repeat, the host access
list -- and reads it back, or asks for a history the server may keep. An
undecoded opcode is BadRequest, which xterm's error handler turns into an
exit; `xset`, `xhost` and their kind are the clients that meet these. What
does an authority that acts on none of this owe them?

## Evidence

The runtime kept none of the state: GetKeyboardControl (103) encoded
constants (`client_output/replies/core_late.rs`), GetPointerControl (106),
Get/SetScreenSaver (107, 108) and ChangePointerControl (105) had no decoder
and no state, ChangeKeyboardControl (102) neither, GetMotionEvents (39) had
no history to read (`XPointerQueryState` keeps the last position only), and
there was no host list: admission is by namespace and peer credentials
(`frontend_types.rs`). SetFontPath (51) was decoded with a minimum length
and refused BadAccess by design (ADR n520o0bl), so a request one unit long
reached the refusal instead of BadLength. The Engine owns pointer
acceleration and the session owns key repeat (`sophia-engine`'s
`KeyRepeatState`), and nothing here blanks a screen.

## Finding and resolution

Advisory state, named so: `XServerControls` (`server_controls.rs`) in the
runtime holds what a client set -- pointer acceleration and threshold, the
screen saver's timings and modes, the keyboard's click, bell, LED mask and
repeat flags -- with the reference server's defaults, and the Get requests
report it. The protocol's validation is the decoders' (`wire/core/input.rs`):
an unused keyboard-control mask bit is BadValue carrying the mask, a value
outside its set BadValue carrying the value, -1 restores a default where
the protocol allows it, a led without a mode is BadMatch, a zero
acceleration denominator BadValue. Nothing acts on the state, and the
concept note says so beside each opcode. GetMotionEvents validates the
window and replies no events, which the protocol allows of a server that
keeps no history. ListHosts (110) replies an empty list with access control
enabled; ChangeHosts (109) and SetAccessControl (111), decoded with their
exact framing, are BadAccess: no client is authorised to change a list that
decides nothing. SetFontPath parses its path list, so the length error comes
before the refusal by design.

`tests/x11_wire/server_controls.rs`, both byte orders: each control set and
read back, the refusals named, the empty history, the locked host list,
SetFontPath's framing. Red on master, green after. The probe case
`server_controls_round_trip` says the same for the core profile, and the
nine opcodes enter the inventory.

XTS (`.artifacts/xts-xproto/run-t166a/`): seventeen purposes retire and
nothing outside the seam moves, 322 passed and 67 declared. Five stay
declared by decision: ChangeHosts 1 and SetAccessControl 1 (BadAccess by
policy), SetFontPath 1 (the ADR), and the purpose-2 halves of the four-byte
requests GetPointerControl, GetScreenSaver, ListHosts and SetAccessControl,
which the suite marks UNTESTED because a one-unit request cannot be made
shorter under BIG-REQUESTS, now that their too-long halves pass.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] The core profile reads PASS, 130 of 130, the new case in both orders.
- [x] XTS: seventeen rows retire, five stay by decision.
- [x] The gate on the committed candidate
      (`.artifacts/x11-profile-595ac2b0-{selected-core,xproto}/`): both
      scenarios PASS, xproto with 322 passed and 67 declared.
- [ ] The rest of t166: the window hierarchy and active grab, connection
      lifetime, and the input maps, each its own seam.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the rows this retires.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the decisions, opcode by opcode.
