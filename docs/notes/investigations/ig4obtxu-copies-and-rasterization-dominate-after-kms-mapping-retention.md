---
id: ig4obtxu
date: 2026-10-05
kind: investigation
status: investigating
tags: [investigation, rendering, x11]
---
# Copies and rasterization dominate after KMS mapping retention

## Question

After retaining Mesa's software framebuffer mappings, what accounts for the
remaining desktop CPU cost? Does the evidence justify SIMD or assembly work,
or is there still unnecessary work to remove?

**Start with the owned raster command's journal insertion.** It receives an
owned command but clones its pixel payload before storing it. Reordering the
last borrows should let it move that command instead. This is a source-backed
candidate in a measured hot path; its CPU saving has not been measured.

## Evidence

One Sophia profile followed by one unprofiled control completed on October 5
local time. Both used the unchanged wmbench `47fdb6b` bundle, Sophia
`825d91460`, and the measured Mesa mapping-retention candidate enabled in both
arms. The host was logged out at greetd, with other development work paused.
Each guest had four CPUs, 3072 MiB and a virtio 2D GPU running llvmpipe. The
1160×680 SHM client ran 120 warmup and 300 measured frames on a 1280×800 output.
No host GPU was passed through.

| Run | Desktop CPU per 300 frames | Elapsed |
| --- | ---: | ---: |
| Profile | 1.47 s | 39.0 s |
| Unprofiled control | 1.42 s | 38.9 s |

Both guest and host wrappers exited zero, both passed compatibility and the
48-frame Mesa pixel/cleanup helper, and loaded binary identities matched.
The control agrees with the earlier ON range of 1.41–1.44 seconds. One pair
does not establish repeatability or a stable profiler overhead. The workload
name says “render 60 fps”; achieved throughput was about 7.7 fps in this guest.

The profile recorded **zero minor and major faults** over the measured desktop
interval, with 1.38 seconds user CPU and 0.09 seconds system CPU. There were
713 CPU-clock samples at a 2 ms period, none lost: 1.426 seconds of sample
weight against 1.47 seconds of process accounting.

| Thread group | Samples | Share |
| --- | ---: | ---: |
| llvmpipe raster workers | 307 | 43.1% |
| X11 client worker | 188 | 26.4% |
| Session owner | 159 | 22.3% |
| Renderer group worker | 59 | 8.3% |

These groups partition the samples. Symbol costs below overlap them:

- `memmove`: **233 samples, 32.7%**, comprising 170 on the X11 worker,
  43 on the owner and 20 on the renderer worker.
- Owner `memset`: 23 samples, 3.2%.
- Mesa `util_fill_rect`: 49 samples, 6.9%.
- Unresolved Mesa JIT code: 209 samples, 29.3%.
- Kernel code: 48 samples, 6.7%.

Whole-session work matches between the two runs: three targets, 429 target
reuses, 432 worker requests and completions, 421 exact-nearest draws, zero
snapshot captures/imports and zero COW splits. Those counters include startup
and warmup. They must not be divided by the measured 300 frames as though they
cover only that interval.

## Finding and resolution

### First candidate: move the command already owned by the journal

At the measured source, in
[raster_variants.rs](../../../crates/sophia-x-authority/src/software/raster_variants.rs):

- `XOwnedImagePixels` owns a `Vec<u8>` and derives `Clone` (lines 134–138).
- `from_put_image` retains the accepted pixels with `to_vec()` (line 231).
- `SurfaceRasterStore::record` takes its command **by value** (line 609),
  then pushes `command.clone()` in both accumulation and replay paths
  (lines 660 and 676).

The proposed small change is to finish coverage calculation or variant replay
through `&command`, then move the command into the journal after its last use.
This should eliminate one payload allocation and copy for a retained PutImage
without changing the payload type or upload interfaces. Every validation,
budget, poison, coverage and baseline-reset decision must stay intact.

The profile supports prioritizing copies, but does not isolate this clone's
share. Only seven X11 copy callchains recovered a caller: three reach
`from_put_image`, two reach SHM dispatch and two reach `packed_patch_region`.
One owner copy chain reaches `compose_layer_clipped`. The measured Sophia
binary contains a `memcpy` call inside `from_put_image` at ELF address
`0xdf0732`, confirming that copy survived optimization. No percentage saving
is assigned to the proposed journal move.

