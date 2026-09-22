---
id: ce2b55uy
date: 2026-09-06
kind: investigation
status: investigating
tags: [investigation, x11, rendering, validation]
---
# Blank Thunar menus and frozen Brave need separate pixel and delivery evidence

## Question

Which boundary fails when Thunar menus have blank, black, or missing portions,
and when Brave Origin stops responding until the user switches windows?

## Evidence

The installed session is `00000001788751946481-31db6852-d07f-4f08-8ed9-87f63a561f59`,
built from `86ab21e4879cc5b3154ca1192775de73e6e6a030`. The binary SHA-256 is
`0d972718c734751aba7fe58eb5075eb9f2f776bb4da7fdea3d69e80f98ac4d06`.
The applications are Brave Origin and Thunar; the user corrected an earlier
reference to Firefox. Thunar's symptom is missing pixels, not a reported menu
position error. Brave accepts a few clicks and then stops responding entirely;
the user reports recovery after switching away and back.

The retained input-lease and explicit-grab records contained only their schema.
Chrome records retained their generation but lost frame and focus counts.
`diagnostics::reduced_record` omitted these fields and most input status values.
Moreover, successful pointer delivery emitted only a first-use marker. Zero
recorder loss therefore did not establish that later clicks reached Brave.

The CPU presentation code in `software.rs::present_window_damage` chooses AR24
only for a bounding shape and otherwise tags the buffer XR24. It does not use
the window's depth-32 visual. The compatibility matrix already names that alpha
loss. It can explain black transparent areas, but no captured Thunar popup
establishes that this is the whole menu failure. Startup smoke tests require no
mapped window or pixel proof, so their success does not accept menu rendering.

## Diagnostic correction

The recorder now keeps record-scoped delivery, grab, frame, and timing counts,
and the fixed status vocabulary needed to interpret them. It still excludes
application identities, coordinates, button/key codes, and payloads. Pointer
button batches retain observed, routed, and suppressed counts after first use.
They use the existing bounded, asynchronous recorder; no disk work enters input
routing. This changes evidence collection, not input policy or presentation.

## Validation and remaining work

Two regression tests pass for diagnostic retention, vocabulary scoping, numeric
bounds, and payload exclusion. `cargo xtask check` passed, including workspace
tests, Clippy, layout checks, fixture verifiers, and the host buffer-age proof.
The first sandbox run stopped at a denied Unix-socket bind; the complete run
passed with local sockets available.

The observed evidence directory is `/tmp/sophia-interaction-evidence-197b2af77a09`.
It holds the source patch, full check log, timing-probe script, and identity.
The source-patch SHA-256 is `197b2af77a093f8cf1c14a23fff4f007e00ff4a9af9fce5ec8b3e5ada2cab217`.
This temporary path is not a durable physical-acceptance archive.
The code change is based on `86ab21e4` and was committed as `e8573cf1`.

The replacement session is
`00000001788753082918-0a84405d-9e24-433d-950f-d3dce25a2607`, installed from
`e8573cf17afc022865009abe75c2e3fbc18db484`. Its binary SHA-256 is
`c3bb971a6925a9366480bb25eb6155d5fa1ebf8e177705d08f47d7f1787fa74f`.
Preflight, input guard, and graphics takeover completed. The recorder reports
no discarded records or storage errors. Chrome events now retain focused,
unfocused, and primitive counts, confirming that the diagnostic change is
installed. No pointer-button batch appeared in the initial sample. The next ordinary-use
recurrence retained routed clicks and repeated explicit-grab rejections, leading
to the [click-lease investigation](744uylx4-explicit-pointer-grabs-must-replace-their-own-click-lease.md).

For t061, reproduce a mapped GTK menu with retained pixels and trace its actual
rendering requests. Compare frontend pixels and alpha format with Engine's
composed result before choosing the repair. Regress the failed boundary, pass
required checks, then accept visible menu text, background, edges, and submenus
in one installed Thunar use. Keep menu placement and drag acceptance under t060.
The new diagnostic counters support the existing Brave t003 investigation;
they do not establish its root cause or accept its usability.

