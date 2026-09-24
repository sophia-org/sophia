---
id: shck8bcm
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, wire, xts]
---
# A set unused value-mask bit was counted for the length and ignored

## Question

XTS5 wants BadValue for a value mask with an unused bit set: CreateWindow
6, ChangeWindowAttributes 4 and ConfigureWindow 4 got nothing back, and
CreateGC 6 got BadLength. What did the decoders do with the bit?

## Evidence

The three window decoders (`wire/core/windows.rs`) counted every set bit
of the mask toward the request's expected length, checked that length,
and then walked only the defined bits: a request carrying the extra value
its unused bit announced was accepted in silence, the value skipped.
CreateGC (`wire/core/resources.rs`) did refuse a bit above the twenty-three
components, but as `InvalidLength`, which the wire mapping turns into
BadLength; ChangeGC and CopyGC already answered `InvalidValue(mask)`. The
protocol names the error for each: a set bit outside the defined set is
a Value error, and the length the bit would imply is a consequence of the
fault, not the fault.

## Finding and resolution

Each decoder reads the mask and refuses an undefined bit with
`InvalidValue(mask)` before the length is judged by it; the defined sets
are named once in `wire/constants.rs` (fifteen window attributes, seven
configure values, twenty-three GC components). CreateGC's refusal changes
kind. `tests/x11_wire/value_mask_bits.rs` sends each request with its
highest defined bit and the first undefined one set, with the values the
mask announces so only the bit is wrong, in both byte orders, and expects
the Value error carrying the mask; and each with the highest defined bit
alone, which still decodes. Red on master, green after.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] XTS CreateWindow 6, ChangeWindowAttributes 4, ConfigureWindow 4 and
      CreateGC 6 retire from the declared rows: the Xproto rerun
      (`.artifacts/xts-xproto/run-t167/`) moves those four and no other,
      292 passed and 97 declared. The core profile reads PASS, 126 of 126.
- [x] The gate on the committed candidate, rebased onto t179
      (`.artifacts/x11-profile-946ae6d6-{selected-core,xproto}/`): both
      scenarios PASS, xproto with 292 passed and 97 declared.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the rows this closes.
