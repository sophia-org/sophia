---
id: i9ljdlt4
date: 2026-10-30
kind: investigation
status: investigating
tags: [investigation, performance, rendering, cpu]
---
# How can Lom render Vello CPU directly into an owned-ring slot across its worker boundary?

## Decision and corrected starting point

CPU direct-fill adoption precedes further GPU implementation on the
[critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md).
This is a source-grounded design, not an implemented API or performance result.
Existing holds, W2, releases and sealed benchmark evidence stay untouched.

Mason's clarification is two native pixel movers: CPU shared-memory pixels and
GPU buffers. The application chooses, with user preference where supported;
Sophia still enforces permissions and negotiated capabilities. Direct fill is
the intended default **within the selected pure-CPU path**, after qualification.
It does not force CPU use or create silent fallback. Slice upload remains useful
for already-owned pixels, not a third renderer policy. Internal CPU/GPU hybrid
rendering also fits these two output pipelines; see the
[application guide](../../native-rendering-choice.md).

At signed Lom `32e0bab31594f70c3d945547d7e81e2806b6fae8`, native `--serve`
requires `GpuGrant::from_environment` (`src/cli/serve.rs:18–24`). Its `lom-gpu`
worker calls `dma_content` or `readback_content` (`src/render/worker.rs`).
`RenderImage::Cpu` currently means **GPU-readback bytes**, not Vello CPU
rasterization. `src/protocol/pixels.rs` says this explicitly. Vello CPU is used
by `tests/ui.rs`, but not by this production worker. Earlier shorthand that
production already rendered Sparse Strips into an owned CPU buffer was wrong.
Adoption needs both an explicit CPU renderer and worker-safe slot custody.

## Existing renderer support and pixel contract

Lom pins SDK `0706ea2c78cd16ba59ed209620354f30a2c78906` and
`imaging_vello_cpu 0.0.1` (lock checksum
`fd411d3ad5f4b8271a45b4bc69df7f88bb4236b71fbd4e5a5177c6e93eb67f44`).
The imaging crate already implements `ImageRenderer::render_source_into` using
a borrowed `ImageBufferTarget`. `finish_into_target` validates dimensions,
tight stride and length, renders into those bytes, then unpremultiplies RGBA
in place (`src/lib.rs:251–289, 542+`). No imaging API fork is needed.

Preserve Lom's conversion from straight RGBA to premultiplied BGRA, including
alpha-zero black and `(channel * alpha + 127) / 255` rounding
(`src/protocol/pixels.rs`). Render into exactly `width * height * 4` bytes and
convert in place. Bypassing the unpremultiply/re-premultiply sequence would be
a separate optimization requiring equivalence proof.

The existing SDK `image_upload_ring_fill` is synchronous. `RingSlotFill` borrows
`&mut RingProducer`; that borrow cannot cross a persistent worker boundary while
the service continues polling the connection. Joining a scoped render thread on
the service thread would also block protocol work. No forged lifetime or shared
mutable raw mapping is acceptable.

## Proposed minimum owned-loan contract

Add one owned fill loan alongside existing borrowed and slice APIs. API names
remain provisional until the SDK implementation review.

1. Acquire only with a ready ring and free slot. Move the existing `MappedRing`
   into a non-cloneable loan exposing only that slot's logical payload, never
   headers or other slots. Allow one detached loan per producer. The exclusively
   owned mapping is already `Send` in `sophia-isb`; no new memory sharing is needed.
2. Keep wire I/O and slot/offer ledgers on the service thread. Other offers can
   receive status and exact SlotReleased events during rendering. Mapping-writing
   APIs return busy while loaned; registration cannot restart through a live loan.
3. Return the loan and render result through Lom's existing bounded completion
   channel. The worker cannot publish. Completion and worker-exit wakes cover
   success, failure and panic; failed sends must recover the owned job/result.
