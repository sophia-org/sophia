---
id: wzxlxbok
date: 2026-09-12
kind: investigation
status: investigating
tags: [investigation, x11, conformance]
---
# Independent X11 socket conformance exposes missing client completions

## Current independent result

The final 2026-09-12 gate executed **100 cases: 100 PASS, zero nonpassing**
on clean source c690b7cd, incorporating runtime 9be53aff. That profile still
passes on 2026-09-19 (`.artifacts/x11-conformance/baseline-d7aa48d4/`); the
[drawing family baseline](#drawing-family-baseline) below then added twenty
executions and is the current result. A fresh host build and
twenty strict reporting regressions pass. Evidence is
`.artifacts/x11-conformance/final-100/`, with exact source and host/harness hashes.

The same isolated checkout also passes `cargo clippy --offline -p
sophia-x-authority --all-features --all-targets -- -D warnings` with zero warnings
and the full x-authority crate suite with an empty XDG configuration and inherited
session opt-ins cleared. Commands, exits and logs are retained in `validation.json`,
`clippy.log` and `crate-tests.log`. The operator-reported full `xtask check` failure
was not reproduced in these checks; no full xtask success is claimed here.

Both byte orders cover all three XFixes selection-notification subtypes,
same-owner reassertion and bursts, masks, invalid subscriptions, original
ownership timestamps, watched-window reuse and self-notification sequences.
The pressure case requires all 4,096 events at a healthy subscriber, disconnect
of a stalled subscriber, continued sender service and fresh admission. It fails
on 3f4b0462: only 10,976 bytes reach the stalled client, which remains connected
after silently losing events. It passes with the 1a631234 repair.

Two further failures were retained and repaired. A peer-owned child survived its
parent client's departure (t091), then GetGeometry reported BadWindow after
actual destruction began (t092). The final cases require real child destruction,
distinct teardown causes, retained timestamps, BadDrawable and a healthy surviving
peer. See the [mixed-owner record](g8c2ey1f-peer-owned-child-selections-outlive-a-disconnected-parent.md)
and [geometry record](78qco9vp-getgeometry-misclassifies-an-invalid-drawable-as-badwindow.md).

Task t063 is accepted for this implementation, including stalled-watcher
handling, subscription cleanup and retirement carried with the release under its
runtime lock. The additional Rust test covers two Confined namespaces with a
same-namespace positive control and a bounded quiet read at the excluded peer;
the independent socket host itself remains single-namespace ClassicShared.

Older destroy/MSC recipient containment remains open as
[t090](psf52z1x-a-stalled-protocol-recipient-can-escape-x11-client-containment.md).
A green selected manifest is not full X11/XFixes certification; t057 retains
coverage debt and unrun XTS5 integration. t089 still requires installed Sophia
repair acceptance. No live display, VT, GPU test or Sophia deployment was used
for this verification.

Retained progression, all under `.artifacts/` in the main checkout:

| Evidence directory | Runtime | PASS | Nonpassing |
| --- | --- | ---: | ---: |
| x11-setup-containment-before | b52fff29 | 64 | 16 |
| x11-setup-containment-after | 528803aa | 76 | 4 |
| x11-setup-containment-final | cb07cafc | 78 | 2 |
| x11-xfixes-before | cb07cafc, expanded cases | 78 | 16 |
| x11-xfixes-increment | 8faab7d9 | 88 | 6 |
| x11-xfixes-final | 3f4b0462 | 94 | 0 |
| x11-xfixes-pressure-before | 3f4b0462, pressure case added | 94 | 2 |
| x11-xfixes-pressure-increment | 1a631234, mixed-owner case added | 96 | 2 |
| x11-xfixes-accepted | 385282b4 | 96 | 2 |
| x11-geometry-before | 385282b4, standalone geometry case added | 96 | 4 |
| x11-conformance/final-100 | 9be53aff, clean gate source c690b7cd | 100 | 0 |

Changed runtime baselines use separate fresh Cargo targets. Expanded before-runs
reuse the already verified host for the same runtime. Artifact directory names
do not override report status: `x11-xfixes-accepted` was a failed attempt. Reports retain source,
host and harness identity, including dirty state where applicable. These are
private software-only socket results. XTS5 remains unrun, and no physical
acceptance or installed Sophia repair is claimed. See the
[separate containment incident](kwhei4x4-preflight-setup-disconnect-precedes-an-authority-exit.md).

UnmapNotify and mapped destruction (t084/t087) independently pass, as do
NoOperation (t085), extension discovery (t086) and extension refusal classification
(t088). Those task closures do not claim complete coverage of every operation.

## Drawing family baseline

On 2026-09-19 the gate gained ten mandatory cases for the pixmap,
graphics-context, drawing and image requests, the eighteen rows the colormap
commit `d461492d` had left as explicit debt among them. The cases live in
`tools/probes/x11_conformance/drawing_cases.py` and judge every request by
reading the drawable back through GetImage, decoded with the server's
advertised image byte order; expected pixels follow the protocol's own
pixelization rules. Coverage moved from 29 to 51 of the 97 decoded core
requests; 46 rows of debt remain. Opcodes newly covered: 53-73 and 97.

Before the cases were written, a throwaway probe confirmed the software
fixture returns exact pixels for both pixmaps and viewable windows in both
byte orders, so a pixel assertion here is evidence about the request, not about
the oracle.

Run on clean source **1468790e** with host SHA256 `29b1be9c87f3a89f27e97d2d40a35895f5294075aae3bb07f0653058fe1bafd0`, evidence at
`.artifacts/x11-conformance/baseline-1468790e/`:
**120 executions: 102 PASS, 18 FAIL/TIMEOUT; gate exit 1.** The fifty
previously mandatory cases keep passing. `pixmap_lifecycle` passes in both
orders. The nine remaining cases fail identically in both orders, each at the
first obligation the host does not meet, so the assertions after that point in
each case are not yet exercised. A timeout below means the host answered a
request with neither the required error nor the required event and the client
waited out its deadline.

| case | first unmet obligation | specification |
| --- | --- | --- |
| `gc_lifecycle` | CopyGC between contexts of different depth completes without BadMatch | "The two gcontexts must have the same root and the same depth (or a Match error results)" |
| `gc_dashes_clip` | SetClipRectangles accepts ordering 4 without BadValue | ordering is `{UnSorted, YSorted, YXSorted, YXBanded}`; Value is in the request's error list |
| `clear_area` | ClearArea with exposures True on a visible window generates no Expose (mapping the same window does) | "if exposures is True, then one or more exposure events are generated for regions of the rectangle that are either visible or are being retained in a backing store" |
| `copy_area` | a source rectangle partly outside the source pixmap produces no GraphicsExposure; NoExposure for a wholly available source does arrive | "if regions outside the boundaries of the source drawable are specified ... GraphicsExposure events for all corresponding destination regions are generated", "Regardless of ... whether the destination is a window or a pixmap" |
| `copy_plane` | CopyPlane generates no NoExposure | "the equivalent of a CopyArea is performed, with all the same exposure semantics" |
| `poly_primitives` | PolyPoint accepts coordinate-mode 2 without BadValue | coordinate-mode is `{Origin, Previous}`; Value is in the error list |
| `fill_primitives` | FillPoly accepts shape 3 without BadValue | shape is `{Complex, Nonconvex, Convex}`; Value is in the error list |
| `put_get_image` | PutImage in ZPixmap format with left-pad 4 returns BadValue, carrying the drawable as the bad value, instead of BadMatch | "The left-pad must be zero for ZPixmap format (or a Match error results)" |
| `query_best_size` | QueryBestSize with class 3 replies 16x16 instead of BadValue | class is `{Cursor, Tile, Stipple}`; Value is in the error list |

Everything the passing prefixes establish is real evidence: pixmap creation,
extents, depth and XID refusals, InputOnly drawables, depth-one pixmaps, free
and reuse; GC creation, ChangeGC, selected-component CopyGC and the drawable
depth check on drawing; dash refusals and rectangle clipping including the
empty list and clip-mask None; ClearArea to the background pixel without
exposures; CopyArea pixel copies and NoExposure; CopyPlane plane expansion
from a same-depth and a depth-one source; PolyPoint in both coordinate modes,
thin horizontal, vertical, joined and diagonal PolyLine, PolySegment and
PolyRectangle pixels; PolyFillRectangle, FillPoly in both modes, the inscribed
full disc, angle truncation and the thin ring; ZPixmap and Bitmap PutImage
round trips, plane-mask, clipping of an outside image, the depth and Bitmap
refusals; and QueryBestSize replies for every valid class. None of that is
certification of the unexercised remainder.

The nine failures were queued as two repair rows, t128 for the six refusals
and t129 for the three exposure events, and the gate stayed red until they
landed, as it did for the destruction family. The repairs are recorded in the
next section.

## Drawing family repairs

The two repair rows landed on 2026-09-19 as commit `1bf9fddf`. Run on clean
source **1bf9fddf** with host SHA256
`ab8e580395bb8f5a8e883c178e54887b41f1d6f0641b0f68f68c14cff0ae033f`, evidence
at `.artifacts/x11-conformance/baseline-1bf9fddf/`: **120 executions, 120
PASS; gate exit 0.** Every case of the drawing family passes in both byte
orders, and the fifty earlier cases keep passing.

The nine obligations of the baseline table are met as the protocol states
them. CopyGC between contexts of different depth is a Match error, judged
after both contexts are found. A SetClipRectangles ordering above YXBanded, a
PolyPoint or PolyLine coordinate mode above Previous, a FillPoly shape above
Convex or coordinate mode above Previous, and a QueryBestSize class above
Stipple are Value errors refused by the decoder. A ZPixmap PutImage with a
nonzero left-pad, and an XY-format one with a left-pad at or above the bitmap
scanline pad, are Match errors. ClearArea with exposures reports the cleared
rectangle within a viewable window as one Expose with count zero. CopyArea
and CopyPlane with graphics-exposures report each destination rectangle whose
source lay outside the source drawable as a GraphicsExpose, in the order a
banded region lists them and counted down to zero, cut to the destination and
to the context's clip list; a wholly available source reports one NoExpose.
The Xorg reference (`dix/dispatch.c`, `mi/miexpose.c`) was read for the
details the protocol leaves open: the region algebra of the exposed area and
the event order.

Once the nine first obligations were met, the unexercised tails of the same
cases found more, all repaired in the same commit and each with a routed test
in `crates/sophia-x-authority/tests/x11_wire/drawing_completions.rs`:

| defect | repair |
| --- | --- |
| a Value error carried zero where the protocol puts the refused value | every decoder refusal of a value now names it in the error's resource field, so a client reads which argument was wrong |
| graphics-context components were stored without range checks, so `function 16` or `line-style 3` was accepted | CreateGC and ChangeGC refuse a component outside its enumeration, a dash of zero, and a line width or dash offset above 16 bits, naming the value |
| a tile or stipple was stored unseen, so an unknown pixmap or one of the wrong depth was accepted | a tile must be a pixmap of the context's depth and a stipple one of depth one; an unknown one is a Pixmap error, the wrong depth a Match error |
| CopyPlane copied a plane at or above the source depth | such a plane is a Value error naming the plane, after the source and destination are validated |
| ClearArea accepted any nonzero exposures byte as True | a byte above one is a Value error naming it |
| CreateWindow never decoded the class, so an InputOnly window was drawn into, cleared and measured like any other | the class is decoded (above InputOnly is a Value error) and recorded; an InputOnly window has depth zero, so no context matches it, ClearArea refuses it with Match, and QueryBestSize refuses a tile or stipple for it |

Two expectations in the cases themselves were wrong and were corrected before
the evidence run, each against the protocol text. An arc angle is an INT16 in
64ths of a degree, so the largest overshoot a request can carry is just under
512 degrees; the truncation case now sends 500 degrees rather than 720, which
does not fit. A thin arc outline follows "the infinitely thin path" that
"intersects the horizontal axis at [x, y+(height/2)] and [x+width,
y+(height/2)]", so a 7x7 outline at (1,1) spans pixel columns and rows 1 to
8, one more than the filled interior; the case now expects that box and
requires the outline to reach each of its sides.