## 2026-09-07: GTK clip reset and a mapped damaged dialog

The new installed session is
`00000001788785369819-028cfec4-8b74-46ef-93b3-6c529dea2ddc`, release
`4299e1cabb650cd481096f43ac8b5186d31ad4f1`, binary SHA-256
`9b152ebc7919635afb086b288bc4bc1cd747d752c55c077e318690d18c659386`.
The installed package is Thunar `4.20.9_1`, linked against GTK 3; this is not
a GTK 4 report. The user sees black and missing areas after interacting with
the sidebar or menu bar, and a black box with a pale border and white area
over Kitty after switching windows in the same workspace.

A read-only X11 probe captured Thunar's window attributes, hierarchy, and
drawable pixels. It sent no input and inspected no clipboard contents.
The main window's retained image has intact menu-bar and sidebar content.
The dropdowns were unmapped at capture time. A separate, still-mapped
`_NET_WM_WINDOW_TYPE_DIALOG` has `WM_TRANSIENT_FOR` pointing to Thunar and a
377-by-112 drawable that is mostly black with a white rectangle. Its only
child is 1-by-1, so missing child content does not explain this capture.
This is a candidate for the reported ghost, not proof of its identity in
the composed output. All observed Thunar windows have depth 24. The zero
fourth byte in their readback is padding, not evidence of lost alpha.