4. Commit only on the service thread, rechecking producer/loan identity, ring and
   connection state, current render identity, dimensions and control capacity.
   Today's borrowed guard prevents intervening producer mutation; an owned loan
   needs these explicit checks. On `Again`, retain/return the completed loan for
   bounded retry without rerendering or consuming offer IDs or slot generations.
5. Explicit abort returns the mapping and frees the unoffered acquisition without
   sending or spending anything. Render errors return the loan for abort.
   Dropping an unreturned loan must instead terminate that ring, never mark it
   reusable. A private per-loan identity and abandonment signal checked by owner
   polling avoid a second SDK worker/return queue. The exact signal and caller
   wake obligations still require implementation tests.
6. Disconnect, revoke, stale allocation and timeout cannot reclaim memory from a
   running worker or authorize a late commit. Retain custody until return/drop,
   reject late results and follow existing failure policy. Preserve one render
   job in flight; do not widen presentation/resource concurrency.

Source trace at the pinned SDK: `sophia-image-producer/src/ring.rs` contains
acquire/commit, event ledgers, close/journal loss and `RingSlotFill`;
`sophia-shell-client/src/files/image_adapter.rs` contains pending control and
final notification; `sophia-isb/src/{lib,mapping}.rs` owns the mapping. Mapping
access is needed for pre-Ready source publication and fill/commit, while ring
events update ledgers. Test every state entry point with the mapping loaned.

Lom currently sets `ring_capacity: 0` (`src/service.rs:268`), selecting the SDK's
negotiated maximum resource capacity rounded down to a multiple of four. That
registration is fixed while panel sizes can vary. Preserve it initially and
expose an exact frame-length slice within it; check untouched guard bytes.
An exact-sized ring per frame would be another lifecycle change, not this copy
removal. The serial benchmark's exact-capacity recipe is not a general Lom rule.

## Integration and qualification

SDK changes reach the producer, shell-client adapter/public exports, API docs and
lifecycle tests. Existing borrowed-fill and slice callers keep their contracts.
The loan itself needs no wire format, protocol allocation, W2 or host change.

Lom changes reach explicit CPU startup selection, a CPU worker using the retained
scene construction, job/result types, scheduler, presentation commit and shared
pixel conversion. CPU startup must not create a GPU grant/adapter. GPU/DMA stays
explicit and unchanged. Carry `RenderIdentity` (grant, output, allocation and scale
generations) with the job and revalidate on return. Prospective resource IDs must
not advance live counters before successful commit.

Before any default change or native recipe:

- Compare identical Vello CPU scenes through owned-buffer/slice upload and direct
  slot rendering, byte for byte after the same conversion. Include text, alpha,
  clipping, scaling and larger surfaces, not only opaque shade fill.
- Prove exact length, untouched guard bytes, foreign/stale loan refusal, one
  acquisition/commit, no spending on abort/Again/error, and reuse only after exact
  SlotReleased. Acceptance or ResourceReleased alone cannot free an offered slot.
- Exercise worker stall/panic/drop, request/completion send failure, disconnect,
  revoke, timeout, late return, pending control and all slots busy. Owner polling
  must continue while the worker stalls. Preserve existing slice/GPU/DMA tests.
- Prove CPU startup with GPU denied and GPU refusal without silent fallback.

Then qualify native pixels, cadence, release custody and cleanup. Isolate the
copy effect with **the same new CPU renderer**: owned buffer → conversion → slice
upload versus slot → conversion → commit. GPU readback is not that baseline.
Pin scenes, renderer, capacity, host and recipe. Predeclare combined/per-process
CPU intervals, elapsed render-to-ready time and cadence at bar and larger sizes,
plus a stop rule; no repeat-until-win. CPU accounting is not latency or GPU time.

A 1920×24 bar is 46,080 pixels / 184,320 bytes. CPU may finish simple small scenes
sooner by avoiding GPU submission/export overhead, but the crossover is unmeasured.
The [matched CPU comparison](iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md)
does not establish a direct scene-rendering gain. The
[GPU qualification work](nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md)
and direct-export-target optimization remain after this CPU work.
