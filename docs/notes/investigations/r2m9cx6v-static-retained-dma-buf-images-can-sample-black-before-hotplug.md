---
id: r2m9cx6v
date: 2026-10-05
kind: investigation
status: investigating
tags: [investigation, rendering, dmabuf, synchronization, validation]
---
# Static retained DMA-BUF images can sample black before hotplug

## Question

Why can a static client image render correctly, then sample as black in a
later retained composition without another client Present? The symptom occurs
on master before any output removal. It needs a separate investigation from
the t306 hotplug repair; its cause is not established.

## Evidence

Evidence paths are under `~/.local/state/sophia/development-evidence/t306-01/`.
The bounded comparison `45-startup-comparison` used four master f87184d20
boots and four candidate e142593fe boots, in order M C C M M C C M. Both
images used f6865c946 tools, the same kernel and libraries, one virtio GPU
with two heads, virgl on host renderD128, and the no-WM static DRI3 client.
The client submitted one LINEAR XR24 400x300 frame, then held it unchanged.
The images differed only in Sophia and dracut source-path metadata.

The frozen first-region outcome was master content 4/4; candidate content
3/4, black 1/4. Keep that outcome unchanged. Four runs per arm establish
neither a failure rate nor a causal effect of t306.

Later reads reveal more than the first-region summary:

- `05-master.log`: line 61 has 120000 nonzero pixels and checksum
  `15913682524319544229`. Line 85 reads the same window region as all black,
  checksum `8572701038929191205`. Frame 6 retires at line 87. The removal
  command is later, at line 90.
- `02-candidate.log`: first region correct at 63, black retained composition
  at 86, black frame retired at 88, removal at 93. Regions 103 and 122 have
  the correct checksum after restoration into successive owners.
- `06-candidate.log`: first region correct at 62, black at 86, black frame
  retired at 88, removal at 91. Regions 101 and 120 are correct after restore.
- `07-candidate.log`: the first region is black at 71; readiness never
  completes, and no removal occurs.

The post-removal correct regions in 02 and 06 precede matching bootstrap
compositions and topology publications. The original verifier chooses the
last pre-removal region as its reference, which is black in those runs, so
it rejects the restored correct content as a mismatch. Their original
FAILED results remain. This reference problem does not dismiss the black
composition before removal.

`SERIES.txt`, `IDENTITIES.txt`, `ANALYSIS.txt` and
`REVIEW-CODEX-14-comparison-disposition.txt` retain the scope and source
anchors. Earlier black-startup observations are in 37, 41 and 44; the
identical-pins repeat in 38 started with correct pixels. The mixed fixture
history is not a matched frequency estimate.

## Current limits

Master demonstrates the later correct-to-black symptom without t306. This
does not prove that every black first frame has the same cause, or that
t306 cannot change its frequency. The region records identify the same
logical retained source; they do not establish the exact texture, storage
or EGL context used by each render. Guest renderer readback is evidence
of composed pixels, not physical scanout. No equivalent live-hardware
failure has been established here.

## Narrowing to the reused slot

Series 53, 55, 57 and 60 traced ten first client draws. Every black one
was composition 7 in frame slot 0, at buffer age 1, into the buffer that
slot's empty composition 1 had drawn. Every correct one was drawn into a
fresh buffer of a newly created target. Series 60 repainted the reused
buffer in full with damage disabled and still read black twice, so a
partial repaint is not required. Focused device tests did not reproduce
it. In series 59 a captured image drew correctly into a reused age-1
buffer on the host GPU and under virgl, with the capture made before the
slot was warmed. In series 61 the capture came after slot 0 was warmed and
the image, still staged, was drawn at once into that pre-existing target:
buffer age 1, target generation unchanged, no target created. It read
exact before and after the swap, in both orders and on both machines
(`61-late-capture/RESULT.txt`, `guest.log` lines 29 to 35). QEMU crashed
on teardown after the guest had written its result in both series; that
fault is in the harness and leaves the guest observations intact.