Validation beyond the gate: the crate's offline tests pass (1153 unit and
367 wire tests, nine of them new), `cargo fmt --check`, `git diff --check`
and `cargo clippy --all-targets` are clean for the crate, and the gate's own
unit tests pass. Coverage is unchanged at 51 of 97 decoded core requests; the
remaining debt is the later slices named below.

## Gate and coverage

On 2026-09-12 the operator assigned Codex the broader independent X11 protocol
gate and subsequently made these protocol gaps the highest priority. t057 owns
the gate ahead of the individual repairs. Priority and open status live in
[todo.md](../../../todo.md), not in this evidence record.

The implementation and runnable commands are in
`tools/probes/x11_conformance/README.md`.
The host uses the production XServerFrontend, routed protocol broker and
concurrent worker paths, with deterministic software output facts and a private
Unix socket. It opens no display, session, device or VT. The Python client uses
independent protocol framing and assertions, not Sophia codecs or observations.
Both byte orders run, including oppositely ordered peers in cross-client cases.

The explicit manifest names mandatory behaviors, their core request numbers,
extension obligations, intentional policy exclusions and fixture limitations.
Missing/unexecuted mandatory results, NORESULT, unsupported/untested verdicts,
duplicates and deadlines fail. A decoder-declaration inventory prevents new
requests from disappearing from the coverage ledger. At the integrated baseline it inventories
77 decoded core requests: 28 have named cases, 49 have explicit coverage debt.
On 2026-09-19 it inventories 97 decoded core requests: 51 have named cases, 46
have explicit coverage debt.
DestroySubwindows and NoOperation are both named; both now independently pass. This is a substantial selected behavioral gate, not full X11 certification.
Query/version coverage does not certify every operation of an extension.

