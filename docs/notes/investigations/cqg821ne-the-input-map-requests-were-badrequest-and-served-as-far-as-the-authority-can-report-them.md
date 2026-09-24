---
id: cqg821ne
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, core-protocol, input, xts]
---
# The input map requests were BadRequest, and are served as far as the authority can report them

## Question

The last four of the twenty-one core opcodes XTS5 named as BadRequest
(t166) rewrite what a client sees of the input devices: SetPointerMapping
(116), ChangeKeyboardMapping (100), SetModifierMapping (118) and QueryKeymap
(44). Sophia owns input through its own authority and Engine routes it;
xkbcommon compiles the keymap that stamps each key event. Which of these can
be served honestly, which only as a stored view, and which not at all?

## Evidence

GetPointerMapping reported the identity (`1..=9`) from a constant, and the
routing's `map_evdev_button` hard-wired the evdev-to-core button numbers.
GetKeyboardMapping and XKB GetMap read the compiled keymap's keysyms
directly, so nothing could be rewritten without both disagreeing.
GetModifierMapping replied a literal (`50, 62, 66, 0, ...`) that happened to
match the compiled map. No MappingNotify event existed, nor any way to tell
every client of a namespace something at once (the only broadcast was
RandR's). Nothing recorded which keys the routing had seen go down.

In the last run before this seam (`.artifacts/xts-xproto/run-t166c/`) the
eight purposes of these four cases read `wanted NOTHING` or `wanted REPLY`,
`got ERROR - BadRequest`, and the purpose-2 halves the BadRequest where
BadLength was wanted.

## Finding and resolution

- SetPointerMapping: nine entries, no logical button named twice, else
  BadValue carrying the offender (the length when it is wrong); Busy while
  a button whose entry changes is held; otherwise stored per namespace in
  the shared input authority and applied where the routing maps a physical
  button (`map_evdev_button_mapped`): the physical button is what is held,
  the logical one what a client receives, and a zero entry is held and
  delivered to nobody. GetPointerMapping reports the map. MappingNotify(2)
  reaches every client of the namespace: the requester through its own
  outputs, before its reply, as the reference server orders them; the
  others through a new registry broadcast, each sequenced by its own writer.
- ChangeKeyboardMapping: a per-namespace overlay (`XCoreKeyboardMap`) starts
  as the compiled keymap's table and is rewritten in place; a keycode
  outside min..max is BadValue carrying it. GetKeyboardMapping and XKB
  GetMap both read the overlay, so the two views agree; MappingNotify(1)
  names the first keycode and count. xkbcommon's compiled keymap still
  drives the modifier state and keysym of each key event: a remapped key
  is reported remapped and delivered as compiled. That is the overlay's
  limit, recorded in the coverage concept rather than hidden.
- SetModifierMapping: the request is normalised into eight keycode sets
  and compared with the compiled keymap's modifier map, which
  GetModifierMapping now reads instead of the literal. Equal: Success and
  MappingNotify(0). Different: Failed and no event, because xkbcommon owns
  the modifier state events carry and a map it did not compile cannot be
  served. A keycode outside min..max is BadValue.
- QueryKeymap: thirty-two bytes of the keys this routing has seen go down
  and not yet up, per namespace, written at the routed key transitions (a
  repeat is not a transition) and at the private path's press.

`tests/x11_wire/input_maps.rs`, both byte orders, on the routed service: a
short list and a repeated button are BadValue with the offender; a swap of
buttons 1 and 3 is Success, GetPointerMapping reports it, and
MappingNotify(2) reaches the requester before its reply and a peer on its
own sequence; a keycode's three keysyms written and read back through
GetKeyboardMapping with MappingNotify(1), a keycode below the minimum
BadValue; the current modifier map restated is Success with
MappingNotify(0), another is Failed with no event; QueryKeymap answers the
thirty-two zero bytes when nothing is down. Red on the tree before this
seam (BadRequest on 100 and 116), green after. Four probe cases enter the
core profile and the four opcodes enter the inventory. With this seam the
twenty-one opcodes of t166 are all decided and decoded, and the Xproto
scenario's declared rows are the suite's own (fonts by design, the
four-byte UNTESTED halves, the two UNSUPPORTED on a TrueColor-only screen)
or a decision with its note: ChangeHosts 1 and SetAccessControl 1
(BadAccess by policy), SetFontPath 1 (the ADR) and SetModifierMapping 1.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] The core profile reads PASS, 152 of 152, the four cases in both
      orders.
- [x] `sophia-session` suite and clippy.
- [x] XTS (`.artifacts/xts-xproto/run-t166d/`): SetPointerMapping 1,
      ChangeKeyboardMapping 1 and 2 and QueryKeymap 1 retire; the purpose-2
      halves of SetPointerMapping, SetModifierMapping and QueryKeymap join
      the suite's own UNTESTED (a four-byte request cannot be made shorter
      under BIG-REQUESTS); SetModifierMapping 1 stays by decision; nothing
      else moves: 339 passed and 50 declared.
- [x] The transport gates, `xtest-selection` (owner_changes=1,
      conversions=2, injected_buttons=4) and `xterm-pointer-oracle`
      (injected_motions=7, injected_buttons=8), pass on the seam: button
      remapping sits on the event path.
- [x] The gate on the committed candidate
      (`.artifacts/x11-profile-705cb338-{selected-core,xproto}/`): both
      scenarios PASS, xproto with 339 passed and 50 declared, selected-core
      with 76 passed and 23 declared.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the rows this retires; t166 closes with this seam.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the decisions, opcode by opcode, and the overlay's limit.
- [KillClient, SetCloseDownMode and ChangeSaveSet were BadRequest](mgoqzum4-kill-client-close-down-retention-and-the-save-set-were-badrequest.md) --
  the previous group of t166.
