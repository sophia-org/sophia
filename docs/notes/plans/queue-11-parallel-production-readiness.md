---
id: queue-11
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# Parallel Production Readiness

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.


These rows do not reorder the critical path.


Previously completed evidence: [Shell reference preparation: generic boundary documented, Quickshell fork and sophia branch established, Void baseline built, and panel/popout requirements and results retained.](../sources/2026-09/todo-cutover-completed.md#legacy-done-015).


Previously completed evidence: [Document descriptor and content shell models and the proposed content-shell behavioral contract: explicit operator admission, panel/popout lifecycle, input and visual trust boundaries, and…](../sources/2026-09/todo-cutover-completed.md#legacy-done-016).


## t024

Repair current native-session readers and guard them against schema drift.
Preserve the two-xterm and Milestone 3 readers for historical archives, and
retire their live launchers before any hardware or service action. Keep the
Milestone 4 GPU diagnostic, including its Present accounting requirements.

The original counts of ten completion readers and nine WM-ready readers were
an August snapshot, not a current inventory. Completion now distinguishes
startup-proof schema 16 from normal-session schema 17. Check record name and
status together, and retain each reader's proof requirements and historical
compatibility. The exit is passing schema mutation tests, the affected verifier
fixtures, retirement checks, and `cargo xtask check`. No physical milestone is
closed by repairing its evidence reader.

The [reader investigation](../investigations/lqicnr4v-evidence-readers-must-follow-message-identity-and-proof-requirements.md)
records the inventory, repairs, archive decisions, and validation.


## t025

**Decision and result, 2026-09-10.** Clean, cryptographically signed local
commits qualify for physical proofs independently of upstream publication.
Implemented in signed candidate `2cb47a1b90c31f24b3c46da55cd24699dc7009d4`.
The frame-fed and critical-path runners now follow this rule, as does the
Hagia policy runner called by the critical-path runner. The preflight reporter
retains an informational upstream column but refuses only missing checkouts,
dirty trees and invalid signatures. Pinned source identities, binary hashes,
archive verification and DRM/input safeguards retain their existing checks.

The prior packaging description was stale: `package_live_session.sh` already
accepted local commits without upstream equality. This change aligns the
remaining gates with that policy and the direct-scanout/Hagia native runners.
[Validation](../../validation.md) and [Hagia](../../project-hagia.md) describe
the resulting proof contract.

**Verification.** Four isolated Python tests exercise 66 preflight scenarios
across the production script sections: matching/missing/divergent upstreams,
dirty and untracked files, invalid signatures, missing checkouts and source
identity changes between checks. The original scripts fail the eight new
missing/divergent-upstream acceptance cases; the repaired scripts pass all
cases. Bash syntax checks passed. `cargo xtask check` passed 3,034 Rust tests
across 260 result groups, with zero failures and 29 intentional ignores, plus
the registered Python checks, formatting, Clippy, archive verification and
host render-node proofs. Local logs are retained under
`.artifacts/t025-proof-identity/` (`check.log`, `baseline-regression.txt`, and
`validation.txt`). No TTY takeover, physical gate or installed-session
acceptance was performed for this tooling change.


## t026

The [Session and native-owner source splits](../investigations/izw9opes-private-session-tests-can-move-without-widening-the-production-api.md)
record the validated non-X-authority portion; X-authority debt stays in the ledger.

Move remaining session-private test modules out of production `src` as
visibility boundaries permit, and split the oversized cohesive units named in
`docs/source-layout-debt.txt`. Do not weaken privacy or add test-only
production APIs.


## t027

Reduce `tools/start_sophia_tty3.sh` to the minimum TTY/display-manager
adapter around `sophia session run`. Typed parsing, verification, archive
handling, and gate orchestration stay in Rust.


## t028

**Result, reconciled 2026-09-10.** Implemented by `a382563ff7860a66eadb2d8eb5aa7f739692b7c0`.
The wire reader preserves partial reads across socket timeout boundaries,
classifies records before reading an extended body, and reports decoded X
errors. Cross-connection tests use a round-trip barrier before referring to
another connection's resources; startup connects retry listener races.

The original diagnosis incorrectly singled out Present GenericEvent type 35:
both replies and GenericEvents legitimately carry a length at bytes 4..8.
Errors and core events carry payload there. Treating that payload as a length
could desynchronize the stream or wait for a body that would never arrive.
The repair addresses framing and ordering rather than only increasing timeouts.

**Evidence.** The implementation commit records an initial four failures in
ten loaded runs and twenty consecutive passes after repair under greater load.
That is historical commit evidence, not a new stress run. The current
`cargo test --offline -q -p sophia-x-authority --test x11_wire` passed all 340
tests at `1f193b35` during this review, with no ignored tests. The suite has
grown from the original 178-test baseline. No new wire implementation or
physical acceptance claim is part of this reconciliation.



Previously completed evidence: [Implement RENDER.](../sources/2026-09/todo-cutover-completed.md#legacy-done-017).


## t029

Implement `SHAPE`. Quickshell asks for it in the same trace. Small: a
handful of requests for non-rectangular window regions, and Sophia already
carries region machinery for XFIXES.

**Result.** Implemented at version 1.1 and advertised. The "small" framing
above was wrong in one load-bearing way, and the correction shaped the work:
Sophia's `Region` was a bare rectangle list with no set algebra, and XFIXES
implemented five minors while answering version 6.0. Region algebra had to be
built before SHAPE could combine anything.

Landed in four commits, each gated and independently useful:

- `87c09c12` region set algebra plus the XFIXES region minors (Copy, Union,
  Intersect, Subtract, Invert, Translate, RegionExtents, FetchRegion).
- `6264db79` the shape store and all nine requests, deliberately unadvertised.
- `4e26bf78` bounding shapes clipping composition.
- `3bf967b2` input shapes honoured in hit testing, and the advertisement.

**Evidence.** `x-authority-quickshell-smoke` reaches opcode 145: 35 opcodes and
331 requests, up from 34 and 329 before, with the
`sophia_x11_authority_extension status=absent name="SHAPE"` line gone. Twelve
dispatch tests cover the tri-state, all five operations, masks read from
depth-1 pixmaps, change gating, and validation; two engine tests cover
click-through. The algebra is compared against a brute-force cell model over
every pair in a small grid, and both `subtract` and the vertical-coalescing
invariant were mutation-checked (an inverted condition fails four tests, a
disabled coalesce fails three). See the `SHAPE window regions` and
`XFIXES region minors` rows in `docs/x11-compatibility-matrix.md`.

**Why the advertisement waited.** A Qt panel's first use of SHAPE is an input
shape for click-through. Advertising with shapes merely stored would have been
the MIT-SHM over-promise again, so phases two and three shipped dark and the
advertisement flipped only once clicks genuinely fell through.

**Adapted from yserver** (`~/src/yserver`, MIT, Copyright (c) 2026 Jos Dehaes):
the half-open banded region design and its brute-force test approach, the
bitmap-to-region reader, the unset/empty/concrete tri-state, and change-gated
notifies. Their `ShapeInvert` aliases to Set, which is wrong; Sophia implements
source-minus-destination and tests it. They also validate no arguments, and
Sophia does.

**Remaining limits.** A descendant window's shapes are stored, answered and
notified but do not clip the parent presentation or affect intra-toplevel
routing. `ShapeClip` is stored and consumed by nothing, because Sophia composes
whole client buffers and draws no window borders. A shaped window falls back to
scaling its 1x raster on a non-1x display. Grabs capture regardless of shape,
which is what X defines. No physical acceptance: the evidence above is the
offline probe and deterministic tests.

**Debt found on the way.** XFIXES answers version 6.0 while implementing a
subset. The region minors now answer and the rest refuse by name with a
two-tier code rather than failing to parse, but the version claim still
overshoots its implementation and should be settled -- implemented on demand,
or clamped -- as its own decision.


Previously completed evidence: [Implement XC-MISC, before something needs it.](../sources/2026-09/todo-cutover-completed.md#legacy-done-018).


## t030

Decide, rather than implement, `Composite`, `DAMAGE`, `XTEST` and `DPMS`.
Each is a domain Sophia owns -- compositing, input, power -- and a client
reaching through one of them is asking to step around that authority. They
belong in the matrix as deliberate exclusions or as admitted surface, not as
gaps that stayed open because the list looked incomplete.

**Reference.** The `~/src/yserver` survey done alongside t029 supplies what each
would cost and what it buys, from a server that implements all four and runs
whole desktops. That is evidence rather than speculation, and it is why these
decisions can be made now instead of deferred again.

**Composite -- excluded, with a named admission price.** Sophia is the
compositor and Hagia is the only window manager; redirection is authority
Sophia does not delegate. yserver resolved the same tension by handing
compositing over: when a client claims the overlay window their scene emits
only root, overlay and cursor, which is Xorg's contract. If a measured client
ever needs this, that is the shape to adopt, and their capability flag
(record redirects and answer `NameWindowPixmap` before allocating real
backings) is the staged path. Panels and thumbnailers want `NameWindowPixmap`
specifically. Until the refusal log names one, absent by decision.

**DAMAGE -- excluded for now, and now measured.** The original rationale said
its consumers are external compositors and screen scrapers. That was wrong
about who *asks*: GTK4 zenity queries it, and the GTK3 probes added under t006
show mousepad and Thunar querying both `Composite` and `DAMAGE` at startup.
What the measurement does support is the decision: all three find neither,
continue, and complete startup with no error, which is the clean fallback the
exclusion assumed and had not previously seen a client perform. One lesson to
keep if it is ever admitted: yserver runs three separate region machineries
with written justification, because client-facing damage reports and internal
repaint damage answer different questions. Their presentation damage subtracts
by exact-match rather than geometrically, on purpose. Do not alias the two.

**XTEST -- excluded, and the reason is not cost.** It is four requests, and
yserver injects at the same entry point real libinput events use, which is
also what Sophia would do. What stops it is that theirs is entirely ungated,
and their own design notes record that as a known gap: any client can drive
the pointer and keyboard. Sophia's input is session-owned authority, so
synthetic input needs an explicit admission story before it exists at all.
Revisit when conformance tooling (xts5 drives the mouse through XTEST) makes
it worth designing that story.

**DPMS -- excluded; power is session authority.** yserver's is real, driving
DRM atomic commits that disable connectors, guarded by a scanout check after a
VT-switch left outputs half-disabled mid-modeset, and coupled to
MIT-SCREEN-SAVER in Xorg's order. If Sophia admits DPMS it must route through
session power ownership rather than authority-side connector writes. Until a
client asks, absent.

**XFree86-Bigfont -- leave absent, log-driven.** Surfaced by the xterm probe
during t029. yserver has nothing, not even a refusal note, and xterm works
there; Xlib falls back cleanly. No action unless a client fails rather than
falls back.

**A pattern worth borrowing if VidMode writes ever appear.** yserver keeps
XF86VidMode deliberately read-only, advertising read permission and failing
writes with the extension's own `ClientNotLocal` error -- a branch clients
already handle -- rather than `BadRequest`, because RandR owns display
configuration. That is a better shape than refusing outright for any legacy
extension whose read surface is useful and whose writes cross an authority
boundary.


## t059

Settle the `XFIXES` version claim. The server answers 6.0 while implementing a
subset of the minors that version defines. Found during t029, which added the
region minors (Copy, Union, Intersect, Subtract, Invert, Translate,
RegionExtents, FetchRegion) and converted the remaining unimplemented minors
from a parse failure to a refusal that names them with a two-tier code. What is
left is the version claim itself: cursor naming and images, pointer barriers,
save-set changes, and the client-disconnect modes are advertised by the version
and not implemented.

Either implement what a measured client asks for, or clamp the answered version
to what is behind it. The precedent is RENDER, whose advertised version moved
only as the requests behind it started answering; the counter-example is
MIT-SHM, which advertised 1.2 with two opcodes missing and sent Qt into its
error handler. Lowering an already-negotiated version is a behaviour change to
shipped clients, which is why it is its own decision rather than a fix folded
into t029.

### Outcome

The task offered two ways out and **the evidence rules out one of them**:
clamping cannot produce an honest claim at any level, because the implemented
set is not a version prefix. XFIXES orders its ladder so that version 1 is
minors 0-4 (save-set and cursor) and version 2 is 5-27 (every region request);
Sophia implements a middle slice, with gaps beginning at version 1 while
everything that works lives at version 2. A version number can only express a
prefix, so no number describes this server.

Clamping was measured rather than assumed. Disassembling this host's libXfixes
3.1.0 shows *no* region entry point gating on the version; only cursor images,
cursor naming, Hide/Show, barriers and disconnect modes gate at all, and xcb
gates nothing. So clamping would neither break regions nor make any claim true.
It would only lower a number.

**Decision: keep 6.0 as explicit documented policy, and let the named
two-tier refusal carry the honesty.** Minor 34 has a reply, so 6.0 usefully
invites a probe this server unblocks with `BadImplementation` where silence
would hang it. Yesterday's RENDER work also established that clients send
requests above the advertised version without reading it, so the version is a
claim the server makes for its own honesty, not a gate the client respects.

Implemented here: the region constructors (minors 6-9) and `ExpandRegion` (28),
each wiring onto a store that already existed. Repaired: `FetchRegion` wrote a
count where the protocol puts extents, and `QueryVersion` never clamped.

**A crash was caught before it shipped.** Minors 20-22 briefly had decoders
with no dispatcher behind them. A decoder without a dispatcher is worse than no
decoder: the request decodes, misses every family matcher, and reaches
`dispatch.rs`'s `unreachable!`, so any unprivileged client could have taken the
server down by sending one. The rule is now that a minor is decoded only once
something answers it, and
`every_xfixes_minor_is_answered_rather_than_escaping_dispatch` sweeps the whole
minor range to hold it. That test was mutation-checked: reintroducing a single
undispatched decoder makes it fail with exactly that panic.

Evidence is `wire`. The `x-authority-zenity-smoke` and `x-authority-gtk3-smoke`
traces both report `first_error=none` but are **not** XFIXES evidence -- neither
reaches opcode 138 offline, because with no window manager nothing is admitted
and GTK never gets far enough to compute a region. This is the same limit
recorded elsewhere: a headless probe cannot prove a path unused.

Limits carried forward, each with a reason rather than as undifferentiated debt:

- Minor 2 `SelectSelectionInput` is accepted and validated, but no
  `XFixesSelectionNotify` has ever been encoded. Clients believe they
  subscribed. This is the top measured demand among the unimplemented and is
  **split out rather than folded in here**: it needs an event fan-out that has
  no in-tree precedent (`ShapeNotify` has none), it lands in
  `protocol_routing.rs` where concurrent work is active, and refusing it
  outright is dangerous because GDK dies on unguarded XFIXES errors.
- Minor 20 `SetGCClipRegion` is blocked concretely: the core
  `SetClipRectangles` decoder discards the clip origins before they reach the
  runtime, so there is nothing stored to install a region against. Minors 21
  and 22 are implementable and were left to their own commit.
- Cursor minors (3, 4, 23-27, 29, 30) are blocked on the same
  authority-to-engine cursor plumbing that RENDER's stored ARGB cursors already
  name as a follow-up, not fresh debt.
- Pointer barriers (31, 32) and disconnect modes (33, 34) sit across the engine
  and session-authority boundaries by design.

## t063

**Accepted 2026-09-12:** 8faab7d9, 3f4b0462, 1a631234 and 385282b4 deliver
all three subtypes, preserve ownership timestamps, sequence self-notifications,
retire subscriptions on normal and early-error teardown, disconnect stalled
watchers and carry retirement atomically with the client release. The independent
final gate on runtime 9be53aff passes 100/100 executions, including twenty XFixes
executions in both byte orders. Separate Rust coverage checks Confined namespace
exclusion against a positive delivery control. The runtime repair does not imply
installed acceptance or coverage of every XFixes operation. See the
[independent evidence](../investigations/wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md).

The original diagnosis and intended contract follow for context.

Deliver `XFixesSelectionNotify`. Minor 2 `SelectSelectionInput` is accepted and
validated, and then dropped: ten measured requests with `event_mask=0x7` against
real selection atoms, and no event has ever been encoded with the advertised
base 66. Clients believe they subscribed. Split out of [t059](#t059) rather than
folded into it, for three reasons that are about risk rather than size.

First, there is **no in-tree precedent for the fan-out**. The obvious model,
`ShapeNotify`, turns out to have no fan-out at all -- it is delivered to the
requesting client, not distributed to subscribers -- so this needs a
subscription-and-delivery path built rather than copied.

Second, it lands in `protocol_routing.rs`, where concurrent work is active.

Third, refusing minor 2 in the meantime is **not** a safe interim position: GDK
dies on unguarded XFIXES errors, which the zenity trace shows. Accepting and
dropping is a lie, but refusing is a crash, so the silent acceptance stays until
the events are real.

`xXFixesSelectionNotifyEvent` is 32 bytes: type, subtype, sequence, window,
owner, selection, timestamp, selectionTimestamp, two pads. Subtypes are
`SetSelectionOwner`(0), `SelectionWindowDestroy`(1) and
`SelectionClientClose`(2), with mask bits `1 << subtype`.

Emit from the existing `XSelectionMonitor` change points -- `apply_event` and
`clear_window_owner` in `selection.rs` already carry owner transitions. Every
valid SetSelectionOwner assertion notifies, including reassertion of the same
owner; it can signal changed selection contents. The earlier owner-XID-change
restriction was incorrect: XLibre `dix/selection.c` invokes the selection callback
for every valid assertion, and `Xext/xfixes/select.c` forwards it. Subscriptions
key on `(client, window, selection)`
and must be dropped on window destroy and on disconnect. Tests: one notify per
owner change with the right subtype, none for a non-subscriber, the mask
filtering subtypes, and teardown on both destroy paths.


## t060

Make pointer queries reflect the latest input admitted to their namespace.
The original diagnosis here missed the socket-layer reply patch: a connection
that had received pointer input could answer correctly, while another
connection in the same namespace still answered zero. The
[investigation](../investigations/knjco01f-pointer-queries-must-share-admitted-namespace-state.md)
records the live observation, reproducer, repair, and validation limits.

Keep Engine's physical-input and scene authority intact. Share query state
within the X namespace, resolve current X descendants and coordinates, and
preserve logical button/modifier state through grabs, freeze, and revocation.
Core and XI replies must agree without requiring event subscriptions. Tests
must cover two clients, both byte orders, confinement, geometry and resource
changes, and input lifecycle transitions.

The acceptance exit is passing deterministic checks followed by one installed
menu-placement and drag check. The unrelated `TranslateCoordinates` offset
repair remains `ae7b0929`; completing its child field is outside this repair.

Every row above came from measurement rather than a survey of what a server
usually has. `QueryExtension` now records what it refuses, so the next live
session extends this list by observation; the four decisions above should be
revisited against a week of real logs rather than against this paragraph.

Completed infrastructure baseline: `sophia-session` owns production lifecycle,
`sophia-conformance` owns development-only evidence logic, `cargo xtask` is the
canonical developer/CI surface, `just` is optional human shorthand, canonical
installed commands live under `sophia session`, and source-layout debt is an
exact identity ledger.

## t064

Resolved 2026-09-24: the region now rides with the surface's projection
metadata into both constructors, proven headless
([note](../investigations/0kkfrvlt-a-shape-input-region-never-reached-the-live-pointer-projections.md)).

The t061 popup audit found that both live pointer-projection constructors in
`production_visual_runtime/projection.rs` set `input_region` to `None`. The
authority transport and direct-layer hit test support SHAPE input regions, but
that coverage does not establish live click-through. This corrects the earlier
t024 evidence claim; it does not undo its wire or bounding-shape work.

Preserve the protocol-neutral region through committed and retired input
projections without giving the blind WM X11 shape data. Keep region coordinates
consistent with the geometry actually presented, retain unmap/destroy guards,
and distinguish unrestricted from explicitly empty input. The exit requires
runtime tests using the real live projection and hit-test path, coverage for
replacement, resize and stale retirement, and an installed Quickshell panel
check that clicks pass through excluded portions. This is a candidate follow-up,
not additional t061 implementation scope.