The request-family dispatch matches have wildcard fallbacks; declaring a wire
variant does not make the compiler require its dispatch implementation. The
inventory checks coverage accounting, while independent mandatory cases must
exercise accepted requests through dispatch and observe their completion.

## Historical candidate and evidence

The first meaningful baseline used c629cf7f. The first retained baseline used
the production source at acaa8453, including Claude's DestroyNotify repairs
fa23b570 and b3941c04, plus the uncommitted conformance host/harness. The report
records the dirty-source flag, harness/manifest hashes and host SHA256:

`e8f9755719481b28ac3007e4e6aecdac3e557c9dc4a8c0fcec9ac4fa29bc6216`

Retained evidence is under
`.artifacts/x11-conformance/baseline-acaa8453/` in the main checkout. Its
`report.json` contains every verdict and identity; host logs are per case.
Reproduce with the documented one-command gate and a fresh output directory.

**62 mandatory case/order executions: 46 PASS, 16 FAIL/TIMEOUT; gate exit 1.**
The eight failing behaviors fail in both byte orders. Timeouts here mean absent
mandatory completions at the fixed three-second deadline, not a claim that a
driver or the physical desktop hung.

Passing groups include setup resource ranges, window creation/tree/geometry,
map/configure, properties, cross-client selection ownership/transfer, focus,
pointer/keyboard grab contention, disconnect cleanup and grab release,
truncated-peer isolation, extension discovery/version negotiation, selected
SHAPE/SYNC stateful operations and deliberate extension absence.

