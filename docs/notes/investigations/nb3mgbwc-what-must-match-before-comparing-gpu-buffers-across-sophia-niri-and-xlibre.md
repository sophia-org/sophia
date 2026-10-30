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

## Private delivery source gate (October 29)

Signed local Sophia `a56ace89116c02064b93462aa8d41e2f82dd2dab`, on
`bench/gpu-capture-delivery` with exact parent `d9116ff0`, implements the
private delivery prerequisite. Its contract is branch-local
`docs/diagnostic-capture-delivery.md`. No frozen CPU/W2 source, release or
package was changed; nothing is pushed or merged.

The default-off `diagnostic-capture-delivery` feature installs separate stdout
and stderr socketpairs only for the exact admitted developer observer launch.
Session creates a new private directory, without giving the child a writable
host path. After authenticated startup, it pins the live inner child by pidfd,
starttime and executable device/inode. The binding is **launch custody, not
per-message writer identity**. At capture publication it rechecks the fully
qualified connector against the live primary head, and retains server-side
request/output/incarnation/presentation-generation/pixel-hash evidence. The
returned frame must match, end at EOF and be followed by a successful reap.
Failures and the shared reader deadline notify the owner; pending checks run
before the namespace try-lock once preparation has returned.

Final device-free checks: feature-on Session lib **1119 passed, 65 ignored**,
including 12 private-delivery tests; real protected/host stdio tests **2/2**;
CLI refusal **1/1** in each build mode; feature-off Session controls **2/2**.
Feature-on and feature-off all-target clippy passed with warnings denied,
layout passed, and scoped formatting passed (two pre-existing config formatting
drifts excluded). Eight mutations were killed: missing wake, missing server
join, unsuccessful reap accepted, checksum bypass, EOF bypass, attempt-limit
bypass, overwrite allowed, and unqualified connector allowed. Final checks ran
after all mutations were byte-restored. Receipts are under
`~/.local/state/matched-gpu-capture-delivery-01/`; receipt SHA256
`c4bb2ed4d8edd5bb6960e371f3e5632bcb2a33193909373ec3754bf763d768d8`,
checksums SHA256 `cc092844af3f7c958aa7637b30eb1973cc83cb441aeb38dd12f07a9a8c0655a8`.

**Next is the controller, not a native launch.** It must wait for terminal
publication before stopping Session, require the exact complete JSON plus
matching PAM SHA256, an existing zero-byte stderr file, no refusal or partial
files, and the package-pinned connector/incarnation/held scene. The two files
are not a crash-atomic pair and workers are detached. A PAM alone never
qualifies. Filesystem preparation/thread scheduling before handoff is not
bounded by the reader deadline; the controller must enforce its own refusal
timeout. Fixture activation is supplied: real authenticated observer delivery,
namespace equality, output joins and consumer pixels remain native gates.
No GPU device, release build, native attempt or package was produced here.

## Private capture controller source gate (October 29)

Signed local niltempus `7214d43372bd019ea4db55bc0ad2ff2662ef722d`, exact parent
`d593d887f`, adds `tools/external-bar/gpu/capture_sophia.py` and
`capture_receipt.py` plus fixtures and the contract in `gpu/QUALIFICATION.md`.
One helper invocation handles one declared READY hold; the parent producer
channel and same-namespace wrapper wiring are still the next implementation.
Nothing was built, packaged, run natively, pushed or merged; W2 and CPU evidence
remain frozen.

The helper checks Session pidfd/starttime, executable hash/dev/inode, UID and
user/mount/PID namespace equality. It verifies the log-derived launch listener
through `/proc` inode ownership without connecting. It calls the exact pinned
self executable once, retains bounded private CLI stdout/stderr, and treats
`Started` only as readiness. A separate 30-second terminal wait exceeds the
host's 20-second delivery deadline; waits share a 75-second outer limit. No
retry, generation skip or stdout pixel fallback exists.

Acceptance requires complete private JSON/PAM/zero-byte-stderr triples, matching
PAM SHA and the declared non-grey pixel oracle, with no partial/refused files.
The output ID comes from this run's log. Review against saved CPU-02 evidence
caught that the actual head record is **bare DP-1 and nominal 120000 mHz**,
not qualified PCI/119880. The receipt must bind that output to the qualified
connector rechecked by Session at publication; actual 119.880 Hz stays a separate
native readback gate. Log reads use complete prefixes, and listener uniqueness
ignores accepted connections. Regression fixtures cover all three corrections.

