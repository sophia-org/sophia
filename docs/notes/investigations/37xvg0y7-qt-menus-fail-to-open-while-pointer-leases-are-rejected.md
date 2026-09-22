---
id: 37xvg0y7
date: 2026-09-07
kind: investigation
status: investigating
tags: [investigation]
---
# Qt menus fail to open while pointer leases are rejected

## Driver and scope

The user opened a text file from Thunar in Okular and reported that menu
clicks open nothing and mouse interaction is spotty. This follows the short
successful t065 hide-and-resume check in the same installed session; that
acceptance did not establish general application usability. This user-selected
follow-up is t066. Inspect the live Qt popup and pointer-grab paths, reproduce
the failure in an isolated probe, repair the responsible boundary, and confirm
menu opening and selection in normal use. Preserve namespace and presented-input
constraints; do not weaken them to make a grab succeed.

## Evidence

Session `00000001788812367995-fe320326-8322-45b2-b23e-7c1781c12d10`
runs commit `2efff4cec91392d85f81d5c8a633bf7711eea3a4`. Its recorder remains
running without loss or storage errors. The root window tree identifies the
application as Okular, a Qt client; this is not evidence of a GTK failure.
Okular's main X window is `0xe00009`, with several separate menu windows.
The tree lists children regardless of mapping, so it does not establish that
any menu is visible.

The trace records repeated explicit pointer-grab rejections and releases.
The installed diagnostics do not name the rejection reason. Code inspection found
two candidate boundaries: preparing a grab requires a target in the retired
input projection, and an active lease rejects any change in its presentation
epoch. Neither is yet established as the cause of this user's menu failure.
Read-only attributes confirm all eight sampled Okular menus are currently
unmapped, override-redirect windows with WM_TRANSIENT_FOR naming the main
window. Absence of metadata-broker log records does not prove absence of a
map: those records describe metadata commits, not every mapping transition.
No live input, focus, configuration or session lifecycle was changed.

## Connections

- [Hidden-window input ownership](ohkzr8kg-unmapped-dialogs-retain-input-ownership-after-leaving-the-scene.md)
  records the prior repair and the scope of its short installed acceptance.
- [Explicit click-lease promotion](744uylx4-explicit-pointer-grabs-must-replace-their-own-click-lease.md)
  covers the previous own-grab replacement defect; do not assume it explains
  every subsequent rejection.

## Synthetic Qt ordering evidence

A temporary Qt6 Widgets probe uses a main window, menu bar and QMenu. Its
first version crashed with QtTest linked, including on the offscreen backend;
that startup fault is not evidence against Sophia. Removing QtTest and using
local QApplication event delivery let the synthetic client run. It injects no
physical events into the user's desktop.

Installed baseline trace `/tmp/sophia-qt-menu-zedm5ck6/session.log` records
MapWindow completing before XIGrabDevice is rejected on the first opening.
Later GetWindowAttributes reports that menu IsViewable. QMenu reports itself
visible despite the rejection. The second opening's grab succeeds in this run.
This reproduces a grab failure, not the user's whole Okular symptom.

A diagnostic build logged the Engine rejection branch without changing its
behavior. `/tmp/sophia-qt-menu-ezuzumos/session.log` shows the first Prepare
rejected because the session's route table did not yet contain the admitted
anchor. The second Prepare is rejected because the anchor has no presented
input entry. Both delayed attribute queries report IsViewable. The temporary
instrumentation was removed from production source after building; it was
never installed. The diagnostic binary remains `/tmp/t066-instrumented-sophia`.

These outcomes expose two ordering dependencies: the Engine must observe the
preceding authority mapping facts before adjudicating its grab request, and
X viewability cannot be equated with already-presented pixels. The existing
protocol separates Prepare from Activate, but both still require a reservation
whose target already exists in a presented projection. A correction must
preserve exact admission, namespace and lifetime checks while separating grab
reservation from physical input eligibility. The synthetic client remaining
visible means another factor may contribute to Okular's missing menus. No
production fix or installed acceptance is claimed.

## Wayland comparison

