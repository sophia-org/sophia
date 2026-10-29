---
id: nb3mgbwc
date: 2026-10-29
kind: investigation
status: investigating
tags: [investigation, performance, benchmark, gpu]
---
# What must match before comparing GPU buffers across Sophia, niri and XLibre?

## Decision and scope

The [matched CPU comparison](iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md)
is qualified. The next critical-path gate is a matched GPU-buffer producer,
not a repeat of GPU render → CPU readback → owned-ring upload. Source tracing
finds a usable Sophia exporter and import protocols for the other stacks, but
no existing three-stack benchmark adapter. This matrix closes the initial source
inventory; it does not establish native import, synchronization or pixel parity.

Use **fresh, producer-completed, single-plane LINEAR XR24 exports** for the first
GPU comparison, with at most two outstanding producer frames and serial frame
progression. Keep the same opaque shade scene, dimensions and display workload.
This is a separate fresh-buffer lane, not an old/new CPU-versus-GPU intervention:
the qualified CPU lane reused two slots (Wayland ARGB8888 shared memory).
Fresh export/allocation and per-frame import work belong in the measured GPU
lane; do not later call it a retained-buffer compositor steady-state cost.
There are 2120 fresh source exports per arm; count actual imports rather than
assuming a driver performs exactly one allocation/import per generation. Do not enable DMA reuse, single-submit,
W2, destination-reuse policy or a new display topology in one GPU arm alone.

## Exact source closure inspected

- Sophia host baseline `d9116ff0905fab5c8bc01a71228ecc9e1a7a0a6d` and current
  image contract (unchanged relevant DMA completion rules).
- Peer `9cb0a37c7b088422c0a71706700f4c7a5adf23e5`, branch
  `bench/cpu-owned-ring`: existing GPU branch and `tools/gpu-bar/common/gpu_canvas.rs`.
  CPU source/artifacts remain frozen; later changes use fresh branches.
- Peer lock: Lom `dcb1eb88cb1e386ed3d3f1f22400de37174e88ea`,
  `masonry_imaging` 0.4.0 at Xilem `b81d8d7a631849def6eeab282561439b963862e5`,
  imaging 0.0.1, Vello 0.8.0, wgpu 28.0.0, wgpu-hal 28.0.1, SDK `0706ea2c`.
  These are the actual lock entries, not the older explanatory PROVENANCE pin.
- External clients at harness `3b24ccdd02a4f91a2bd0f1c6db12b82cdfae410f`:
  `tools/external-bar/{wl-panel/wl-panel.c,x-panel/x-panel.c,Makefile}`.
- XLibre source `56be9f4320ef121dc5d4bc40a6365d995512d3bc`: DRI3 request/import
  and Present COPY source, matching the prior installed source receipt.
- Niri checkout `5f4469b6a992492cf7221b269e9379f42e737649`:
  `src/handlers/mod.rs:440` and `src/backend/tty.rs:854` expose DMA-BUF import
  and renderer-derived feedback. **Not an installed-binary provenance claim:**
  `/usr/bin/niri --version` reports 26.04 with unknown commit; the checkout has
  an unrelated deleted `docs/uv.lock`. Keep the installed binary hash pinned and
  qualify its advertised interfaces/formats at runtime; do not equate the two.
- Installed protocol/API material: linux-dmabuf-v1 XML, xcb-dri3 and xcb-present
  headers/libs 1.17.0, wayland-client 1.26.0. Availability is not a native import test.

## Compatibility matrix

