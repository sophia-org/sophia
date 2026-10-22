---
id: h38nf8bk
date: 2026-10-20
kind: plan
tags: [plan, rendering, gpu, 9p, performance, t339]
---
# Qualify GPU native shell images through owned capture

## Current resumption — October 22, 2026

niltempus requested completion of t339 with Claude and Muse working in parallel
through the local herdr session. The starting Sophia revision is `944e10902`.
t331 and t252 are complete. Shell r10 now uses owned rings for CPU images;
the historical ISB comparisons below remain evidence baselines, not a request
to restore the retired shell CPU protocol. Preserve current owned-ring clients
and the lock plane's ISB support.

The existing experimental capture executor, completion receipts, leases and
native lowering are present. The missing critical connection is negotiated
DMA-BUF admission: no production Source reaches that executor. Claude owns
runtime admission and source custody; Muse owns renderer capability discovery
and physical destination reservation; Codex owns Session integration, SDK parity,
contract updates and native qualification. All three share this checkout with
disjoint file ownership. No native run has been launched for this resumption.

Use the existing optional DMA-BUF capability at r10, with source mask 6 only
when complete capture limits and format rows are published. Initial admission
is one-plane LINEAR XRGB8888 on one independently granted render device and
output. Reserve the actual renderer destination before ImageAdmitted; match
Source to that admission, then settle through the existing exact-job completion
owner. The normative image-channel contract records this additive opt-in.

Completion still requires the lifecycle and device controls below, both SDKs,
and matched GPU-producer evidence. Neither supplied-descriptor tests nor a
successful import establish immutable capture, safe source release or measured
end-to-end benefit. Wider t343 performance qualification is separate; t339's
own comparison and correctness requirements remain in force.

## Scope and exit

Deliver one bounded GPU-native shell-image path under the existing
[image-channel contract](../../sophia-image-channel.md): producer-completed
DMA-BUF, same-device capture into Sophia-owned storage, immutable acceptance,
and source release only after proven safe completion. Keep current owned-ring
shell clients and ISB lock clients working. This is t339's source audit and comparison design, following
niltempus's October 20 instruction to proceed.

The first scope is opaque XRGB content on one admitted output/device. Reuse the
existing image service, renderer and presentation owners. Draw protocols,
direct composition from mutable producer buffers, direct scanout, changed
custody-dispatch defaults and changes to lock behavior are outside this slice.

Success requires correct custody and presentation plus a measured end-to-end
benefit over a matched GPU-render/readback baseline. Comparing with old IPC is
a separate product result, not proof of 9P efficiency or t331 acceptance.
No runnable experiment or native launcher is frozen by this plan.
Task state and execution order live in [todo.md](../../../todo.md).

## Source audit at 7a57dca1f

The audit reads production source without builds, device access or captures.
Paths below are relative to the repository; renderer context paths abbreviate
`crates/sophia-renderer-native-egl/src/gbm_platform/scanout/context`.

| Boundary | Existing implementation | Missing integration |
| --- | --- | --- |
| Wire metadata | Vendored `sophia-shell-protocol/src/shell_files/image.rs` defines DMA-BUF kind 2, planes, formats and coherence checks. | Codec support does not imply production admission. |
| Session policy | `crates/sophia-session/src/shell_component_connections/images.rs::configure_with` advertises kinds 1, empty formats and zero capture budgets. | Derive per-output/device formats from supported capture capabilities; reserve jobs and destination capacity before admission. |
| Channel/export | `crates/sophia-runtime/src/shell_transport/files/export/image_offers.rs` admits sealed sources, stores one `OwnedFd`, and reserves terminal journal credits. | Typed multi-plane custody and GPU-job handoff. Preserve metadata checks, terminal reservations and duplicate/late-source fencing. |
| Resource owner | Runtime `shell_transport/image_owner.rs::ShellIsbImageOwner` owns jobs beside the registry and survives connection replacement. | GPU job ownership, completion notification, stopped-interest/quarantine handling and settlement. GPU execution does not belong in the CPU validation loop. |
| Accepted resource | Runtime `shell_content/resources.rs::ContentResourceLease` owns a sealed mapping and exposes `bytes()`. | An owned GPU backing alternative with immutable identity, charges and retirement ownership. An unowned renderer ID is insufficient. |
| Rendering handoff | `crates/sophia-engine/src/compositor_graphics/content_identity.rs` assumes bytes; `crates/sophia-renderer-live/src/head_composition.rs` lowers content images to CPU layers. | Carry a GPU lease through existing identity/placement and lower to an already-captured owned image. Handle unsupported CPU consumers explicitly, without hidden readback. |
| Native capture | Renderer `renderer_images.rs::capture_renderer_image` copies imported content into owned storage; snapshot reuse tracks allocation and GPU uses. | A standalone shell-facing job/completion interface. Mixed-frame export captures while assembling a frame; shell release must not depend on a candidate or flip. |
| Completion | Renderer `image_capture.rs::capture_completion` and snapshot fences exist for specific paths. | Prove exact source reads ended on success, error and cancellation. Capture return, destination insertion, flush or FD closure alone is insufficient. |
| SDK | Generic codec/producer machinery exists, but the shell adapter neither requests DMA-BUF capability nor accepts it in `enable_image`. The generic producer can map the capability bit. | Extend the upstream shell request/opt-in layer and release ownership, then pin it. Do not implement by editing generated vendor snapshots. |

The X capture path supplies reusable implementation evidence, not shell
admission. `crates/sophia-renderer-live/src/native_scanout/owned_mixed_export.rs`
has different publication/release callers, and capture fallback can cross
devices. Shell first-phase same-device scope must be explicit.