The [xdg-shell protocol source](https://chromium.googlesource.com/external/anongit.freedesktop.org/git/wayland/wayland-protocols/+/e8f7d4ebbd3d85b174e965ad601d806a49238696/stable/xdg-shell/xdg-shell.xml)
permits the explicit popup grab before mapping and requires a triggering user
input serial and seat. Parent and nested-popup rules constrain its lifetime;
dismissal ends the popup interaction. Mapping and presentation are separate
from authorizing that request. This is a useful design distinction, not a
proposal to impose Wayland's user-event requirement on existing X11 clients.
Sophia's frontend must keep X semantics; Engine must retain authority over
physical routing and namespace boundaries.

## Brave recurrence and active-grab hypothesis

The user subsequently opened Brave-Origin and reported mouse input dying.
The same session remained running without recorder loss or runtime-fatal
records. A read-only query identified Brave's mapped main window `0x3800003`;
focus was on Kitty by the time of inspection. That observation does not identify
Brave's focus at the time of failure. Recent input records show explicit-grab
preparation, activation and release as well as pointer routing. The existing
counts do not establish why an individual application appeared unresponsive.

A separate code audit confirms that ApplicationRouteLeaseState::authorize
rejects any presentation-epoch mismatch. The backend advances that per-output
epoch on interaction-projection changes, including adding or removing an
unrelated layer. This can revoke a held grab after an unrelated popup changes
the scene. It is a demonstrated mechanism, not yet a live attribution of the
Brave failure; installed diagnostics do not distinguish StalePresentation from
other authorization failures.

Any replacement must revalidate the scope at the pointer using the actual
presented scene, while preserving the original lease's admission, authority,
control epoch, surface lifetime, device and output restrictions. It must also
respect the existing [grab-scope contract](../../target-resolved-input.md):
confined grabs may cross application regions in their namespace, and
classic-shared grabs may cross application surfaces in that profile. Another
application above the original target does not automatically revoke a grab if
it lies within the granted scope. Shell, secure and foreign-scope regions do.
The pointer need not remain inside the original target's geometry; requiring
that would break the intended retained grab.

The proposed regression matrix therefore distinguishes unrelated scene
updates, another permitted application under the pointer, and forbidden
shell/foreign-scope occlusion. Testing authorize alone cannot demonstrate
occlusion: that function receives already-derived scalar scope and epoch
values. The regression must exercise presented-scope resolution as well as
lease validation. This is a proposed contract refinement, not permission to
remove the epoch guard without replacement or to widen an existing grant.

## Local Rust references and contract amendment

The user identified `~/src/wayland-rs`. The inspected checkout is
Smithay/wayland-rs at `0813584ea50379bd22e95dc1e0b50f02a4b36ca2`, with an MIT
license in LICENSE.txt. It supplies protocol bindings, transport and object
lifetime machinery; it is not the Smithay compositor-framework repository.
Its ObjectId contract prevents equality across different clients or recycled
protocol identities, making it relevant to stale-handle tests. Protocol XML
submodules are not initialized in this checkout. Niri's local Cargo.lock pins
Smithay/smithay at `4cf0b62028039661477d482ec4758b687d8f4392`, which is the
separate implementation reference to inspect for compositor popup/grab logic.

The existing normative input contract explicitly requires an exact presentation
epoch. Preserving a grab across a scene update therefore requires a documented
contract amendment, not merely a code correction. Pin current behavior first,
then change the expected scope-preserving case alongside the specification and
an ADR. Keep the scope-exit and target-evidence-loss regressions unchanged.


## Implementation and deterministic acceptance

The accepted [grab ownership ADR](../decisions/mbvdvhk5-separate-grab-ownership-from-presentation-evidence.md)
separates authoritative mapping, lease ownership, and retired presentation.
The implementation now carries the last actually published observation receipt
into Prepare, accounts it after the owner applies lifecycle effects, and releases
frontend state locks during arbitration. Mapped client-positioned windows
publish their exact creator route even before drawing; their eligibility no
longer depends on a CPU buffer or WM layout node.

Engine keeps phase and presentation binding separate. An unpresented reservation
can succeed before drawing; it cannot receive physical input until binding and
current evidence checks pass. Promotion preserves its output and earliest
pending deadline. Retained delivery revalidates original target identity and
the presented scope, including compositor occlusion, without treating unrelated
scene revisions as revocation. All existing namespace, control epoch, admission,
authority-session, device and output restrictions remain. Held input and pending
requests have bounded, exact-identity cancellation. The WM interface is unchanged.

The unchanged Qt/raw probe rejects the installed baseline in its mapped,
owned, draw-after-grab case: grab status 3 prevents the client from drawing its
marker. Candidate tests receive grab success before drawing, verify the exact
own-window marker, open and select Qt menus, and report a nonempty composed
scene. These are separate observations: own-window pixels and nonempty frame
counts do not prove the exact composed menu image or physical input recovery.

Review also caught two integration faults before acceptance. Binding an already
bound lease returned InvalidPhase; the physical routing path now compares its
pinned output and only pins an unbound lease. A regression through the actual
input loop verifies motion and button release across a scene change, and fails
when the old call is restored. The GTK probe then exposed an Engine release
racing the client's ungrab. Repeating release for the same exact identity now
joins the existing release without extending its deadline. The rejected release
had also stranded a frontend transaction ticket; later disconnect cleanup waited
behind that gap. Frontend error paths must account for every allocated ticket
and preserve completed dispatch effects before cleanup.

The model checks cover bounded admission ordering, cancellation, readiness and
scope-preserving scene changes, with negative controls that violate prerequisite
accounting, cancellation, exact scene equality and target eligibility. They are
contract models, not a Rust trace-refinement proof. Their coverage and limits are
listed in the [model record](../../../validation/tla/pointer-grab/brief-coverage.md).


The repaired ticket path let GTK complete all five captures, then exposed a
second teardown race: a queued FocusSurface command arrived after its target
was destroyed. The exact correlated UnknownSurface completion now retires the
obsolete focus command without counting it as applied. Engine clears only that
surface's focus, handoff and lease claims, including staged claims that could
otherwise restore it. Admission and configuration rejections remain failures.
This uses the existing lifecycle cleanup and leaves replacement focus to the WM.


## Validated candidate and remaining acceptance

Base commit: `2efff4cec91392d85f81d5c8a633bf7711eea3a4`, plus the retained
working-tree source snapshot. The candidate was built with
`cargo build --offline -q --bin sophia --features native-session`.
Binary SHA-256:
`c527f55d86b0acc51ea0baca8366522e268a25f980cdfd065e496c3ce52b971a`.

Both unchanged private headless probes pass against this same binary:

- Qt/raw: five successful grabs, four Qt captures, exact own-window marker
  drawn after grab success, menu selection and dismissal, and 84 nonempty
  frames. Client, session and verifier exit 0.
- GTK: five captures spanning main window, dialog, menu, remap and redraw;
  focus after dialog unmap passes; 44 nonempty frames; client, session and
  verifier exit 0. Health and cleanup are clean. This run did not log the
  stale FocusSurface branch, so it does not independently reproduce that race;
  the control tests pin its changed classification.

The session regressions cover actual retained motion and release, same-scope
application overlap, foreign and compositor occlusion, mapped popups before
pixels, observation prerequisites, control-epoch cancellation, held-input
bounds, active Abort and simultaneous release. Engine tests cover all twelve
readiness combinations, stale identity/evidence, promotion pins and deadlines,
and output loss. Frontend tests cover transaction gaps, lock progress during
arbitration, cancellation and capacity, early and late dispatch failure, and
preservation of peer-owned mapping and CPU effects. The actual physical-routing
regression fails when its old bound-output call is restored.

Final `cargo xtask check` exits 0: all-feature workspace tests and Clippy,
formatting, source-layout and diagnostics checks, installed verifier fixtures,
and the retained physical archive checks pass. The relevant bounded TLA+
models and their named negative controls also pass their expected outcomes.
The Qt verifier has eight passing tests. Task IDs and note links were checked;
`zk index` and `git diff --check` are clean.

Evidence is retained at
`~/.local/state/sophia/development-evidence/t066-pointer-grabs-20260907`.
It includes the installed comparisons, rejected candidates, final probe
identities and logs, model runs and negative controls, source manifest and
patch, and the compressed final binary. The failed GTK runs remain part of the
record; they are not replaced by the passing run.

No live session was installed, restarted or driven with synthetic physical
input. t066 remains open for normal installed use: open and close Okular menus,
select entries, and exercise Brave clicking and dragging through ordinary window
changes. Confirm that applications retain responsive input and release it on
exit. A green synthetic client does not attribute or close every Brave stall.


## Installed feedback: menus work, pointer remains unreliable

The new session `00000001788819836482-32a69abb-6c1f-4f92-babd-7ec11a92d629`
reports release commit `c1e827e0a89085e081c8fcdddddc7a20d2d9e4a2` and binary
SHA-256 `c090ac03e708d1ec91f03812a4bdaefa36409c9b5b26ce7b408b882d156d67e4`.
Startup completed without a recorded fatal error; recording has no loss or
storage errors. The user confirms that menus work, but reports spotty mouse
behavior in Okular. Its General Options/Configure dialog remained drawn over
Kitty and could not be clicked. The user clarifies that clicks work intermittently;
keyboard use may have been incidental. Wheel scrolling also fails in Okular while
working in Kitty/herdr. No keyboard-dependent recovery has been established.

A read-only root-tree query found Configure window `0x60007b`, 620x411 at
+970+530. That listing does not establish map state. The subsequent per-window
attribute request returned BadWindow: the dialog had disappeared before its
map state and transient-owner property could be captured. Okular main
`0x600009` and Kitty `0x40000e` were individually confirmed viewable.

Recorded button batches continued through Engine routing without lease waits
or refusals during the initial report. Later, at boot msec 302005427, a lease
refusal named outside_scope; its release acknowledgement followed at 302005435.
A pointer-focus handoff to the dialog completed at 302011405, followed by routed
clicks. This sequence makes retained grab ownership a useful lead, but counts
do not identify the held target or the frontend's active X grab. The refusal's
predicate also includes owner, device and control validity; its name alone does
not prove that the pointer crossed a namespace boundary.

Retaining an explicit grab's original target is intentional. It is not itself
a defect, and explicit grabs should not automatically end on button release.
The next investigation must identify the surviving lease, its X grab owner and
lifetime, and owner-events delivery before changing retention rules. The menus
are improved; the whole pointer symptom is not accepted as repaired.

The live snapshot and bounded observations are retained separately at
`~/.local/state/sophia/development-evidence/t066-installed-c1e827e0-spotty-pointer`.
No input was injected and no live state was changed during inspection.


## Wheel decoding and virtual source investigation

The installed Qt 6.11.1 decoder confirms that little-endian FP3232 valuators
were encoded with their two words reversed. A requested +120 decoded as
0.000000027939677; -120 decoded as positive 0.999999972. XIQueryDevice used the
same incorrect encoding for bounds, current values and scroll increments.
Tests had decoded a packed i64 and repeated the implementation error. The
candidate shares one integral-then-fraction encoder between replies and events,
with independent field decoding in tests for both byte orders, signs and
fractions. Both new regressions fail against the original codec.

A private Qt receiver establishes a separate discovery defect: the master-only
inventory leaves Qt's scroll orientations empty. Correcting both reply and
event numbers still produces no wheel events. Adding an attached virtual source
through Qt's own device setup produces four correctly directed wheel events and
moves a real editor scrollbar. Delivering source128 and master2 copies gives
one Qt press/release pair and four wheel events, without duplication. These
controls test Qt's receiver; they do not inject physical input or prove delivery
through the Engine. The independent broker-to-socket-to-Qt probe is the candidate
acceptance gate for that remaining frontend path.

The candidate exposes a fixed XI2-only source128 attached to master2, with
standard scroll labels and current namespace-filtered baselines. XI1 stays on
the core pair. Source selection is distinct from AllMasterDevices; selected
source packets precede master packets. Source grabs are explicitly refused
before Engine reservation, preserving the existing master lease boundary.
Encoding and route selection finish before socket writes, so no shared input
state lock is held while a client consumes those packets.

Protocol reference: [XI2 types and device hierarchy](https://xorg.freedesktop.org/archive/current/doc/inputproto/XI2proto.txt).

## Browser report now points toward delayed visual updates

The user refined the Brave symptom: the first click after opening or returning
to the window appears to work; further clicks appear ineffective until another
window switch. Entering a website and pressing Ctrl+Enter left the old display
visible, but switching away and back revealed the loaded page. This supports a
stale visual-update lead; it does not establish input loss or locate the stalled
stage. Current X focus was Kitty when inspected because the user had returned
there to report. Brave main0x1800003 was viewable. Its recent schema2 Present
records represent native DMA-BUF retirements, not frontend submissions; they
cannot be read as CPU redraw counts. Presentation scheduling and retained-image
ownership need investigation independently of the confirmed wheel defects.


The real frontend writer exposed a third defect: a single routed press emitted
both a core ButtonPress and a master XI ButtonPress. Qt handled both. Master XI
now wins at the same event window, while a nearer core subscriber still stops
propagation to an XI ancestor. Source delivery remains independent. Wire tests
cover axis, press and release in both byte orders, all-device/master/source
selections, and the nearer-core case.

The final private receiver run
`/tmp/sophia-qt-wheel-delivery-7447-1788822761574296917` passes through the actual
routing broker, X socket writer and Qt 6.11.1: four wheel deltas (-120, +120,
+120, -120), editor scrollbar 50→53→50→47→50, four source and four master Motion
packets, one source/master press and release each, exactly one Qt press/release,
zero core button packets, and seven flushed broker deliveries. The C++ receiver
uses public Qt APIs; it does not call Qt's event handlers directly. Source
packet counting disables Qt's high-frequency compression after QApplication
construction. No production setting depends on Qt.

The probe first delivers pointer motion and waits for the client to handle it.
Without that entry handshake, Qt's Enter-triggered QueryDevice can absorb an
already-observed wheel value into its initial baseline. That separate first-axis
boundary remains outside the passing case. Engine hit testing, physical grab
recovery and Brave rendering are also outside this frontend proof.

Read-only review found two further scheduling candidates, neither attributed
to Brave: future Present NotifyMSC currently advances only when another Present
completes; ordinary CPU damage in a mixed GPU/CPU scene may not schedule retained
composition after an idle cadence interval. A possible owner-loop feedback skip
may instead be bounded by the native service deadline. No scheduling code was
changed on those unproven incident hypotheses. Brave's gaps between retirements
remain unclassified without corresponding submission or damage observations.

The subsequent [visual-progress investigation](vwo9wmie-window-switches-reveal-delayed-visual-updates.md)
reproduced an immediate NotifyMSC sequence race and the mixed-scene CPU repaint
scheduling gap, and repaired feedback delivery before the owner loop's no-work
exit. It records the implementation and test limits separately from physical
acceptance; the earlier scheduling paragraph above describes the pre-repair
audit, not the current candidate.

## t066 accepted: pointer input reaches an open menu — 2026-09-21

Accepted on the installed session carrying `8316414f`, by hand. Menus open,
clicks inside them land in the menu, and the menu dismisses.

The repair is not this note's defect. This row was opened against rejected
pointer leases and grab ordering; the session that accepted it shows
`lease_rejected_count=0` and explicit grabs cycling prepared, activated,
released with `rejected=0` throughout, so nothing here was being refused. The
cause was a held grab routing every event to the surface its lease anchored to,
and a toolkit anchors that grab before its menu exists. It is recorded in
[ce2b55uy](ce2b55uy-blank-thunar-menus-and-frozen-brave-need-separate-pixel-and-delivery-evidence.md)
and fixed by `grab_routed_surface` in `live_session/input/grab_routing.rs`.

The acceptance evidence is a record rather than an impression.
`sophia_live_session_pointer_projection` prints what the hit test was given
beside what it chose, and before the fix the two disagreed on exactly the
popup:

    before   under=4195333:2,4194310:1   target=4194310
    after    under=4203354:3,4195330:2   target=4203354   role=client_positioned

The routed target is now the top-ranked layer under the pointer. Ordinary
clicks are unchanged: surfaces that are themselves under the pointer still
route to themselves, which was the risk in touching grab routing at all.
Popups also enter and leave the projection across epochs rather than
accumulating, which is the menus dismissing.

Qt menus are not separately re-accepted here. This row's text is pointer input
and redraws after popup-menu acceptance, and that is what was accepted; if a Qt
toolkit still fails to open a menu, it is a live defect against this note
rather than a reopening of this row.
