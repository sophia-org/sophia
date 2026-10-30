---
id: 4rifczhw
date: 2026-10-28
kind: plan
tags: [plan, shell, performance, custody]
---
# Reduce continuous-frame CPU without weakening custody

## Scope and exit

Reduce Sophia's CPU per completed frame for the changing 1920×24 owned-ring
panel without weakening immutable acceptance, admission, authority freshness,
revocation or release-after-last-lease. Mason requested this plan on October 28,
2026, after the matched desktop comparison and the architectural review with
Muse and Claude.

This follows the [t331 evidence and t343 qualification plan](mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md)
and informs [t289's broader animated-workload work](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md).
It does not reopen their completed exits or replace `todo.md` as the task ledger.
The exit is a separately qualified improvement, or a documented no-change result
for each selected mechanism. A profile share is not a savings target.

## October 29 current gate: matched CPU comparison qualified

`desktop-matched-cpu-02` completed all six arms. Root independently recomputed
raw CPUwin bounds, anchors, process identities and latency/cadence; qualification,
producer receipts and PAM/terminal cleanup passed. All 266 saved files remain
unchanged, and all 196 external pins match. Claude's independent read-only review
found no discrepancy. Failed01 remains failed; no row was reused or pooled.

Mean bound-process CPU per frame: Sophia [1027.5,1070], niri [305,347.5], XLibre
[85,125] microseconds. Sophia minus niri is [680,765], versus XLibre [902.5,985].
This is the qualified CPU shared-memory reference, not a performance promotion,
whole-desktop accounting or proof of intrinsic compositor efficiency. Pixel
production matches; custody, copies and presentation contracts still differ.
Two interleaved observations per stack are tick-accounting bounds, not confidence
intervals. Latency endpoints differ and cannot rank end-to-end responsiveness.

[The producer comparison note](../investigations/iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md#october-29-matched-cpu-result-and-root-verdict)
contains the per-process results and limitations. Next is the GPU-buffer
compatibility matrix (renderer/scene, transport, format/modifier, synchronization,
lifetime and feedback) before adapter implementation or another native package.
Preserve CPU artifacts/evidence as the reference. The native capture hold is
closed; no new builds or native run are implied. W2 stays frozen, W3 remains
written-only, and the collect lane stays closed inconclusive.

## Owner approval and W2 allocation, October 29

Mason approved the next sequence: retain the matched CPU reference; complete
GPU-buffer compatibility mapping; implement and qualify the smallest matched
GPU path; run its separate comparison; use both references to select attributable
optimizations; resolve the canonical W3 standing-credit design gaps before a
prototype. These are successive gates, not permission to bypass producer parity,
correctness, package sealing or attended native execution. Do not bundle W2,
single-submit or other optimizations into the producer comparison.

Mason also requested resolution of W2's protocol allocation. Root assigned it
**shell r12 / bit 17**, while adopting and preserving **draw r11 / bit 16** from
t341's unmerged contract. The authoritative
[shell extension allocation ledger](../../../protocol/sophia-shell-allocations.md)
closes the identifier decision. Overview and W3 remain unallocated; no r13/bit18
reservation is made. The old informal overview “next available” pointers are
superseded. t341 is deferred, not a source of reusable identifiers.

Cumulative grammar and explicit capability grants are required, with exact
image/header/record mapping rows rather than numeric `>=` widening. r12 does
not imply draw or DMA. Unsupported required combinations refuse; a compatible
revision may be selected within the offered range. Frozen W2 measurements remain
prototype evidence, not retroactively conforming cumulative-r12 evidence.

W2 integration can now proceed past D1 on fresh branches: coordinated byte
contract/goldens plus C/Rust parity, explicit negotiation/coexistence tests,
reviewed upstream-reachable SDK commits, then xtask vendoring and host review.
C parity, public pin reachability and production qualification remain open.
The allocation ledger changes no runtime, SDK archive, measured artifact or
policy. Existing mirrored contracts remain byte-identical until that coordinated
integration. Native GPU comparison remains the immediate benchmark critical path.

### GPU source matrix and producer gates

The [GPU-buffer compatibility matrix](../investigations/nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md)
traces the pinned exporter and existing external clients. Sophia already has
producer-completed XR24/LINEAR export and owned capture; the external clients
need linux-dmabuf and DRI3 adapters. Lom's ordinary diagnostic constructor cannot
export, so a narrowly scoped exact-device external constructor is first, followed
by one in-process renderer bridge shared by the external clients. Preserve the
Sophia grant path and use the same renderer/scene closure in all three arms.

The first GPU lane uses fresh exports with a two-frame ownership cap, not unsafe
redraw after a compositor notification. X IdleNotify follows a flush and is not
proof for Lom's foreign-GPU-complete reuse precondition. Keep CPU/default paths,
W2 and single-submit unchanged. The matrix names device-free failure/lifetime
tests, then separate native pixel/import/synchronization qualification before any
comparison package. Installed niri source identity and runtime format support
remain open evidence, not inferred from a nearby source checkout.

The external constructor's device-free gate is closed at signed local Lom
`1a0dc2329` (`bench/gpu-device`, parent`dcb1eb88`). Exact node/adapter selection
and separate external evidence are implemented; grant behavior and render/export
commands are preserved. Full gate: 81 Rust tests, 17 tooling tests, strict Clippy;
four guard mutations fail. No GPU/native execution or release build. The matrix
records the remaining hardware checks.

The in-process bridge gate is now closed at signed local niltempus `554935ee2`
on `bench/gpu-bridge`: shared scene, physical PCI binding, bounded two-frame
ownership and a C ABI. Six bridge tests, the linked C ABI check, 14 peer tests,
strict Clippy/formatting and six assertion-killed mutations pass. Both producer
closures pin Lom `1a0dc2329` under a local-only provenance exception. No GPU open,
release build or native run occurred.

Both external adapters are implemented at signed local `622145d4a`, parent
`554935ee2`, as separate GPU build targets. Device-free feedback/import-call,
FD ownership, release and CLI controls pass; six mutations are killed and the
CPU clients preprocess identically to their signed baseline. GPU journal labels
separate render/export from CPU fill. The matrix records the exact limitations.
The supporting GPU receipt/config gate is closed at signed local `d81ccf6ad`:
strict transport-specific journal order, metadata and counts, seven Python tests
and three killed mutations. Installed niri 26.04 accepts the pinned renderD128
configuration and refuses a misspelled key, with no device open during validation.
Its source commit remains unknown. `receipts_valid` is not native qualification;
the self-reported PID/output still need process and actual-mode binding.
Next: real non-grey pixel/import/synchronization/release proof, process/feedback/
anchor/output/cleanup checks, then a separately sealed comparison package.
Frozen CPU/W2 references remain unchanged; no performance claim follows.

Untimed consumer preparation advanced at signed local `8a32087cb`: explicit
three-stack capture/hold/refusal/lifetime contract and a strict non-grey pixel
checker (five synthetic tests, three killed mutations). This is not a native pass
or launchable package. Source review found that d9116ff0 observer recipes cannot
export a PAM through a writable host destination. The wrapper also makes inherited
stdout shared/untrusted, discarded or VT-visible. Immediate prerequisite: review
and prove a private process-bound diagnostic pixel channel, without widening
recipe filesystem authority, plus the same-namespace capture controller.
Colour/hold entry points and per-stack capture normalization follow; only then
release/link receipts, a sealed diagnostic package and attended native proof.
The [matrix](../investigations/nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md#untimed-consumer-capture-preparation-october-29)
records the exact remaining gaps. That preparation did not launch native work.

The private delivery **source gate** is now closed at signed local
`a56ace891` on `bench/gpu-capture-delivery` (exact base `d9116ff0`). The
feature-on lib passed 1119 tests with 65 ignored, protected stdio passed 2/2,
feature-off refusals passed, clippy/layout passed, and eight guard mutations
were killed. The [matrix's delivery receipt](../investigations/nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md#private-delivery-source-gate-october-29)
keeps the scope and hashes. This proves device-free framing/custody machinery,
not a real authenticated capture or pixel match.

The single-capture controller source gate is now closed at signed local
niltempus `7214d43372bd019ea4db55bc0ad2ff2662ef722d`: 14 controller fixtures,
5 pixel fixtures, 7 journal fixtures and 8 killed/restored guard mutations.
It waits beyond `Started` for complete JSON/PAM/zero-byte-stderr evidence,
checks a held candidate before/after, and refuses stale sequences or any failed
attempt. Its join is serialized directory custody, not launch-monitor PID or
per-message writer identity. The [matrix's controller receipt](../investigations/nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md#private-capture-controller-source-gate-october-29)
records the log-format correction and proof limits.

The Sophia colour/hold source gate is closed at signed local niltempus
`80afd166859156cd8628a63b4b73d5571ecada2b`: separate feature-required peer,
SDK external readiness loop, parent pidfd/ancestry-bound READY/capture/ACK and
local sibling wrapper. Final checks: 10 hold + 14 controller + 5 pixel + 7
journal tests; Rust 17+17 feature-on and 14 feature-off, clippy/fmt, historical
shared-canvas check, and 6 killed/restored mutations. The
[matrix receipt](../investigations/nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md#sophia-colourhold-source-gate--october-29)
records custody limits, separate generation domains and the single-producer
profile requirement. This is device-free, not authenticated native delivery.

External colour/hold and consumer-normalization source gate is closed at signed
local niltempus `0f3bfc42c506d4379f14b64092702253672afe91` (parent `80afd166`).
Wayland's barrier is inside fresh submission; X holds after presentation and
may already have released its client export. Default and diagnostic bridge
artifacts occupy separate slots, and timed links refuse the diagnostic symbol.
Final device-free checks: 20 Python tests, Rust 7 feature-on / 6 feature-off
bridge tests plus 14 DMA tests, adapter/C ABI controls, 6 killed/restored
mutations and both wrong-library link refusals. The matrix records fixture
limits; these are debug source controls, not release or native evidence.

The external native-wrapper source gate is closed at signed local niltempus
`286eb0ed75ee91ac6fc4cbc3ec06188d728ef78d` (parent `0f3bfc42c`). Fixed
niri/XLibre recipes now bind processes, devices, outputs, capture and hold
events, with bounded local cleanup. Final checks: 63 Python tests, C capture
controls and 12 killed/restored mutations. The installed niri binary accepts
the second-GPU ignore syntax and rejects a misspelling without opening DRM;
this does not establish runtime exclusion. The matrix records the receipt
and remaining qualification limits.

External handoff staging is implemented at signed local niltempus
`fb21242f9df7a42fc6b6b13af1a7c91120c4c701`, parent `286eb0ed7`. It reuses
the hash-pinned CPU02 PAM/VT template unchanged, with enclosing 300/360/420
second budgets, complete seal/pin checks before privileged helper import,
and an exclusive attempt record. Staging leaves the seal absent. Eight new
handoff tests, 19 wrapper tests and eight killed/restored mutations pass;
all 166 inherited host-helper hashes match. No native handoff was executed.

Sophia's fixed diagnostic configuration and handoff source are integrated at
signed local niltempus `8c527e8bcb142ca534b4bb41717b8d93468733e0`, parent
`fb21242f9`. One capture observer, one GPU-direct DMA bar and explicit artifact
pins feed the existing sibling Session/driver wrapper and PAM handoff. Nine
recipe, eight handoff and ten hold tests pass; eight mutations are killed and
restored. The available debug host accepts the generated configuration and
rejects five invalid controls with no DRM opens under strace. That parser-only
control uses a non-launched stand-in peer; it is not a release-ready recipe.

The startup-readback source gate is now signed locally at niltempus
`d997e29f76bd845c5394a10ac3dcb16a5222cdac`. It reuses the credential/live-TGID-
pidfd receiver with an explicit ready acknowledgement, a pinned helper and a
15-second startup window concurrent with the hold driver. It requires the
actual 119880 mHz row, not the nominal 120000 head log. Thirty-four Python
tests, 10 socket plus 32 binding tests, clippy and nine killed/restored
mutations pass. The full xtask run hit an unchanged bridge/check.sh source-
audit failure; the rest passes with only that named test skipped. This is
startup/source evidence, not a native mode or capture qualification. Receipts:
`~/.local/state/matched-gpu-sophia-environment-01/` (58 entries, manifest
`e29fa433ffb648348b9c9115ed6a39e837c96af66077e68c9d9be0f69972de37`).

**Immediate order, Mason's October 30 instruction:** CPU direct-fill
custody design/adoption before further GPU implementation. Do not let the
GPU qualification or direct-render follow-up displace or delay CPU work.
Existing holds, sealed packages, W2 trees and evidence remain untouched.
`desktop-matched-cpu-01` failed qualification and `-02` completed; neither
measured Sparse Strips scene rendering directly into an owned-ring slot.

1. **Production pure-CPU direct fill.** Follow the
   [worker-custody design](../investigations/i9ljdlt4-how-can-lom-render-vello-cpu-directly-into-an-owned-ring-slot-across-its-worker-boundary.md).
   Source correction: Lom `32e0bab3` native `--serve` always requires a GPU grant;
   its `RenderImage::Cpu` is GPU readback. The consumer source gate below adds the
   explicit pure-CPU renderer and an owned SDK slot loan, keeping the service
   thread polling while the worker renders. The imaging
   API already supports a borrowed destination, so no imaging fork is needed.

   The SDK custody source gate is signed at `5d895652` with parity documentation
   at `99b05e34`: 948 workspace tests, zero-warning clippy and four killed/restored
   mutations. Only baseline `role.rs` formatting remains outside the changed-file
   clean check. Receipts are in `~/.local/state/cpu-owned-fill-sdk-01/`.
   C `9adb3c5` closes behavioral parity under documented caller ownership:
   74 test binaries, five worker scenarios, Nim binding and three killed/restored
   controls. Its existing pixels/offer API needs no new helper; callers enforce
   lifetime, exclusion, geometry and identity explicitly. Receipts are in
   `~/.local/state/cpu-owned-fill-c-01/`. No verified SDK pin update or production
   adoption follows yet. Signed local Lom `ceb89664` closes the consumer source
   gate: explicit `--renderer cpu`, default GPU unchanged, 106 Rust and 18 Python
   tests, clean full check and seven killed/restored snapshot/abort controls.
   Owner snapshots survive through commit; Accepted alone blocks reuse. Receipts
   are in `~/.local/state/lom-cpu-direct-fill-01/`; the linked design records
   hashes and coverage limits. Its SDK `file://` pin remains a development
   exception. Signed successor `9b1cf041` adds protected panel `renderer "cpu"`
   selection because Sophia launches components with fixed `--serve`; CLI values
   override config and the default remains GPU. Its full gate passes 110 Rust
   and 18 Python tests plus five negative controls. Release `123746c6` is prepared
   in the separate `lom@lom-cpu-native-config` pool, with receipts under
   `~/.local/state/lom-cpu-native-config-01/`.
   Ordinary native CPU startup, seconds-clock presentation, custody and cleanup
   now qualify through the saved-data audit below. Next qualify the same-renderer
   diagnostic copy reference,
   preserving the scene and conversion baseline. Fixed-time clock observations
   do not provide an exact frame denominator or a 120 Hz workload. No
   native/default or performance promotion follows from source tests.

   `lom-cpu-native-01` spent its single attempt: native collection and cleanup
   succeeded, but postflight incorrectly required a field Provlita never emits.
   Keep that package failed and immutable (manifest `560eed90`). Separate
   `lom-cpu-native-audit-01` corrects the dock schema and binds the grant through
   the host join; 20 tests and the full saved-data audit pass, without a native
   rerun. All 74 original files are unchanged. Its receipt is `c7ed9eb0`, manifest
   `e46e91b6`. The 100 Lom presentations, denied GPU grant, stable bindings and
   clean custody/cleanup qualify this ordinary seconds-clock path. No pixel
   readback, comparison or default-adoption claim follows.

   The diagnostic copy-reference source gate is now signed and pushed as Lom
   `8358cb3f` (`cpu/copy-reference`), using published Rust SDK `99b05e34`.
   Default and feature gates pass 113 and 120 Rust tests; both arms share the CPU
   renderer and conversion, and actual SDK peer bytes match device-free. The
   direct-then-copy native qualification used one diagnostic binary and
   a separately pinned feature-on config checker. Each arm is an independent
   startup/custody/cadence/cleanup check with descriptive fixed-time CPU only.
   No ABBA, cross-arm pixel equality, CPU difference or savings claim belongs to
   this first native reference gate. The
   [direct-fill investigation](../investigations/i9ljdlt4-how-can-lom-render-vello-cpu-directly-into-an-owned-ring-slot-across-its-worker-boundary.md)
   records the source receipt and the later performance gate.
   `lom-cpu-reference-native-01` is now complete (manifest `4ad78dd3`,
   65 files and 268 pins): 46 package tests and both post-seal check-only arms
   pass. Runtime `2facb26d` and checker `134291da` share signed source `8358cb3f`.
   Both arms passed: 100 Lom presentations each, 71 stable binding checks each,
   zero late sampler slots and clean PAM/TTY cleanup. Root's separate
   `lom-cpu-reference-native-audit-01` rechecked raw evidence and reproduced both
   fixed-time CPU results exactly, without changing the package. The attempt is
   spent: do not relaunch it. The unpaired clock runs establish no speedup.

   The separate `lom-cpu-pixels-native-03` gate now passes for both CPU arms:
   actual capture brackets and launch-custody receipts, one opaque static 1920x24
   scale-1 scene, 46,080 checked pixels per arm, identical produced FNV64 and
   byte-identical captured BGRA strips under equal configs/model inputs. All 71
   manifest entries verify; each arm has 118 stable binding checks and clean
   Session/PAM/TTY cleanup. Root audit `lom-cpu-pixels-native-audit-03/RECEIPT.json`
   is `0ee9a4972c3737f981c2d17684b6fac24476d883d0e654090738c2b28c77239e`.
   Failed predecessors 01/02 stay failed; 03 is spent. This closes that narrow
   opaque displayed-path gate, not dynamic/blended/scaled rendering or performance.
   The predeclared same-scene performance gate remains before benefit or
   default-adoption claims; GPU direct-render work still follows CPU adoption.
   Its selected design is a separate diagnostic cadence executable: re-render the
   static scene after exact Presented, using the existing release and scheduling
   path, then stop after 120 warmup frames plus 2,000 measured completion intervals.
   Compare direct/copy/copy/direct separately at 1920x24 and 1920x512 using kernel
   completion anchors, Session/Lom/Hagia CPU accounting bounds and explicit owner
   timing endpoints. Cadence is an outcome, not an assumed 60 Hz result. This is
   instrumented repeated-scene qualification, not isolated memcpy or ordinary idle
   desktop performance; overlapping bounds are inconclusive. The diagnostic source
   gate is signed and pushed as Lom `3fe206dc` (tree `289f14e4`): 239 Rust tests
   passed, two existing entries ignored, and default/feature Clippy is clean.
   Release provenance is verified: runtime `889219dc`, feature-on config checker
   `16c4881a`, and all four static configs pass offline checks. The source and
   pool remain frozen while the fresh attended package is prepared; see the
   direct-fill investigation. No performance or adoption result follows.

   Two native pixel movers remain: CPU pixels and GPU buffers. App/user policy
   chooses the supported renderer; Sophia enforces authority. Direct fill is the
   intended default **within CPU mode**, not forced CPU rendering or hidden fallback.
   Hybrid rendering inside an app still produces one of these output forms. See the
   [application guide](../../native-rendering-choice.md).

   Preserve exact SlotReleased reuse, one render job, fresh commit validation,
   abort-without-publication, existing-pixel slice upload, GPU/DMA behavior and
   Session's custody copy. Keep registration capacity fixed and expose an exact
   width×height×4 frame slice; do not import the serial benchmark's per-frame
   capacity assumption into a multi-panel service. Preserve pixel conversion.

   Qualification compares the same new Vello CPU renderer with slice upload versus
   direct fill, including real scenes, alpha, scale and larger surfaces. GPU
   readback is not the copy-only baseline. Require device-free custody/equivalence
   proofs, native GPU-denied CPU arms, and predeclared combined/per-process CPU
   intervals, elapsed render-to-ready time and cadence before any default flip.
   The removed client copy is 184320 bytes at 1920×24; its benefit is unmeasured.
   Neither the shade-fill results nor a plausible small-scene CPU advantage proves
   that benefit. GPU implementation must not displace this CPU work.

2. **GPU direct render, strictly after CPU direct fill.** Record the proposed
   Vello render into the exportable target as a later design/qualification
   item, not implementation authority from the recommendation alone. Today's
   reported client path renders Rgba8Unorm then blits into LINEAR BGRA/XR24,
   with two submissions and an extra full-frame GPU copy before Session's
   unchanged custody capture. First qualify a LINEAR RGBA-order export format
   on the same device through exact SDK capability/runtime format rows,
   Session EGL capture and compositor imports, using the non-grey Vulkan→EGL
   oracle. No channel-swizzle shortcut or assumption that an existing texture
   view API proves export-format/storage usage compatibility.

   Only after that gate consider rendering into the export view while keeping
   foreign release, fence and SourceReleased semantics. Judge DMA stage timing
   and cadence at bar and large-surface sizes; CPUwin is not GPU time or
   end-to-end latency. Do not move Vello GPU into Session's capture/composition:
   the proposed saving is client-side, and no extra Vulkan/wgpu context beside
   the EGL/GBM owner is justified here.

The existing GPU qualification prerequisites remain recorded, in that lower
priority lane: real import-refusal and delayed-release controls, separate
signed diagnostic release/link receipts, then an attended sealed package.
Authenticated capture, actual native mode, consumer pixels, release and outer
PAM/VT cleanup remain unqualified. Real server shutdown codes and niri's
second-GPU exclusion are first-attempt calibration checks; any refusal remains
a failed attempt. No timed GPU comparison or W2 change follows from source gates.

## Earlier resumption context

The first question is how much repeated control and lifecycle work can be
removed while keeping the current guarantees. There is no measured irreducible
Session floor. Muse withdrew the earlier “about 1.2 ms/frame” floor and the claim
that matched external comparisons were absent. Historical 60 Hz arms, profiled
120 Hz arms and the uninstrumented comparison must remain separate.

W2 retires a predecessor on presentation of its successor. Its device-free
codec, negotiation, custody and lifecycle proofs are on `cpu/w2-prototype` in
`~/dev/sophia-cpu-w2`; option (b) closed at signed local `7cc36ebde`. The
owner-wait proof closed at `c3abceac6`. It proves collection at the next explicit
resource-service call when leases and capacity permit, not a 25 ms wall-clock
release bound. The 61 failures in the earlier broad native-session run were
reproduced and isolated to group-writable checkout KDL inputs, as recorded below.
That closes their cause, not a new candidate's full test gate. The three focused
planner tests passed and their mutation failed.

Mason said “Implement the plan” on October 28. The lanes below are now active
in parallel, rather than waiting for W2's native result. Claude owns W2 wiring
in its isolated host, peer and runner trees. Host wiring is signed at
`88c1cda50`, peer counters and SDK repin at `b12b0a1c`; runner wiring and
strict counter parsing closed review at `17dc6a6f`. The original host already emits negotiated revision, but no capability
word. W2 artifacts extend that existing receipt; the original-artifact screen
keeps its original acceptance rather than inventing missing fields.
The native-owner agent owns S1's remaining fixtures and default-off Session
wiring. W1's delayed event delivery remains shelved. Earlier custody-dispatch
and batching screens did not resolve a gain; frequency alone is not cost.

`continuous-release-overlap-01` is complete and inconclusive, with no promotion.
Its temporary build hold is lifted; its exclusive native attempt must never be
rerun. The latest successful uninstrumented comparison is `desktop-cpu-04`.
No old package, gate, result or source pin is to be rewritten.

Codex owns integration review, the test/provenance blockers and package seals.
Producer owns single-submit source/build receipts and a fresh unsealed ABBA;
transport owns an opt-in baseline attribution diagnostic. Muse owns a written
bounded frame-stream design, not a production protocol change. Coordinate
through herdr; source notes and receipts are authoritative and mem0 is an index.
No agent changes another lane's checkout or a frozen package. Mason launches
native attempts over SSH with sudo, one at a time; builds pause during captures.
Local SDK/import pins and missing C parity are permitted only as explicitly
recorded diagnostic exceptions, never as public protocol allocation or merge
approval. No storage or authority-window change is authorized by attribution
alone. The exact mechanism must receive its own custody review.

## Immediate critical path

The task ledger is `todo.md` t344. Mason's October 29 priority is the
[matched-producer benchmark](../investigations/iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md):
CPU-filled shared-memory comparison first, GPU-buffer qualification second,
then further native optimization experiments. Written W3 and single-submit
reviews may continue; they do not bypass these measurement gates.

| Lane | Owner | Immediate implementation and exit |
| --- | --- | --- |
| Matched CPU producers | Codex, first implementation gate | Use the existing SDK direct-fill ring API; match external CPU pixels, buffers and pacing. Preserve the GPU default, qualify the new producer and pin a fresh cross-stack package before native launch. |
| Matched GPU producers | Codex, second qualification gate | Verify the same renderer/scene and compatible GPU-buffer transport/synchronization on all three stacks. No readback fallback disguised as equivalence; missing support blocks this lane. |
| W2 effect | Complete / Codex | Separate02 and03 blocks resolve combined and Session CPU benefit and the expected counter. Performance gate passed for this workload; no fourth run or default-on promotion. Frozen evidence moves to integration review. |
| W2 side effects | Complete | `continuous-retire-on-present-screen-01` passed the predeclared upper margins; not equivalence or a gain. |
| Single-submit | Parked | Completed combined interval [-55,+25] us/frame; inconclusive, no promotion. |
| S1 | Parked | Completed Session regression [+25,+65] us/frame and rare quiet conversion; no promotion or wider-predicate work. |
| Cost attribution | Complete diagnostic | Two arms validated; use separate inclusive scopes and thread windows to rank investigations, never as additive savings. |
| Empty owner work | Closed: inconclusive CPU, no promotion | [Collection guard and proofs](../investigations/p4jd1ymh-which-complexity-and-repeated-work-costs-belong-on-the-frame-cpu-critical-path.md) are signed at `75ff298ab` on a separate `d9116ff0` lane. Source review and runtime tests pass; diagnostic02 qualifies actual skip/scan movement. CPU ABBA combined [-35,+45] us/frame is inconclusive; no repeat or sub-resolution benefit claim. Settlement state, expiry caching and authority checks are unchanged. |
| Frame stream / W3 | Written reconciliation complete; authority decisions pending | Signed `dbe426932` records the post-W2 trace and six contract gaps. Resolve them against canonical `tubnycgx`; implementation/live wiring stays held. Matched benchmarks precede further native experiments. |
| Single-submit disposition | Codex / Mason, written review | Decide fork maintenance versus upstream API while matched benchmarks are qualified. Combined CPU remains inconclusive; no integration or promotion from the peer interval alone. |
| Integration gates | Codex / Claude, active read-only | Map minimal W2 C parity, public SDK pin and protocol allocation work, including the t341 boundary. No publication/source changes from this mapping; preserve frozen evidence and review the concrete proposal. |

W2 off/on confirmation has completed; no further W2 measurement is scheduled.
The empty-work candidate is closed as inconclusive; do not rerun it. Matched
CPU and GPU producer qualification now precedes further native optimization work.
Single-submit, attribution, S1 and the W2 side-effect screen remain complete.
Frame-stream proof remains separate; do not bundle mechanisms. Each new candidate
has fresh inputs and one exclusive attempt. Attribution arms are not CPU comparators.

For the W2 side-effect screen, predeclare a maximum unresolved regression of
50 µs/frame combined and 25 µs/frame for either bound process, using the upper
candidate-minus-baseline accounting bound. Exceeding a margin is not accepted;
insufficient precision is inconclusive. This is a regression screen, not an
equivalence claim. A beneficial effect requires an upper CPU difference bound
below zero, its predicted mechanism and the existing correctness/cadence gates.

### Matched-producer priority, October 29

The [benchmark investigation](../investigations/iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md)
records the accepted plan, exact baseline, direct-slot CPU API, GPU feasibility
matrix and acceptance gates. `desktop-cpu-04` compared different producers;
keep its numbers as concrete-path evidence, not intrinsic compositor efficiency.
The XLibre PageFlip-off COPY/no-compositor condition and different feedback clocks
remain explicit even after producers match. Niri represents one compositor.

W3 reconciliation `dbe426932` is signed and local on `cpu/wake-design`. Its six
open areas are rate enforcement, response-reservation timing, supersession/W2
cancellation, exhausted-credit semantics, cancellation/withdrawal and expiry
notice. No session-side rate gate may be assumed from the advertised limit.
A benchmark's serial dependency trace does not remove admission/capacity checks
or price server work. `tubnycgx` remains canonical and production work held.
This ordering supersedes the earlier instruction to advance W3 immediately after
collect-idle. Written review can proceed; native optimization work waits on the
matched comparisons. No source/release/package mutation follows from this note.

### Collection gate and the next two decisions

The minimal guard is signed at `75ff298ab`, parent exactly `d9116ff0`.
Correctness is closed; performance is not. The separate diagnostic source is
signed at `6f82de2c9`, parent `75ff298ab`. It counts actual calls, eligible
entries, skips, first-walk scans and selections in both variants. The original
scan is selected only by the diagnostic-baseline build feature. The opposite
feature tests pass, and removing the actual return makes the scan test fail.
Receipts: `~/.local/state/collect-observe-proof-01/RECEIPT.json`, SHA-256
`ea7752e47a012e3e02a5e801725b49126ac88e4bda7ae6b0996e315d78941b62`.

`continuous-collect-work-01` is sealed as two diagnostic arms, not an
ABBA CPU comparison. The reducer joins exact owner/thread timestamps with the
existing five-second clock observations, requires at least two strictly inside
the anchors, checks global-call and owner-call deltas, and reports any fixed
pre-observation offset. Selections precede journal emission. Ratios cover only
the observed interval; baseline eligible selections must be zero, whereas the
candidate's zero is structural. Instrumented hosts never enter the CPU package. The sealed check-only path
passed both SSH/idle-tty4 and runner checks without a Session launch. Manifest
SHA-256 `3e4f306b80ea1d35f1f59e2f76565764b0168c0df4e6042f2c0555f639c54045`
binds 133 files and 37 external pins. Root handoff receipt:
`~/.local/state/t331/continuous-cpu-optimization-01/collect-idle/diagnostic-seal-review.json`,
SHA-256 `4038399aba5cb77de2c453e18f677e5ed6dc59dcfdfc0462c81e43c17d06f552`.
Diagnostic baseline/guard binaries are `28ef2813`/`fcaf1573` in separate slots;
the uninstrumented CPU candidate is `682a781f`. Original peer and runner remain
fixed. The original helper-pin path set contained two historical hash changes;
preparation records verified values from the completed W2-03 pins and refuses
later drift. Old packages were not edited.

Package01 later stopped after a passing baseline native postflight: root's
reducer wrongly required positive `present_clock_service.completions`, a router
counter that stays zero on this owned-ring path. The candidate never launched.
All cleanup passed;01 stays failed and immutable. Three interior observations
show46920 calls,45721 eligible calls,78250 eligible entries,80648 scanned entries
and1199 selections over1199 native completions. The counter scopes are distinct;
these instrumented counts do not price CPU cost.

Fresh `continuous-collect-work-02` replaces that router-activity predicate with a
descriptive positive native-journal completion count in(first,last]. Router delta
is still reported, every collection invariant and exact timestamp join remains,
and all sources/binaries/workload are unchanged. Saved01 inputs fail the old
reducer and pass the new one; boundary/refusal tests and hashes are retained.
Both read-only reviews and sealed check-only gates pass. Manifest SHA-256:
`46427174695f867477b4d19b57bc4406fa5c50ae5345fab66b9974eb99b5da5d`
(140files,37external pins). Root receipt `diagnostic-seal-review-02.json` in the
same collect-idle evidence directory has SHA-256
`cb080f6f2633363947797a7a0480de6728afbf2ad70d25738d116757514c65fd`.
Mason completed02, both arms passed postflight and root saved-data audit, and
mechanism qualification is closed. Across three interior observations per arm,
baseline executed 46942 scans (45743 eligible); candidate skipped 45819 of
47019 calls and executed 1200 scans. Candidate skip fraction is 97.45%.
Baseline eligible selections were zero; the non-eligible retiring walks remain.
The covered intervals are about 10.01 seconds, with 1199/1200 descriptive native
completions. There is no per-call/frame join, full-window extrapolation or CPU
claim. Identities, anchors, input hashes and PAM/VT cleanup passed. Root audit
`post-run-root-review-02.json` in the same evidence directory has SHA-256
`a3eb4ee105b3de9f1cb07c759cda64a0d38bd5979c685fe6df16e676b8c4266d`.

`continuous-collect-idle-01` is now sealed for the isolated uninstrumented ABBA
against the original host. Its prerequisite binds 70 saved evidence inputs;
validation was refreshed after the prerequisite and refusal test were added.
Manifest `77da74f6173358fd0562381c028e34f591dfd7b8620e0c5df814bf2dcaa8a736`
binds 127 files and 45 external pins. All four exact-artifact SSH/panel check-only
gates passed without launching Session. Root receipt `cpu-seal-review-01.json`
has SHA-256 `2179f7f8e88d2358708f8e2165712724419cd48035766fa76c551e2cc0e22849`.
Original host `3897c98c` and guard-only host `682a781f` use the same feature recipe;
peer `e730fef6` and runner `62df9c30` are fixed. No diagnostic host enters this ABBA.
Mason launches with `bash ~/.local/state/t331/continuous-collect-idle-01/run.sh`.
Builds, tests and mutations are held through the native attempt and root audit.
One ABBA remains the benefit/regression screen;
if inconclusive, stop this lane and move to W3. A100ns/walk estimate is a prior,
not a measurement: neither counter movement nor an unresolved CPU interval
establishes a sub-resolution gain. No repeated tiny-gain hunt. Builds are serial
and use separate cargo slots;
no other optimization is bundled, and Mason alone launches native packages.

The CPU ABBA subsequently completed all four arms. Root independently checked
raw tick brackets, native anchors and joins, executable/device/inode/argv and
PID/starttime identities, all source and artifact pins, the mechanism prerequisite,
latency/cadence and PAM/VT/outer cleanup. Every gate passed; no diagnostic collection
records appeared. Combined candidate-minus-baseline is **[-35,+45] us/frame**
(midpoint +5, +0.38%); Session **[-22.5,+17.5]**, peer **[-12.5,+27.5]**.
Pairs are +10 and 0. All intervals contain zero: **INCONCLUSIVE, NO PROMOTION**.
This closes the collect lane under its stop rule; no repeat, equivalence or
below-resolution saving claim. Mechanism qualification remains separate.
Submission-to-completion medians in b1/c1/c2/b2 order are 7513/7511/7510/7507 us;
p99 is 7620/7633/7631/7628. Completion-gap medians are 8342 us throughout,
maxima 8347–8348. These are accounting bounds and kernel completion spans,
not confidence intervals or end-to-end latency. Root receipt in the same evidence
directory: `cpu-post-run-root-review-01.json`, SHA-256
`7c3af5ec0478b1a2a0318b90a2c0f9044b0934f775ec4d315a94799e3c868b38`.
The benchmark uses Vello/wgpu in the peer, GPU texture readback to RGBA, then
owned-ring pixel upload. It does not exercise the experimental shell Draw protocol
or direct DMA-BUF transport. Native-run hold is lifted after audit; frozen W2 and
sealed packages stay untouched. Next work is written/source-trace only.

Mason requested the next two decisions on the immediate critical path after
collect-idle qualification. First, W3's written reconciliation must use the
existing [standing-credit design](../investigations/tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md),
trace post-W2 demand/permit dependencies, and retain bounded credits, expiry,
fresh pacing/admission, response reservations and cancellation/revocation
obligations. W2 demonstrates retire-exchange removal and CPU benefit, not a
measured reduction in wake counts. D1/t341 still gates any new allocation.
Second, decide fork maintenance versus upstream encoder API for single-submit.
Its combined interval is still [-55,+25] us/frame, with an unexplained Session
increase. The peer result alone does not authorize productization. No stacked
experiment, new native package or implementation is authorized by these written
follow-ups; each needs its own reviewed mechanism and qualification decision.

Do not reject an improvement because its interval excludes zero.

Keep initial Cargo jobs at two per lane, reuse existing pools where their source
identity is correct, and never share a target directory across worktrees. Record
signing failures without changing gpg-agent; unsigned work remains reviewable but
cannot enter a sealed source set. No unrelated cleanup, push or merge is part
of the immediate experiments.

### Implementation checkpoint, October 28

`continuous-single-submit-01` completed all four arms after independent root and
Muse reviews. Root rechecked the sealed manifest, raw CPU identities and tick
brackets, native generation/anchor joins and interval arithmetic; Muse separately
reproduced the result. Combined candidate-minus-baseline is **[-55,+25] µs/frame**,
midpoint -15 (-1.145%): inconclusive, no promotion. Peer is [-50,-10] and Session
[-5,+35]; the Session movement is unexplained. Pair midpoints are 0 and -30.
Completion gaps have median 8342 µs and maxima 8347–8350; all postflights and
cleanup receipts passed. Pair latency medians move -17/-4 µs, without a common
end-to-end latency claim. The native submission count was not instrumented;
the baseline two-submit control remains reported/reconstructed.

The no-retry package remains unchanged. Root's durable saved-data audit is
`continuous-cpu-optimization-01/single-submit/post-run-root-review.json`, SHA-256
`fb601ff619bd60c1e4f9354e711d38a9a210063e0de60dd4745edf70c9c9bbc7`.
The build/test hold ended after completion; attribution and other isolated work
resumed. Keep runner binary `62df9c30` from source `2329fca7` for packages that
inherit its validated recipe; do not substitute a current main-pool binary.

S1 default-off wiring is signed and clean at `18eb3302`. Root and Muse found no
code blocker; the full native-session/capture/probe library gate passed 1119,
zero failed, 65 ignored. Its isolated release is `6508a9ed`; actual compiler
commands confirm owner-probe, capture and native-session with custody-dispatch
disabled. `continuous-owner-probe-01` is being staged, unsealed, as a host-only
ABBA against d9116ff0 with the original peer. Its authentication fixture uses
supplied authority and does not prove Factotum or protected launch custody.
Probe re-waits add waits to experimental counters: compare full passes and
completed quiet re-waits, not baseline wait attribution as identical semantics.

W2 host `88c1cda50` passed 1108 native-session/capture/W2 tests, 65 ignored,
zero failed. Its 36 tracked KDL files changed from 0664 to 0644 without changing
bytes. Peer `b12b0a1c` counts successful sends and validated releases; historical
peer end bytes stay unchanged. Runner `17dc6a6f` rejects explicit-null counters
and Python bool/float numeric aliases; its 472 xtask tests and focused Python
gates passed. The runner proves paired settings and counts, not host build
features. Before sealing, the package must bind the exact host binary hash to
its signed source/build receipt and run that artifact's validate-session-args
on the paired argv. Local experimental source review is closed; isolated
host/peer/runner releases are authorized, not native execution or promotion.

The attribution host is signed at `5e6db9f` (parent pass-cost `026eb3282`), release
`16f10191`. Runtime capture tests passed 793 with 41 ignored, Session parser 10;
Clippy, fmt and layout passed. Independent review covered actual decode/admission
and journal publication boundaries. The separate runner `8fbd1708` forwards only
exact `SOPHIA_SHELL_WORK_COST=1` in addition to the two existing opt-ins; release
`e0b7f1a1`. `continuous-shell-work-cost-01` is being staged with the unchanged
Phase-0 peer `1d9a76a`/`95aa00f8`, two fresh diagnostic arms, and no CPU comparison.
Twenty-seven bounded rows per snapshot distinguish incoming staged-record classes,
turns containing each class, outgoing FramePermit journal commits, inclusive
worker stages and authority sites. Each thread retains its own window; invalid
clocks/counters refuse attribution. This is mechanism evidence, not a saving.

### Completed captures and next gate, October 28

The earlier staging descriptions above are historical checkpoints. Attribution,
S1 and the W2 side-effect screen have now completed; their sealed packages remain
unchanged. Root recomputed saved reductions/raw CPU bounds and checked integrity,
identity/native anchors and cleanup. Claude and Muse independently reproduced
CPU arithmetic and receipt constraints with their stated review limits.

- `continuous-shell-work-cost-01`: two valid diagnostic arms. About 91 custody
  checks and 121 settlement calls per descriptive completion, 99.17% of settlement
  calls empty. Inclusive custody 100–101 us and settlement 67–68 us are not an
  additive budget. Worker copy 17–18.5 us/job is inside validation 43–46 us/job;
  clocks and accounting perturb execution. See the [complexity/repetition note](../investigations/p4jd1ymh-which-complexity-and-repeated-work-costs-belong-on-the-frame-cpu-critical-path.md).
- `continuous-owner-probe-01`: all four arms passed, but combined difference is
  [0,+80], Session [+25,+65], peer [-25,+15] us/frame. Both combined pair midpoints
  are positive (+30/+50). Quiet re-waits are 35/10187 and 46/10177 attempts over
  selected intervals; this proves reachability, not passes saved or fallback cause.
  No promotion; park S1. One screen does not identify marginal probe overhead.
- `continuous-retire-on-present-screen-01`: all four arms passed. Combined
  [-40,+40], Session [-17.5,+22.5], peer [-22.5,+17.5] us/frame meet the declared
  50/25/25 upper margins. Original versus new host+peer with W2 off is a side-effect
  screen, not equivalence, zero effect or a gain. Candidate receipts are exact
  r10/0x8183 with explicit-retire/release counters (2119,2119).
- Next, `continuous-retire-on-present-01` uses host `2eeb7a71`, peer `e2a47b47`
  and runner `7a952dfc` unchanged in every arm. Only paired W2 policy changes:
  off r10/0x8183/(2119,2119), on r12/0x28183/(0,2119). Exact final host argv
  preflights accepted 13/14 arguments device-free. The package is staged,
  awaiting final root review/seal; no enabled native result exists yet.

Durable root receipts under `continuous-cpu-optimization-01/` are
`shell-work-cost/post-run-root-review.json` (91a209c6),
`turn-proof/post-run-root-review.json` (7b819266), and
`w2-screen/post-run-root-review.json` (d2dce4e3). Bounds are tick-accounting
intervals, not confidence intervals. No completed candidate above is promoted.

### W2 enabled attempt qualification correction

`continuous-retire-on-present-01` stopped after baseline b1 and enabled c1.
The enabled capture completed all 2120 joins, r12/0x28183, zero explicit retires
and 2119 validated predecessor releases, with clean PAM/outer cleanup. Its
postflight nevertheless failed; the package stays failed and is never edited
or rerun. Root's failure receipt is `w2-effect/failure-review-01.json` under
`continuous-cpu-optimization-01/`, SHA-256
`d26b14e2c14e736aeaf4cee7092b96bc7eaeef7929b6aff41f62494418cdc993`.

The old resource check required zero retiring bytes and 368640 final presented
resident bytes. That encodes explicit retirement after Presented. With W2, the
Presented event transfer commits the predecessor intent before the Session logs
usage; resource-service journal confirmation happens later. All 2119 successor
presented rows therefore show resident184320, retiring184320, backing368640,
staging0. Successor prepared rows remain resident368640, retiring0. This is
retained accounting at a different phase, not evidence of a leaked extra buffer.
Root's package review missed this semantic difference; synthetic receipt fixtures
had not represented the changed accounting phase.

A fresh `continuous-retire-on-present-02` is being prepared with no source or
binary change. Preserve off/legacy acceptance. For declared-on mode require exact
prepared/presented generation coverage and phase tuples: first generation both
184320 resident/0 retiring/184320 backing; successors prepared368640/0/368640,
presented184320/184320/368640; staging0 throughout. Keep exact release counters,
joins and cleanup checks. There is no final drain accounting row: the last
predecessor's release is qualified by the peer counter, not a nonexistent next
prepare. Saved failing data and wrong-phase/count/bytes mutations must discriminate
the new gate before independent review and seal. No CPU verdict is drawn from
this incomplete ABBA and no acceptance is retroactively granted to attempt01.

Package02 is now sealed after root, Claude and Muse review. Its exact enabled
gate also requires alternating prepared/presented rows for each generation.
Saved01c1 reproduces both old failures and passes only the revised accounting;
saved01b1 has identical old/new results. Phase/charge/order/declaration/release
mutations refuse. All four exact-artifact argument checks and sealed SSH/panel
check-only gates passed without a Session launch. Sources and binaries did not
change. Manifest SHA-256 is
`b7fc9b00345fe9f71a40e59ab349e1496ba80cba7288a77276078a96a24cb0c6`
(109 files, 534 external pins). Root's receipt is
`~/.local/state/t331/continuous-cpu-optimization-01/w2-effect/package02-seal-review.json`,
SHA-256 `0bd375ff66a16a1115d5b27adcbff7da259620658d4b94d5f20e67609bbb7729`.
The saved01 sibling is an offline test fixture, not a launch-time test.

### W2 effect result and confirmation, October 28

Package02 completed all four arms. Root verified the unchanged manifest and
external pins, every retained analysis input hash, exact postflight reduction,
bound process identities through the CPU brackets, kernel generation joins,
phase accounting, release counters and clean PAM/VT/outer recovery. Claude and
Muse independently reproduced the arithmetic and mechanism from saved data.

| Scope | Disabled mean bounds, µs/frame | Enabled mean bounds | Enabled minus disabled |
| --- | --- | --- | --- |
| Combined | [1300,1340] | [1207.5,1247.5] | **[-132.5,-52.5]** |
| Session | [835,855] | [765,785] | **[-90,-50]** |
| Peer | [465,485] | [442.5,462.5] | **[-42.5,-2.5]** |

Combined midpoint reduction is 92.5 µs/frame (7.01%); pair midpoints are -100
and -85. Raw Session ticks are 170/154/156/168; peer ticks 95/91/90/95 in ABBA
order, with inner and outer deltas equal. Explicit retire requests fall from
2119 to zero while all arms retain 2119 validated predecessor releases and all
2120 presented generations. Disabled receipts are r10/0x8183; enabled are
r12/0x28183. This proves the exchange path, not syscall or owner-wake reduction.

Submission-to-kernel-completion medians are 7503/7589/7598/7503 µs: enabled
pair increases are **86 and 95 µs**. P99 also rises. Completion gaps retain an
8342 µs median and 8347–8348 µs maximum; all sampler late-slot counts are zero.
Earlier submission relative to fixed scanout is a possible explanation, not
an observed cause. There is no end-to-end or visual-latency measurement.

Verdict: positive W2 effect in this experiment; all three CPU accounting
intervals exclude zero. **No promotion.** One ABBA is not a confidence interval
or a general result. Root's audit is
`~/.local/state/t331/continuous-cpu-optimization-01/w2-effect/post-run-root-review-02.json`,
SHA-256 `01aaecb38f047b8e432af80fb885325ed7b0c00b9ac235ba562e1855b3282b66`.

Prepare one fresh `continuous-retire-on-present-03` confirmation ABBA with
identical frozen artifacts, workload, gates and analysis. Report it independently
beside02, with latency visible; do not pool away disagreement or run until a
desired result appears. No production changes or additional mechanisms enter
this comparison. Public protocol allocation, C parity and upstream SDK pin
remain separate integration gates. Attempt01 stays failed and untouched.

Confirmation03 is sealed after root, Claude and Muse read-only review; all four
sealed SSH/panel check-only gates passed without native execution. Verifier,
analysis, runner, recipe and binaries are unchanged from02; only paths and prior
evidence differ. Manifest SHA-256 is
`1409625822a51769f0c4b279e4a910ab848755daa18ec5211d4212b423d2ccc3`
(116 files, 534 pins). Root receipt
`~/.local/state/t331/continuous-cpu-optimization-01/w2-effect/package03-seal-review.json`
has SHA-256 `e8e37dc998438de1151d17cdf483d87ddbd92c8a3ca583915f54da288401f3aa`.
Launch remains Mason's through `continuous-retire-on-present-03/run.sh`; the
build/test/mutation hold lasts through the attempt and saved-data audit.

### W2 confirmation result, October 28

Package03 completed four arms and passed root's saved-data audit: unchanged seal
and pins, exact signed clean source closure, all analysis input hashes, raw
process identities and CPU brackets, native joins, phase-accounting and release
receipts, and clean PAM/VT/outer recovery. Claude and Muse independently agree.
The two experiments remain separate, without pooling:

| Scope |02 enabled minus disabled, µs/frame |03 enabled minus disabled, µs/frame |
| --- | --- | --- |
| Combined | [-132.5,-52.5] | **[-112.5,-30]** |
| Session | [-90,-50] | **[-75,-35]** |
| Peer | [-42.5,-2.5] | **[-37.5,+5]** |

Confirmation03 combined midpoint is -71.25 µs/frame (-5.44%), with pair midpoints
-75/-67.5. Session midpoint is -55 (-6.57%); the peer-only interval includes
zero in03 and is not independently resolved by this block. Raw Session ticks are
168/156/157/167; peer 94/91/91/94 inner, with95 outer only for b2. That last
bracket explains03's slightly wider combined interval; it is retained.

All arms again present2120 generations. Explicit retire requests are2119 off
and zero on, with2119 validated releases in both modes. The end event remains
schema7 with `retire_counter_schema=1`. Negotiation and phase tuples agree with
the declared modes. This demonstrates the protocol path, not measured wake or
syscall reductions.

The latency observation also repeats: submit-to-kernel-completion median pair
changes are **+77.5/+67 µs**, p99 **+75/+69 µs**. Completion gaps retain median
8342 µs and maxima8347–8351, with zero sampler late slots. Cause and visual or
end-to-end latency remain unmeasured; do not call this shift harmless or a
photon-latency regression from these receipts alone.

Verdict: **W2's combined and Session CPU benefit is reproduced for this workload;
the experimental performance gate is closed successfully.** No fourth run is
planned. This does not close public protocol allocation, C SDK parity, upstream
SDK provenance or product acceptance of the latency tradeoff. Preserve the
default-off policy and frozen experimental trees until reviewed integration.
These are accounting intervals, not confidence intervals or a general CPU floor.

Root audit:
`~/.local/state/t331/continuous-cpu-optimization-01/w2-effect/post-run-root-review-03.json`,
SHA-256 `8f47eb4433a538d818594d4466b90b541f69eabe5932cf07f379d505f7fc2869`.
The native hold is lifted after this audit; sealed packages and release/source
identities stay frozen. Claude maps the remaining integration work, and Muse
traces empty settlement/collection obligations, both read-only before new code.

### Test-environment disposition, October 28

The reported native-session run failed 61 tests. Using the same retained test
executable (SHA-256 `f7c8f4bdcbe2262ab762cd66eb86e633ce996f65687342eab7e26b70ac6908d6`),
root reran exactly those 61 in a device-free Bubblewrap namespace with a fresh
`/tmp`: original checkout KDL modes produced 0 passed / 61 failed; byte-identical
private read-only overlays for 36 tracked KDL files produced 61 passed / 0 failed.
The original checkout remained unchanged, including its 0664 modes. Both outcomes
and the earlier incomplete-overlay attempt are retained under
`~/.local/state/sophia/cpu-gates-20261028/`; the complete counterfactual receipt is
`tracked-kdl-mode-proof.json`. This proves the metadata cause for those failures,
not the full source provenance of that old executable or a passing new release
gate. Each new host still needs fresh tests on its exact source with owner-safe
fixture/config modes. Never weaken `ConfigIoError::UnsafeMode` to make tests pass.

The renderer `filter_map` lint is feature-dependent: the ManagedCapture arm uses
`?` only with `experimental-shell-capture`. The baseline release recipe enables
`native-session,experimental-shell-capture`. Check that actual feature closure;
do not blindly replace `filter_map` with `map`, suppress warnings globally or
call an unrelated feature configuration passing. Fresh results remain required.

### Frame-stream architecture

Muse's [standing-credit design](../investigations/tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md)
is on the critical path now. It keeps W2 retirement and image custody separate,
replaces repeated demand/permit with bounded pacing credit, and adds exact
terminal-outcome replenishment. Initial credit expiry starts at server grant;
replenished expiry starts at server commit, never client receipt. Prepared cannot
replenish; configured rate and per-generation response reservations still apply.
Expiry closes the stream and requires explicit reopen. The note is a design
deliverable, not permission to bypass those checks or implement a public wire
revision. Counter attribution and the device-free state-machine proof precede
production wiring. Record counts alone do not predict turns or CPU savings.

## Evidence, with its limits

All CPU figures below are microseconds per completed frame. CPUwin ranges are
accounting bounds, not confidence intervals. External rows cover bound server
and client thread groups, not the whole desktop or kernel workers.

| Evidence | Result | What it establishes |
| --- | --- | --- |
| `desktop-cpu-04`, Sophia, instrumentation none | Session 835–855; peer 465–485; combined 1300–1340 in both repeats | Current concrete owned-ring path at 119.88 Hz; not a lower bound on the architecture |
| Same comparison, XLibre | Server 75–95; combined 85–125 in both repeats | CPU-filled SHM strip, PageFlip off workaround, COPY at vblank; not proof of per-buffer scanout or identical producer work |
| Same comparison, niri | Combined 300–340 and 295–335; server 275–295 | Same strip geometry/cadence; CPU producer differs from Sophia's GPU render/readback |
| `continuous-pass-stage-cost-01` | About 9.05/9.02 wire-started passes per interval; 455/452 owner-body µs per interval, about 71% of owner body | Work to investigate, including useful turn work and all services in those passes; not removable overhead |
| Same diagnostic, peer main thread | Render/readback 327/311; ring upload about 17; total 497/480 | Different frame-function window, generations 121–2120; helper threads and GPU time excluded |
| `continuous-ack-coalesce-02` | Combined difference [-62.5, +22.5], midpoint −1.52% | Inconclusive; neither cheap framing nor per-record causality is proved |
| `continuous-single-submit-01` | Combined [-55,+25], peer [-50,-10], Session [-5,+35] | Complete isolated ABBA; combined inconclusive, no promotion; no native submission counter |
| `continuous-release-overlap-01` | Combined difference [-55, +25], midpoint −15 (−1.14%); peer [-15, +25], midpoint +5; Session [-40, 0], midpoint −20 | Inconclusive; no resolved peer saving and no syscall counter proving fewer waits |

Release-overlap's latency medians increased by 39 and 46 µs in the adjacent
pairs, with counted completion gaps 8335–8348 µs and no missed interval. These
are submit-to-vblank-completion spans, not a common end-to-end latency metric.
The c2 Session CPU dip is unexplained; an unchanged binary does not exclude an
indirect scheduling effect. All four postflights and cleanup receipts passed;
120 input hashes, CPU bounds and 2120 kernel joins per arm were checked.

The earlier 10–20 us memcpy-speed estimate from Claude, repeated by Muse, was
withdrawn as a measurement. The later attribution capture now measures the
worker's inclusive immutable-copy stage at 17–18.5 us/job and custody calls at
about 1.09–1.11 us/call under instrumentation. These are not uninstrumented savings
or isolated memcpy cost; peer `ring_upload` is still a different scope. Allocation
churn as a dominant explanation remains unmeasured. Flat self-time does not
establish irreducibility. Do not add inclusive profile scopes together.

## Contracts that every candidate must retain

- The client cannot change bytes after immutable acceptance. Reusing storage
  does not transfer authority or permit premature reuse of a leased version.
- Admission reserves the required bounded credits. Failure, cancellation,
  supersession, expiry and revocation settle their obligations exactly once.
- Keep exact resource generation, grant, output and candidate identities.
  A stale completion or replay cannot affect a replacement grant.
- Candidate outcome ordering and release-after-last-consumer remain intact.
  Retiring storage stays charged while a renderer or capture consumer holds it.
- Preserve the existing fresh authority checks at admission and image-service
  boundaries. A cached positive result or a wider observation window is a
  separate authority change, not an incidental optimization.
- No busy loop, renewed deadline on each wake, lost wake, starvation or skipped
  failure cleanup. Preserve the complete readiness set and retained input.

These follow the [one-core authority decision](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md),
the [shell file contract](../../sophia-shell-files.md),
[image-channel contract](../../sophia-image-channel.md) and
[launch authentication contract](../../factotum-launch-authentication.md).
This plan changes none of them.

## Investigation and candidate order

### 1. Reduce admission/control exchanges: implement the reviewed W2 design

W2 removes one explicit `ResourceRetire` exchange per successor, not a proven
number of socket calls or owner wakes. For a successful 2120-frame run there
are 2119 predecessors: the original path sends 2119 retire records; W2 sends
zero. Both receive 2119 predecessor releases. The final displayed resource
remains owned until disconnect. No permit is made implicit by this proposal.

The draft follows the existing runner request/plan/CLI plumbing: an off-by-default
build feature, an explicit host flag, an owner-local permit for the authenticated
bar attempt, and a paired peer key. Reject policy/key mismatches before launch;
initially refuse non-files, non-owned-ring and untested DMA paths. A CLI flag is
an experimental owner setting, not a new privilege boundary or proof that only
the runner can enable it. Pin the complete host, SDK, peer and runner closure.

The reviewed design closes these requirements; preserve them in implementation:

1. The current schema-7 journal has no retire/released events. Use aggregate
   end-event counters, not thousands of new per-frame lines. A versioned additive
   extension (`retire_counter_schema=1`) records `explicit_retires_sent` and
   `predecessor_releases_received`: explicit mode (2119, 2119), W2 (0, 2119).
   Count successful logical sends and exactly validated releases, not expected
   frame totals. This is not a syscall counter. New qualification requires and
   validates the extension in both W2-build modes; old readers ignoring it are
   not evidence. The original control has no such fields. Preserve old packages
   and per-frame/prepare lines; acknowledge the new end line is not byte-identical.
2. Use a peer SDK revision containing the reviewed r12 owned-ring mapping and
   exact welcome checks (`1951d597` or a reviewed successor). Peer `752ef08b`
   pointed at `8b1f7042`, which was insufficient for live r12 owned-ring;
   `b12b0a1c` now pins `1951d597`.
3. Distinguish a side-effect screen (original artifacts versus W2 artifacts with
   policy off) from the W2 effect (identical W2 artifacts, paired policy/key off
   versus on). An interval containing zero does not prove equivalence. Declare
   an acceptable regression/equivalence margin before launch; insufficient
   precision is inconclusive, and a real improvement is not a failure.
4. Specify instrumentation **none** for the CPU comparison, not merely “no
   profiler.” Work-stat diagnostics remain separately labelled. Require the
   exact negotiated revision/capabilities and attempt identity; the extended one-time
   host receipt must have tested parsing and rejection cases.

Implement only the approved wiring in isolated trees,
test feature-off/flag-absent defaults, role/source/key refusals, exact negotiation
and missing/malformed counters. Then review provenance and package one candidate
with its own ABBA. No native package is authorized by this note.

### 2. Avoid unnecessary whole-owner passes: S1 tested and parked

S1 is an event-file-read-only neutral-prefix probe, not the earlier broad
`turn_connected` shortcut. ACK, flush, submit, open and other requests remain
normal-pass work. Existing scaffolding stops before consuming a nonpermitted
request and resumes it once in the normal path.

The original pre-wiring requirements were to close authentic-custody integration and
quietness fixtures, complete the Session gate wiring review, and prove the
send-before-notify case and keyed failure retention. Exit on mixed/error
readiness, authority work, ring, disconnect, nonquiet state or burst limit.
Keep the original finite deadline and never clear the wake in the probe.

Predict converted event-read wakes, full passes per completed interval and
unchanged outcomes. Measure conversion first. If conversion is rare, stop; the
whole wire-started body is not a defensible savings estimate. Use a separate
candidate and ABBA, without W2, to identify its effect.

### 3. Use attribution to select a bounded empty-work candidate

The completed diagnostic supplies the first split. Follow the
[bounded complexity/repetition investigation](../investigations/p4jd1ymh-which-complexity-and-repeated-work-costs-belong-on-the-frame-cpu-critical-path.md)
for empty settlement/collection gates after W2. For any unresolved distinction, propose a
small opt-in diagnostic split at actual worker stages: immutable capture/copy,
admission/reservation, completion settlement and retirement. Name the thread and
window for every clock and reconcile the measured stage totals with their own
denominator. Peer upload time cannot stand in for Session copy time.

Only then select a change: bounded allocation/storage reuse if allocation or
retirement dominates; a copy-path change if destination-copy CPU is material.
Keep the current two-slot workload and lifetime contract. Never call a writable
producer slot immutable merely because it is reused predictably. Test partial
updates, stale pixels, wrong generation, cancellation, size/layout changes,
late consumer release and device-loss failure where applicable.

The existing single-GPU-submit peer prototype targets render/readback separately.
It is not mixed into W2, S1 or a custody candidate, and its offscreen correctness
and queue-count evidence does not establish a native saving.

### 4. Price repeated authority checks before proposing a change

Count `export.poll` and liveness checks by call site, operation and owner turn;
distinguish required boundaries from repeated checks with the same inputs.
Measure their CPU contribution as well as counts. A low count does not prove a
small cost, and a high count does not prove redundant authority work.

Any diagnostic uses bounded aggregation and an opt-in default that reads no
extra clocks when disabled. Its clock/read counts and perturbation are recorded;
no per-operation log flood. If a new check-sharing design emerges, specify which
revocation observation can arrive between checks and how it is handled before
requesting authority review. Existing negative custody-dispatch results remain
part of that decision. Do not revive the same experiment without new evidence.

## Measurement and stop gates

Use existing captures to inventory logical records per turn and turns per
completed interval before adding instrumentation. Classify record kinds and
ordering constraints: multiple records per turn alone do not rule out scheduling
improvements. Earlier batching results and EBUSY-until-covering-ACK semantics
must be accounted for before selecting another batching candidate.

For each approved experiment:

1. Freeze the hypothesis, predicted counter, exact source/dependency/build
   closure, workload, CPU window and latency/cadence/cleanup limits before runs.
2. Prove the changed contract path device-free with focused failure mutations.
   Resolve relevant failing tests or document why they are outside the gate;
   do not present the unresolved broad session suite as passing.
3. If new diagnostic instrumentation is needed, propose it separately on the
   pinned baseline. Diagnostic CPU is not silently compared with uninstrumented
   CPU. A counter should identify the mechanism, not count wrapper calls alone.
4. Root reviews and seals a fresh no-overwrite package; Mason launches through
   the existing SSH/PAM/VT handoff. Old attempts and failures stay intact.
5. Use the same changing strip, two slots, 2120 frames and kernel completion
   anchors 120→2120 (2000 intervals), DP-1 at actual 119.88 Hz, VRR off. Preserve
   all arms of each isolated ABBA, process identity, joins and cleanup receipts.
6. Promote only with the declared CPU benefit resolved by the accounting bounds,
   the predicted mechanism demonstrated, and correctness/latency/cadence gates
   met. One screen is not a confidence interval. Require repeatability before
   generalizing; if bounds or counter are inconclusive, retain the result and
   stop or propose a specific next discriminator. Do not rerun until a desired
   answer appears, bundle candidates, or claim shifted work as a saving.

## Connections and exact evidence

The source baseline for the continuous Sophia diagnostics is host `d9116ff0`
with integration/peer `748c325`, unless a package explicitly pins another pair.
All local evidence below is under `~/.local/state/t331/` on crunch:

- `desktop-cpu-04/report/{report.md,report.json}`: uninstrumented display/cadence-matched rows with different pixel producers,
  bound-process and pixel-producer limits; XLibre COPY-at-vblank qualification.
- `continuous-pass-stage-cost-01/analysis/sophia{1,2}.json`: diagnostic owner
  pass and peer-stage costs; the windows and threads differ.
- `continuous-ack-coalesce-02/analysis/summary.json`: inconclusive A1 ABBA.
- `continuous-release-overlap-01/{result.json,analysis/summary.json}` and its
  per-arm postflight/input hashes: complete, inconclusive, never rerun.
- `continuous-frame-cpu-02-recovery/analysis/`: profiled attribution only;
  the original failed capture remains failed.

Branch-qualified investigations are not claimed merged into this notebook:

- `~/dev/sophia-cpu-w2`, `cpu/w2-prototype`,
  `docs/notes/investigations/qyktai1j-w2-retire-the-predecessor-on-presentation.md`:
  lifecycle proof `7cc36ebde`; host wiring `88c1cda50`, peer `b12b0a1c`,
  runner qualification `17dc6a6f`; local-only diagnostic provenance exceptions.
- `~/dev/sophia-cpu-turn-proof`, `cpu-turn-proof`,
  `docs/notes/investigations/4cm5nuda-turn-only-owner-pass-for-wire-wakes.md`:
  S1 boundary, default-off wiring and exact fixture/qualification limits.
- `~/dev/sophia-cpu-wake-design`, `cpu/wake-design`,
  `docs/notes/investigations/516tpvf5-wire-wakes-per-bar-frame.md`:
  W1 ordering proof and shelving decision.
- `~/.local/state/w2-prototype/lifecycle-receipts/{run3,f1-m4}/` and
  `owner-wait-m4-receipts/`: durable focused baseline/mutation receipts.

Related existing questions are [bounded independent upload concurrency](../investigations/gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md)
and [prior 9P transport cost evidence](../investigations/yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md).
Neither is an authorization to change protocol semantics. This note records
the October 28 synthesis, implementation checkpoints and remaining gates;
it does not claim a measured improvement.
