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

## Both private teardown pairs discriminate (2026-10-08)

Series 125 repeated B, P, B, P with an independent kernel exit witness.
The watcher held a pidfd bound to the loaded executable's PID and start
time, then read `PIDFD_GET_INFO` after exit. CPU controls covered the
record grammar, descriptor ownership, missing exit information and
runner stop rules. A debugger-lost exit had its own declared class;
neither patched guest needed it.

| Guest | Build | Result | Kernel termination |
| --- | --- | --- | --- |
| 1 | Unpatched B | Observer passed; matching main-thread SIGSEGV and cleanup chain. | SIGKILL after debugger capture. |
| 2 | Patched P | Observer passed; clean inferior, debugger, wrapper and harness endpoint. | Exit 0. |
| 3 | Unpatched B | Same matching fault and chain. | SIGKILL after debugger capture. |
| 4 | Patched P | Same clean endpoint as guest 2. | Exit 0. |

All four loaded identities, witness records and endpoint records joined.
The series ended after 36 seconds with no replacement, invalid outcome
or remaining QEMU, debugger or watcher. Independent classifier replays
matched all four saved outputs exactly. Both pairs support the complete
`baca25172d8c` cleanup-order hunk on this private build and stack.
The individual ordering edge and shader-cache mechanism remain open;
gdb still changes timing, and the installed Void binary is unchanged.
Series 120 guest 4 remains invalid because it had no kernel witness.

This resolves the private observer recipe's teardown qualification.
The next step is a reviewed package using the qualified patched QEMU
for the original observer and conditional virgl handle-lifetime test.
The lifetime invocation remains held. No Session black-frame repair,
hotplug acceptance or task closure follows from the teardown comparison.

Evidence: `121-qemu-debugger-endpoint`, `122-qemu-bp-package-4`,
`124-qemu-bp-package-5` and `125-qemu-bp-series`. The series 125 manifest
is `71007227874dbb07b96ebc76bae4b080851346fef4a5ae5ff39cffe93fca0a51`;
its `REVIEW-CODEX-75.txt` records the independent review.

## The lifetime guest lost its test record (2026-10-08)

Series 127 ran the original observer and then the virgl handle-lifetime
test on the qualified patched private QEMU under the debugger wrapper
and kernel exit witness. Each guest received separate test and
infrastructure classifications. The observer passed with a clean host
endpoint and clean infrastructure, permitting the lifetime guest.

| Guest | Test | Infrastructure | Kernel termination |
| --- | --- | --- | --- |
| 1 | Observer passed; refill reached. | Clean. | Exit 0. |
| 2 | Invalid: no test output or exit record. | Invalid: no guest exit record. | Exit 0. |

The lifetime guest's log preserves the identity chain through the test
environment record: the frozen test, virgl on radeonsi and Mesa 26.2.3.
The kernel then reported that PID 1 exited with status 101 and panicked.
The guest init runs under `set -eu`. Its device-test branch ran the test
as a plain command and only afterwards read the status. A nonzero status
therefore ended init before it relayed the redirected test output,
recorded the exit or powered off. With `panic=-1` and `-no-reboot`, QEMU
then exited 0. Status 101 does not identify the failed assertion. The
before, alive, after-drop and read records were not preserved, so this
is neither an observed handle loss nor a preserved handle. Both
classifiers refused the guest. The observer's passing test did not
exercise this failure path.

Sophia `bac569dbe` captures both eglinfo and test statuses in explicit
conditions. A failing test then keeps its output, exact status and
power-off. `crates/xtask/tests/qemu_device_test_init.rs` runs the branch's
own text under `/bin/sh` with `set -eu`, replacing its device, file and
power operations. Against the earlier init, the failing-test case
reproduces series 127: the shell ends with 101 immediately after the
environment record. After the repair all five cases pass: success,
exit 101, failing eglinfo, the bound's SIGKILL and a SIGTERM. The full
gate passed on `bac569dbe`: 7290 passed, 0 failed and 111 ignored;
the layout check also passed.

The rebuilt lifetime image `fdd2f505` carries the same frozen test
`d1e1339a` and pinned binaries. Of its 5971 entries, only the init and
dracut's record of its input directory differ in content from `c4d13b3f`.
An independent comparison also verified unchanged ownership, modes,
device nodes and 123 hard-link groups. Rebuilding changed 1363 mtimes.
The original images remain intact.

A further lifetime guest needs its own reviewed package and exact
command. No handle-lifetime conclusion, Session black-frame repair or
task closure follows from series 127. The descriptor-isolation control
remains conditional on an observed handle loss.