A bounded private session using the installed binary reproduced incomplete
GTK dialog content. A basic menu rendered correctly. Interposing Xlib calls
in this synthetic client showed GTK/Cairo repeatedly setting temporary
RENDER rectangle clips, then sending `ChangePicture(CPClipMask=None)`.
The frontend decoded the latter as an accepted no-op and retained the old
clip. The [RENDER contract](https://www.keithp.com/~keithp/render/protocol.html)
requires None to remove that restriction. The repair carries an explicit
clear request through decoding and clears the picture's rectangles before
later drawing. Omitted attributes preserve the clip; unsupported pixmap masks
remain refused. This stays within X11 protocol state and changes no WM or
namespace authority.

The pixel regression fails before the repair and passes after it. Tests also
cover omitted attributes, preservation after a refused mask, and both wire
byte orders. `cargo xtask check` passes. In the same synthetic GTK probe the
previously missing Close button becomes visible, but other dialog content
remains incomplete. The probe also encounters a separate core ChangeProperty
BadAccess on dialog remapping; GTK labels it `GLXBadPbuffer`, although the
reported request is core opcode 18 and error code 10. Session exit zero only
establishes bounded session cleanup: the synthetic GTK client exits with an
error. Neither result accepts Thunar's live behavior.

Extra readbacks between the synthetic client's drawing operations preserve
both label and button; removing that instrumentation again loses the label.
Keep this timing sensitivity separate from the deterministic clip-reset
regression. The final probe uses no interposition preload.

Private evidence is retained under
`~/.local/state/sophia/development-evidence/t061-77c1ab98b5a1` with validated
checksums. It includes two live Thunar captures, the synthetic client and
tracer, baseline and candidate images, the full check log, and the four changed
source/test files against `4299e1ca`. The source identity is
`77c1ab98b5a199bdd36285fd58f3a959db874bfa5f7fc7b8d39a4ed6f78106c2`;
`identity.json` records its per-file hashes and candidate binary hash.

t061 stays open for the remaining drawing defect, mapped-menu evidence,
comparison with composed output, and installed acceptance. Do not hide a
mapped dialog merely to remove the visual symptom, or infer popup ownership
from application identity in the blind WM.

## 2026-09-07: complete clipping, dialog reuse, and mapping before policy

The preceding evidence records the first, incomplete clip-reset candidate.
The next candidate also preserves core GC clip origins, implements
`ChangeGC(clip-mask=None)`, and distinguishes an empty clip from no clip in
both core drawing and RENDER. Empty regions remain extractable through XFIXES;
replay cannot mistake an empty clip for an unrestricted image upload. Invalid
and unsupported writes preserve the old attributes. Validating picture values
before mutation also removes the need to clone retained clip rectangles on
each `ChangePicture`.

With these changes, the original synthetic GTK dialog contains its label and
Close button without interposed readbacks. A second failure had killed GTK on
dialog remap: it rewrote `_NET_WM_STATE` after hiding, but the property table
still classified that initial hint as immutable Engine feedback. The exception
now requires an accessible, unmapped window with no pending policy admission.
It changes the X property, not Engine state. Both byte orders, foreign namespace
denial, pending admission, mapped feedback, and protected `WM_STATE` are tested.

The reusable <a href="../../../tools/probes/README.md">GTK probe</a> checks actual client
exit and synthetic pixels in both dialog halves and every menu row. The first
run is `/tmp/sophia-gtk-redraw-iw35swgi`: all five captures pass and GTK exits
zero. Running the same probe against installed `4299e1ca` produces only three
captures and a GTK exit of one on remap (`/tmp/sophia-gtk-redraw-yy_bg22v`). Its
controlled CSS changes drawing requests: that baseline's initial pixel checks
pass, so this comparison proves the remap regression, not the original themed
dialog's missing pixels. The original probe and exact wire-pixel regressions
supply the separate clipping evidence. Batched, three-byte fragmented, and
paced socket writes produce identical final pixels and published buffer updates.

Parallel review with the adjacent Claude agent confirmed another defect:
managed scene visibility trusted cached WM projection after authority unmap.
Popups could also remain visible through an unmapped managed owner. Both paths
now require authority mapping before consulting policy. The two regressions
fail without that gate. A third test preserves existing popup remap, destroy,
and generation-reuse behavior. Destroy already purges the layer and mapping;
the repair does not hide a valid dialog merely because focus moves elsewhere.

The input audit found a second stale projection. Native pointer routing reads
the last retired frame; a scene fix alone leaves an unmapped target eligible
until another flip completes. Successful layout publication now prunes departed
surfaces immediately and advances the input epoch. Retirement also intersects
its frame with current eligibility, preventing an older pending frame from
restoring a dismissed target. Both guards have independent mutation checks.
Survivors keep their retired geometry. Membership uses a set, avoiding a
quadratic scan at the 1,024-surface bound. These are protocol-neutral lifecycle
checks, not X11 policy in Engine or application identity in the WM.

A fresh read-only capture at `/tmp/sophia-thunar-pixels-1788788253` still finds
the six menu windows unmapped and the damaged Error dialog mapped. Listing
root children is not evidence that those children are mapped. The running
session predates these repairs and the separately committed chrome-focus fix
`2d81faa3`. t061 remains open for installed Thunar acceptance; synthetic
frontend pixels alone do not establish the physical composed result.

### Combined candidate validation

The combined `cargo xtask check` passes, including workspace tests, Clippy,
fixture verification, and the host buffer-age pixel-equivalence proof. The
final rebuild passes all five GTK content checks and exits zero, with no
protocol errors (`/tmp/sophia-gtk-redraw-gh6ydc2y`). The remapped dialog and
menu PNGs were also inspected. Formatting, diff checks, task links, and all
64 task IDs pass validation. Earlier checks that overlapped the input-helper
signature edit were discarded; the final check used the completed code.

Private evidence is archived and checksum-verified at
`~/.local/state/sophia/development-evidence/t061-229aac9b83b7`. Its source
identity is `229aac9b83b7634108752e8f979a80b8c771202c2700ab1b9a1c18bdb87dd898`
against `2d81faa3`; the candidate binary SHA-256 is
`7362c6964d1699434b71381db7c874f5bd6c3ef07d9f5dc440066278331a1157`.
The archive contains source copies, the patch, full check log, the baseline
and final synthetic runs, and the latest private Thunar captures.

The audit separately found that live pointer projections discard SHAPE input
regions. [t064](../plans/queue-11-parallel-production-readiness.md#t064) records
that candidate follow-up, and the compatibility matrix now limits its claim
to the wire and direct-layer evidence. It is not folded into t061.

Install the combined candidate and accept one ordinary Thunar session: open
menus and submenus, interact with the sidebar, dismiss a popup, switch to Kitty,
and reuse a dialog. Text, backgrounds and edges must remain complete; dismissed
surfaces must neither linger nor take clicks. t061 closes only after that
installed observation, not after the headless probe.

## 2026-09-07: Kitty starts but never enters the composed scene

Installed `34128d80` regressed managed-window visibility. In session
`00000001788789965467-4ea7b32f-136f-44be-9553-10a55dfc445c`, both Kitty processes
remain alive and both X windows are `IsViewable`. Their admission and resize
epochs committed. The windows are 1,258 by 1,390 at (17, 41) and (1,285, 41),
yet the user sees neither. This is not a failed launcher command.

The new mapping guard exposed a missing state transition: the authority maps
the window during `AdmitSurface`, but its control path does not publish a new
surface-presentation observation. The session's mapped set therefore retains
the pre-admission false value. The guard correctly removes unmapped surfaces
but also removes these successfully admitted windows.

Our earlier GTK check read each window's frontend storage. Its final log also
reported `cpu_max_nonzero_pixel_bytes=0` and `cpu_nonzero_frames=0`; those fields
were not part of its pass criteria. The strengthened probe now requires
nonempty headless composition. Installed `34128d80` fails this check
deterministically: all five captures pass while composition evidence and
nonempty frames are both zero. This is retained at
`/tmp/sophia-gtk-redraw-yo37ak_u`; live identity
and reduced events are at `/tmp/sophia-kitty-admission-34128d80`.

The repair records mapping only after the existing authority acknowledgement
matches a pending admission and its transaction. It needs no subsequent client
traffic. Wrong, duplicate, withdrawn and destroyed admissions remain rejected;
a blind-WM proposal alone remains insufficient.

The first probe revision misread `cpu_max_nonzero_pixel_bytes` as an exact
count throughout the run and imposed a 458,084-byte threshold. The renderer
switches to bounded composition evidence after three initial proof frames,
so the repaired candidate's value of 10 with 40 nonempty frames is not ten
actual pixels or bytes. That threshold was removed before committing the probe.
Its scene gate is a coarse witness; exact composed pixels require a separate
test, and physical Thunar acceptance remains outstanding.

The final candidate passes `cargo xtask check`, including ten presentation-owner
tests and the host buffer-age proof. The added compositor regression compares
exact bytes: a retained two-pixel marker is absent before acknowledgement,
present immediately afterward without more client traffic, and absent after
unmap. The final real GTK run passes all five content captures, exits zero,
and records nonempty composition (`/tmp/sophia-gtk-redraw-mpaufwat`). The same
probe rejects installed `34128d80` (`/tmp/sophia-gtk-redraw-wrkkmesa`). Its new
completion reader is registered with the repository's schema audit.

Source and private evidence are archived with verified checksums at
`~/.local/state/sophia/development-evidence/t061-admission-5f5c9ead3764`.
Source identity: `5f5c9ead37648258bc111be29f92d2abcb3c746efbfd72e82c64ff1e7d7bb3ce`
against `34128d80`; candidate binary SHA-256:
`6879d30225e9bbed11fabfe5da0fc9572f09c8d91c0a755dfa3d9d1a351da967`.
The running desktop was not replaced. Commit and install this correction before
the next Kitty/Thunar acceptance attempt; t061 remains open.

## 2026-09-07: Installed Kitty and menu behavior confirmed

In session `00000001788792259184-92b32790-8c54-4552-a0c7-811d5af1c39c`,
release `71b9b0d1960403ccbb8922ae1f1b91f3d34d60b9`, the user confirmed that
both checks “seem to be working”: Super+Enter opens a visible Kitty window,
and Thunar menus work and dismiss without following the switch back to Kitty.
This supplies physical evidence for the admission repair and the reported
menu/popup regression, in addition to the retained deterministic tests.

The installed binary SHA-256 is
`11c76ed1889e891efac957fbe05227bc5287bafd4d61712db16cee012b4d92eb`;
the session's manifest and reduced events remain under
`~/.local/state/sophia/sessions/` in that session directory.
The subsequent report of general sluggishness and all-input loss is tracked
separately in [t065](ohkzr8kg-unmapped-dialogs-retain-input-ownership-after-leaving-the-scene.md).
The visual report does not separately establish submenu/sidebar behavior or dialog
reuse. Those remaining t061 observations can come from ordinary Thunar use;
the successful Kitty and menu checks do not need to be repeated.

## Connections

- [Brave watchdog investigation](h0vxis10-brave-gpu-watchdog-repeats-during-live-use.md)
  owns browser dumps, waiting-state samples, and frame-timing probes.
- [Pointer queries](knjco01f-pointer-queries-must-share-admitted-namespace-state.md)
  now return nonzero live state; pixel correctness remains separate.
- [Compatibility matrix](../../x11-compatibility-matrix.md) distinguishes startup,
  RENDER resource support, alpha limitations, and actual visual acceptance.

## The pixel boundary and the delivery boundary are one predicate — 2026-09-21

> **Superseded.** This diagnosis is wrong; see "The cause, found by the record"
> below. The predicate divergence it describes is real and still worth
> repairing on its own terms, but it is not why a dropdown swallows clicks.

Reported again from a live session on release `0.1.0-2e7031d6b9ae`: a Thunar
dropdown opens, and the mouse does nothing inside it. This is the delivery
symptom rather than the pixel one, and it is not either defect already
recorded against menus. The session's own records rule both out --
nineteen `sophia_live_explicit_pointer_grab` records cycling
prepared, activated, released with `rejected=0 aborted=0 cancelled=0`, every
`sophia_live_session_pointer_batch` reading `observed=1 routed=1` with
`suppressed_no_target_count=0 suppressed_policy_count=0`, and
`lease_rejected_count=0` throughout. Nothing is being refused, so neither
[37xvg0y7](37xvg0y7-qt-menus-fail-to-open-while-pointer-leases-are-rejected.md),
which is about rejected leases, nor
[744uylx4](744uylx4-explicit-pointer-grabs-must-replace-their-own-click-lease.md),
whose grab-replacement defect was corrected, accounts for it.

**Traced in code, and it makes this note's question the wrong question.** The
two boundaries this note set out to separate are decided by the same predicate.

`hit_test_layers` (`sophia-engine/src/input/hit_test.rs:65`) skips any layer
where `!should_render(layer)`, and `should_render`
(`sophia-engine/src/render.rs:144`) is

    layer.opacity > 0.0 && !layer.geometry.is_empty() && layer.source != BufferSource::None

So **a surface that has not committed a buffer cannot be hit**. In X11 that is
a category error: a mapped InputOutput window answers pointer events whether or
not it has painted anything, and nothing in the protocol makes input
conditional on content.

The session already holds the correct notion and disagrees with the engine.
`LiveWmLayout::input_eligible` (`wm/layout_support.rs:32`) decides eligibility
by *mapping* -- its own comment says "eligibility here is the authority's
mapping" -- and routes a client-positioned surface through
`client_positioned_visible`. `ClientPositioned` (`wm/layout.rs:689`) is the
role a menu carries: it "carries its own coordinates and no policy places it".
So the session says a mapped popup can answer input and the engine's hit test
says it cannot until it has pixels.

**Why that produces exactly this symptom.** A GTK menu maps, takes its pointer
grab, and commits its buffer some time after. In the interval, or permanently
if the buffer never arrives, the popup is absent from the hit test, so the
pointer resolves to the window beneath it -- Thunar's own main window. Delivery
then honours `owner_events` correctly at
`x11_socket/routing/registry/delivery.rs:303`: the grab owner and the surface
under the pointer are the same client, so the event goes to
`surface_route.window`, which is the main window rather than the menu. Every
layer behaves as written and the click lands one window too low. The grab
records stay clean because the grab genuinely is held; what fails is which
surface the pointer resolved to.

This also predicts the original pixel symptom and the delivery symptom
appearing together on the same popups, which is what has been reported, and it
explains why chasing them as separate boundaries did not converge: a popup with
no buffer is both blank and deaf, for one reason.

### Not yet established

This is traced through the code, not observed in a live capture. What would
settle it is a reproduction showing, while a menu is open, either a layer
snapshot carrying the popup surface with `source: None`, or a pointer route
whose target surface is the parent window rather than the menu. The session log
records pointer batches only as sampled tallies with no per-surface target, so
neither is visible today; the delivery layer would need a record naming the
surface a grabbed pointer event resolved to. Until that exists this is the
best-supported explanation rather than a measured cause.

t061 owns the pixels and t066 owns the input. If this holds they are one row.

## The live capture refutes the predicate diagnosis — 2026-09-21

> **Half superseded.** Its refutation of the predicate diagnosis stands. Its own
> conclusion -- that the popup never reaches the session -- does not: the
> absence of admission records is designed for `ClientPositioned`, not
> diagnostic, and the popup is in both the scene and the hit-test projection.

The section above is wrong about the cause, and the record added to test it is
what showed that. `should_render` gating the hit test is real and is still
worth repairing on its own terms, but it is **not** why a Thunar dropdown
swallows clicks: the hit test never sees the popup at all.

`sophia_live_session_pointer_target` (d4c4863a) names the surface a routed
button reached and its presentation role. Captured on the installed session,
with three menus opened and clicked:

    ...323000  CLICK         surface=4194310   role=policy_managed
    ...323003  POPUP MAPPED  surface=4194690   ClientPositioned 205x26
    ...323158  CLICK         surface=4194310   role=policy_managed
    ...325100  POPUP MAPPED  surface=4194738   ClientPositioned 245x406
    ...325225  CLICK         surface=4194310   role=policy_managed
    ...328356  POPUP MAPPED  surface=4194939   ClientPositioned 325x380
    ...328516  CLICK         surface=4194310   role=policy_managed

Sixteen routed buttons, every one landing on Thunar's main window, each about
150ms after a menu mapped. So the symptom is confirmed exactly: clicks aimed at
an open menu reach the window beneath it.

**But the popup is absent from the session, not filtered by it.** Across the
whole session, surface 4194738 appears in exactly two record kinds:
`sophia_x_window_lifecycle` and one `sophia_live_metadata_broker`. It has no
`sophia_live_surface_admission`, no `sophia_live_surface_geometry`, no
`sophia_catalog_placement`, no compositor chrome — all of which the main window
has. `sophia_live_surface_admission` admitted exactly two surfaces in the entire
session: 4194310 and 2097166. Neither is a popup.

Its X lifecycle is complete and correct: CreateWindow with
`role=ClientPositioned`, three property changes, ConfigureWindow, MapWindow
(`major=8`) flipping `mapped=true`, then UnmapWindow and destroy. The authority
saw a real override-redirect window map and unmap. The session never admitted it
as a surface.

So there is nothing in the layer vector for `should_render` to reject, nothing
for the hit test to skip, and nothing for `pointer_event_target` to descend
into. A predicate change in Engine would have repaired a defect that is real and
changed this symptom not at all — which is what the confirmation step existed to
find out, and why it ran before the fix rather than after.

### Where that points, without claiming it

Admission runs through the window manager. `next_unmanaged_surface`
(`wm/layout.rs:678`) only returns a surface the layout `knows_surface`, and both
of its call sites (`owner_loop/authority.rs:648`, `:814`) additionally require a
live `wm_session` before requesting admission. This session had one
(`sophia_live_wm schema=4 status=ready`), so the gate was open and the popup
still did not arrive.

The open question is therefore why a mapped `ClientPositioned` surface does not
become known to the layout, and an override-redirect window is exactly the kind
a window manager must *not* manage — so a path to admission that runs through
the WM is the first thing to examine. That is a different investigation from
this note's original pixel/delivery split and from the predicate above.

### What this does and does not establish

Established: the clicks land on the parent; the popups map at the X level; the
session never admits them; the hit-test predicate is not implicated because the
hit test is never given the chance.

Not established: why admission does not happen, whether the blank-menu pixel
symptom shares this cause (it plausibly does — an unadmitted surface is never
composited either, which would make t061 and t066 one row after all, for a
different reason than argued above), and whether any of this is recent.

## The cause, found by the record — 2026-09-21

Fixed in `8316414f`. A held pointer grab routed every event to the surface the
grab anchored to, and a toolkit anchors that grab before its menu exists.

`sophia_live_session_pointer_projection` (`f299bf3e`) prints what the hit test
was given beside what it chose. For a click inside an open dropdown:

    under=4195333:2,4194310:1   target=4194310

The popup is in the projection, under the pointer, and ranked above the window
beneath it. The hit test would have chosen it. Nothing consulted the hit test.

The session log orders the cause exactly:

    636789  click on the menubar        surface=4194310
    636791  grab prepared
    636792  grab activated
    636797  popup CreateWindow          surface=4195333
    636798  popup MapWindow  mapped=true

GTK grabs the pointer on the window that was clicked and only then creates and
maps its menu, so the lease anchors to a surface the pointer is about to leave.
`input.rs` then routed every held-lease event to `lease.target_surface`,
discarding the hit test, so each click inside the menu reached the window
underneath it. The menu never received the press that dismisses it and stayed
mapped over whatever came next; Escape dismissed it, because keyboard delivery
follows focus rather than the pointer.

This is X11's `owner_events` rule decided at the wrong layer. The authority
already applies it per window -- the window under the pointer for an
`owner_events` grab, the grab window otherwise -- and can only choose within
the surface the session names. A wire control in the focus lane pins that half:
given a correctly named popup surface, delivery reaches the popup; given the
main surface while the popup is open, it reaches the main window. Both
directions assert different windows, so the authority was never the defect and
a repair attempted there now fails a test.

`grab_routed_surface` (`live_session/input/grab_routing.rs`) moves the choice to
where the surfaces are known: the surface under the pointer when it belongs to
the grabbing client, the anchor otherwise. Same-client-only is what preserves a
drag -- once the pointer leaves the client's surfaces there is no eligible hit,
the anchor stands, and grab ordering survives a drag outside its own geometry.

### What this leaves for t061 and t066

t066 is the input half and this is its repair, pending installed acceptance.
t061 is blank or black menu *pixels*, and nothing here touches rendering: these
popups composited correctly and were drawn properly throughout, which is itself
evidence against the earlier claim that the two rows share a cause. They are
two rows again, for the opposite reason to the one argued this morning.

### Why it took six mechanisms

Five were proposed and refuted before this one, three of them mine, each traced
through real code and each plausible: a render predicate gating input, the
popup never reaching the session, the popup never drawing, stale composited
pixels, and the input projection lagging behind composition. Two readers agreed
on the first. The system refuted every one, twice within minutes of it being
stated.

Reading code is a proxy for observing the system, and agreement between readers
is agreement about the proxy. What ended it was a record that named the surface
a click reached and what the hit test had been given -- neither of which
existed when the investigation started, and both of which had to be argued for
against the temptation to fix the first plausible cause.