The setup gate on the t306 branch holds the probe's pixmap import, its
MapWindow and its Present until Session records release each one. With
it, the client's first draw reaches the reused slot-0 buffer at age 1 in
every run, through the composed path rather than the original paced one.
In pair 68, a readback of the capture destination held the exact client
image while the output region of the same draw read black.

Pair 77 also read the imported texture in the output context, through a
temporary read framebuffer, just before the draw. Image `0b861919`, source
`2d5a73cd9`. The comparator boot without either read reproduced the black
region. In the boot with both reads, `77-on2-pair/boot2.log` lines 106 to
116 chain one occurrence: the capture destination for allocation 2,
generation 1 held 120000 nonzero pixels, checksum `8975981465688749989`;
the copy completed before first use; texture 3, imported fresh in the
slot-0 context, read back the same image with the same checksum; the layer
was reported drawn; the window region then read black, checksum
`8572701038929191205`, and the whole 1280x800 frame read black before the
swap. Both reads are interventions on the run they observe.

The discrepancy therefore lies after the imported texture's contents: in
sampling, in the draw, or in the output target. A framebuffer read does
not prove that sampling of the texture is complete or correctly
configured. A reading of the source finds no GL state that differs between
the warmed slot and a fresh one at the draw. The draw sets its viewport,
blend, scissor, texture unit and binding, filter, vertex buffer, program,
uniforms and attribute arrays itself. Output contexts keep the default
framebuffer bound for drawing and reading; the diagnostic import read
binds a temporary read framebuffer and restores the prior bindings before
the draw. No output context changes its colour mask, depth, stencil,
culling or sampler objects. The focused tests reproduced the reused age-1
buffer and the late capture into an existing slot under virgl, and drew
correctly. What only Session has remains open: the client's DRI3 buffer
written by another process, capture and lowering inside one mixed export
on the renderer worker, two heads composing concurrently, KMS scanout and
retirement of the reused buffer before its reuse, and the slot's startup
history.

## Colour controls on the reused target

Series 89 observed the original draw with the draw-state diagnostic. In
its black boot, the compared GL API state was complete with no mismatch,
and the draw passed 120000 samples for the 400x300 window. Neither that
count nor API state establishes the colour written to the target.

Series 95 used source `ed5ed064e`, image `a0714e87`, and the frozen
`classify10` recipe. The OFF boot reproduced black in reused slot 0, age 1.
The second boot repeated that result, then ran two production operations
in the same context and target after the original region, frame and sample
records: a CPU-texture draw of an independent pattern, then a scissored
solid clear of `0x2080e0`. Each was followed by its own region read.
Both reads returned 120000 black pixels, checksum `8572701038929191205`,
with no GL error. Expected checksums were `13356461631286439117` for the
pattern and `16104153671652024613` for the solid. The classifier reported
`BLACK_EXPECTED_SAMPLES_CLEAN_STATE colour=TARGET_WRITES_DIFFER`.

The original whole-frame read also reported black and carried the same
output 1, head 1 and composition 7 identity. Both boots reached the composed
setup-gate path and ended with the recognized startup-not-ready failure.
The host runner completed, but neither guest was a successful Session.
Evidence and hashes are in `t306-01/95-dc-pair`; no replacement run was made.

The identical frozen host test in `93-colour-control-device` read both
control oracles exactly on radeonsi, verified the solid pixels in the
exported reused buffer, and then verified the original pattern in a fresh
buffer. That was offscreen correctness with no real head trace identity.
It does not clear the virgl Session path.

These observations make the shared target and readback path the next
focus. A failure limited to snapshot texture sampling cannot explain the
later solid-clear read by itself. However, black readback does not prove
that either operation failed to write: it may read stale or different
storage. The controls ran after synchronization and under the state the
original left; C2 inherited C1's state. There is no driver-defect finding.
The intervened output and its later frames cannot qualify hotplug or
image continuity, and there is no performance claim.

The proposed discriminator after series 95 was an independent observation
of the exported buffer after the existing swap and front-buffer lock. The
source review first needed to establish whether the existing DMA-BUF
import probe could read that exact buffer through a private framebuffer
without CPU mapping, changing its lease, or disturbing the producer
context's cleanup. A solid result there
would disagree with the earlier default-framebuffer read; a black result
would leave writes, buffer selection and export visibility open. This was
a source/design task at that point; the subsequent controls follow below.

