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

## October 30 SDK source gate

The Rust custody layer is implemented locally on `cpu/owned-fill` in
`~/dev/sdk-rs-cpu-owned-fill`: signed implementation
`5d89565266c3a066c32c2eebac6fad40dc1d0a66`, parent exactly
`0706ea2c78cd16ba59ed209620354f30a2c78906`; signed documentation head
`4d81630c5125c88cb74c1b45c1a0fe890a12a64a`. The tree is clean and unpushed.
Public operations are `image_acquire_ring_fill`, `image_commit_ring_fill` and
`image_abort_ring_fill`, with an owned `OwnedRingSlotFill`. Failed commits retain
the loan, successful operations leave it inert, and abandonment is terminal.
One lazily allocated producer identity is reused across fills; consumed-payload
checks prevent an old handle claiming a later fill. No per-frame token allocation,
new thread, wire record or unsafe block was added.

All 948 workspace tests pass (174 result lines, including doc tests). Fifteen new
cases cover producer custody and scripted shell-client integration, including
actual mapping identity, wire-visible payload, owner progress, foreign/stale
handles, exact release, failures and late commit after disconnect. Four guard
mutations are killed and restored; the restored producer/client tests pass.
Workspace/all-target clippy has no warnings. Changed Rust files pass formatting;
whole-workspace fmt still refuses one unchanged baseline hunk in
`sophia-image-producer/src/role.rs`, hash-checked against the base. No DMA edit was
included to conceal that exception.

Receipts: `~/.local/state/cpu-owned-fill-sdk-01/`, 25 manifest entries verified;
`RECEIPT.json` SHA256 `bb8d771d7b3f886b4a0f7ce718655e53c2c01d84e838d595411eb8b7d2423e5c`,
manifest SHA256 `30ff435ff65698052f1499c09a8c665728a2afbf7805b98c13b9fef3a6b7d73c`.
This is device-free Rust source evidence, not Lom integration or native acceptance.

**C behavioral parity is proved under documented caller ownership.** Signed
`9adb3c5b83a52baf2a8728cb10fcf821d1d6729b`, tree `cd509040`, based on
`3ccc7d04c28309ebed6fd53f399af7c7a5682d80`, needs no new C operation or ABI.
The existing pixels/offer split supports a worker while owner dispatch and
disconnect leave its mapping intact. C callers enforce one fill, exclusion of
other pixel/offer calls, exact geometry and identity, synchronized completion,
and no result surviving close/init. Image close must wait for the worker to stop.
Rust enforces its loan rules; C does not automatically detect misuse or abandonment.

All 74 C test binaries and the Nim binding smoke test pass. Five new scenarios
use a real pthread parked mid-fill and independently transferred ring FD to prove
owner progress, abort/refusal without ID spending, backpressure/retry, exact
release reuse, and mapping lifetime across session/channel failure. Three
negative controls are killed and restored; M1's SIGSEGV site is inferred, without
a stack trace. The final checks bind tree and diff before/after. Receipts:
`~/.local/state/cpu-owned-fill-c-01/`, 33 manifest entries;
`RECEIPT.json` SHA256 `ac856c095c80861bfe941970db5c406166457228372e982c9b96233d9148a323`,
manifest SHA256 `5554687862534df6be7cabe11d32bd7f1fc45dcb2fc35286bac89d9602a67b7c`.
Rust's documentation follow-up is signed `99b05e345be34678128733bf0166ded85b06239a`;
its implementation and original receipts are unchanged.

## October 30 Lom consumer source gate

Signed local Lom `ceb89664ab07abae57bf6c56fe772820d37a8786`, tree
`674f10ca9ad90d9ab4afa69fb8dba99347aa1927`, parent `32e0bab3`, adds explicit
`--serve --renderer cpu`; GPU remains the default. CPU requires owned-ring and
does not read the GPU grant. Its existing worker renders the retained scene into
an owned slot loan and applies the same pixel conversion as the copy reference.
The owner retains its acquisition snapshot through completion and commit,
rechecks it at both stages, and commits that exact metadata. A stale completion
aborts before staging and re-dirties the panel. Errors after staging are terminal:
abort an uncommitted loan, then drop/restart the service. A successful Offer is
never rolled back by a later timeout. Accepted alone does not permit reuse.

The final device-free gate passes 18 Python tests and 106 Rust tests (one existing
ignored subprocess entry), source layout, formatting, dependency audit and
warning-free clippy. Seven snapshot/abort mutations were killed and restored.
Byte equivalence combines an actual owned-loan worker comparison against the
same CPU renderer's copy reference with the scripted peer's slot-byte check;
it does not establish CPU/GPU pixel equality. A real Accepted-only hold proves
that no second job or Offer starts before exact SlotReleased. SDK hard-commit
Refused/Closed/Argument is not induced end-to-end; only the components are covered.

Receipts: `~/.local/state/lom-cpu-direct-fill-01/`. Candidate and committed diff
SHA256 `22abd46edcf093a957818ff434dce6bd0fc38a92d6edefdbb957c13953fbe0b0`;
`RECEIPT.json` SHA256 `da0399053fd56b4707d7798ef77db7dc8df6e4cce7521bd8853376201be2d5d6`;
manifest SHA256 `a9009ef09bfa4aa5579a27cd5a53ae24df2a292ca3a9f49e3cd5ca5d1708e6f2`.
The SDK `file://` pin is a recorded local development exception. There is no
verified Sophia SDK pin update, release, native qualification, default adoption
or performance claim. C consumer enforcement remains caller-specific. Next is
native CPU qualification against the same-renderer copy reference; GPU direct
rendering stays later.

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
   reusable. A private producer identity, consumed-payload checks and an abandonment signal checked by owner
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