Claude's explicit DestroyNotify repair independently passes: unmapped destruction,
both event addresses for StructureNotify/SubstructureNotify on a second client,
neither-mask suppression, invalid/repeated destroy without a phantom event,
and XID reuse without inheriting a retired subscriber. Those results do not
close the destruction family.

## Integrated baseline after the destroy repairs

The coordinated rerun used clean committed source **75b9e167**, merging the gate
with master **34413a16**, including descendant repair **4ede41bd** and
DestroySubwindows repair **cc9f2f7b**. Host SHA256:

`f507d8109858312d0198c8abf510abf4a569768c95413a094a013b753bf3a80d`

Evidence is retained separately at
`.artifacts/x11-conformance/baseline-75b9e167/`; the acaa8453 evidence remains
historical and was not overwritten. **62 executions: 50 PASS, 12 FAIL/TIMEOUT;
gate exit 1.** Twenty reporting regressions also pass.

Only `destroy_descendants` and `destroy_subwindows` changed verdict, both from
FAIL to PASS in both byte orders. The six remaining failing behaviors are
UnmapNotify (t084), NoOperation (t085), ListExtensions (t086), peer-close
DestroyNotify (t087), XFIXES selection notification (t063), and unknown extension
minor error classification (t088). No other verdict changed. The selected
explicit-destroy, subscription, invalid-ID and XID-reuse cases remain passing.
The gate and remaining coverage work stay first under t057; all six repairs
remain open at the highest priority in todo.md.