The launch-to-receipt join is the sole new receipt in a serialized exclusive
directory. Transaction 1 is constant and only a format check; the CLI monitor PID
is not the observer PID. Across patterns 1,2,1, namespace and output incarnation
stay fixed while launch generation, request and capture presentation generation
increase. The held owner candidate must agree before and after capture, without
equating shell presentation_epoch to capture presentation_generation. A partial
log tail is deferred; this is why the parent hold barrier remains necessary.

Final device-free checks: **14 controller tests, 5 pixel tests, 7 journal tests**;
all **8 guard mutations killed and restored** (hash, stderr, sequence, connector,
after-hold, namespace, listener ownership and receipt mode). Claude reviewed
the saved final diff read-only with no blockers. These fixtures use local
processes, pidfds, Unix sockets and files; they execute no Sophia session or GPU.
Receipts are `~/.local/state/matched-gpu-capture-controller-01/`:
`RECEIPT.json` SHA256
`0341ccb3657bbd002f656a900a994ff18840b3794e0840794fb15cf45e56171a`;
`SHA256SUMS` SHA256
`a22ba66a0c88f1d62f0a7a4861ee78a53cd568852fe478424436384967b4c07f`,
29 entries verified. The committed diff equals the reviewed final diff.

Limits: custody-only delivery; CLI path reconnect has a time-of-check gap in
the private runtime directory; filesystem/scheduling are not hard real-time.
The helper kills only its launch client; Session owns observer retirement and
the parent owns full cleanup. Next: parent READY/ACK and diagnostic colour/hold
entry points, real same-namespace wiring and per-stack normalization, then
separate releases and native identity/pixel/refusal/lifetime qualification.

## Sophia colour/hold source gate — October 29

Signed local niltempus `80afd166859156cd8628a63b4b73d5571ecada2b`, parent
`7214d43372bd019ea4db55bc0ad2ff2662ef722d`, closes the Sophia parent hold-channel
source gate. The branch/worktree is `bench/gpu-bridge` at
`~/dev/niltempus-matched-gpu-bridge`; it is clean and unpushed.

The separate feature-required `gpu-bar-colour-hold` binary renders three fresh
GPU DMA patterns 1,2,1. READY follows exact presentation and predecessor release;
ACK follows the existing capture helper and a fresh owner-log hold check. During
holds the SDK 0706ea2 external loop services session IN/OUT, image readiness,
immediate work and retry deadlines. Unexpected typed content refuses; no cloned
9P socket or sleeping hold is used. Sophia releases the DMA source at settlement,
so the oracle concerns Session's imported copy, not producer-buffer scanout.

The parent derives the Session-owned shell listener, opens its private directory,
listens before publishing hold.sock, and pins the connecting peer with
SO_PEERPIDFD/starttime/executable and the single-child bwrap -> Session ancestry.
It removes its own endpoint before Session shutdown. A second hint, peer refusal,
changed grant, early data, process exit or stale challenge refuses without ACK
or retry. The diagnostic profile must contain only this shell-content producer;
other producers' grants are not multiplexed. stderr diagnostics are formatted
into complete bounded lines before write_all; JSON journals already did that.

READY.shell_output_generation is explicitly separate from the native capture
incarnation declared in the recipe and verified by the helper. The head log has
no incarnation field, so it is not derived from the shell facts or an earlier
run. A fixture deliberately uses shell generation 90 versus native incarnation 1.
The local wrapper starts Session/driver siblings and records owned-child cleanup;
it does not prove protected launch, PAM/VT cleanup or a native device path.

Final checks: **10 hold-driver, 14 capture-controller, 5 pixel, 7 journal tests**;
Rust feature-on **17+17**, feature-off **14**, both-mode clippy/fmt and the
historical shared-canvas check pass. **4 Python and 2 Rust mutations** were killed
and restored. Claude's saved-diff review closed all three findings read-only.
No GPU, protected launch, native run, release build, package, merge or push occurred.

