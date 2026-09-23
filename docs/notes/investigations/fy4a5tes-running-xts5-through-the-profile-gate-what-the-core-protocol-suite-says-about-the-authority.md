---
id: fy4a5tes
date: 2026-09-23
kind: investigation
status: resolved
tags: [investigation, x11, xts, conformance, validation]
---
# Running XTS5 through the profile gate: what the core protocol suite says about the authority

## Question

`cargo xtask check x11-profile` reported `XTS5 BLOCKED` on every run. The
checkout at `~/src/xts` was built, `selected-core` had run by hand on
2026-09-20 and found seven repairs, and nothing since had asked the suite
anything. What does the gate need to run it, and what does the suite say
about the authority today, beyond the nine cases already selected?

## Evidence

**The gate could not run XTS at all.** `xts()` in `m3_acceptance/profiles.rs`
requires `x11_conformance_host` in the gate's own target namespace, and
none of the gate's profiles builds it -- the `core` profile that does is
`check.py`'s, not this gate's. Every run with the three XTS arguments
therefore read BLOCKED for want of a binary the gate could have built.
Fixed: the gate builds the host into its namespace before `xts()` when XTS
is requested (`build_xts_host`).

**The whole Xproto section, 122 cases.** Enumerated by `xts_select.py`
from the built suite into `xts_expected_xproto.json`. The first run stopped
inside its first case: the `TOO_LONG` purpose deadlocks the connection,
[t165](l1z9cldd-a-client-that-writes-without-reading-deadlocks-its-connection-replies-are-written-blocking-on-the-reading-thread.md),
and TET had no per-case timeout. Two changes: `xts_check.sh` runs tcc with
`-t 120`, and the selector learned `--exclude-test-type TOO_LONG`, which
narrows a case's scenario line to the purposes kept and writes the
exclusions beside the manifest (`xts_expected_xproto.excluded.json`, 120
purposes, one per case that has one). The second run completed:

| | purposes |
| --- | --- |
| manifested | 269 (389 less 120 excluded) |
| started and completed | 269 |
| PASS | 177 |
| FAIL | 55 |
| UNRESOLVED | 24 |
| UNTESTED | 11 |
| UNSUPPORTED | 2 |

Run under `.artifacts/xts-xproto/run-2/`, journal included. The 92 non-PASS
purposes, by what the suite's own report lines say:

| cause | cases | what the report says |
| --- | --- | --- |
| **Undecoded request.** The authority answers `BadRequest` to a core opcode it does not implement. | ChangeHosts, ListHosts, SetAccessControl; GetMotionEvents; GetPointerControl, ChangePointerControl, SetPointerMapping; QueryKeymap, ChangeKeyboardControl, ChangeKeyboardMapping, SetModifierMapping; GetScreenSaver, SetScreenSaver; KillClient, SetCloseDownMode, ChangeSaveSet; RotateProperties, CirculateWindow, UnmapSubwindows; ChangeActivePointerGrab; SetFontPath | `wanted REPLY - X_ListHosts, got ERROR - BadRequest`; `wanted NOTHING, got ERROR - BadRequest`; `wanted EVENT - MappingNotify, got ERROR - BadRequest` |
| **Value mask not validated.** A set unused bit in a value mask must be `BadValue`. | CreateWindow 6, ChangeWindowAttributes 4, ConfigureWindow 4 (`got NOTHING`); CreateGC 6 (`got ERROR - BadLength`) | `wanted ERROR - BadValue, got NOTHING` |
| **SendEvent's send_event bit.** An event delivered by SendEvent carries bit 7 of its type set. | SendEvent 1 | `Expected MSB set in event type ClientMessage; got 0` |
| **Static visual colormaps.** Allocating cells or planes on the only visual, and storing into read-only cells. | AllocColorCells 2, AllocColorPlanes 2, CopyColormapAndFree 1-5, FreeColors 2, InstallColormap 2, UninstallColormap 2, ListInstalledColormaps 1-2, StoreColors 1 UNSUPPORTED and 3, StoreNamedColor 1 UNSUPPORTED and 3 | `wanted BadAlloc`; read-write cells `UNSUPPORTED` |
| **No server-side fonts, by design.** `XT_FONTPATH` is empty because no font opcode is decoded. | OpenFont, CloseFont, QueryFont, QueryTextExtents, PolyText16, ImageText16, CreateGlyphCursor (all UNRESOLVED); GetFontPath 2 UNTESTED | `No, or empty, XT_FONTPATH set` |
| **Inherent UNTESTED.** A four-byte request cannot be made shorter than its minimum. | Bell 2, ForceScreenSaver 2, GetInputFocus 2, GetKeyboardControl 2, GetModifierMapping 2, GetPointerMapping 2, GrabServer 2, UngrabServer 2, ListExtensions 2, NoOperation 3 | the "too short" purpose |

The first three rows are the authority's to repair and are filed as t166,
t167 and t168. The fourth is a decision about what a TrueColor-only
authority owes the colormap requests, filed as t169. The last two are what
the suite is, not what the authority lacks.

**`selected-core` through the gate.** See the section below, written from
the run on the committed candidate: the gate refuses a dirty tree.

## Finding and resolution

The gate runs XTS now, and `XTS5 BLOCKED` means what it says: the checkout
or the manifest was not named. Two scenarios are enumerated and committed;
`xproto` reads FAIL with every one of its 92 non-PASS purposes named, which
is the honest state of the core protocol against this suite, and the
manifest is the suite's account of itself, not a list of what passes.
`~/src/xts` is the operator's checkout; the invocation is in
`docs/validation.md`.

What the suite does not cover is unchanged: nothing of XKB, and the input
purposes it marks UNTESTED because it is not configured to drive XTEST
against the fixture host.

## Validation and remaining work

- [x] The gate builds the fixture host and runs XTS; `xts_select.py` excludes
      by test type, with a test; `xts_check.sh` bounds each case.
- [x] Xproto enumerated, run, and its non-PASS purposes named and filed.
- [ ] `selected-core` through the gate on the committed candidate: below.
- [ ] Rejoin the 120 `TOO_LONG` purposes once t165 lands.

## Connections

- [Independent X11 socket conformance exposes missing client completions](wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md) --
  the adapter, the first `selected-core` run and its repairs.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the decided and undecided opcodes; the first row of the table above is
  the suite naming which of the undecided ones matter to it.
- [A client that writes without reading deadlocks its connection](l1z9cldd-a-client-that-writes-without-reading-deadlocks-its-connection-replies-are-written-blocking-on-the-reading-thread.md) --
  why `TOO_LONG` is excluded.
