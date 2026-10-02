---
id: qrmqf4gy
date: 2026-10-02
kind: investigation
status: investigating
tags: [investigation, rendering, session]
---
# Alt-Tab exposes missing cross-head retained image custody

## Incident

The first attended Alt-Tab on `niltempus-d99119dbc46629083eea` ended
Sophia's session. The machine stayed up, and keyboard and TTY restoration
completed. This is failed live acceptance, despite the earlier build and
supplied-client gates.

The release contains Sophia `9679b820dabf5ca1406eacb919003232de6b88c9`,
Hagia `03be1d1f2b1c9e555773578ec1001dd7c098830a`, and C SDK 0.7.0
`4608010f31848c153d5c0918b149b9f86a239048`. The personal profile is
`863f09d0ffb67bec54ddf814f01634899411dbfa3e116f7186c197c82d0a26b5`.

The frozen evidence is under
`~/.local/state/sophia/development-evidence/t279-alt-tab-session-exit-01/`.
`failed-session/` copies the original archive, and `session-digests.json`
records its files. The original session is
`00000001790952216617-d0752291-aa69-4809-afc7-1b048306a76c`.

## Observations

Output 1 was 2560x1440 at 120 Hz, with kitty surface 2097166. Output 2
was 1920x1080 at 60 Hz, with kitty surface 4194318 and active focus.
Hagia's default all-windows scope selected the output-1 window and placed
the strip on output 2. The strip contained the ordinary output-2 surface
again as a clipped instance, plus a cross-output instance of 2097166.

Action 186 committed as transaction 16. It opened a hidden switcher. About
151 ms later transaction 17 committed the Held update, making the Overlay
visible for the first time. Nine milliseconds later the owner loop recorded
a fatal error in its lifecycle phase. The archive retains only
`failure_code=unclassified`; it does not retain the original error string.
There is no basis for claiming a kernel crash or an application panic.

## Reproduced renderer defect

The installed default uses separate renderer workers per head. Sharing is
an explicit `SOPHIA_ENABLE_SHARED_RENDERER_WORKER=1` option. The release and
checked launch configuration do not set it; the dead process environment
cannot now be read. Each private worker owns its own image store.

Ordinary head composition resolves sources from the global retained surface
map, then lowers the per-output lists. A RendererImage identity can therefore
reach a worker that never captured it. At the installed base, batch admission validated head and frame identities
but did not establish image custody. Topology preparation had a donor
export/restore pass; ordinary composition did not.

The off-screen tests reproduce this missing-custody failure on a DRM render
node, with no KMS access or session socket:

- The same-head application plus clipped instance renders successfully.
- At the installed base, adding the other worker's image gave
  `Degraded / InvalidTarget` through
  the real backend worker/exporter path.
- A shared worker store renders the identical cross-output frame.
- Exporting a snapshot before donor eviction, then restoring it on the
  recipient, renders successfully after that eviction.
- The lower-level pixel test checks the backdrop, clipped same-head instance,
  and cross-head pixels through four source revisions after explicit transfer.

Base EGL frame admission reported `InvalidTarget` for an absent image.
The repair reports the specific `InvalidRendererImageId`.
This is the `contains_key` check in `context.rs`, before drawing. It does not
require an image to be promoted: a staged image is drawable. Promotion is
instead the commit/export boundary.

These tests reproduce the renderer failure and its discriminating controls.
The archived fatal lacks the original detail, so the complete live
owner-loop failure has not been replayed. The code path, publication and
timing support this diagnosis; a final qualification must exercise the
whole native publication path. The tests contain no held-key stamp, so the
renderer defect does not require capability bit 22.

## Two additional defects

At the installed base, a source Present selected outputs from its own
geometry and policy routing. It did not schedule other outputs sampling it through SurfaceInstance.
Consequently a remote preview stays stale until another repaint, which then
names a newer image absent from that worker too.

At the installed base, the export report contained the typed failure, but
scanout preparation kept only the export status. Later formatted errors
became `unclassified` in Session. The repair carries `export_detail` through
preparation, submission and runtime classification. The worker warning is not admitted by the ordinary diagnostic sink.
Diagnostic repair must retain a fixed compiler-owned cause, without copying
arbitrary error text or client data.