Receipts: `~/.local/state/matched-gpu-hold-01/`, 28 entries verified.
`RECEIPT.json` SHA256
`8617082a58660851fbf173784a068937a340a3b38b9c08fa310857c25fe20e1e`;
`SHA256SUMS` SHA256
`1bb2b2f686e34087692e450502d2662c50237d1ce0380033237c8999115a5df2`.
The committed diff matches final-candidate.diff. Superseded failed build logs
remain labeled in the receipt rather than being removed.

Next: external Wayland/X colour holds and consumer-capture normalization, then
review the complete same-namespace/native recipe, separately build and pin its
releases, and qualify actual identity/mode/import/pixels/refusal/lifetime/cleanup.
Wayland must continue dispatching releases during a hold. No timed GPU comparison
is authorized by this source gate; W2 and CPU evidence remain frozen.

## External colour/hold source gate — October 29

Signed local niltempus `0f3bfc42c506d4379f14b64092702253672afe91`, parent
`80afd166859156cd8628a63b4b73d5571ecada2b`, closes the external source gate.
The worktree is clean. Separate diagnostic clients render three fresh patterns
1,2,1, publish READY only after presentation, service display events while
waiting, and accept one exact nonce/generation ACK after the parent pixel oracle.
Wayland's barrier is inside the fresh submission path before its continue;
X's is inside the validated presentation path. PNG decoding and XImage mask,
byte-order and padding normalization produce the shared RGBA PAM oracle input.
The parent sequence is an embedding API, not yet the complete native launcher.

Default bridge slot 1 and diagnostic slot 2 have separate paths/hashes. `nm`
shows the colour symbol absent from the default library and both default-check
clients, present in the diagnostic library and both hold clients. Timed Make
rules additionally refuse a supplied library containing that symbol before
linking. Both wrong-library controls refused without changing the clients;
default links then passed. These are debug/source controls only. No existing
CPU client binary was rebuilt; both CPU translation units passed syntax-only.

Final checks: **7 external-hold, 5 normalization, 8 journal Python tests**;
bridge **7 feature-on / 6 feature-off** tests, **14 DMA** tests, both-mode
fmt/clippy, C ABI controls with explicit rc=0, production-helper adapter
fixtures and existing GPU mock controls. **6 mutations** were killed and
restored, including removal of each production hold barrier. Claude's saved-diff
review found no blockers; the subsequent timed-link guard implements its
remaining artifact-separation suggestion.

Fixture limits matter: the Wayland fixture releases the generation during its
own hold, so it does not prove attached-buffer lifetime across the next commit.
The X fixture covers IdleNotify before completion, not during hold dispatch.
Neither exercises a real compositor or the complete client main loop. Under
X Present Copy, the pixel oracle checks the server's screen-pixmap copy, not a
client export retained through ACK. Wayland retains its attached buffer until a
later commit. Sophia captures its own imported copy. These are separate lifetime
contracts, not identical retention claims.

The niri native recipe must pin its IPC/output identity and screenshot path,
use `--show-pointer false`, and enable only DP-1 at scale 1 with no transform.
X requires the pinned server/output and no compositing manager redirecting
windows. The parent owns bounded capture callbacks, sampled-process identity,
output proof and successful reap. Socketpair holds provide custody, not
per-message writer identity. Real imports, screenshots, delayed releases,
protected Sophia capture and PAM/VT cleanup remain unqualified.

Receipts: `~/.local/state/matched-gpu-external-hold-01/`, **48 entries verified**.
`RECEIPT.json` SHA256
`c612d9752f10cb2d878f63d4d8e71ce10a3dfa61b5f515c76fc57cb4d0d3d2dc`;
`SHA256SUMS` SHA256
`5a57c7bbfc4ffc6faae4c68fedd2aaeb56612bd5c25bf767e570e54f1aab0c35`.
Committed diff equals final-candidate.diff. Early failed/superseded builds are
retained and labeled. No GPU/native run, release build, package, merge or push.

Next: complete the native controller/wrapper and diagnostic recipe, review them,
then separately build/pin releases and qualify actual device delivery. No timed
GPU comparison is opened by this gate; CPU and W2 evidence remain frozen.

## External native-wrapper source gate — October 30