| Property | Sophia | niri / Wayland | XLibre |
| --- | --- | --- | --- |
| Producer today | Vello/wgpu DMA exporter exists | Existing benchmark is CPU `wl_shm` only | Existing benchmark is CPU MIT-SHM only |
| Common producer target | Same pinned renderer, opaque full rectangle, shade=g%128 | Same renderer and scene through an in-process bridge | Same bridge and renderer |
| Device | Sophia-granted exact DRM render dev_t | Explicit benchmark render node, exact Vulkan DRM identity; match compositor feedback/device | Explicit benchmark render node, exact Vulkan DRM identity; match display device |
| Transport | r10 image source kind2, SCM_RIGHTS, granted DMA capability and exact advertised row | New linux-dmabuf v4 adapter to existing layer surface | New DRI3>=1.2 PixmapFromBuffers adapter; plain PresentPixmap to existing window |
| Layout | XR24, modifier0, one FD/plane, actual offset/stride/FD size checked | Require advertised XR24/LINEAR, send actual plane layout | Require DRI3 modifier support, depth24/bpp32, actual offset/stride; checked import |
| Producer completion | Export returns after render/blit and foreign-family release fences | Identical producer completion before attachment | Identical producer completion before Present |
| Consumer lifetime | Accepted plus exact capture-complete SourceReleased ends source use; ResourceReleased is later | Retain exported object through matching wl_buffer.release; presentation feedback is separate | Retain through matching pixmap+serial IdleNotify; CompleteNotify is separate |
| Copies retained | Vello source → export blit; Session capture into owned storage, then composition | Same export blit; compositor import/composition, further copies not assumed absent | Same export blit; DRI3 import and existing COPY-at-vblank, not a zero-copy/flip claim |
| Feedback | Existing candidate/native kernel-completion join | wp_presentation timestamp plus release/frame-callback pacing | Present CompleteNotify UST/MSC plus IdleNotify |
| Current qualification | Existing Sophia path/source evidence; new matched recipe still required | Transport/producer bridge missing; native format and lifetime proof pending | Transport/producer bridge missing; native DRI3/import and lifetime proof pending |

The common scene is in `gpu_canvas.rs:47`: an opaque rectangle whose RGB channels
are equal. XR24 stores B,G,R,X and ignores X; it is not a promise that the stored
X byte equals 255. Sophia's capture forces alpha to one. Qualify channel order
with a separate non-grey pixel test too; grey alone cannot expose an R/B swap.
GPU readback may be used by an isolated correctness oracle, never by the timed
submission path or as a fallback when import fails.

## Device and Sophia admission prerequisites

Pin the physical GPU to PCI **0000:03:00.0**, currently card0/renderD128. The rig
also exposes 0000:16:00.0 at card1/renderD129; refuse that device and cross-device
PRIME routing in this lane. Require opened FD dev_t, Vulkan DRM identity and
sysfs/by-path physical-device mapping to agree. Device numbers alone are not
persistent identities. X retains its BusID/kmsdev binding; Sophia retains its
private admitted render node and the profile excluding the other GPU.

The existing harness `bench.py:459` does not pin niri's render device. The source
supports `debug { render-drm-device "/dev/dri/renderD128"; }`; validate the exact
syntax against the installed binary before packaging, then verify the selected
device at runtime. Bind linux-dmabuf **version 4** explicitly and require a complete
feedback batch with main_device and a matching XR24/LINEAR tranche on the same
physical GPU. Validate format-table size/indexes and device-array size; refuse
feedback changes that invalidate the selection. Do not require raw main_device
or tranche dev_t to equal renderD128: the protocol explicitly allows either a
primary or render node and forbids comparing dev_t alone for physical equality.
Resolve its DRM device identity to 0000:03:00.0 and match the bridge's render node.
This also avoids treating version 6's absent main_device as version 4 evidence.

Sophia `negotiation_policy.rs:23–56` requires the compiled capture feature, an
owner capture configuration advertising DMA (source_kinds 6), and requested
CONTENT_SURFACE|OWNED|DMA (bits 7|15|14) at r10. Exact discovered format/output/device
rows and capture budgets must be available. GPU admission alone is insufficient.
Use the existing owner configuration path (`shell_component_connections/images.rs:81–119`);
source_kinds is derived there, not an arbitrary new peer key. Do not alter the
CPU profile or assume its binary's capture feature from its filename. Before
packaging, bind the exact host hash to its signed source/build feature receipt
and run its device-free argument preflight on the final DMA profile/argv.
If the existing 3897c98c receipt cannot prove this closure, produce a separately
pinned host in an isolated lane; do not relabel CPU provenance as DMA proof.

## Export and synchronization findings