## Expanded lifecycle baseline after disconnect notification landed

The unchanged 62-execution profile was rerun on **d9c49d85**. Peer-close
DestroyNotify changed from TIMEOUT to an ordering FAIL: all three structure
events arrive, but name parent, child, grandchild in that order. The other
verdicts remain unchanged: 50 PASS and 12 FAIL/TIMEOUT, exit 1.

Four additional mandatory cases then landed in **cc577db5**, tested against the
same runtime repair on clean committed source. **70 executions: 54 PASS,
16 FAIL/TIMEOUT; exit 1.** The increase includes eight newly required
case/order executions; these counts must not be compared as the same profile.
Host SHA256: `fa48aecbbb6d14b02d22cd23eed20c84d0590b7549d5b8786fd526d6b2e42a95`.
Evidence: `.artifacts/x11-conformance/baseline-cc577db5/`. The original-profile
rerun is retained at `baseline-d9c49d85/` beside the earlier baselines.

- `destroy_subwindows_order` passes both orders after moving the newer child
  below the older one. Each child's subtree dies first, in the required sibling
  stack order. A temporary isolated runtime mutant replacing stack-rank sorting
  with XID sorting fails this case in both orders; the original passes. Source
  and build cache were restored before the clean baseline. The selected-case
  experiment is retained in `destroy-order-mutation/`; it is not a full gate run.
- `destroy_subwindows_invalid` passes both orders: empty/repeated requests do
  not destroy the parent, and invalid/already-destroyed targets produce BadWindow
  without phantom events.
- `destroy_peer_close_subscribers` times out in both orders. It receives
  `(parent,parent)`, `(root,parent)` and `(child,child)`, but never
  `(parent,child)`. Teardown retires the parent's subscriptions before routing
  the child's parent-addressed event. This is a residual t087 defect, alongside
  the independently failing descendant-before-ancestor ordering case.
- `destroy_mapped` fails both orders: after confirmed Viewable state, explicit
  DestroyWindow produces only DestroyNotify, omitting the automatic UnmapNotify.
  This extends t084's existing notification gap and t087's lifecycle acceptance;
  no duplicate task is needed.