## Repair design and exit criteria

Task t284 owns the implementation below. Code qualification is complete;
release assembly is recorded separately. Attended acceptance remains open.

A queued frame must own the immutable image it actually references. A donor
lookup performed later is insufficient: a later Present can replace and
evict the donor image while the preview waits. Use bounded snapshot leases
for foreign instance sources and their exact generations. Attach them to
queued frames and import them as read-only DMA-BUF layers. Do not recapture
them into an unowned long-lived image store. Local/shared-store references
stay in their original stores, with read guards held by queued frames and
frozen retry sources. Eviction waits until the last guard drops.

Count both bytes and identities across current snapshots, queued frames,
worker work, frame leases and lingering EGL imports. Supersession and cache
eviction must release that custody. Initial publication must obtain the
current retained image even when no new source Present arrives. Later
promotion publishes snapshots only for demanded foreign sources, keyed by
image and recipient GPU group. Each uses a donor in that group; admission
reads that exact cached snapshot and never selects a different donor. A same-device foreign-preview snapshot that is missing at rendering, or
refused by the preview budget, withdraws its publication. Cross-device hot
previews are refused before admission. Busy workers defer. Missing or cross-device refusals without a matching
publication/source remain classified fatal invariants. Generic import,
EGL, worker and device faults retain their typed fatal path; this is not a
recovery protocol for arbitrary renderer faults.

Foreign preview updates coalesce independently at the recipient output's
cadence. They must not add the slow preview head to the visible source's
existing Present retirement cohort or change its MSC clock. A source with
no visible ordinary sampling output needs an explicit deterministic preview
owner when a visible instance samples it; otherwise use the existing Skipped
settlement. Cover hidden workspaces and ReplaceApplications in this rule.

Device/context generation changes invalidate incompatible queued snapshots
and publication resources through the existing suspend/recovery lifecycle.
A descriptor alone is not a current-generation authority. Cross-device
hot previews are unsupported in this repair and revoke their publication.
Ordinary moved surfaces use the existing cross-device transfer fallback;
that separate, one-shot path retains normal renderer-store failure semantics.

Release criteria (status at final code qualification):

1. Implemented: bounded ownership reducers cover supersession, eviction,
   epoch invalidation and retirement. No GPU reset recovery claim.
2. Passed: native-worker and pixel regressions for initial publication,
   repeated source updates, donor eviction while a frame waits and
   same/shared stores. Hidden-source ownership is a runtime-fixture proof
   (`foreign_previews_do_not_join_the_ordinary_present_retirement_cohort`).
   Preview partial repaint and the full native owner loop remain
   source-reviewed; these suites do not prove buffer-age preview pixels.
3. Budget/import/missing-image failure tests proving revocation and continued
   ordinary frame service; complete receipt and input withdrawal behavior.
   Runtime fixture tests pass; physical KMS/input acceptance remains open.
4. Passed: typed diagnostic preservation, fixed refusal strings and the
   updated classification regression.
5. Passed: 35 named mutant controls, the full repository gate, architecture
   models and source review. Master integration and the newly pinned release
   are recorded under `t284-release-01`; release assembly follows this commit.
6. Attended multi-head Alt-Tab acceptance, including hold/release, navigation,
   cancellation and source updates. Existing acceptance remains open.

## Implemented recovery boundaries

Foreign snapshots are exported with promotion only when another store needs
that source. Initial publication prepares the already-displayed revision once;
a busy donor defers it. Admission only attaches owned, budgeted snapshots.
The recipient imports those planes directly without another capture. The
snapshot budget is 128 allocations and 256 MiB, charged until the last frame,
EGL import or rendered output releases the allocation. Retired import leases
prevent a delayed frame from leaving an unaccounted cache entry behind.

Hot demand is keyed by image and instance output. An image previewed on one
output can still take the ordinary cold path on another.