At pinned Lom, `crates/shell-gpu/src/dma.rs:210–283` renders a source texture,
blits into a fresh exportable BGRA target, waits for completion, then exports its
FD. `release()` begins at 442; its fence-wait section at 539–553 waits on its Vulkan fence and wgpu submission. The
handoff transfers from the producer queue family to FOREIGN and transitions to
GENERAL. XR24/LINEAR and exportability are checked; offset/stride come from the
actual image layout. There is a bounded four-frame library retention pool and
a two-second export budget. The benchmark must impose its stricter two-frame cap.
A timeout/uncertain submission retains GPU objects and refuses further work;
never manufacture completion or reclaim an ambiguous frame to keep the run alive.

Sophia's existing `dma_frame.rs` offers the FD, waits for Accepted and settles
the exact source offer before dropping the fresh frame. The image contract's
DMA-BUF completion section requires an owned capture and real GPU completion;
no candidate/flip is needed to finish capture. SourceReleased and the later
immutable ResourceReleased are different events. Keep those semantics and costs.

Wayland's installed linux-dmabuf XML specifies implicit DMA-BUF synchronization
unless another extension overrides it. Use ordinary linux-dmabuf first; do not
claim explicit synchronization from a producer CPU fence wait. Import must be
acknowledged by created/failed (or checked immediate creation), and the advertised
format/modifier must match exactly. Neither frame callbacks nor presentation
feedback permit writing or reclaiming a busy buffer.

XLibre `Xext/dri3/dri3_request.c:410–495` validates and imports plane FDs;
`glamor/glamor_egl.c:1336–1419` imports with the supplied modifier through GBM
or the direct path. These establish a source-supported route, not that every
installed driver accepts the chosen allocation. `Xext/present/present_execute.c:121–136`
copies, flushes and sends IdleNotify on the ordinary path. Use plain
PresentPixmap, check DRI3>=1.2 before PixmapFromBuffers with a modifier, and
record/refuse any alternate syncobj submission path. The release_syncobj branch
at 127–131 imports a release fence instead of sending this IdleNotify. **A flush/IdleNotify
is not by itself proof of a completed foreign GPU read for Lom's unsafe reuse
API.** That API explicitly overwrites without waiting on implicit fences.
Do not call it in this first lane. Fresh frames are never redrawn; imported
objects retain their own backing references. Device qualification must still
cover FD ownership, destruction ordering and delayed consumer release. Do not
interpret CompleteNotify as source-idle or GPU-complete either.

Both external adapters need a bounded last-buffer/teardown rule: after the final
frame, detach/destroy the surface or pixmap as appropriate, drain required release
notifications under a deadline, and retain uncertain objects until process exit.
A Wayland compositor may keep the currently displayed buffer; waiting for its
release before submitting its replacement can deadlock. Permit a second retained
frame, but retain serial presentation pacing and refuse more than two. This is
an explicit client ownership limit, not proof that driver allocations are bounded
to two or that each stack has identical internal queue depth.

## Smallest implementation, in dependency order

1. **External device selection and shared producer.** Lom's existing
   `new_diagnostic()` sets `adapter_drm=None` (`lib.rs:170–186`), and
   `render_dma_buf()` refuses that state (`dma.rs:220`). Do not fake a Sophia
   grant or weaken `GpuGrant::from_environment`. Add a narrow explicit-render-node
   diagnostic export constructor in a fresh Lom lane, reusing exact DRM adapter
   selection and `from_adapter`. Verify the opened character device and selected
   adapter dev_t, refuse software/missing identity, retain the FD, and label its
   evidence as external device selection, not Sophia authority. Preserve the
   grant constructor and the no-export ordinary diagnostic path. No change to
   render/export commands or dependency versions is needed.
2. **Use that same renderer in the existing clients.** A small in-process Rust
   static library with a C header can supply open/render/descriptor/drop to the
   existing C clients. Keep a bounded opaque frame handle and checked plane fields;
   no frame helper process, IPC or extra uncounted renderer. Share the scene code
   with the Sophia peer in the new source closure rather than maintain a second
   renderer. Catch/report errors at the C boundary; no unwinding or dangling FD.
   All arms pin the same renderer implementation/lock closure; Sophia continues
   to use the granted constructor. Keep fresh DMA export and default capture
   policy unchanged.