Signed local niltempus `286eb0ed75ee91ac6fc4cbc3ec06188d728ef78d`, parent
`0f3bfc42c`, adds the untimed external wrapper and fixed niri/XLibre recipes.
It binds opened executable identities, process starttimes/pidfds, owned
listeners, physical DRM mapping, live output readbacks and the three held
colour captures. Helpers have bounded private output and deadlines. The
client journal must join each READY presentation timestamp; local cleanup
checks client then server, including a server exit after DONE. This is a
source gate, not native qualification or a comparison row.

The niri config now ignores `/dev/dri/card1`; SHA256 is
`58948f4c081a5f29f45af0ebff91a71d072795912eef9a5f4b87c05ba972c976`.
Pinned installed niri `5b8c2191` accepts it and refuses a misspelled key under
strace with no DRM opens. This proves syntax only. Four-node lstat/sysfs
checks pin this boot's card0/renderD128 to 03:00.0 and card1/renderD129 to
16:00.0 before launch and during identity checks. Live server FD checks must
still establish that no second GPU was opened. Newly attached DRM devices
are outside that four-node preflight, but remain subject to live FD checks.

X uses the saved CPU recipe, checks filesystem and any abstract listener
ownership, and uses `xrandr --current --verbose` during holds. X capture
refuses a registered compositing manager. XKB data, xkbcomp and the input
module remain explicitly host-unpinned; the package must bind their closure
or preserve that limit. The opened executable FD is intentionally inherited.
The bridge hash alone does not prove static linking: release/link receipts
remain necessary.

Final saved checks: **19 wrapper, 7 external-hold, 8 journal, 14 capture,
10 Sophia-hold and 5 pixel tests** (63 total), C capture controls with rc=0,
and **12 killed/restored mutations**. M12 removes coupled server exit guards;
it is not three independent mutation proofs. Fixtures use real local
process/socket/pipe/inotify mechanisms with compositor/device stand-ins.
Both saved CPU output fixtures parse unchanged. Claude's final read-only
review found no blockers.

Receipts: `~/.local/state/matched-gpu-native-wrapper-01/`, **44 entries verified**.
`RECEIPT.json` SHA256
`6de1ea095193975bbd941d6615213416a62028bf12af580f12e1c367d5508889`;
`SHA256SUMS` SHA256
`cd475674cfe16bc5e5b42776393d030b3cd39ca1a92f7bb7edd70ca0804e4a5e`.
Committed diff equals final-candidate.diff (`96881ed8`). Earlier logs/diffs
are retained as superseded history. No Cargo/release build, GPU device open,
native run, package, merge or push occurred; frozen W2/CPU evidence is unchanged.

Next is protected handoff/package integration, concrete real import-refusal
and delayed-release controls, and separate release receipts before attended
device qualification. Server shutdown codes and whether niri's primary-node
ignore excludes all second-GPU nodes remain runtime-unknown. A first refusal
is a calibration finding retained as failed evidence, not permission to count
the attempt as qualified. Outer descendant, PAM and VT cleanup remain separate
from this wrapper's local process cleanup.

## External handoff staging — October 30

Signed local niltempus `fb21242f9df7a42fc6b6b13af1a7c91120c4c701`, parent
`286eb0ed75ee91ac6fc4cbc3ec06188d728ef78d`, adds `gpu/handoff.py` and its
device-free controls. The committed diff equals the saved candidate diff
(`e514f00c`). The worktree is clean and the signature verifies G.

Staging checks the exact immutable CPU02 helper templates and reuses its PAM/VT
program byte-for-byte. It renders only the explicit external colour command,
extends the enclosing deadlines to wrapper/PAM/outer 300/360/420 seconds, and
creates a single-use attempt record before stopping getty. The root entry
requires a complete nine-file seal and verifies external hashes before loading
the privileged control helper. Staging creates no seal and launches nothing.
These checks do not replace the package's release/link provenance review.

The saved terminal-receipt check combines local client/server cleanup with
PAM close, credential deletion, pam_end, TTY restoration and empty outer
occupant/session/kill lists. Its result is `handoff_cleanup_verified`, not
device qualification. Missing, failed, mismatched or incorrectly typed
evidence refuses. External stacks alone are covered; this does not supply
Sophia's diagnostic namespace/core/feature configuration.