## Export read and its handle-lifetime hazard (2026-10-08)

Series 101 used source `4c717a475` and image `f4cc3541`. Its OFF
comparator reproduced black. The intervened boot again read black after
the original draw and both colour controls, then imported the exported
allocation through a separate probe. The export read also found all
120000 window pixels black and no nonzero RGB pixel anywhere in the
frame. These two views agree; they do not show a default-framebuffer
read/visibility discrepancy. Writes, target selection, export and
visibility remain open.

Immediately after the probe, frame 7 could not create a KMS framebuffer:
all three AddFB forms failed, and the Session exited with
`native_frame_service_failed`. No errno was recorded. This differs from
series 95, which had the same colour controls without the export probe.
The intervened frame cannot qualify t306, and the probe is held out of
Sessions. Evidence is in `101-export-pair/RESULT.txt`.

The source and binary review in `102-export-addfb-source` and
`103-handle-lifetime/provenance` exposed a flaw in the probe design. In
Mesa 26.2.3 virgl, the winsys table hashes the integer fd, even though its
equality function compares open file descriptions. Lookup uses the caller
fd, while insertion uses a duplicate. A checked duplicate therefore does
not establish shared Mesa ownership. Separate winsys objects can import
the same kernel GEM handle on the shared DRM file; dropping the probe can
then close a handle still cached by the producer. Sophia passes that
cached handle to AddFB without reimporting it. This is a source-consistent
explanation of the new failure, not an observed close sequence. The
original same-description safety claim has been corrected in the evidence.

Two ignored tests separate the handle observer from the lifetime claim.
The observer only exports existing GEM handles; it never imports a
DMA-BUF and cannot repair a missing handle. Its own sacrificial allocation
proves that live handles name their buffer, missing/closed handles return
ENOENT, and a reused number names the new buffer. The lifetime test holds
a producer allocation and checks its DMA-BUF identity before probe
construction, after the read while the probe is alive, and after drop,
without allocating another producer buffer between checks.

On host amdgpu, series 104 qualified the observer, including numeric
handle reuse. Series 105 preserved the producer handle through probe
drop. These are render-node controls for that host driver; they do not
exclude the predicted virgl loss. Neither test proves the Session age-1
case or the primary-node/KMS submission path.

## QEMU teardown blocks the virgl lifetime control (2026-10-08)

Series 107 ran the frozen observer under virgl on radeonsi, Mesa 26.2.3.
It passed, including a refilled handle 1 that named a different DMA-BUF.
After the guest recorded `test_exit=0` and powered down, QEMU died with
SIGSEGV. The host endpoint recorded `qemu_exit=139 logger_exit=0`; the
harness failed and the series correctly stopped before the lifetime
guest. The runner exit 0 only records completion of that stopped series.

The virgl lifetime result is therefore still unknown. QEMU teardown also
crashed in device-test series 59 and 61. This is an infrastructure failure,
separate from the successful guest observer and from the Session black
frame. Existing passing guest observations are retained; none of these
crashed harness runs becomes a clean pass. Sixteen earlier virgl
output-unplug logs record QEMU exit 0; the teardown failure is not
universal across these recipes. The difference remains unexplained.

The next step is one bounded debugger run of the unchanged observer image
to locate the host fault. The wrapper must keep guest output separate
from debugger records, distinguish an inferior signal stop from the
debugger exit, and prove cleanup on timeout and wrapper death using CPU
controls first. QEMU 11.1.1 destruction order is a source lead: its
`egl_cleanup` destroys the GBM device before releasing the thread and
terminating EGL, whose Mesa display borrows GBM resources. The still-live
virgl sync thread is another candidate. Neither is a located cause.
A run that does not crash under the debugger would be a nonreproduction,
not a repair. Designs and source review are in
`108-qemu-teardown-design` and `109-qemu-teardown-source`; the source
review correction distinguishes the release from later upstream changes.

## Located teardown fault and private QEMU comparison (2026-10-08)