3. **Add only the two missing imports.** Extend the existing layer-shell/Present
   event loops with explicit GPU modes: generated linux-dmabuf bindings and DRI3
   checked requests. Preserve CPU/default bytes and old acceptance. Reuse current
   generation/presentation journals and strict release matching; add explicit GPU
   producer, exact device/layout, fresh allocation and no-readback receipts.
   Refuse wrong/repeated events, missing release, unsupported format, import failure
   or mixed producer evidence. Keep the PageFlip-off COPY topology for XLibre;
   adding DRI3 does not authorize a different presentation topology.
4. **Qualify before comparing.** Device-free tests cover layout bounds, generations,
   two-frame lifetime, delayed/stale/duplicate releases, final drain, wrong device
   and cross-mode rejection. A separate device qualification must show exact
   pixels on all three import paths (including non-grey), exact format/modifier,
   render-node identity and safe teardown with delayed release. Pin generated
   protocol sources and native dependencies. Missing native support is a refusal,
   not a fallback. Only after these pass extend runner/harness qualification for
   a fresh matched-GPU lane and freeze its workload and packages.

Changes must reach Lom constructor/tests, the shared renderer bridge, both C
clients/build rules/tests, Sophia peer scene linkage and lock, runner/recipe
producer checks, harness journal/report qualification, package postflight and
provenance. Avoid allocating a protocol revision: the existing DMA, linux-dmabuf
and DRI3 transports suffice for this proposed shape. Do not change SDK/W2 policy.

## Acceptance and limits

The next actionable gate is device-free producer/adapter implementation in fresh
lanes, followed by dedicated device qualification. Source availability does not
close the native support gate. Niri installed-source identity, consumer lifetime,
format advertisement and imported pixel checks remain explicit open receipts.
No new benchmark result, zero-copy claim, GPU execution-time claim or comparable
end-to-end latency follows from this matrix. Use the same bound-process accounting
as the CPU reference; a helper process must not shift renderer work outside it.
Keep frame cadence and each stack's feedback span separate from CPU us/frame.

All sealed CPU/W2 evidence and binaries remain unchanged. No build, GPU open,
native run, package or production change was performed for this source gate.

## External constructor: device-free gate closed

Signed local Lom commit `1a0dc2329e18fe17f9bdf08e0d9ddcb03f57fbc7`, parent
`dcb1eb88`, implements the first step on `bench/gpu-device` in
`~/dev/lom-matched-gpu-device`. It adds `GpuPreview::new_external(path, major,
minor)` and separate `ExternalGpuEvidence`. Exact canonical path, NOFOLLOW open,
opened character-device metadata and exactly one matching hardware Vulkan adapter
are required. The opened FD stays retained. The shared selector's diagnostic text
now names the requested render node; grant-selection behavior is unchanged.
An externally created instance cannot produce Sophia admission evidence. Ordinary
`new_diagnostic()` still cannot export. No existing application calls the new API.

The complete repository gate passes: 81 Rust tests, zero failures, one ignored;
17 tooling tests; layout, formatting, dependency audit, doctests and strict Clippy.
Three new device-free tests cover path rejection before Vulkan initialization,
metadata and adapter selection, and the evidence collector's origin refusal.
Four focused mutations (character type, FD minor, adapter minor and external-origin
refusal) fail in the intended tests; each is restored and the three tests pass.
The initial Clippy run stopped on two unnecessary test clones; fixed. An initial
character-type mutant failed compilation on an unused import and is retained as
invalid mutation evidence; the corrected mutant fails its assertion.

Claude's read-only source review found no blocker. `Cargo.lock`, manifests,
toolchain, render/export/reuse/readback and Vulkan DRM query code are unchanged.
Receipts, exact diff, per-target results and mutation logs are under
`~/.local/state/matched-gpu-device-01/`, with SHA256SUMS. No release build, GPU open,
native run, push, SDK repin or frozen artifact change occurred.