Evidence: `126-qemu-lifetime-package`, `127-qemu-lifetime-series` and
`128-guest-init-repair`. The series 127 manifest is
`5ec5a1c079fd28adac31828c639b45242a0e3eda831ad6a392d9c74d1a140d24`,
with `CORRECTION-01.txt` for guest 1's time window. `REVIEW-CODEX-77.txt`
records the independent review.

## Probe destruction loses the producer handle in virgl (2026-10-08)

Series 130 used the repaired init and unchanged frozen lifetime test
`d1e1339a` on the private patched QEMU. The observer passed with clean
infrastructure, permitting the lifetime guest. This time the failing
guest preserved its complete test output, exit 101 and power-off.

The producer retained the allocation and its DMA-BUF `11:2`. The probe
used a dup of the producer's DRM file, whose shared open file description
the test verified. No producer allocation or repairing import occurred
between the handle observations:

| Observation of producer handle 1 | Result |
| --- | --- |
| Before probe construction | Exports the held DMA-BUF `11:2`. |
| After the probe read, while the probe is alive | Exports `11:2`. |
| Immediately after dropping the probe | Refused with `ENOENT` (2). |

Probe construction succeeded. Its read, performed before destruction,
returned all 1228800 bytes and matched the expected pixels. The sole
panic was the named assertion that probe teardown must preserve the
producer's handle: expected `Exports((11, 2))`, observed `Refused(2)`.
The guest classifier reported `LOSS_AFTER_DROP`.

The infrastructure result was separately `P_EXIT0_DEBUGGER_LOST`.
The pidfd witness joined the loaded patched binary to QEMU PID 14846
and recorded exit 0. Gdb reported one `No unwaited-for children left.`;
the wrapper exited 125 and the harness endpoint remained `INFRA_FAILURE`.
This is not a clean debugger or end-to-end harness result. Both guests
finished, no processes remained, and the declared series ended without
a replacement.

REVIEW-CODEX-79 accepts the complete guest loss observation with that
endpoint limit. All 41 manifest entries verified, and independent
replays of both classifiers matched both guests' saved output exactly.
REVIEW-CODEX-78's clean-endpoint wording was not satisfied for the
lifetime guest; the review does not convert that failed endpoint into
a pass. The loss was observed before host teardown, and the independent
kernel witness confirms the joined QEMU process exited normally.

This demonstrates the diagnostic probe's render-node handle hazard on
this stack and agrees with the source prediction. It does not trace
the exact close mechanism or establish the original Session black-frame
cause. The primary-node, DRM-master, KMS AddFB and worker-placement paths
remain untested. Series 127 and series 120 guest 4 remain invalid.

The result admits CPU preparation of a descriptor-isolation control:
open the same explicit render node separately for the probe, verify a
different open file description, and preserve the held buffer and all
producer-file observations. The original dup test and evidence remain
unchanged. Another device run requires its own reviewed package.

Evidence: `129-qemu-lifetime-package-2` and
`130-qemu-lifetime-series-2`. The latter's manifest is
`d41aca5f0a19500f10480b2676c207b7bd99a166274d1170be58d6a2bd450358`.
No Session integration, installed QEMU change or t306/t307 acceptance
follows from this diagnostic.

## A separately opened probe file preserves the handle (2026-10-08)

Series 132 ran the descriptor-isolation control from signed Sophia
`254204a74`, after a passing observer. Its frozen test `b70605df` is the
artifact exercised by that commit's full CPU gate. The device test is
the lifetime recipe with the probe on a separate open of the same
explicit render node. Before constructing the probe, it requires equal
character-device identity and a different open file description. The
producer retains its original file, allocation and DMA-BUF; its handle
observations remain on that file.

| Guest | Test | Infrastructure |
| --- | --- | --- |
| 1 | Observer passed; refill reached. | Clean. |
| 2 | Handle preserved after probe destruction. | Clean. |

The isolation guest recorded device `226:128`, driver `virtio-pci`, and
`probe_file` on that device with `description=separate`. Producer handle
1 exported the held DMA-BUF `11:2` before construction, while the probe
was alive and after its destruction. Construction succeeded, and the
read returned all 1228800 bytes with an exact match. As in the lifetime
test, the read occurs before destruction even though its record is
printed afterward. The named test passed, exited 0 and powered off.

Both guests had clean endpoints and `P_CLEAN` infrastructure. The
kernel witness joined the isolation QEMU PID 22024 to patched binary
`6da4d8f3` and recorded exit 0; debugger, wrapper and harness also exited
0. The series ran once, ended after guest 2 and left no processes.
REVIEW-CODEX-81 verified all 41 series manifest entries and all 50
package entries, then replayed both classifiers for both guests with
byte-identical results. The ordered guest records independently agree.

