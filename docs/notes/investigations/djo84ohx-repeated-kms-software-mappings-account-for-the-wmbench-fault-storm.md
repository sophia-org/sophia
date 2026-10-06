---
id: djo84ohx
date: 2026-10-05
kind: investigation
status: investigating
tags: [investigation, rendering, validation]
---
# Repeated KMS software mappings account for the wmbench fault storm

## Question

Why does Sophia still incur about 570,000 minor faults over the fixed 300-frame
wmbench workload after retaining SHM attachments and sharing CPU patch bytes?
The [t289 plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#t289)
owns this work. This investigation concerns the QEMU software path; it does not
establish the cost of Kitty's DMA-BUF path on a hardware GPU.

## Evidence

`development-evidence/t289-framebuffer-faults-01/PAIR-03-RESULT.txt`
(SHA256 `aeb4be2c6459417b2797b911e3db43d4fc8999183b82bf73f76cdbc854edef2f`)
records one Sophia then one XLibre guest, both passing the controls and completing
the unchanged workload. Its checksum receipt covers 579 files. The first two
attempts failed collector preflight and remain failures, not measurements.

The harness was wmbench `59d9af6`, benchmark package `47fdb6b`, Sophia
`825d91460`, XLibre `9d03c0a6`, and nixpkgs `c59305ba` with Mesa 26.2.3.
The VM had one 1280×800 virtio-vga 2D output, four vCPUs and 3072 MiB.
Both guests first passed retained-mapping, same-address-remap and
`MADV_DONTNEED` controls. Fault samples and mapping syscalls shared one clock;
mapping history was checked throughout the interval, not just at its endpoints.

| Recorded quantity | Sophia | XLibre with xfwm4 |
| --- | ---: | ---: |
| Completed workload frames | 300 | 300 |
| Process minor faults | 569,892 | 15 |
| In-window fault samples | 2,208 | 0 |
| `mmap` calls | 603 | 0 |
| `munmap` calls | 613 | 0 |
| Desktop CPU, seconds | 2.14 | 0.23 |

The syscall recording extends slightly beyond the accounting interval. CPU
includes diagnostic overhead and this single pair is **not** a new accepted
performance ratio. Neither arm incurred major faults; these are page-table
first-access faults, not disk reads.

Every Sophia fault sample joins to a successful unmap, a new mmap, and exactly
one matching runtime MMAP2 record. The samples span 601 mapping generations of
three `/dev/dri/card0` buffers, each 4,259,840 bytes. The initial and final maps
are identical despite the intermediate churn. All 1,187 samples with an unwound
stack fault in Mesa's `util_fill_rect`; 1,021 samples have no stack and remain
unattributed beyond their address and thread.

Sophia's whole-session counters show three target, pipeline and frame-surface
creations, 430 target reuses, and zero snapshot captures or imports. This is an
SHM workload. Retained target objects do not imply retained CPU mappings.

## Finding and resolution

The Mesa package path matches the library in the sampled stacks. Release-tag
source shows `lp_scene_begin_rasterization` mapping color buffers and
`lp_scene_end_rasterization` unmapping them. For these display targets,
`llvmpipe_resource_map/unmap` forwards to the KMS software winsys.
`kms_sw_displaytarget_unmap` calls `munmap` when its logical map count reaches
zero. The next scene maps the same backing buffer and faults its pages again.
That source mechanism matches the syscall history and thread roles. Mapping
syscall stacks were not collected, so this is source-supported attribution.

XLibre's server log explicitly rejects llvmpipe for glamor and takes its mapped
software framebuffer path. Its modesetting code retains that mapping with the
buffer until destruction. The two clients reporting llvmpipe does not mean the
two display servers use the same composition path. The guest gap cannot be
applied directly to native GPU performance or laptop battery use.

The rejected clear-coverage candidate remains rejected: its four CPU pairs
regressed, while minor faults stayed nearly constant. The new finding supports
testing mapping lifetime before removing another clear; it does not retroactively
prove why that candidate regressed. Evidence: `t289-clear-coverage-01`.

## Validation and remaining work

Proceed with a Mesa-only measurement candidate, keeping logical map counts and
locking while retaining owned dumb-buffer mappings until final destruction.
Caller-supplied `displaytarget_create_mapped` memory must remain borrowed.
Imported buffers, read-only/write mappings, shared planes, errors and cleanup
need explicit coverage. There is no Sophia snapshot or renderer redesign here.

First verify the locked source and mapping lifetime controls. Then compare one
unchanged Sophia workload with retention off and on, checking actual loaded
libraries, completed frames, pixels, mapping calls, faults and CPU. An
unprofiled before/after result must establish savings before any promotion.
No live install is part of this experiment. Candidate evidence belongs in
`development-evidence/t289-kms-map-retention-01`.

The source review used files fetched from the official Mesa 26.2.3 tag. The
first full Nix source fetch stalled and was cancelled; its complete NAR was not
verified. Candidate construction must realize the locked source normally.
This limitation and the file hashes are in `25-source-review.json` beside the
pair-03 report. No saving from mapping retention has yet been measured.

## Connections

The [SHM and patch results](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#shm-upload-costs-in-the-wmbench-guest-2026-10-05)
explain the accepted reductions leading to this baseline. The remaining task
stays [t289](../../../todo.md); this note does not create a separate queue.
