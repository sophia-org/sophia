---
id: h38nf8bk
date: 2026-10-20
kind: plan
tags: [plan, rendering, gpu, 9p, performance, t339]
---
# Qualify GPU native shell images through owned capture

## Scope and exit

Deliver one bounded GPU-native shell-image path under the existing
[image-channel contract](../../sophia-image-channel.md): producer-completed
DMA-BUF, same-device capture into Sophia-owned storage, immutable acceptance,
and source release only after proven safe completion. Keep current ISB clients
working. This is t339's source audit and comparison design, following
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

The source audit is complete. Implementation starts with the owned lease and
completion boundary plus deterministic lifecycle controls. Production GPU
capability is not enabled by this plan, and no native experiment has run.
C42 remains independent: at this audit its stages and launcher receipts were
absent, with no capture process running.

### Completion handoff and accounting requirements

The renderer owner retains its Rc/EGL context and performs context-bound fence
operations. Before enabling admission, define a bounded job-ID-to-terminal-state
handoff to the runtime image owner, with a completion wake and renderer device
generation. A successful completion identifies the exact copy job and retained
source descriptors; a destination fence for another operation grants nothing.
Keep fault injection private to tests. Stale or duplicate completions cannot
publish a resource, release a newer job or return its reservation.

Existing snapshot fences guard destination-allocation reuse and are polled
inside renderer operations. Their quarantine and internal EGLSync destruction
do not yet expose the required standalone source-completion event. Neither an
Arc-backed snapshot nor a current snapshot epoch supplies that event.

Specify how a retained image is charged against shell resource limits and the
renderer snapshot budget. The same backing can be constrained by both limits,
but mirrored reservations must not become two physical allocations or duplicate
release credits. Logical resource bytes and actual owned allocation accounting
remain distinct; every reservation has one owner and one terminal disposition.

Muse independently reviewed the source audit at 7a57dca1f on October 20,
read-only, and confirmed the source gaps and sequence. These handoff and budget
requirements incorporate that review. Neither the review nor this plan claims
new tests, GPU execution or performance results.

## Connections

- [Portable-core proposal](../investigations/oofo1zrq-a-portable-9p-core-with-optional-drawing-and-surface-clients.md)
  separates optional drawing, immutable import and mutable pools.
- [Accepted image-channel decision](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
  and [current channel contract](../../sophia-image-channel.md) own custody.
- [Presentation versus execution](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
  preserves renderer neutrality and states GPU isolation limits.
- [Shared shell/lock review](x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
  retains existing lifecycle evidence; lock coverage is outside the first fixture.