The debugger run in series 111 located the fault on QEMU's main thread,
inside Mesa during `eglTerminate`: `libgallium+0x5c7f37` dereferenced a
null table-entry array. The virgl sync thread was idle at that stop.
Exact QEMU and virgl debug build IDs supplied their function names.
Mesa 26.2.3 debug files were unavailable; the Mesa names in series 112
come from matching instruction windows to 26.2.4, not substituting its
addresses. Screen/shader-cache destruction remains a source-consistent
mechanism rather than a demonstrated identity of the faulting table.

Two private QEMU 11.1.1 binaries differ by upstream `baca25172d8c`, which
reorders thread release, EGL context/display destruction and GBM teardown.
Neither binary was installed. Series 117 stopped before boot because
the relocated binary could not find its BIOS. Series 118 supplied a
private data layout and verified both arms with firmware lookup and a
paused CPU-only startup. That did not qualify virgl teardown.

The frozen series 120 then ran B, P, B, P under the same debugger wrapper:

| Guest | Build | Result |
| --- | --- | --- |
| 1 | Unpatched B | Observer passed; exact main-thread fault; QEMU exit 139. |
| 2 | Patched P | Observer passed; inferior, debugger, wrapper and harness all exited 0. |
| 3 | Unpatched B | Same observer result and exact fault site/chain as guest 1. |
| 4 | Patched P | Observer passed, but gdb did not obtain the process exit status; invalid. |

Both B faults joined the loaded binary identity and build IDs through
`libgallium+0x5c7f37`, `libEGL_mesa+0x12a47` and the private QEMU's
`egl_cleanup` return at `+0x71fc4c`. Guest 4 reported an unknown stop,
then debugger cleanup and wrapper exit 125. It is neither a clean P
result nor an observed matching fault. The declared bound ended with
no replacement and no remaining QEMU or debugger process.

The first pair supports the upstream cleanup-order patch on this private
build and stack. The patch changes several ordering edges, so this does
not uniquely establish GBM-before-Terminate as the mechanism. The second
P is inconclusive, and gdb changes timing. No installed-package repair,
performance result, Session black-frame repair or t306 acceptance follows.
The virgl lifetime guest remains held. The next step is to understand the
lost debugger endpoint and propose a bounded qualification without
weakening the clean-exit requirement.

Evidence: `111-qemu-gdb-observer`, `112-mesa-frame-names`,
`113-private-qemu`, `117-qemu-bp-series`, `118-qemu-bp-layout`,
`119-qemu-bp-package-3` and `120-qemu-bp-series`. The series 120 manifest
is `f437b3378e669bd555b8ea42875b344b3529c230aa8713f5ece3bad8e9c3ea34`.

## t307

1. Trace the successful mixed Present and subsequent retained composition:
   source buffer, snapshot identity and storage, texture/import identity,
   EGL context and completion dependencies. Distinguish GBM producer writes,
   snapshot capture and later sampling before choosing a repair.
2. Propose the smallest bounded control that separates a named hypothesis.
   A source readback, explicit synchronization or second Present changes
   execution and must be recorded as an intervention, not substituted into
   the original evidence. Do not continue general screening unboundedly.
3. Make verifier diagnostics distinguish an unstable pre-removal baseline
   from failed preservation. Use the expected submitted pixels as the
   reference. A run with contradictory baseline pixels still fails overall;
   post-loss content can be reported separately with owner/frame proof.
   Preserve original logs and verdicts when adding corrected analysis.
4. Repair the demonstrated lifetime or synchronization defect and require
   a regression that fails without the repair. Retain snapshot immutability,
   source release, bounded storage and per-output ownership guarantees.

The task is high priority because pixels can disappear without a new client
frame, and this currently obscures hotplug qualification. t306 need not
absorb its root-cause repair, but failed or unstable runs are not acceptance
of the still-unproved managed-head loss and all-return cases.

Task state and execution order live in [todo.md](../../../todo.md).

## Connections

- [KVM hotplug recovery](kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md)
  exposed this separate symptom while testing static DMA-BUF preservation.
- [CPU reduction plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md)
  includes the published snapshot reuse work; no causal link is assumed.