This proves the device-free boundary only. Real render-node open/NOFOLLOW behavior,
live constructor/export and `evidence()` on a live external renderer remain device
checks. The opened FD and Vulkan agree by dev_t; Vulkan opens its own device.
Physical PCI binding to03:00.0 is the next bridge's responsibility, not claimed
by this constructor. Next is the in-process shared producer bridge, then both
consumer adapters and their independent device qualification.

## Shared producer bridge: device-free gate closed

Signed local niltempus commit `554935ee25f286a453bcb9906fad12716bbdd358`, parent
`9cb0a37`, adds `tools/gpu-bar/bridge` on `bench/gpu-bridge` in
`~/dev/niltempus-matched-gpu-bridge`. The static library executes in each external
client's process. It shares the existing scene function byte-for-byte with the
Sophia peer, extracted to `common/gpu_scene.rs`. Both pin signed Lom `1a0dc2329`
through an explicit local `file://` exception. Registry dependency versions are
unchanged; the peer lock changes only the Lom source. No public-pin or packaging
approval is claimed.

The C ABI fixes the first workload at 1920×24. It returns actual offset, stride
and source bytes for fresh producer-completed XR24/LINEAR exports. The opened
node and selected adapter must agree with the requested physical PCI device:
sysfs resolves the DRM identity before and after construction, and a nonempty
Vulkan PCI address must also match. Empty PCI evidence refuses. The recipe must
pin `0000:03:00.0`; the fake-sysfs test distinguishes the other GPU `0000:16:00.0`
and permits primary/render aliases of the same device. This is identity agreement,
not use of the retained FD for Vulkan submission.

At most two frames are retained. Generations advance on successful render;
process-wide nonwrapping tokens permit exact out-of-order release but reject
foreign, stale and duplicate releases. C owns a CLOEXEC duplicate only on success.
Render/dup failures poison the owner; close refuses uncertain or outstanding
custody. Panic is caught at each calling-thread C boundary and aborts the process.
Consumer release proof and valid, disjoint C pointers remain adapter obligations.
No unsafe redraw/reuse entry point is exposed.

Final device-free checks: six Rust bridge tests, an actual linked C ABI/refusal
test, 14 DMA-peer tests, formatting and strict all-target Clippy for both crates.
Six mutations (physical identity, adapter identity, capacity, token, generation,
empty FD) each fail assertions; restored source passes the full gate. The first
compile exposed a u32-to-u64 offset mismatch, corrected explicitly. Claude's
read-only review also prompted the mandatory nonempty PCI evidence and poisoned
dup-failure behavior. Receipts and checksums are in
`~/.local/state/matched-gpu-bridge-01/`.

Real render/dup/commit, release of a real frame, renderer close and post-Vulkan
verification remain device receipts. Next are the linux-dmabuf and DRI3 consumer
adapters, then pixel/import/lifetime qualification. No GPU was opened, no native
run or release build occurred, and no frozen source, binary or package changed.

## External import adapters: device-free gate closed

Signed local niltempus `622145d4ad8408ca3029c0ca80b0d3dacf31d1c1`, parent
`554935ee2`, adds explicit `wl-panel-gpu` and `x-panel-gpu` build targets in the
same `bench/gpu-bridge` worktree. Both use the bridge above. The ordinary CPU
clients preprocess byte-identically to the signed parent; no CPU artifact or
frozen package is replaced. No Rust source or renderer dependency changes here.

Wayland binds linux-dmabuf v4, uses surface feedback, and requires the main GPU
plus an XR24/LINEAR tranche to resolve to `0000:03:00.0`. Format-table bounds,
indices, batch state and unknown flags are checked. Missing tranche flags are
zero. Compatible updates may complete under a bounded wait. Immediate import
keeps a fatal failed-event listener alive until buffer release; this avoids an
extra creation round trip without silently dropping failed imports. The caller
closes its outgoing duplicate after libwayland queues its own FD. Each exact
wl_buffer release drops its bridge token. Final teardown waits for presentations,
detaches the last buffer and drains releases; timeout never counts as release.