### Follow-up: share immutable upload ownership where it is safe

If the small slice is useful, inspect the next ownership boundary. SHM dispatch
already holds an owned normalized upload, but passes a slice through drawing;
`from_put_image` then allocates another retained buffer. Separately,
`SurfaceRasterStore::satisfy` clones the journal when staging an atomic change
to retained density variants (line 744).

An immutable owned payload shared across those retained readers could remove
further copies. The initial client-memory snapshot remains necessary. Shared
bytes must never alias mutable client or canonical drawable storage. Preserve
format conversion, crop/stride checks, byte order, graphics-context semantics,
journal budgets and atomic requirement satisfaction. This is a separate,
broader candidate; do not combine it with the first measurement.

The earlier whole-image crop candidate remains inconclusive and unmerged.
The accepted shared CPU-patch forwarding already avoids queue-to-queue copies;
patch packing and applying rows into the destination are distinct remaining
operations. Zero COW splits here gives no reason to weaken snapshot isolation.

### SIMD is already used in the largest named copy routine

All sampled copies use glibc's `__memmove_avx512_unaligned_erms`. Disassembly
at sampled instruction offsets shows ZMM vector loads and stores
(`vmovdqu64`/`vmovdqa64`). Replacing this with handwritten SIMD is not the
first candidate. There are no hardware bandwidth, cache-miss or IPC counters
in this capture, so these instructions do not prove memory-bandwidth saturation.

llvmpipe is the largest thread group, but its generated routines lack symbols
in this binary. Mesa's `lp_profile` symbol/assembly dump is compiled behind
`PROFILE`; its identifying dump-path and environment strings are absent from
the measured Gallium ELF. A later renderer investigation needs a separately
qualified diagnostic build with JIT symbols before choosing a specific loop.

`util_fill_rect` uses `rep stos` and library fills in the measured binary.
Its 6.9% share is an upper cost bound, not a forecast for vectorization.
The last periodic record attributes 419 full repaints to the coverage decision;
the client covers 77.0% of the output. This is not evidence of an incorrect
damage decision. The measured clear-coverage candidate remains rejected.

## Validation and remaining work

Next slice under t289:

1. Implement only the owned-command move. Test allocation identity in both
   journal paths, exact variant replay, full opaque baseline resets, partial
   coverage, budget/unsupported refusals and independence from later writes.
2. Run the focused tests and required full gate on the frozen candidate.
3. Compare against this accepted Sophia baseline with the **same Mesa ON in
   both arms**, unchanged client bundle and frame count, and four balanced
   pairs (BC CB CB BC). Keep one attribution profile per arm. Coordinate the
   quiet window; stop on a failed run and keep it.
4. Apply the existing requirement that every pair improves, with no pixel,
   lifecycle or work-count regression. If the change is within noise, retain
   that result and move to the next measured cost. Source simplicity alone
   does not establish a performance gain.

No new implementation was made for this profile. Reviewed production crates
on master `a7e4aedf9` are identical to measured `825d91460`.

Limits: 468 samples have no callchain and 54 have only one frame; the remaining
191 have multiple frames. Per-callsite copy percentages are unavailable.
This is the **SHM/software guest path**, with no DMA-BUF captures. It does not
measure Kitty's hardware path, whole-machine energy or laptop battery life.
XLibre was not rerun, so no new cross-server ratio follows.

Evidence: `~/.local/state/sophia/development-evidence/t289-remaining-hotpath-01/`:
`READY.json`, both guest trees, `ANALYSIS.json`, `SOURCE-RECEIPT.json`, raw
`perf.data`, self/callchain reports and disassembly. `python3 analyze.py`
rechecks identities and recomputes the attribution. `RESULT.txt` SHA256:
`584dbc7b9b05e444e24aeac9d59ff0004d7c40c6f45a6e2bd7a3d9c857998232`.

## Connections

The [framebuffer mapping investigation](djo84ohx-repeated-kms-software-mappings-account-for-the-wmbench-fault-storm.md)
establishes why this profile uses the opt-in Mesa candidate. Its 30.6% software
CPU saving and remaining device/suspend limits still stand. The
[CPU plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#remaining-hot-path-after-mapping-retention-2026-10-05)
owns this next experiment, and [todo t289](../../../todo.md) remains the queue.
