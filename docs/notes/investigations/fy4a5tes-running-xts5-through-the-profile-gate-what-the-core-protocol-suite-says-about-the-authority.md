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

**The third run, with `TOO_LONG` rejoined.** After t165 the manifest was
enumerated again without the exclusion, 389 purposes, and run
(`.artifacts/xts-xproto/run-3-full/`, journal included). Every purpose
started and completed in four minutes twenty; no case reached its timeout,
and no purpose that had a result in the second run changed it. The 120
rejoined purposes: 110 FAIL, 10 UNRESOLVED.

| | purposes |
| --- | --- |
| manifested | 389 |
| started and completed | 389 |
| PASS | 177 |
| FAIL | 165 |
| UNRESOLVED | 34 |
| UNTESTED | 11 |
| UNSUPPORTED | 2 |

The 110 all fail the same way, and it is not the deadlock. The suite's own
Xlib is of X11R6 lineage: it sends `BigReqEnable` at open and takes any
reply as the extension being on, so `_Send_Req` frames the `TOO_LONG`
request as a big request -- a zero length field, then a 32-bit length one
past the maximum, then the body. The authority advertises `BIG-REQUESTS`
and answers `BigReqEnable`, but its reader frames nothing: the zero-length
header is a four-byte request, answered `BadLength`, and the 262 KB body
is some 65 000 requests after it, each answered. The purpose wants one
`BadLength` and then nothing (`Expect: wanted NOTHING but got at least 42
unexpected ... replies/errors/events`); the reference server frames the
extended length, discards a request beyond its maximum whole, and answers
once. Filed as t174; the 110 are declared against it by `case#purpose`.
The 10 UNRESOLVED are the cases whose setup already fails for t166
(`ChangeHosts`, `ChangePointerControl`, `SetScreenSaver`) or for want of
fonts, and keep those reasons. A modern client is not exposed: xcb and the
Xlib over it take the advertised maximum of 65535 units as no extension
at all and never send the extended frame; the exposure is a client of the
suite's lineage, or a hand-rolled one, and the inconsistency of advertising
what is not framed.

**`selected-core` through the gate**, on candidate `0b4b7415`
(`.artifacts/x11-profile-0b4b7415-selected-core/`): the first run in which
`XTS5` read anything but BLOCKED. 99 purposes started and completed, 76
PASS, 0 FAIL, and 23 dispositions the suite itself reports: `Multiple
screens not supported` for the focus purposes that need two screens
(UNSUPPORTED), assertions the suite has retired (NOTINUSE), and its own
`no known reliable test method` omissions (UNTESTED, and around backing
store one UNRESOLVED). The focus contract's seven repairs of 2026-09-20 all
hold.

**Declared dispositions.** The evaluator required every manifested purpose
to PASS, so a suite with retired assertions and single-screen omissions
could never read PASS through it, and a manifest could only "pass" by
leaving purposes out -- which is the baseline-by-omission the adapter was
built to refuse. A manifest row may now declare an expected disposition
(`FAIL`, `UNRESOLVED`, `NOTINUSE`, `UNSUPPORTED`, `UNTESTED`) with a
mandatory reason; `NORESULT` and `MISSING` describe a run and cannot be
declared. The evaluator requires the observed disposition to equal the
declared one: a declared purpose that starts passing fails the gate as a
stale manifest, so an improvement is recorded rather than silently
absorbed. `xts_declare.py` writes declarations from a real journal and a
reviewed reasons file keyed by case or `case#purpose`, and refuses any
disposition without a reason. The gate's verdict line carries the
accounting: `XTS5 PASS (76 passed, 23 declared)`. The reasons files are
committed beside the manifests (`xts_reasons_selected_core.json`,
`xts_reasons_xproto.json`); for Xproto every declared failure names its
row.

## Finding and resolution

The gate runs XTS now, and `XTS5 BLOCKED` means what it says: the checkout
or the manifest was not named. Two scenarios are enumerated and committed
with their declarations: `selected-core` reads PASS with 76 passed and 23
declared, all the suite's own; `xproto` reads PASS with 177 passed and 212
declared, 165 of them FAIL and 34 UNRESOLVED that are the authority's and
carry their row (t166 to t169, and t174 for the rejoined `TOO_LONG`
purposes) in the reason. A PASS here means the suite
said exactly what the manifest says it would, no more; the declared count
is the debt, in the verdict line where it cannot be missed. `~/src/xts` is
the operator's checkout; the invocation is in `docs/validation.md`.

What the suite does not cover is unchanged: nothing of XKB, and the input
purposes it marks UNTESTED because it is not configured to drive XTEST
against the fixture host.

## Validation and remaining work

- [x] The gate builds the fixture host and runs XTS; `xts_select.py` excludes
      by test type, with a test; `xts_check.sh` bounds each case.
- [x] Xproto enumerated, run, and its non-PASS purposes named and filed.
- [x] `selected-core` through the gate on the committed candidate: 76
      passed, 23 declared, 0 FAIL.
- [x] Declared dispositions with reasons, `xts_declare.py`, tests in
      `test_gate.py` and `test_xts_select.py`.
- [x] Rejoin the 120 `TOO_LONG` purposes once t165 lands: rejoined in the
      third run, none hangs, 110 declared against t174 and 10 against the
      reasons their cases already carry.
- [ ] As t174 lands, the 110 `TOO_LONG` declarations turn stale and the
      gate says so; remove them with the repair.
- [ ] As t166 to t169 land, their declarations turn stale and the gate says
      so; remove each with its repair.

## Connections

- [Independent X11 socket conformance exposes missing client completions](wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md) --
  the adapter, the first `selected-core` run and its repairs.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the decided and undecided opcodes; the first row of the table above is
  the suite naming which of the undecided ones matter to it.
- [A client that writes without reading deadlocks its connection](l1z9cldd-a-client-that-writes-without-reading-deadlocks-its-connection-replies-are-written-blocking-on-the-reading-thread.md) --
  why `TOO_LONG` deadlocked, and what let it rejoin.