Compared with series 130's dup-based `LOSS_AFTER_DROP`, this supports
the shared-description hypothesis for this recipe on this stack. The
negative was not rerun, and its debugger-lost endpoint limit remains.
One comparison across two series neither proves necessity or
sufficiency nor traces the closing mechanism. This is a render-node
probe result, not the original Session black-frame diagnosis. The
descriptor control also leaves an unanswered-query refusal branch
unexercised; its surviving mutant is not claimed equivalent.

The isolation image `dd2be216` differs in content from corrected
lifetime image `fdd2f505` only in the test binary and dracut's build-path
record. All 5971 paths, modes, ownership, device nodes and 123 hard-link
groups match. The repaired init is unchanged. The observer still uses
the older init and would refuse a failed test without complete output.

This closes the bounded diagnostic comparison at niltempus's requested
install-assessment checkpoint. It does not provide a production repair
or t306/t307 acceptance. The live release `niltempus-f18fc2ed` uses
Sophia `825d9146`, which current master contains but the diagnostic
branch does not. That branch combines earlier topology, seat and
restore repairs with experimental instrumentation; it is not a live
upgrade candidate as a whole. Any candidate must retain the installed
baseline, have an explicit reviewed repair scope and pass its own gates.
No installation, passthrough or live GPU reassignment occurred.

Evidence: `131-handle-isolation-package` and
`132-qemu-isolation-series`; the latter's manifest is
`028cc94b7732c93e432b3ae32c93d0333a656969509a3d808ebcb906e6821e87`.
The exact-command approval is `131/REVIEW-CODEX-80.txt`.

## Production descriptor audit after the maintenance login (2026-10-09 UTC)

The source audit on published `9b1c86b18` found two production shapes with
several renderer screens sharing one DRM open file description. By default,
each head's worker receives a duplicate of the card descriptor; the shared
worker remains opt-in. Separately, image-import render nodes are opened once
and duplicated for each worker's lazy import contexts. The AddFB path can use
the worker's cached GEM handle directly because it shares the KMS file.

Mesa 26.2.3's virgl screen table hashes descriptor numbers but compares open
file descriptions for equality. Different descriptor numbers can therefore
create separate handle owners for one DRM file. This source reading supports
the probe result in 130 and its separate-open control in 132. The inspected
amdgpu path instead looks up the device and reuses a screen owner for an equal
file description; that agrees with host control 105's preserved handle. It
does not establish a general absence of descriptor defects on AMD hardware.

No production trigger was demonstrated. Multiple virgl heads importing one
DMA-BUF is a candidate, but no reviewed callsite imports a sibling head's
scanout buffer into another screen. The audit follow-up does identify
cross-context imports of sibling-allocated renderer-image snapshots in
cross-head preview, cold migration and handoff restore. Those are sampling
buffers, so they strengthen the production hypothesis without demonstrating
an AddFB-handle failure. The search was not exhaustive. These findings also
do not explain series 95's single-head black read after a clear.
No renderer policy or worker default changed on this evidence. A focused
test through the actual context import/release APIs is being prepared before
choosing a repair; a structural test that merely forbids shared descriptions
would assert an unchosen policy.

Evidence: `t306-01/134-drm-descriptor-audit/AUDIT.txt` (`206232c6`).
The audit names the construction, worker, transfer, AddFB and Mesa callsites.
The complete t306 restore candidate is being qualified separately; neither
this source audit nor the successful maintenance login closes t307.

## Real-context control and a freshness-oracle gap (2026-10-09 UTC)

Series 141 ran the reviewed observer and context recipe once, from 03:38:28Z
to 03:38:48Z, under `REVIEW-CODEX-140-GO.txt`. Both guests were `P_CLEAN`,
with clean endpoints, zero kernel exit status and no leftover processes.
The observer passed. The frozen context verdict is `INVALID` and remains
unchanged: the shared arm printed `got error from kernel - expect bad
rendering 2` between its sibling-alive and after-drop records, which the
strict classifier refused as an unexpected line.

The raw records report matching pixels at all stages and a preserved output
handle in each arm: separate-open `Exports((11, 2))`, shared-description
`Exports((11, 6))`. Both libtest tests passed and the guest reported exit 0.
These measurements do not override the frozen verdict or establish fresh
rendering after the sibling's teardown.