X requires DRI3 1.2, XR24 visual masks, LINEAR support and a DRI3Open FD whose
sysfs physical device matches. Each generation imports a fresh pixmap, transfers
FD ownership to XCB, and uses plain PresentPixmap without fences. The exact
event/window/pixmap/serial must match IdleNotify before release. Completion alone
does not free a token; stale, foreign, duplicate or skipped events refuse. Both
backs drain before bridge close. X COPY may release before completion, Wayland
may retain the displayed buffer, and Sophia releases its source after capture:
two frames is a common cap, not identical instantaneous retention.

GPU journals name `vello_shade_mod128_opaque_rgb`; the preparation start spans
render/export and producer completion, not CPU fill. The future GPU qualifier
must require exactly one complete producer/import/end receipt, exact transport
and device values, and rendered/released counts equal to frames plus warmup.
These are supporting self-reports, not authenticated custody. The existing CPU
harness is not claimed to qualify these GPU journals.

Device-free evidence: both CPU/GPU C targets compile with warnings as errors;
three adapter test executables pass, including 13 Wayland and 12 X refusal cases,
11 invalid layouts, stubbed actual import-call/FD-transfer checks, exact releases,
fake sysfs aliases and an installed-libwayland socket-pair timeout. Two CLI tests
cover 12 refused recipe combinations before display connection. Existing CPU
fill parity and X event controls pass. Six mutations are killed (five assertions,
one final-drain refusal), followed by the restored full gate. A first main-device
mutation survived a nondiscriminating test; its duplicate-state masking was fixed.
The first listener mutation failed compilation on an unused test function and is
not counted; the corrected mutation fails at the import listener assertion.

The tests use memfds and protocol stubs, never a real GPU or display; the mock is
not linked into GPU clients. Claude's read-only review identified the immediate-
import failure listener and evidence-label issues, now addressed. Source, compiler,
XML and compile-only binary hashes, parity, tests and mutations are recorded at
`~/.local/state/matched-gpu-adapters-01/` with checksums. No native run, package,
publication or performance claim follows.

Next is the qualification harness: strict GPU receipts and pinned installed
niri/device configuration; then real single-frame pixel/import/sync/release proof,
including a non-grey channel-order oracle and final detach/drain. Only after those
gates may a separately sealed matched GPU comparison be prepared.

### GPU receipt and installed-config gate, October 29

Signed local niltempus `d81ccf6ad` on `bench/gpu-bridge`, parent `622145d4a`,
adds `tools/external-bar/gpu/qualify.py` and the separate `gpu/niri.kdl`.
The validator requires declared transport/frame/warmup values, exact GPU
producer/import/end receipts, exact header and allowlisted metadata, each
transport's actual writer order, contiguous counted rows, monotonic submission
and presentation times, native timestamp sources and X clock-check counts.
The client PID is returned for later binding; it is still a self-report.
Wayland's selected-output row must be DP-1/1920×1080/scale 1 and precedes its
header. Its end/drain receipt precedes the counted rows. The GPU-only clock note
now describes DMA-BUF implicit synchronization rather than inherited SHM seals;
CPU preprocessing is unchanged.

The result is **receipts_valid**, never a native qualification pass. The future
harness must bind that PID to the sampled TGID/starttime and pinned executable,
cross-check presentation feedback and sampler anchors, check actual compositor
output/refresh, and establish pixel correctness, real import/synchronization,
release and cleanup. The selected wl_output line is not actual mode readback.
Self-reported drain counts do not independently prove consumer completion.

Installed `/usr/bin/niri` reports `26.04 (unknown commit)`, binary SHA256
`5b8c21910f17ab1eaed6562bc54a09532c8f15dbd3abf563817158511acbe986`.
It accepts the exact renderD128/DP-1 diagnostic configuration and refuses the
same file with an unknown device key. The successful validation syscall trace
has no device opens, connections or ioctls. This proves syntax only, not runtime
device selection. The native gate still requires main/tranche physical-device
resolution to 03:00.0 and actual output readback, retaining the niri render-device
log if emitted. The nearby source checkout is not claimed to identify this binary.