Final checks: **8 handoff and 19 wrapper tests**, plus **8 killed/restored
mutations**. All **166 inherited host-helper hashes** match in a read-only
check. Fixtures use the pinned templates but execute only the pre-import
seal/pin guard and receipt predicates; no PAM, VT, getty, elogind, compositor
or GPU operation ran. No Cargo/release build, native package, merge or push.
W2 and CPU source/artifacts/evidence remain unchanged.

Receipts: `~/.local/state/matched-gpu-handoff-01/`, **21 entries verified**.
`RECEIPT.json` SHA256
`6d030f88b9319a5511f25d50f06148c7e466b1336cc7f578bc9da3b33be54431`;
`SHA256SUMS` SHA256
`605086ec6dc8ecf866b6f8d8ef543e5b5dd63621884dbea3ab1de471c9126813`.
Earlier development failures and signing interruptions are retained/labeled;
final tests passed on the unchanged committed source.

Next: review this handoff source, finish Sophia protected launch configuration
and real import-refusal/delayed-release controls, then separate diagnostic
release/link receipts and an attended sealed package. Actual server shutdown,
second-GPU exclusion, authenticated capture and outer cleanup remain runtime
qualification requirements. No timed comparison is opened by these checks.

## Sophia diagnostic configuration and handoff source — October 30

Signed local niltempus `8c527e8bcb142ca534b4bb41717b8d93468733e0`, parent
`fb21242f9`, adds `sophia_recipe.py` and connects it to handoff staging and
terminal cleanup checking. Signature G and clean tree verified; the committed
diff matches the saved candidate. No host, SDK or peer rendering source changed.

The fixed schema-2 core contains the trusted-host shell-timing catalog and one
developer-launch observer recipe restricted to `pci-0000:03:00.0/DP-1`, with
no writable destination. The desktop profile enables host-admin control,
excludes GPU 16:00.0 and runs one GPU-direct bar. The peer is three fresh GPU
DMA frames, height 24, no profiling/reuse/W2. Artifact pins include the host,
peer, bwrap, WM, factotum and PAM helper. All generated bytes and pins are
rechecked before preflight or launch.

The explicit run entry requires an unprivileged text-VT context, filters the
environment and installs interruption handlers for the existing wrapper's
cleanup. Session and the hold driver remain siblings; no namespace is entered
after launch. The host limit is 280 seconds within wrapper/PAM/outer
300/360/420-second bounds. The same unsealed handoff now pins the Sophia
configuration files. Cleanup additionally checks the driver's terminal state,
three-capture record, host argv and unforced matching exits. This does not
replace the capture pixel/custody audit or native admission proof.

Final checks: **9 recipe, 8 handoff and 10 hold tests**, **8 killed/restored
mutations**. The available diagnostic debug host `94c52141` accepts the real
generated core/profile and private-delivery argument; five controls refuse
(invalid control mode, observer profile, writable-destination key, missing
native opt-in, relative capture directory). Strace shows no DRM opens and the
capture directory is not created. `/usr/bin/true` is a non-launched peer
stand-in in this grammar fixture. It is not a native-ready recipe or release
provenance, and must not be reused as one.

Receipts: `~/.local/state/matched-gpu-sophia-recipe-01/`, **48 entries verified**.
`RECEIPT.json` SHA256
`25f468c4ad635bd086ff15ed2c6967c9aa225e7c4b5ef630c9e73c6e8bbdd5f7`;
`SHA256SUMS` SHA256
`1d8b949ea0cdf21a3d5fe21e282919b3b2ae98364c4f08a8b491931bcff86e51`.
No Cargo/release build, GPU device open, PAM/VT/native launch, package, merge
or push. Frozen W2 and CPU evidence remains unchanged.

Remaining before sealing: actual 119.880 Hz requires the credential-bound
output-environment receiver and live readback, not the nominal 120000 startup
row. No readback stream is enabled without a receiver. Also retain the real
import-refusal/delayed-release controls, signed diagnostic host feature and
release/link closure, source review and attended protected-launch audit.
Argument acceptance alone does not prove experimental-shell-capture is built;
the package must bind all three required host features in its build receipt.

## Connections

- [Critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md).
- [Producer comparison and CPU result](iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md).
- [Owned GPU capture qualification](../plans/h38nf8bk-qualify-gpu-native-shell-images-through-owned-capture.md).