In the matching Mesa 26.2.3 source, the exact diagnostic occurs in
`virgl_drm_winsys_submit_cmd` when `DRM_IOCTL_VIRTGPU_EXECBUFFER` returns -1;
the printed value is errno, here 2 (`ENOENT`). The ordinary `virgl_submit_cmd`
caller does not propagate that return value. The guest log does not identify
the issuing context, command buffer or handles, so this source mapping does
not trace the failure mechanism.

The run also exposes a test limitation: P reused slot 1 with the identical
window and expected pixels at baseline, sibling-alive and after-drop. A
failed new submission could leave the preceding matching frame in that slot.
The observations therefore cannot exclude stale output. The source follow-up
moves the same client DMA-BUF to a distinct position at each producer stage
and checks that every earlier frame fails the later expectation. This is an
explicit test intervention, with its own qualification; no production repair
or replacement guest is implied.

Frozen evidence: `140-context-sharing-package`, manifest
`e4b8cb0f64cdc564d0f57f5e333356c4731a289495173bf5cf6133d56a24b808`,
and `141-qemu-context-series`, whose 39-entry manifest is
`cf5f7105a2ec8512b7f9449a934fd8562500c9e48e095a9b9a3e160cd7f18a5d`.
The separate result note is `141-qemu-context-result/RESULT.txt`. This run
does not establish a production trigger, KMS/AddFB failure, the original
black-frame cause or t306/t307 acceptance.

Signed follow-up `9dee187ae` changes only the context test: producer stages
place the same client buffer at x=16, x=24 and x=8, within the same output
slot. Its two CPU tests pass, and both deliberate mutations that reuse an
earlier placement after the sibling's drop fail at the intended assertion.
The full device-hidden gate and layout check passed from 03:50:43Z to
03:57:59Z, with 7,227 passing, zero failing and 103 ignored entries across
524 raw libtest summaries, including nested summaries. Both device arms
remained ignored. The exact gate artifact is frozen as
`0d9c10c8b42a91a5f28c2e508cfc4e4a2638a256d12bded339371b0bb5813be6`.

Evidence `142-context-freshness-cpu` has 35 entries under manifest
`9aa2fda79fac7ce27ed263cf5f1c8e5e87acba65bf5fa3698beb516164bf3529`.
Its package policy fixes the treatment of unexpected output before another
run: foreign lines, including the Mesa diagnostic, still refuse the test
window. A successor classifier must bind each recorded placement and the
new binary's identity. Fresh pixels would exclude an earlier producer frame;
they would not attribute a failed submission that never touched that output.

## Freshness run catches a mismatch; the strict verdict stays INVALID (2026-10-09 UTC)

Series 144 ran the frozen freshness test once from 04:19:35Z to 04:19:55Z,
under `REVIEW-CODEX-143-GO.txt`. The observer passed and both guests were
`P_CLEAN`, with clean endpoints and no remaining processes. The context
test exited 101; its harness exit 1 follows that test failure, while QEMU
exited 0 after guest power-down. Infrastructure and test outcomes remain
separate.

The frozen context verdict is `INVALID`. The shared arm again printed
`got error from kernel - expect bad rendering 2`, which the policy fixed
before this run requires the classifier to refuse. Neither this run nor
series 141 is normalized or reclassified.

The raw separate-open records match all three placements, x=16, x=24 and
x=8, and preserve the AddFB handle `Exports((11, 2))`. The shared arm
matches at x=16 and x=24. After the sibling's drop and the Mesa diagnostic,
its x=8 record reports a mismatch: pixel (8,12) reads `[0, 0, 0]` instead
of `[3, 2, 1]`. The AddFB handle still exports `Exports((11, 6))`; its
assertion passes before the named composition assertion fails at line 683.
Libtest reports one passing and one failing device arm.

The reported 2,304 black pixels cover the whole 64x48 output: that is the
black count for a frame containing one 32x24 window. Together with the
black pixel at the new window's origin, this is consistent with an older
frame remaining in slot 1. It does not identify which frame was read or
which context, buffer or ioctl produced the diagnostic. The revised oracle
has exposed a mismatch that the repeated placement in 141 could hide;
it has not established a production trigger, KMS/AddFB failure, the original
black-frame cause or t306/t307 acceptance.

Evidence: `143-context-freshness-package`, manifest
`718870b2caed4ace2f29500ffbaaa228c4604cb4fa3fb8480316bef6d29ae15b`,
and the read-only `144-qemu-context-freshness-series`, whose 39-entry
manifest is `c6c520cef70867b2d3d4e3c189e04582a1121a256924d6ae050b8c1a70897344`.
The separate result note is `144-qemu-context-freshness-result/RESULT.txt`.
The guest log is `9ada2735`, and its frozen classifier output is `dafbd6ab`.

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