Seven Python tests pass, including missing/duplicate/foreign receipts and metadata,
every adjacent record-order swap, bad devices/layout/counts/timestamps and a CLI
refusal with no success output. Three scratch-copy mutations are assertion-killed
(receipt values, record order, canonical metadata integers). Both GPU adapters
compile; the three adapter tests and two refused-CLI tests pass; preprocessed CPU
sources remain identical to their signed baseline. Claude's final read-only review
found no blocker. Receipts and checksums are in
`~/.local/state/matched-gpu-receipts-01/`. No GPU/display/native execution, release
build, package, push or performance claim. Next is real non-grey pixel/import/
synchronization/release qualification before a separately sealed comparison.

## Review receipt

### Untimed consumer-capture preparation, October 29

Signed local niltempus `8a32087cb`, parent `d81ccf6ad`, adds
`tools/external-bar/gpu/QUALIFICATION.md`, `pixels.py` and synthetic controls.
This advances preparation; the native gate remains open and no launch package
is ready. The contract separates diagnostic colour/hold producers from timed
grey binaries, with three held generations (red/blue, green/white, red/blue),
consumer-side captures on all three stacks, actual import refusal and exact
release/drain/cleanup evidence. No producer-readback substitution is allowed.

`pixels.py` accepts only a declared pattern and exact full-output 1920×1080
RGBA8 PAM. It checks every pixel in the top 24 rows except the two split-edge
columns, checks captured alpha and hashes the full capture and strip. Its result
is `pixels_match` only. Five synthetic tests and three assertion-killed mutations
cover wrong channels, grey fallback, stale patterns, alpha, dimensions, truncation,
trailers and the intentionally excluded region. Seven existing receipt tests
still pass. No cargo, C build or device execution occurred.

Claude's source review found a necessary correction to the capture route:
**d9116ff0 cannot configure a writable host destination for an observer recipe.**
The private `/tmp` is discarded at observer exit; the naming recipe's `source`
property does not grant writable host paths. The guide's conditional PAM output
description is not a capability that the recipe grammar exposes. The contract
now names this prerequisite instead of pretending a configuration change suffices.

The complete wrapper trace also rules out inherited stdout as-is: it is Session's
fd 1, not the launch caller's pipe, and may be discarded or tee into shared logs
and the VT. The separate untrusted-output branch is explicitly not acceptance
evidence. Atomic framing does not bind a writer. The immediate prerequisite is
a reviewed private diagnostic delivery channel, preferably a dedicated observer
stdout pipe installed by the process owner at spawn, with a separate pixel helper
or CLI change. This is new diagnostic plumbing, not existing configuration.
Normal recipe/Session output policy stays unchanged; no writable filesystem grant,
broad FD inheritance or checksum-only substitute is assumed. Process/request/
size/checksum/generation binding and same-namespace launch must be proved before
native sealing. Signed local `d593d887f`, parent `8a32087cb`, records this
delivery correction without changing the pixel checker.

Niri's PNG normalization and X's mask/byte-order RGBA writer also need explicit
tests; existing alpha cannot be overwritten to pass. Formats without alpha may
normalize to opaque with that synthesis recorded. Sophia's output generation and
presentation generation must be tied to the held candidate/capture interval;
the colour match alone is insufficient. Source trace, tests and mutations live
at `~/.local/state/matched-gpu-device-prep-01/` with checksums. Next is proving
the diagnostic pixel-delivery/controller route, then implementing colour/hold
entry points and normalization before release receipts and attended native sealing.

Claude independently checked the pinned exporter, external clients, XLibre import
and Present source, and niri source. His device-selection and Sophia feature/
capability prerequisites are incorporated above, with the protocol's primary-
versus-render-node identity correction. Source hashes and link checks are recorded
outside the repository in `~/.local/state/matched-gpu-matrix-01/source-check.json`.
No independent review is treated as native or device-free execution evidence.

## Connections

- [Critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md).
- [Producer comparison and CPU result](iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md).
- [Owned GPU capture qualification](../plans/h38nf8bk-qualify-gpu-native-shell-images-through-owned-capture.md).
