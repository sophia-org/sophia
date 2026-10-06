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

The initial source review used files fetched from the official Mesa 26.2.3 tag.
Its first full Nix source fetch stalled and was cancelled, leaving the complete
NAR unverified at that point.
This historical limitation and the file hashes remain in
`25-source-review.json` beside the pair-03 report. The candidate and its measured
result follow below.

### Candidate construction, 2026-10-05

The complete source is now verified. The Mesa mirror archive produces exactly
the locked NAR hash `sha256-vhoX4anFe68PNpkOsdtme1fnGSCmKit+dyNYq6ox9AM=`
and store path `/nix/store/6pnvm1jkh0a144pmcsbkykq5crhn695a-source`.
Receipt: `t289-kms-map-retention-01/04-source-verified.json`. This resolves the
source limitation for the candidate; the cancelled fetch remains recorded.

wmbench measurement branch `measurement/kms-map-retention`, commit `18fb200`,
contains the default-off Mesa patch, an actual-winsys lifetime fixture, and the
guest loader and pixel checks. Lifetime checks cover nested and read-only maps,
map failure, borrowed/imported memory, shared handles and destruction churn.
Four negative controls fail their named assertions; 45 harness tests pass.
The software-only package initially failed configuration because the VA video
frontend needed a hardware driver. Successor `69bda56` disables that frontend;
the driver and VM build pass, including the lifetime fixture compiled with
Mesa's actual flags. Failed and cancelled build attempts remain in the evidence.

### Measured result, 2026-10-05

The diagnostic OFF/ON pair confirms the mechanism. During the same 300-frame
workload, minor faults fall from **568,893 to zero**, and framebuffer mapping
cycles from **602 to zero**. These counts exclude startup first-touch work.
Diagnostic CPU is 2.25→1.53 seconds, including profiler overhead; the unprofiled
pairs below provide the CPU result.

Both arms use one Mesa binary and the unchanged Sophia binary. Every guest
checks the loaded libraries and switch, 48 pixel-exact helper frames through
three contexts/surfaces, and no helper DRM mappings after destruction. The
benchmark client retains its original loader settings. The diagnostic controls
for retained mappings, remapping and explicit page discard pass in both arms.

Four unprofiled pairs ran in the fixed order OFF/ON, ON/OFF, ON/OFF, OFF/ON:

| Pair | OFF CPU, seconds | ON CPU, seconds | Reduction |
| --- | ---: | ---: | ---: |
| 1 | 2.05 | 1.41 | 31.2% |
| 2 | 2.05 | 1.44 | 29.8% |
| 3 | 2.01 | 1.42 | 29.4% |
| 4 | 2.03 | 1.41 | 30.5% |
| Median | 2.040 | 1.415 | **30.6%** |

Every pair improves, with 300 completed frames each. Median CPU per frame is
6.80→4.72 ms. Elapsed medians are 39.2→39.3 seconds, with overlapping ranges;
this is a CPU reduction, not a throughput or latency qualification. Two ON
guests log a 100/101 ms soft-stall warning near their initial content frames.
Those warnings are kept; no worker failure or hard stall occurred.

Whole-session work matches across all eight guests: three targets, pipelines
and frame surfaces; 429 target reuses; 432 worker requests and completions;
421 exact-nearest draws; zero captures or imports; no leased slots at exit.
All ten guests passed without replacement. Evidence:
`t289-kms-map-retention-01/RESULT.txt`, `diagnostic-01/RESULT.json`,
`comparison-01/RESULT.json` and `28-resource-check.json`.
The report SHA256 is
`7031c486c888de2471caf60672cb40195a306a1ee483affeeb0a842812952360`;
its receipt verifies all 1,562 retained evidence files.

This is a demonstrated saving in the **QEMU software rendering path**. It does
not measure Kitty DMA-BUF capture, radeonsi, or laptop battery consumption.
XLibre was not rerun in this slice, so no new cross-server ratio is claimed.
The candidate stays on the local wmbench measurement branch. A wider Mesa
change needs review of real-device lifetime, resize, suspend and reset behavior.
No live driver, installed desktop or Sophia production code changed.

### Lifetime qualification and opt-in package, 2026-10-05

The follow-up is published on wmbench
[`qualification/kms-map-lifecycle` at `51488da291`](https://github.com/sophia-org/wmbench/tree/51488da291aaafc8e666ab95d56af9a7062010e5/packaging/mesa).
It exposes `.#mesa-software`, `.#mesa-lifecycle-vm` and a device-free lifetime
check. The Mesa patch is byte-identical to the measured candidate and remains
off unless `MESA_KMS_SW_RETAIN_MAPPINGS=1`. Ordinary benchmark and Sophia
packages keep their original Mesa. This publishes an opt-in software candidate;
it does not enable the change in the desktop.

The source audit checks owned versus borrowed/imported storage, shared plane
offsets, logical map counts, separate read/write mappings, final-reference
cleanup and kernel refusal after retention. Every logical map still asks the
kernel for permission. Retention adds at most two VMAs per owned backing object,
lasting until destruction; it does not add a global memory bound.

Actual patched-winsys tests pass on the host, under ASan/UBSan and in the Nix
sandbox. Two additional mutants fail named lifetime assertions. The full
software package builds; the final clean-head `nix flake check` passes, including
49 Python tests. The rebuilt Gallium library has the same `.text` hash as the
measured one, with a different full ELF hash. No new performance claim follows.

The final cleanup pair (`guest-07`, fixture `1256ca815`) passes OFF and ON with
both VM exits zero. Helpers check 12 resizes while holding a previous front
buffer, destruction after guest PCI/virtio driver removal, and exact pixels
after rebind/reopen. Every helper leaves zero DRM mappings. These are
software/virtio device checks with no host GPU passed through.

**Suspend remains unqualified.** The OFF deep-sleep control rebooted; the OFF
suspend-to-idle control did not return within 240 seconds. Neither reached ON.
Earlier device-selector, test-expression closure and virtiofs setup failures
are kept. `guest-06` also stays FAILED: both guest payloads passed, but ON timed
out in ordinary Nix shutdown. Its premature END and attempted termination of
the previous OFF process are recorded. The final fixture syncs evidence and
powers off directly after object cleanup, as the existing benchmark does.
Both final results explicitly say `suspend_tested=false`.

Evidence: `t289-kms-map-lifecycle-01/RESULT.txt` (SHA256
`fe05e2a71a25ac4da0b2bfe3c4e8a174f0a3b04f264ebfda62af6e453709a4db`),
`guest-07/RESULT.json`, `16-mutants.json`, `24-package-identity.json`, and
`41-final-check.log`. The package's `REVIEW.md` records the source review and
distribution boundary. Wider enablement still needs a working OFF sleep
control and matching ON result, plus qualification on the intended device.
The established 30.6% saving remains limited to the measured software workload;
t289 stays open and no live install was made.

## Connections

The [SHM and patch results](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#shm-upload-costs-in-the-wmbench-guest-2026-10-05)
explain the accepted reductions leading to this baseline. The remaining task
stays [t289](../../../todo.md); this note does not create a separate queue.