Owned GPU storage is not a missing renderer primitive:
`crates/sophia-renderer-live/src/native_scanout/renderer_images.rs` already has
`LiveRetainedRendererImageSnapshot`, backed by an Arc, snapshot budget and
epoch; `LiveOwnedMixedCompositionLayer::Snapshot` consumes it. Reuse that
ownership where applicable rather than building a second snapshot registry.
The gap is connecting a completed capture to the shell resource/grant lease
and existing compositor-content path. Snapshot liveness and promotion must
not be mistaken for proof of source-read completion.

The [t289 plan](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md)
and current code contain snapshot reuse work; the
[renderer import summary](../../renderer-import-boundary.md#capture-execution-and-immutable-storage)
still describes fresh storage. Establish the selected runtime mode from code
and pin it in both arms. Do not silently change reuse policy between arms or
assume all remaining t289 work is a prerequisite.

## Contract constraints

First phase requires provider-completed rendering, no fence FD in the Source
packet, same-device capture and exact advertised formats/modifiers. Producer
completion cost belongs inside the measured workload. Explicit acquire-fence
transport would require a later protocol change.

Capture isolates future consumption from producer mutation but cannot prevent
a hostile producer racing the copy. Metadata and bounds remain independently
validated. XRGB capture must force alpha to one. Alpha-bearing images require
full premultiplication validation of the captured owned image, not a sampled
pixel or source-side scan. Transparent clients remain on ISB until that exists.

The comparator's current `Canvas::fill_pixels` produces opaque grayscale with
alpha 255. An opaque fixture can preserve that visible content, but GPU
generation is a new producer workload. Its results cannot replace C23's
CPU-fill measurements or establish performance for translucent bars and docks.

Existing renderer entry limits and pitch-based accounting do not alone prove
the shell contract's pre-admission destination reservation. Specify a supported
allocation bound, including overlap and quarantined work; refuse unknown
requirements. Logical pixel bytes are not a hard bound on opaque driver VRAM.
The accepted execution contract distinguishes known storage accounting from
portable hard GPU-resource isolation.

SourceReleased follows GPU completion or proven pre-use rollback, independently
of presentation. Resource retirement waits for retained scene and renderer
consumers. Timeout, channel closure, revocation and cancellation may stop
publication but cannot free in-use backing or return its capacity.

Older t304/t339 language about mandatory byte fallback and source release
“after the capturing flip” is superseded by the ISB amendment and the current
image-channel contract. Keep historical evidence, but implement today's rules.

## Implementation sequence

1. **Typed accepted backing and capture ownership.** Bind the existing owned
   renderer snapshot to a shell lease beside sealed backing, tied to resource,
   grant, connection epoch, renderer
   owner/device generation and content generation. Keep host handles in
   adapters. Define CPU/GPU access explicitly and preserve Engine placement and
   input semantics. Start with deterministic supplied-completion controls.
2. **Standalone capture executor.** Reuse the existing renderer owner through
   bounded jobs and its wait/wake mechanisms. Retain descriptors and reservations
   through uncertain GPU work; quarantine without busy waiting. A capture must
   complete or safely fail without frame submission. Avoid a second reactor.
3. **Role admission and settlement.** Extend source-kind handling, per-output
   formats, offer limits and epoch withdrawal. Publish Accepted after complete
   valid capture, then SourceReleased in journal order. Preserve fresh admission
   and authorization boundaries; journal pressure must retain custody.
4. **GPU content handoff.** Thread the lease through candidate/resource
   validation, scene retention and native lowering. Handle output replacement,
   renderer restart and target failure explicitly. Repaint must not re-capture
   mutable source storage or perform an implicit CPU readback.
5. **SDK and GPU fixture.** Use one independently granted render device and one
   producer implementation across arms. Extend SDK source-release and disconnect
   handling. Update both language contracts and conformance coverage before
   public promotion; retain ISB clients.
6. **Native qualification.** Freeze executable artifacts and launch plans only
   after lifecycle tests and device-specific pixel/custody checks pass.

Minimum deterministic controls cover wrong connection/epoch/device, malformed
planes/offsets/strides, unadvertised formats, exhausted resource/job/journal
capacity, cancellation before Source and after submit, completion after timeout
or revocation, duplicate/late Source, no early or duplicate release, disconnected
journal, renderer-generation replacement, consumer-held image after source
release and no use after retirement. Unsignalled completion must retain charges
while control/input progress continues.

Device controls must prove exact dimensions, generation/placement, alpha one,
and immutable captured pixels after the producer rewrites its source following
SourceReleased. A successful import alone does not prove these properties.

## Matched comparison

Use three arms with the **same GPU producer**, pixel pattern, target, dimensions,
cadence, update count and presentation semantics:

| Arm | Path | Purpose |
| --- | --- | --- |
| I | GPU render, completion, readback, old comparator IPC bytes | Product baseline; not restoration of a production byte protocol. |
| S | Same GPU render, completion, readback, ISB over files | Current files data path with the new producer. |
| G | Same GPU render and completion, DMA-BUF over files, owned GPU capture | Candidate. G/S tests data-path change; G/I tests the product goal. |

Use an opaque 1920x24 panel and a separately reported larger fully presented
surface, initially 1920x1080 if supported by the admitted target. Every
generation changes content. Do not substitute a transfer-only bulk probe for
the large presented surface. A simple deterministic GPU pattern establishes
mechanism first; representative textured/drawing work is needed before broader
claims. All arms pay identical producer rendering and completion work.

Pin device/driver, format, output mode, scale, VRR, renderer reuse policy,
source-pool size, outstanding-offer bound and warmup. Keep allocations required
by each arm's real ownership contract and report them; do not hide setup or add
avoidable baseline allocations. Pixel qualification may read back separately;
the timed G arm must not read back a full frame merely to collect evidence.

Freeze signed source and artifact provenance. Retain all runs. Use balanced
G/S and G/I blocks with both orders represented; choose repeat count and
decision thresholds before observing treatment results, using measured
no-change variation. Differences below that variation are unresolved, not
equivalence. Refusals and outliers remain in the record.

Report Session and peer CPU/frame and workload bounds, latency distribution,
completion cadence, joined generations, allocations/retained-resource peaks,
faults, bytes read back/uploaded/captured, source hold time, capture queueing,
and GPU completion timing where directly measurable. CPU time is not GPU work;
submission elapsed time is not GPU execution time. Label missing GPU timing
instead of inferring it. Keep heavy attribution instrumentation separate.

Promotion requires correct joins, cleanup and custody, repeatable benefit and
no unacceptable latency/input or memory regression. A capture microbenchmark
or lower syscall count is supporting evidence only. The existing native
`capture_cost` test uses a CPU-filled GBM source and manual eviction; it is not
this GPU-producing shell experiment.

## Current next deliverable

The native exact-copy adapter, private mailbox, resource-lease binding, native
snapshot lowering, completion-retention hooks and retirement service hooks are
implemented behind `experimental-shell-capture`. The existing renderer worker
now has bounded capture installation, command/request wakes, fence follow-ups
and close-before-context-drop. The backend supplies its shared snapshot budget
and a capture epoch tied to an explicit output, device generation and store.
Session now coordinates that endpoint with its single retained capture owner,
whose jobs route by connection slot and grant. Installation is demand-driven by
a current logical reservation, with no production ingress caller while admission
is disabled. Next is negotiated DMA-BUF source intake and destination reservation,
then the complete device-backed settlement/journal qualification. No admitted
source reaches the executor yet.
Real-device controls must follow before GPU admission. Tests use supplied
storage/backends and real mailbox pre-use rollback, not an EGL device. Production
GPU capability remains disabled; no native experiment or performance result
exists for this path.
C42 remains independent: at this audit its stages and launcher receipts were
absent, with no capture process running.

### Completion handoff and accounting requirements

#### First implementation prerequisite: reserve before capture

The October 20 reservation slice adds `experimental-shell-capture` in
renderer-native-egl and renderer-live, disabled by default. A non-cloneable
snapshot reservation holds one entry and a caller-proven allocation byte bound
before submission. Retention transfers that same charge into the existing
snapshot and reduces it to actual distinct backing-allocation bytes. Refusal
returns the original snapshot descriptors and the entire reservation, including
wrong-budget, invalid-generation, oversized-allocation and descriptor failures.
Generation invalidation does not release storage still held by a snapshot.

This is accounting infrastructure, not a completed first ownership/completion
slice. It does not implement the capture executor, Session mailbox, shell
resource binding, or source-completion receipts. Session still advertises zero
GPU capture jobs. Reserving capacity does not constrain the allocator by itself:
the executor must establish the destination allocation bound before GPU work,
and retain source descriptors and reservations through uncertain work.
`retain_reserved` neither proves source-read completion nor permits
`SourceReleased`.

Controls use ordinary memfds solely to exercise descriptor custody and allocation
accounting; they perform no DMA-BUF import or GPU work. Both library suites pass:
39 tests with the experimental feature and 35 with the existing GBM path and the
experiment disabled. Four new controls cover reservation limits, transfer and
shared-plane accounting, refusal ownership, and retry after a privately injected
charge-uniqueness failure. Muse reviewed the implementation read-only; the
suggested panic-to-refusal correction is included. Live wrapper refusal mapping
still needs an executor-level control once real snapshots can be supplied through
that path.

Evidence is retained at
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/reservation-01/`.
No native capture, admission change, or performance result accompanies this slice.

#### Private completion handoff

The next October 20 slice implements `shell_capture_jobs` privately in
renderer-live under the same disabled experimental feature. It has no public
API and no real completion supplier yet. Generic payloads let the lifecycle
controls own real descriptors and snapshot reservations without constructing
fake public GPU snapshots. Context-bound work stays on the renderer owner:
the executor is not Send, spawns no thread, and exposes a request wake; the
inbox can cross owners and exposes the completion wake.

Four shared permits bound queued/active jobs and untaken terminal receipts.
Quarantined jobs retain their permits and block new admission, including through
replacement mailboxes sharing the same graveyard. A failed enqueue returns the
whole request unchanged. A submission gate serializes enqueue with shutdown,
so an accepted job cannot fall between queue draining and receiver destruction.
Shutdown closing admission and finishing settlement are distinct published
states; the consumer observes settlement before draining to avoid reporting a
receipt that is concurrently arriving as missing.

Globally unique job IDs and exact epoch Arc identity bind private completion
proofs. `begin` marks submission before exposing the payload for import or any
GPU use. Unknown/duplicate begin is distinguished from a completed rollback.
Cancellation/deadline before submission allows rollback. After submission,
including after renderer completion but before receipt consumption, it forbids
publication and still requires exact completion before source release.
The private native driver now constructs a job completion only after consuming
an opaque completion from the exact native pending handle. Supplied completions
remain confined to private test children.

Explicit close and unwind settle selected/queued work as rollback and submitted
work as uncertain. The generic handoff alone requires a retained graveyard and
does not prove teardown ordering. In the real adapter, the entire source request
and reservation move into the native escrow before import/copy is attempted;
the generic submitted payload then contains neither. Native uncertainty retains
that escrow for process lifetime, as described below. EGL context/display
destruction alone is never a source-read completion event.

Ten supplied-completion controls cover these transitions with sockets standing
in for source planes; they exercise no DMA-BUF import, rendering or EGL fence.
Four mutations are detected: bypassing epoch identity, dropping quarantined
payloads, omitting receipt-time cancellation, and treating closed admission as
finished settlement. Restored library suites pass 49 tests with the feature and
35 with existing GBM behavior, with Clippy and formatting clean. Muse reviewed
the lifecycle and its races read-only; the native adapter/teardown obligation
remains explicit. Evidence is in sibling directory `handoff-01/` under the
reservation evidence root above.

#### Private native copy-fence observation

The `experimental-shell-capture` native module now owns a private, zero-wait
observer for the fence inserted and flushed by `NativeCaptureAllocation::draw`.
Its real entry takes the execution/display, allocation, generation and fence
from the same `NativePooledCapture`; callers cannot inject a successful status.
Only `CONDITION_SATISFIED` issues a non-cloneable, renderer-thread-bound proof.
The proof retains allocation and generation identity and matches the exact sync
handle. Each capture issues at most one proof. Timeout leaves it pending;
missing or mismatched ownership, errors and unexpected statuses refuse proof
and abandon reuse. Observation never takes the fence or its GPU-use guard.

This observer alone neither releases source FDs nor settles jobs, grants
publication or enables admission. The adapter below is now its real consumer. Its private fault
seam exercises supplied statuses with real native allocation accounting and
use guards, without EGL execution or device access. Seven controls cover
pending/once-only completion, errors, missing fences, generation ownership,
proof identity, retained allocation lifetime and already-abandoned reuse. Five
mutations are caught by test failures: treating timeout as completion, ignoring
use-generation identity, allowing repeat observation, and ignoring proof
generation or sync identity. The restored controls pass. Library checks pass
56 tests with the experiment and 35 on the existing GBM path without it; Clippy
is clean. Evidence is retained under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/native-proof-01/`.
No real GPU fence or native capture was tested in this slice.

The [EGL fence-sync contract](https://registry.khronos.org/EGL/extensions/KHR/EGL_KHR_fence_sync.txt)
binds the signal to preceding commands. The
[EGL 1.5 specification](https://registry.khronos.org/EGL/specs/eglspec.1.5.pdf),
sections 3.2 and 3.7, distinguishes resource termination/destruction and flushing
from completion. Consequently, destroying a sync or terminating an EGL display
must not manufacture this proof. The adapter binds that proof to the retained native request and shell job;
uncertain custody uses explicit process-lifetime retention rather than teardown
as a substitute proof.

#### Native source custody and mailbox driver

The default-off native API takes an owned, single-plane XRGB source plus a
pre-reserved snapshot budget, image ID and epoch. It uses the existing renderer
owner and capture pool, with no transfer/readback fallback or frame-assembly
coupling. Metadata/epoch/budget mismatch or lack of a process slot returns the
original request before any source use. The real allocation size, including
padding, is checked against the reserved bound before the draw; a successful
completion transfers that reservation into the retained snapshot.

A native pending handle owns the request, destination capture and exact context
identity. It is non-cloneable and stays on the renderer thread. Each poll performs
one zero-timeout native fence observation. A signalled exact-copy proof permits
source custody to return even if subsequent snapshot export or retention fails;
that failure returns the unused reservation. A retired or different context is
refused before any EGL call. A withdrawn epoch still requires copy completion;
it can settle custody but cannot publish a retained image.

**Uncertain-copy policy for this experiment:** four process-wide slots bound
submitted copies. Abandoning a pending handle, unwinding after possible source
use, receiving an uncertain fence result, or failing after entering native
capture keeps the source FDs, reservation and any owned pending capture for the
process lifetime. The slot stays charged and all further experimental native
captures are refused, including through replacement channels/contexts. There is
no reset, timer, destructor completion or automatic release credit. This is
intentional bounded retention, not a claim that EGL teardown finishes reads.
Errors after entering capture are conservatively uncertain even when they may
have preceded a draw. This can stop the entire experimental GPU-capture lane;
qualification must assess that availability tradeoff before production admission.

The private renderer-live driver connects these real pending handles to the
existing bounded mailbox. It checks exact snapshot-epoch identity before begin,
restores the original request on native pre-use refusal, and constructs a job
completion only from its own handle's opaque native receipt. Cancellation,
deadlines and generation withdrawal still forbid publication, including at
receipt consumption. Pending fences rotate to the tail with at most four native
begin/poll operations per service call and no repeated poll of one fence within
that call. A service unwind guard settles every remaining job even if its caller
catches the panic and retains the driver. Close parks native handles before
publishing uncertain receipts.

Controls use owned socket FDs, real reservation accounting, supplied native
statuses and a private scripted backend. They cover process capacity, retained
custody on abandon/unwind, refusal return, linear-source validation, padded
allocation bounds, context/epoch identity, pending completion, cancellation,
withdrawal, tail rotation, exact job/source matching and caught unwind. They do
not execute EGL import/copy, prove rendered pixels or measure performance. The
feature library suites pass 75 tests; the default GBM suites pass 35. Nine mutations fail at runtime: premature custody drop, reopened admission,
returned quarantine credit, bypassed allocation bound, wrong context or epoch
identity, head requeue, omitted unwind settlement, and timeout-as-completion.
Restored suites pass; Clippy is clean. Logs are retained under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/native-adapter-01/`.

#### Session completion handoff and connection binding

The next October 20 slice exposes an experimental renderer-live mailbox façade
and adds a private Session correlation owner. The façade accepts a reserved
capture request, returns an opaque job ID, and produces a non-constructible
completion receipt. Its executor services the existing renderer-live context
through a private bridge; it creates no context, thread or device. The inbox is
Send; the executor remains !Send. Requests wake the renderer and completions wake
Session. Pending fences still require explicit follow-up owner turns: the request
fd does not become readable when a GPU fence signals.

Completed receipts return the exact source descriptor and either the already
charged immutable snapshot or an output failure with its unused reservation.
Withdrawal/deadline flags survive receipt conversion. Pre-use rollback returns
the original request. Uncertain receipts return neither source nor reservation;
the native escrow's existing quarantine policy continues to own them. The live
snapshot wrapper adds no second allocation or budget charge. Public enum values
are not proofs: only the opaque receipt is issued by the mailbox/native adapter.

Session's bounded table retains the slot, connection/content grant epochs, full
image metadata and the single plane descriptor until the exact job settles.
DMA-BUF metadata stride is zero; offset/stride/fd index come from its plane row.
Mismatched metadata, duplicate offers/resources, full tables and closed queues
return source and destination reservation without creating a binding. Withdrawal
and resource cancellation keep old bindings alive; a reconnect with reused offer
or resource numbers cannot take their receipts. An unmatched receipt is retained
and closes further submission. A settlement is not permission to publish: the
resource owner must still revalidate live authority and reconcile its logical
shell reservation, even for stopped or uncertain jobs.

This is an opt-in integration prerequisite, not a running GPU shell path.
The Session owner is not installed in production connection state or its service
loop yet. Runtime's ContentResourceLease still exposes CPU bytes; logical shell
resource reservation/acceptance, compositor lowering and actual owner-turn fence
scheduling remain outstanding. Admission still advertises sealed-only, and
`experimental-shell-capture` is in neither Session's default nor native-session
features. No native GPU execution, snapshot-pixel validation or performance claim
is attached to this slice.

Controls use supplied private completions and real mailbox shutdown before GPU
use. They pin FD/reservation return, reconnect and generation isolation, metadata
matching, cancellation, capacity, exact-once receipt handling and fail-closed
identity faults. Six mutations remove these guards individually and fail at
runtime. The restored feature suites pass 91 tests; the default GBM suites pass
58 (one pre-existing ignored Session test in each configuration). All six new
Session controls also pass with native-session enabled. Clippy is clean. Two
compile-fail controls prohibit external receipt construction and moving
the executor between threads. Logs and the retained initial metadata-stride test
failure are under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/session-handoff-01/`.

#### Logical capture reservation and failed-copy settlement

The following October 20 slice connects the Session binding to the actual
runtime allocation/resource owner. `reserve_capture_image` checks the live grant,
output/allocation and topology, the opaque single-plane XRGB metadata, and the
existing content limits before issuing an opaque, non-cloneable reservation.
The ticket binds the full admitted metadata and the exact transfer identity;
identical grant/resource numbers in a different store cannot settle it.

Capture and CPU transfers share resource generation history, quotas and two
terminal-record credits. A capture transfer has a separate identity marker, so
`complete_isb` refuses both successful and failed CPU results without consuming
them or releasing capture credit. Cancellation, timeout, revocation and even
loss of the ticket retain the transfer and its charges. Logical staging,
reserved-resident and backing bytes are quota accounting; they do not describe
a CPU staging allocation or duplicate the renderer's physical allocation.

Session submission now requires the runtime ticket and checks its grant and
full metadata against the binding. A refused enqueue returns that ticket along
with the original source and native destination reservation. Pending jobs keep
their ticket through resource cancellation and disconnect. Resource observation
cancels publication interest while continuing to reconcile the native job.

For failed/unpublished work, Session consults the opaque native receipt before
settling runtime credit. Proven pre-use return or completed reads can produce a
resource failure status; uncertain work cannot enter reconciliation, including
after timeout. A usable successful snapshot keeps its ticket for the future
accepted backing path. Wrong-owner and clock errors preserve both ticket and
receipt for retry. Disconnected jobs settle against their retained old epoch,
without reopening authority or touching a successor. The first cancellation,
expiry or revocation reason remains authoritative.

Runtime's failure-accounting API is a trusted-owner operation, not a GPU
completion proof. It emits only ResourceStatus. Neither it nor the Session
wrapper emits SourceReleased, ResourceReleased, ImageAdmitted or acceptance;
those require their respective journal/owner transitions. The snapshot receipt
remains owned even after logical failure credit is returned. No renderer type
or dependency has been added to runtime.

Controls cover cross-class completion, exact owner identity, shared budgets and
generations, first terminal reason, clock recovery, retained uncertain credit,
authoritative allocation checks, retired-epoch settlement and Session receipt
binding. Eight mutations fail at runtime. The uncertainty-to-accounting control
uses a private supplied-disposition seam with the real resource registry; it is
not a GPU execution or public receipt-construction path. Selected restored
feature suites pass 270 tests and their default counterparts pass 222, with two
pre-existing ignored library tests in each selection. All ten Session handoff
controls pass with native-session enabled; three compile-fail controls pass.
Clippy and formatting are clean. Evidence is retained in
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/resource-reservations-01/`.

GPU-backed accepted leases, successful-copy resource publication, compositor
lowering, owner-loop installation/fence follow-up and native qualification remain
outstanding. The experimental feature remains off by default; production image
configuration remains sealed-only. No hardware run or performance claim is
attached to these accounting controls.

#### Shared snapshot ownership prerequisite

The accepted GPU lease needs a storage owner below both runtime and the native
renderer. `sophia-renderer-image` now owns the existing image identity, descriptor
planes and borrowed views, snapshot, budget/charge, epoch and experimental
reservation types. It depends only on rustix/std, with no renderer context, GBM,
EGL or runtime dependency. Native EGL re-exports those types and maps the three
storage errors into its existing refusal vocabulary; renderer-live keeps its
existing retained wrapper. The single-plane GBM admission adapter stays native.
Descriptor duplication and budget retention still use the same implementations
and charge, rather than another image registry or an erased backing trait.

An openly constructible `OwnedSnapshotData` creates only an **uncharged**
snapshot. Neither owning descriptors, charging them nor observing a current
epoch proves immutable capture or ended source reads. The native exact-copy
completion and opaque receipt remain the proof. `is_retained()` is an ownership
fact only; a future registry accept operation must not treat a caller-supplied
retained snapshot as a completion receipt.

Review identified an additional release requirement before successful GPU
acceptance. Shell resource leases have an outer Arc, but native imports and
queued scanout buffers retain the inner snapshot Arc. `try_clone()` also creates
a different snapshot with duplicated descriptors and the same physical charge.
The leaf's `custody_is_exclusive()` therefore checks both the snapshot and charge
strong counts. Future resource collection must require outer-lease uniqueness
**and** this inner-storage condition. Epoch invalidation never releases storage;
import-cache retirement is per charge and does not invalidate sibling resources.

Uniqueness alone does not make retirement progress: an idle native import cache
can retain a snapshot indefinitely. Session must mark imports non-cacheable and
request explicit native import eviction from **retirement/revocation intent**,
before waiting for exclusivity. `isb_release_pending` is too late as the trigger
if it already depends on collection. Frames and submitted GPU uses must still
drain through their existing completion owners. Do not claim a fixed release
latency from reference counts or from an eviction request.

The existing descriptor and budget tests moved with their implementation. New
controls distinguish uncharged data, same-snapshot consumers, duplicated
snapshot consumers, cache retirement and epoch invalidation. Their ordinary
memfd/socket descriptors test ownership, not GPU rendering. Evidence lives in
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/snapshot-storage-01/`.
Muse independently reviewed the design and extraction read-only; test results
are the parent's, not the reviewer's.

Selected validation passed: 88 default-mode tests, 156 feature-mode tests
(including the native-session handoff controls), three compile-fail docs, and
Clippy. Five deliberate mutations failed their runtime controls: ignoring either
ownership count, losing the shared charge on descriptor duplication, omitting
cache retirement, and treating uncharged data as retained. Restored controls
passed. The ordinary native Session configuration also type-checked with the
experimental feature; this was compilation, not a native launch.

At that prerequisite, runtime GPU backing, successful-copy acceptance and native
lowering were still absent. The following slice adds those storage paths; the
live owner-loop installation remains outstanding. Production GPU admission
remains off, and no device run or performance claim accompanies either slice.

#### Accepted capture leases and native lowering

The next October 20 slice gives the runtime resource store a concrete captured
backing under `experimental-shell-capture`. The resource lease retains the exact
snapshot Arc from the native receipt, including its existing physical charge.
There is no backing trait, replacement allocation, descriptor duplication or
pixel readback at acceptance. Runtime logical resource bytes and renderer
allocation bytes remain separate limits on the same storage.

The private Session entry accepts only a completed, unstopped opaque receipt.
It binds the requested renderer image ID, exact reservation, connection/grant,
resource metadata, and the full allocation snapshot recorded at reservation.
The registry rechecks the current allocation and topology: reusing an allocation
number with identical dimensions but a new generation cannot accept an old copy.
Refusals keep both reservation and snapshot. The lower public registry operation
is trusted accounting, not a verifier of GPU execution; retained storage alone
is never a completion proof.

Successful accounting emits the resource status once. It does **not** emit
`SourceReleased`. The completion owner must still retain the native receipt and
source-release obligation until that separate journal transition succeeds.
That live journal owner is not installed in this slice, and post-acceptance
receipt retention is not yet enforced by a dedicated pending-release type.

CPU consumers use `cpu_bytes()` or narrow to a `ContentCpuResourceLease` before
an infallible byte view exists. CPU rendering explicitly refuses GPU content.
Native head lowering carries the same snapshot Arc into the existing snapshot
layer; identity uses declared logical bytes without reading pixels. Native
imports, queued snapshots and duplicated descriptor owners continue holding
the existing physical charge after the runtime lease itself is dropped.

Retirement intent now exposes a bounded eviction operation across active and
disconnected epochs. It marks storage non-cacheable and requests the existing
native owner's import eviction. A failed callback retains the obligation and
both budgets. Collection requires eviction acknowledgment, the last outer
resource consumer, and the last inner snapshot/charge consumer. For a live
epoch, backing and credit then remain held until `ResourceReleased` enters the
journal and the owner confirms it. Disconnected epochs need no live journal
confirmation, but revocation does not bypass physical ownership. The Session
wrapper is present, but the live owner loop must still
service it even without a future composition frame and revisit after consumers
retire; otherwise custody is safely retained but progress can stall.

Supplied memfd storage controls exercise accounting, exact binding, CPU refusal,
native lowering and retirement. They do not fabricate native success receipts,
prove a GPU copy, or qualify DMA-BUF admission. Evidence is retained under
`~/.local/state/sophia/development-evidence/t339-owned-capture-20261020/accepted-leases-01/`.
Selected suites passed 666 test executions across configurations (including
three compile-fail docs; not a count of unique tests). Both Clippy configurations
and changed-file formatting passed. Eight deliberate mutations produced runtime
test failures, and restored controls passed. Muse's final read-only source review
found no accepted-lease custody blocker; builds and mutation results are the
parent's evidence, not independently run by Muse.
The standalone renderer-live default build has a pre-existing optional-native
dependency error in `renderer_image.rs` (unchanged since `7faa3a735`); CPU
compatibility checks use `gbm-probe` with the experimental feature off. That
substitute configuration must not be described as a standalone-default pass.
The eight native-session reconnect fixtures also fail before resource work:
their setup is refused because namespace recipes require normal Session with
Factotum launch authentication. The same eight setup failures were reproduced
at committed baseline `01dc5e19f` with the same features; current sources were
restored byte-for-byte. This slice does not relax that authentication requirement.

#### Bounded owner retention and retirement hooks

The October 20 owner-retirement slice retains each opaque completion receipt and
source in the same four-slot owner until resource accounting succeeds and the
exact connection's `SourceReleased` is journaled. Journal saturation retries;
other publication errors also retain custody. Only proof of the exact journal's
disconnection permits disposal without that append, after resource settlement.
The runtime export retains its independent status hold, so ResourceStatus must
enter the journal before SourceReleased. A completed copy that cannot be
accepted settles the original reservation without publishing the copied image;
uncertain completion never releases its source.

Pending jobs register their completion fd through the existing component wait
subscription. Completion retention and retirement cap the component maintenance
wait at 10 ms. This is a retry deadline, not a liveness or authorization cache.
Completion servicing still follows the existing component/runtime service gate;
there is no new owner loop. Installing the executor must explicitly preserve
progress and teardown when that gate stops servicing components.

Retirement runs on existing native service turns without requiring a future
frame or an open topology frame-service gate. A resource eviction visits every
current render store, including disabled heads, deduplicating shared workers.
Any busy/error store leaves the obligation pending. The Session operation
requires the native owner by reference: absence cannot acknowledge eviction.
The four-resource per-turn budget bounds traversal. An eviction acknowledgment
does not authorize ResourceReleased: exact snapshot/charge exclusivity remains
required, with maintenance continuing while native consumers hold storage.

Source review corrected the proposed integration seam: the context does not
expose `snapshot_budget()` / `snapshot_epoch()` accessors. Backend preview state
owns its persistent budget and store-generation epochs. A native context can
also reside on the existing renderer worker, whose loop waits for commands.
Session may not borrow that context or move its non-Send executor across owners.
The next slice must use the backend's budget/generation ownership and extend the
existing worker protocol; a Session-created budget or second EGL context is not
an acceptable shortcut.

Controls cover real Returned receipts under repeated journal pressure, bounded
slots across both lifecycle phases, publication-error retention, clock-refusal
retry, completion-fd subscription, shutdown refusal with pending custody, and
maintenance through the last sampled-storage owner. Pure backend controls pin
all-store traversal, shared-worker deduplication and error retry. The real image
journal fixture pins status-before-source ordering and rejects using capture
accounting to release a sealed offer. These are composition controls, not a
real GPU Completed-receipt integration verdict. Evidence is retained under
`t339-owned-capture-20261020/owner-retirement-01`.

Muse reviewed that slice read-only. The missing-native-owner acknowledgment was
removed following that review. At that point no executor was installed and
policy advertised zero capture jobs; native worker installation,
close-before-context-drop, graveyard retention and actual source-completion
controls remained open. The following slice addresses worker installation.

#### Existing renderer-worker installation

The subsequent October 20 slice supplies an experimental backend endpoint for
one explicit output on an existing renderer worker. It does not create a thread,
context or device. Inline exporters return Unsupported. The worker constructs
the non-Send executor on its own thread and closes it before context replacement
or drop. Capture service failures close capture work while keeping the renderer
available for its other commands.

The endpoint uses the backend's existing snapshot-budget Arc, with a separate
capture epoch so closing capture does not invalidate preview snapshots. The
driver rejects a foreign budget or epoch before native use. Persistent custody
survives mailbox replacement; uncertain jobs cannot regain capacity by opening
a fresh mailbox. Neither context teardown nor a close-command acknowledgment
proves that GPU reads ended.

With the feature enabled, the existing command channel uses SignalSender and a
WakeSlot. An active executor waits on command and request readiness together.
The wake is cleared before queue inspection. Fence-only work requests a 1 ms
follow-up; spending the turn budget requests an immediate next turn. Idle
mailboxes block without periodic polling. Service remains bounded to four
capture operations per turn. Feature-off command and wait behavior stays as it
was.

Generation, output and store changes invalidate capture epochs and retain close
intent until the command can be sent. Queue pressure retries on existing native
service turns, independently of component and frame gates; pending installation
or close caps the owner wait at 10 ms. Close commands are deduplicated while an
installation reply is outstanding. Endpoint publication rechecks the enabled
output, store, generation and epoch. Source receipts still belong to Session's
settlement owner; backend close does not publish SourceReleased.

Device-free tests cover pre-use source return, budget/owner identity, quarantine
retention across replacement, request and command wakes, fence-only progress,
capture-local error isolation and generation/output/store withdrawal. Evidence
and mutation controls are retained under
`t339-owned-capture-20261020/worker-installation-01`. These controls do not prove
real GPU completion, end-to-end Session installation or performance. Production
admission remains sealed-only with zero capture jobs.

#### Session endpoint coordination

The next October 20 slice connects the existing native maintenance turn to the
connections-level capture owner. There is one inbox consumer and one global
four-slot bound; jobs retain their exact connection/grant bindings. A private
link requests an endpoint only after a current reserved allocation supplies its
output. It refuses another output while that endpoint is pending or active.
The installation reply supplies the backend's budget and epoch; submission
requires those exact Arcs and revalidates the logical ticket and allocation.
Failure returns both source request and reservation unchanged to their owner.
The query and endpoint are not permission or source-read completion proofs.

Pending demand is tied to the original connection key. Disconnect or slot reuse
before the reply closes the pending endpoint; another waiting connection must
prepare again from its own live reservation. Once installed, the mailbox is
shared and survives individual connection replacement. Reservation expiry alone
may leave an idle endpoint installed, but cannot authorize submission. No
production ingress invokes preparation/submission with admission disabled.

Native lifecycle runs before endpoint polling; endpoint service precedes receipt
drain and eviction maintenance on the native turn, outside runtime/frame gates.
Shutdown or an invalid backend epoch withdraws submission/publication. Transient
presentation unavailability alone does not close capture. Close-queue pressure
keeps the exact intent for a later maintenance turn. Receipt reconciliation still
runs when installation fails. Existing ordinary component turns also reconcile
receipts; settlement is idempotent and preserves journal pressure.

An owner is replaceable only when closed and completely drained: pending jobs,
settlements and orphaned receipts all count. Uncertain reads retain source and
reservation, blocking replacement. The old direct installer is test-only, so
production cannot replace the inbox while leaving link destination facts stale.
Source publication routes only to the exact Connected slot/grant; a replacement
grant has a different journal, and publication errors retain custody.

Controls use real mailboxes with supplied endpoints and pre-use rollback. They
cover idle demand, request/close pressure, output mismatch, stale replies,
disconnected requesters, foreign budgets/epochs/tickets, owner replacement and
exact publication routing. The existing retained-receipt controls now also pin
replacement refusal during journal pressure and orphan retention. Evidence is
`t339-owned-capture-20261020/session-endpoint-01`. These controls do not constitute
a live GPU completion, DMA-BUF admission or performance result. Outer shutdown
still refuses disposal of unresolved capture custody; it does not fabricate
completion from backend teardown.

#### Pre-submit source retention (2026-10-20)

Session now privately retains an unsubmitted descriptor with its exact binding,
logical reservation and validation deadline while the existing native endpoint
is prepared. Waiting sources, submitted jobs, retained receipts and orphaned
completions share the four-slot bound. Capacity and identity refusals return the
original source and ticket. Pending sources participate in maintenance waits
and prevent final owner disposal.

The explicit handoff requires a renderer allocation byte bound and reserves
against the endpoint's exact physical budget before submission. It does not
infer destination allocation size from source bytes. A missing endpoint or
unavailable budget retains the source; a failed enqueue restores it and releases
only unused physical credit. The original deadline cannot be renewed by retry.

Before native submission, timeout, withdrawal or refusal can failure-settle the
exact logical ticket without GPU completion. The source descriptor and its slot
remain retained until the terminal status precedes SourceReleased in the exact
journal, or that journal is disconnected. Clock errors retain custody. After
successful enqueue, the existing opaque-receipt path owns all settlement;
uncertainty still cannot release anything.

Evidence: `t339-owned-capture-20261020/source-retention-01`. Supplied-descriptor
controls cover endpoint absence, timeout/journal pressure, old-grant disposal,
shared capacity, physical-budget refusal, expired submission, malformed/foreign
tickets, clock regression and actual native-inbox enqueue refusal. They prove
pre-use retention and rollback, not a real GPU copy or DMA-BUF admission.

The production offer path remains sealed-only. No live source-ingress caller
uses the new private retention API yet. Next are admission-time capture
reservation and kind-aware Source transfer, negotiated device/format and byte
bounds, and real provider-completed GPU qualification. The intake must derive
its deadline from negotiated validation/transfer limits. No GPU capability,
default behavior or performance claim changes in this slice.

#### Remaining source intake and qualification

Use the existing owner, not a second capture thread or context. The bounded
mailbox carries runtime-neutral job IDs, source descriptors and metadata; Session
retains the grant/connection/content-generation binding. The renderer alone may
produce a completion receipt for the exact job and renderer generation. A full
or failed enqueue returns source ownership and capacity unchanged.

Cancellation before submission permits rollback. After submission, cancellation
only forbids publication: the job must still reach proven completion or retain
its source and capacity in quarantine. Executor failure and generation change
must settle every pending job as uncertain, never fabricate success or silently
drop its reservation. The runtime owner reconciles even withdrawn jobs and
rechecks authority before publication. Tests must pin these transitions before
the GPU admission path is enabled.

The renderer owner retains its Rc/EGL context and performs context-bound fence
operations. Before enabling admission, wire the private handoff to the runtime
image owner and real context-bound completion operations, with the exact
renderer device generation. A successful completion identifies the copy job and retained
source descriptors; a destination fence for another operation grants nothing.
Keep fault injection private to tests. Stale or duplicate completions cannot
publish a resource, release a newer job or return its reservation.

Existing snapshot fences guard destination-allocation reuse and are polled
inside renderer operations. Their quarantine and internal EGLSync destruction
are not the standalone source-completion event; the new shell adapter observes
its own capture fence before it is transferred to those existing paths. Neither an
Arc-backed snapshot nor a current snapshot epoch supplies that event.

Specify how a retained image is charged against shell resource limits and the
renderer snapshot budget. The same backing can be constrained by both limits,
but mirrored reservations must not become two physical allocations or duplicate
release credits. Logical resource bytes and actual owned allocation accounting
remain distinct; every reservation has one owner and one terminal disposition.

Muse independently reviewed the source audit at 7a57dca1f on October 20,
read-only, and confirmed the source gaps and sequence. These handoff and budget
requirements incorporate that review. That audit review claimed no new tests.
The later implementation controls above do not claim GPU execution or performance.

## Connections

- [Portable-core proposal](../investigations/oofo1zrq-a-portable-9p-core-with-optional-drawing-and-surface-clients.md)
  separates optional drawing, immutable import and mutable pools.
- [Accepted image-channel decision](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
  and [current channel contract](../../sophia-image-channel.md) own custody.
- [Presentation versus execution](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
  preserves renderer neutrality and states GPU isolation limits.
- [Shared shell/lock review](x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
  retains existing lifecycle evidence; lock coverage is outside the first fixture.