Ordinary foreign Surface layers use lazy, per-output demand recorded at
admission. A later preparation pass transfers the displayed immutable image
once through the existing donor export/target restore path, records the new
local owner, and removes the demand. It does not charge the preview budget
or rebuild display lists every idle service pass. WorkerPending and
WorkerQueueFull retain demand until worker completion wakes the owner; no
new retry timer is used. Other outputs continue independently. No donor for
a displayed image is a classified invariant. Restore uses the existing
import-or-transfer fallback across devices. Export/restore are synchronous
maintenance visits on idle stores; a large cross-device copy can pause the
owner during that move; copy latency has not been measured. At most one
export/restore pair runs per owner pass, with a one-second timeout per visit.
A restore that reports an existing image needs an export confirmation (one
additional bounded visit). A still-staged copy defers: promotion makes it
exportable, while rollback lets the next pass capture it afresh. Ready
remaining misses report preparation work separately from scanout
frames; busy stores rely on completion notifications. Store exhaustion and real
renderer/device failures retain ordinary capture's fatal semantics. Cold copies
are bounded by one copy per image and store under the existing store budget.

Retired foreign imports have per-store cleanup requests, including across
inventory invalidation. Busy stores retain requests until completion. The
requests hold weak snapshot references and prune dead entries, so their
bound follows the shared pool of at most 128 live charged snapshots across
this native owner, hence at most 128 per store. They retain no FDs.
A cache hit on a different donor's copy replaces the old import, even when
the immutable image identity is the same. Early stale-epoch rejection clears
imports too.

Local reads use a shared weak registry and reference-counted guards. Frames
carry guards into worker custody; frozen software and GPU Present sources
hold their own guards until settlement. A closed or superseded window can
request eviction immediately, but the actual broadcast waits for the last
reader. These images remain charged inside the existing renderer store; no
extra plane exports or pixel copies are needed. Ordinary guards end at render
completion, before scanout retirement. A cloned frame keeps its guards.

A render-device inventory refresh does not destroy live image stores. It
invalidates foreign snapshot epochs while preserving owners in surviving
stores and outstanding eviction requests. Actual store clear invalidates
owners and cancels those requests. Registry replacement fails with a classified invariant if it would orphan
readers or outstanding eviction requests. Device/context loss during a partially submitted
Present remains a fatal device failure; this repair does not introduce a
GPU-reset recovery protocol.

Only a foreign-preview failure with InvalidRendererImageId or
RendererImageStoreFull may withdraw an unsubmitted frame. Generic slot,
client-import, EGL and KMS failures keep their fatal path. Recovery records
are per output; a second failure is a fatal invariant. The one-second deadline starts
at the first recovery attempt and continues after mirror withdrawal. A
recorded failure reports owed owner work. Withdrawal also waits for any
older protected mirror frame, whose flip watchdog remains active. Every tick and admission checks recovery
state, including ticks made by the Present driver on neighbouring outputs.
These native call sites and superseded-mirror skip are source-reviewed;
worker/KMS observations are injected at the runtime test boundary.
The driver now checks those neighbouring reports instead of silently
ignoring their failures.

The failed publication is revoked only if its owner and generation still
match. A GPU Present retries from its original prepared candidate, frozen
source realizations and presentation order, with no WM presentation tier.
A newer publication stays installed but appears only in a subsequent
retained repaint. Software Presents use the same rule. Only the unsubmitted native-frame
mapping changes: the output cohort, already-submitted owners and MSC clock
stay fixed. A preview-only Present with no physical submission can take the
existing Skipped settlement. Errors rebuilding a required replacement remain
classified invariant or device failures, never a silent wait on a withdrawn
frame. With guarded local sources, a frozen CPU source set and the tier
omitted, software retry has no expected custody refusal. A new multi-output
software cancellation protocol is outside this repair.

Shell first-frame claims bind to an exact (output, frame), with at most one
bound frame per output. Recovery re-arms exactly that frame's claims. A newer
frame number cannot satisfy them. Topology preparation waits for bound claims;
suspension re-arms them after draining and settles abandoned recovery records.
Topology begins only after recovery settles. Candidate and rollback first
frames omit the WM tier, preserving topology completion accounting; rollback
drain settles residual records and re-arms every unretired discarded shell
claim, including frames without a preview failure. A claim re-arm requests a
retained repaint. Rebind also requests the omitted WM tier after the topology
barrier. Rollback keeps the existing topology disposition: once physical
ownership drains, even a mixed Present that partly submitted can settle as
Skipped. This differs from ordinary preview recovery, which preserves all
submitted owners. Forced device revocation cannot inspect old native failure
records; it withdraws previews of the retained sources being discarded, skips
client obligations and re-arms claims before the old native owner is destroyed.
Grant revocation removes both pending
and bound claims.