The [X11 protocol](https://xorg.freedesktop.org/archive/X11R7.7/doc/xproto/x11protocol.html)
requires descendants before ancestors in the DestroyNotify event definition,
including when destruction follows connection close. DestroySubwindows also
requires bottom-to-top child order. DestroyWindow on a mapped window performs
an automatic unmap before destruction. These checks do not impose a sibling
order on ordinary DestroyWindow beyond the protocol's ancestor constraint.

Twenty reporting regressions still pass. Actual XTS remains unrun for the
previously recorded dependency blockers. The temporary mutation touched only
the isolated worktree; no runtime repair is included in the harness commits.

The installed `1a59ab8c1406` v6 pinentry observation remains separate historical
evidence: its successful DestroyWindow API call was not observed as probe-client
major 4 dispatch, and `running_drop` did not return. The socket experiments above
identify their compiled source and distinguish explicit requests from connection
cleanup; they neither ran that installed release nor establish a pinentry cause.

A follow-up build at clean **c2745124** used a completely new dedicated target,
`/tmp/sophia-x11-fresh-cc577db5`, after p5 reported possible include-file freshness
problems when sharing a target across archived sources. All 70 verdicts match
exactly (54 PASS, 16 FAIL/TIMEOUT, exit 1). Its host SHA256 is
`fa48aecbbb6d14b02d22cd23eed20c84d0590b7549d5b8786fd526d6b2e42a95`; evidence is retained in
`.artifacts/x11-conformance/baseline-fresh-c2745124/`. This validates this gate's
comparison independently of its earlier build cache; it does not resolve p5's
separate historical/repaired arboard comparison.

## Independent acceptance of the enumeration and disconnect repairs

The next clean merged candidate **5b4d3b02** includes **18cc2488** (ListExtensions)
and **9afce409** (deepest-first disconnect cleanup and recipient retirement).
It was built in another fresh target, `/tmp/sophia-x11-fresh-9afce409`.
**70 executions: 60 PASS, 10 FAIL/TIMEOUT; gate exit 1.** Host SHA256:

`035f6569fceedf668d75d8631d0ac6b66de7fad27ab515a1acc3ddb58cbfd731`

Evidence is retained at `.artifacts/x11-conformance/baseline-5b4d3b02/`.
Exactly three cases changed to PASS in both byte orders: `extensions`,
`destroy_peer_close`, and `destroy_peer_close_subscribers`. All other verdicts
match the prior expanded profile. Enumeration now lists the fifteen expected
software-frontend extensions, agrees with their independent QueryExtension
checks, and excludes DRI3 without a provider. Claude's dispatch/frontend tests
also cover the declared set and provider-absence filtering. This satisfies t086's
enumeration repair exit; no GPU-provider or physical acceptance is claimed.

Disconnect now delivers descendants before ancestors, preserves both subscribed
event addresses until routing completes, suppresses events for an unsubscribed
peer, retires the resources, and permits a healthy watcher to continue. The
mapped-destroy UnmapNotify case remains failing, so t087's remaining selected
acceptance is tied to t084. The other failures remain NoOperation (t085), XFIXES
selection notification (t063), and unknown extension minor errors (t088).
Twenty reporting regressions pass. Actual XTS remains unrun.

## UnmapNotify

The `unmap` case receives MapNotify and confirms the window is Viewable, issues
UnmapWindow, completes its following round trip, then receives no UnmapNotify.
This is independently reproduced in both byte orders. The UnmapWindow arm of
`dispatch/core/windows.rs` changes runtime state but returns an empty output
vector on success. The existence of an event encoder on other paths does not
deliver this request's notification.

t084 must implement the successful transition's structure and immediate-parent
notifications, preserve subscription filtering and ensure an already-unmapped
window or invalid ID does not generate a phantom transition. Its exit is the
real routed wire case plus named no-transition/parent-subscription controls.

## NoOperation

The `reply_errors` case establishes BadWindow, BadLength and BadRequest
completions, then sends core opcode 127 followed by a GetGeometry round trip.
Sophia emits BadRequest for the NoOperation sequence. This is not a missing
GetGeometry reply: the extra error arrives first and breaks correct completion
accounting. Opcode 127 is absent from the core decoder.

t085 must accept NoOperation without output, preserve subsequent sequence
completion and handle its permitted padding. Removing the unexpected error
from the test would conceal the omission.

## ListExtensions

Repaired by **18cc2488** and independently verified on **5b4d3b02**; see the
acceptance baseline above. The following is the original defect evidence.

At the original baseline the `extensions` case received an empty ListExtensions
response, while the
separate `extension_discovery` case confirms fifteen advertised names and their
distinct opcodes. `client_output/replies/core_early.rs` hardcodes zero names.
t086 must enumerate the actual frontend's advertised surface and keep it
consistent with QueryExtension, including provider-dependent availability.

An early harness expected DRI3 in this software-only host. That expectation was
wrong: `connection/dispatch.rs` explicitly suppresses its advertisement without
a render-device provider. The corrected manifest records DRI3 as a fixture
limitation. No DRI3 runtime defect is filed from that observation.

## Destruction family

Three independent cases failed at acaa8453 after b3941c04:

- `destroy_descendants`: GetWindowAttributes on a child still returns a valid
  reply after its parent is destroyed; the child lifecycle has not ended.
- `destroy_subwindows`: opcode 5 returns BadRequest before the round-trip reply.
- `destroy_peer_close`: another subscribed client receives no required
  DestroyNotify after the owner connection closes.

The [destruction-family investigation](ksbt5d8f-the-window-destroy-family-is-incomplete-beyond-destroynotify.md)
owns the source analysis. Its two source findings are now repaired and the note
is closed. The later independent peer-close and remaining manifest family cases
now pass, and t087 is closed with the current wire evidence above. Descendant destruction must
precede truthful descendant notifications; adding only events would announce
destructions that never happened. Keep nested ordering, peer-close parent
subscriptions and resource/subscription retirement in that same lifecycle repair.

## XFIXES selection notification

`xfixes_selection` negotiates XFIXES, successfully selects owner-change events
on another connection, changes ownership, and waits for the advertised event.
At the original baseline no event arrived in either byte order, independently
confirming t063. Commits 8faab7d9 and 3f4b0462 implement the notification lifecycle;
all twenty expanded XFixes executions now pass. The current-result section
records the remaining review conditions and evidence; the t063 plan remains
the scope owner.

## Extension error classification

At the original baseline, the first failure in `extension_errors` was Present minor 255. Sophia returns
BadImplementation (17), with the correct major/minor/sequence, instead of
BadRequest (1). `wire/extensions/present.rs` categorizes every unmatched minor
as PresentUnimplemented; its dispatcher then returns BadImplementation.

t088 must distinguish an unknown minor from a recognized operation that is
not implemented, preserving explicit completion and healthy-client continuation.
The core [X11 specification](https://xorg.freedesktop.org/archive/X11R7.7/doc/xproto/x11protocol.html)
defines Request for an invalid major/minor opcode; the inspected XLibre
`Xext/present/present_request.c` also returns BadRequest after its dispatch
switch. The current grouped case stops at this first refusal mismatch; it does
not establish that later extensions' error classifications passed.

At runtime tip 528803aa the grouped case passes Present and reaches Generic
Event Extension opcode 136, minor 255. A complete four-byte request receives
BadLength (16), in both byte orders. `wire.rs` validates QueryVersion's eight-byte
length before checking whether the minor is QueryVersion at all. Repair cb07cafc moves the minor check ahead of that request-specific length
check. The final grouped case passes all advertised extensions in both orders;
t088 is now independently accepted. The intermediate failure remains retained.

## XTS and reference-test limits

Reviewed yserver's `xts-run.sh`, `xts-vs-baseline.py` and standalone XCB/Xlib
probes. The comparator accepts baseline PASS becoming NORESULT, UNSUPPORTED,
UNTESTED or NOTINUSE and ignores missing candidate purposes for its exit.
Those semantics were not reused. Its hardcoded /home/jos paths and live-display
launcher were not run.

XLibre's `test/pyxtest` and `test/xi2` provide useful independent framing,
malformed-request and swapped-byte-order patterns. No XLibre/Xorg/Xvfb server
or hardware test was launched, and no external source was copied.

The optional runnable XTS adapter uses a private copied suite, fresh configuration
and journal, isolated socket/network/device namespaces and exact mandatory TET
purpose accounting. Dependency preflight reports missing separate
`~/src/xts/check.sh`, built `xts5`, and TET `tcc`; a real selected scenario and
purpose manifest also require that build. That was true until 2026-09-20;
see the section below, which supersedes it. **A real XTS5 scenario has now
run.**

The adapter was executed with explicitly synthetic fixtures: PASS exits 0;
PASS-to-NORESULT, a missing selected purpose and timeout after a PASS journal
each exit 1. Evidence is in `.artifacts/x11-conformance/synthetic-xts-*`;
the dependency report is in `xts-dependencies`. These prove adapter mechanics,
not XTS coverage. Missing dependencies remain an explicit t057 integration
limit rather than a fabricated skipped-suite success.

## XTS5 selected-core, and the repairs it found

The suite was obtained, built and run on 2026-09-20 against the software
fixture host. `selected-core` is six cases and 58 purposes, enumerated from
the built suite by `tools/probes/x11_conformance/xts_select.py` rather than
written by hand. The progression, each step measured:

| run | PASS | FAIL | UNRESOLVED |
| --- | --- | --- | --- |
| first | 0 | 0 | 58 |
| after ForceScreenSaver (115) | 31 | 16 | 5 |
| after WarpPointer (41) | 31 | 16 | 2 |
| after five repairs | 36 | 11 | 2 |

Nothing ran at first: every test's startup calls `XResetScreenSaver`, opcode
115 was undecoded, and the harness's `unexp_err` deletes a purpose on any
unexpected error. Two opcodes were then decided and decoded under t125, and
five differences repaired:

| defect | repair |
| --- | --- |
| ChangeProperty accepted an atom naming nothing, storing it as a new property | both atoms validated, property before type, both after the window |
| Append or Prepend with a differing type or format answered `BadValue` | it is a `Match` error; the dispatch no longer collapses every non-`AuthorityOwned` property error into one code |
| GetSelectionOwner answered "no owner" for an atom naming nothing | an atom that names nothing is refused; unowned and unnamed are different facts |
| destroying the root answered `BadWindow` | the root is a real window with no parent: the request succeeds and destroys nothing |
| destroying a window announced an `UnmapNotify` for every mapped inferior | only the named window is unmapped; `XDestroyedWindow::is_subtree_root` gates both destroy arms |

Each has a routed control in `tests/x11_wire/transport_events.rs`. No
existing test pinned any of the old answers.

**Still open, and the sharpest of them: SetInputFocus accepts a window that
is not viewable**, where the protocol requires a `Match` error. The check is
four lines. The difficulty is that every placement reaches Engine's own focus
application: in `runtime.set_input_focus` it fails two tests, and at the
socket request path it fails eight. Those fixtures focus a bare window id
that no window was ever created for, so they are unrealistic rather than
evidence that Engine legitimately focuses a window before it is viewable.
The right resolution is therefore to make the Engine focus authority obey the
X11 rule and correct the fixtures to build real viewable windows, rather than
to exempt Engine from the protocol this system implements. That is a change
to the focus contract, not a conformance repair, and is tracked as its own
row.

Also measured and not repaired here, in the same run: focus does not revert
when a focus window becomes unviewable and no `FocusIn`/`FocusOut` is
generated (the largest remaining item); mapping an already-mapped window
emits a `MapNotify`; `SubstructureRedirectMask` is not honoured, so no
`MapRequest` is generated and the window is mapped anyway; atoms are not
cleared when the last connection closes; and three `XMapWindow` purposes fail
on pixel checks.

## Validation and remaining work

Twenty gate/reporting regressions pass, including absolute timeout despite
continuing output, empty/missing results, duplicate records, unexecuted manifest
obligations, numeric/textual TET disagreement, inventory drift and Python -O
refusal. The new host passes Clippy with warnings denied. `cargo fmt --all
--check` and the X-authority's offline all-target tests pass, with inherited
SOPHIA_/HAGIA_ opt-ins and display endpoints removed.

The full `cargo xtask check` was not run: it invokes render-node hardware proofs,
which the operator explicitly excluded from this assignment. No physical or
pinentry acceptance is claimed. The user-facing gate deliberately remains red
on the retained baseline until the mandatory protocol repairs land.

The new priority order is recorded only in todo.md. Gate implementation,
remaining core/extension coverage, missing XTS dependencies and individual
runtime repairs have separate exits; none is closed by an encoder round trip.

## Connections

- [Destruction-family record](ksbt5d8f-the-window-destroy-family-is-incomplete-beyond-destroynotify.md)
  owns t087 and cites the independently verified partial repair.
- [t063 plan](../plans/queue-11-parallel-production-readiness.md#t063) owns the
  previously filed XFIXES event omission.
- [t082 investigation](iux6ctsy-pinentry-submission-stalls-before-gui-exit-and-input-recovery-remains-blocked.md)
  remains separate. These protocol findings are not proof of its native-loop cause.
- [Family conformance](../plans/queue-09-cp-15-2-one-family-level-conformance-surface.md#t023)
  concerns Sophia's native role protocols; this X11 gate does not close it.