The offscreen worker tests, pure ownership reducers and supplied retirement
fixtures exercise these components. The runtime recovery entry point is
executed through a narrow native failure/drain boundary in the fixtures;
worker/KMS readiness is injected, while lowering and scheduler changes use
production code. GPU and software tests preserve the original candidate,
clock and exactly one completion after the retry. Retained recovery defers
its output while another output admits shell work. Cold preparation tests
execute demand/owner transitions with an injected transfer; they do not
claim a native drag or cross-device transfer measurement.

Render-node pixel tests check all output pixels across eight immutable
snapshot revisions after donor eviction. A separate three-context test
alternates same-ID copies from different donors through the same import
cache. Existing import-transfer policy tests force an import refusal and
prove the fallback while preserving resource and device errors. Worker
fixtures cover lifetime, budgets and stale-epoch refusal (log 72: five
passed, including the corrected all-slot cache cleanup budget assertion). They do not run the complete native
owner loop or claim that this release has passed live Alt-Tab acceptance.

## Final code qualification

The review-05 code manifest is
`7d9fc9ce066221b6150277c10cadda2f0c92d543d4a99cb5047e90d85df9c380`.
Its 111 file digests remained unchanged through the final gates. Only this
note was updated afterwards with results and the shared-pool wording.

- Logs 86/87: on each of renderD128 and renderD129, three EGL pixel tests
  and six real-worker tests passed. No KMS device or input access was exposed.
  The additional worker test covers an existing staged copy, followed by
  promotion or rollback and a fresh restore.
- Log 88: full `cargo xtask check` passed, including SDK binding checks,
  strict Clippy, format, layout and workspace tests. Raw summaries report
  6,615 passed, zero failed and 91 ignored; child-helper output can repeat
  summary lines, so this is not a count of distinct tests.
- Log 89: generator, Alloy and Z3 architecture checks passed.
- Logs 79, 83 and 85: 35 named fault controls were killed by their targeted
  tests on separate source copies and targets. Log 83 also preserves a
  broken early-exit mutant that failed to compile; log 85 corrects it and
  records the targeted failure. It is not counted as a compile-time kill.
- Log 82 retains the preceding gate's layout failure at 1,007 lines in
  compositor_graphics.rs. Moving the topology composition function, unchanged,
  to its own module brought the final tree within the layout limit.

The full native owner loop, physical input and attended Alt-Tab remain the
release's live acceptance boundary.

## Probe history

The evidence retains every initial failure. Logs 01 and 02 are probe compile
errors. Log 03 expected the wrong reduced error; source inspection established
that admission emits InvalidTarget. Log 04 is the corrected EGL pixel proof.
Log 05 is the first worker probe's compile failure (unavailable direct test
dependencies). Log 06 used an unsupported direct-CPU source allocator and
also exposed inadequate probe teardown. Log 07 passes the private-worker
case but fails shared-worker startup because the predecessor was not joined.
Log 08 uses explicit bounded worker shutdown/join and passes both cases.
Those fixture failures are not additional claims about the installed session.

## Connections

- [Renderer import boundary](../../renderer-import-boundary.md) owns immutable
  storage, import and renderer resource lifetime requirements.
- [Compositor graphics](../../compositor-graphics.md) owns surface-instance
  composition and content identity.
- [Held capture plan](../plans/kgo1ugnz-held-capture-and-blind-wm-capabilities-for-niri-parity.md)
  remains awaiting attended acceptance.
- [Renderer performance investigation](wwr7oaer-reduce-per-frame-capture-and-cpu-raster-cost-without-reusing-live-image-storage.md)
  retains the measured optimization scope; it does not prove this UI path.
