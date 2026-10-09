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
an AddFB-handle failure. The search was not exhaustive. Series 95 observed
black pixels after a clear on head 1, but ran two active heads with two
renderer workers. It therefore cannot exclude cross-worker interaction on
the premise that the experiment had only one head. It does not establish
such an interaction or attribute the black pixels to descriptor ownership.
No renderer policy or worker default changed on this evidence. A focused
test through the actual context import/release APIs is being prepared before
choosing a repair; a structural test that merely forbids shared descriptions
would assert an unchosen policy.

Evidence: `t306-01/134-drm-descriptor-audit/AUDIT.txt` (`206232c6`).
The audit names the construction, worker, transfer, AddFB and Mesa callsites.
The topology correction is retained in
`t306-01/162-series95-topology-correction/REVIEW.txt`; the original audit and
series remain unchanged. Both guest logs name two ready heads at lines
16–17 and separate worker bootstraps at lines 38 and 46. Their final resource
records report `renderer_workers=2` (boot 1 line 203; boot 2 line 219).
The complete t306 restore candidate is being qualified separately; neither
this source audit nor the successful maintenance login closes t307.

The subsequent source review in `163-virgl-subcontext-source` identifies a
second hazard under the separate-screen/shared-DRM-file premise. Mesa's
virgl screens each allocate subcontext numbers starting at 1, while the
kernel assigns their duplicated descriptors one host context namespace.
Virglrenderer silently accepts a duplicate subcontext creation; both screens
can then select the same host GL state, and one screen's destruction can
destroy that shared subcontext. This does not require importing the same
DMA-BUF and is separate from GEM handle ownership. It remains a source
candidate: no recorded subcontext IDs or commands attribute the pixels in
95, 144 or 153 to it. An EXECBUFFER missing-handle trace alone would not
exclude this second mechanism. No worker policy or installed driver changes.

The stderr audit in `166-series-stderr-audit` supports expected delivery of
host context errors through the four series' launchers and loggers. Series
117 supplies a positive QEMU stderr witness through the wrapped chain used
by 141, 144 and 153. No retained log contains the expected context-error
text. As narrowed by `ADDENDUM-01.txt` (`df709ce6`), this weighs against a
reported sticky context error; it does not prove one absent in a particular
run. Series 95 did not pin its QEMU binary and has no positive witness at
its harness version, and none of the four series recorded its loaded host
library mapping. Silent subcontext collisions remain possible. The audit
does not attribute pixels or change any series verdict.

## CPU control of the virgl screen-cache key (2026-10-09 UTC)

Evidence `165-virgl-cache-key-control` reproduces the cache-key defect using
the actual Mesa 26.2.3 hash table, allocator and descriptor helpers, with
verbatim virgl hash and equality callbacks. Temporary regular files provide
decisively checked shared and separate open file descriptions. No screen,
EGL, GBM or DRM operation is involved.

The original integer-descriptor hash passed 8 of 15 checks. A duplicate
lookup missed without invoking the equality callback because its hash
differed from the stored descriptor's hash. Six failures exercise this one
production lookup defect. The seventh checks removal through a duplicate,
an additional table contract that production does not use: production
destroys through the stored descriptor. Different descriptor integers can
also accidentally collide in the original 32-bit hash.

The reviewed candidate changes only the hash to Mesa's existing common
file-identity formula, retaining virgl's conservative description equality.
It passed all 15 checks, including independent opens of one inode, table
growth, caller close and descriptor reuse. Independent opens can hash alike
and are still distinguished by equality. The patch is `fbfe928a`; the frozen
evidence manifest is
`02cf3236cd01fb4962cf3dbbd57b29d6127e75a7070864145414267ac798507c`.
The independent source review is included unchanged.

This establishes the key correction, not screen construction or a rendering
repair. A patched shared-description arm would use one shared screen instead
of the separate-screen shape under investigation, so any guest comparison
requires new premises and evidence. No installed Mesa or Sophia code changed,
and t306/t307 remain unaccepted.

The paired private builds in `167-virgl-cache-private-build` carry the
correction into compiled Mesa. Both use the same source and build paths,
compiler, options and dependencies; the original stage was frozen before
applying the one-file patch. The 15 staged files differ only in
`libgallium-26.2.3.so`: original `be3ca218`, patched `bd3df816`. Links,
permissions, ownership, dynamic dependencies and exported symbol names/types
match. The compiled patched hash calls `fstat64`; the original hashes the
integer. The winsys object, its archive and libgallium are the only changed
artifacts among 982 tracked build outputs. Unmodified incremental builds
leave those outputs unchanged; Mesa's always-run Git-version generator is
accounted for explicitly. Preparation failures and collector corrections
remain in the evidence under their original results.
The frozen pair manifest is
`793c752dabb7c6a5f8663a6b6f8fd79af13a64be657d7c15ac930a244d98d20f`.

This pair uses EGL/GBM with X11 support and only the virgl driver, GLVND,
debug symbols and NDEBUG, without GLX or software fallback. It is a matched
private build, not a claimed reproduction of the distro build. At that build
freeze, no image or guest used it. Rendering qualification and the runtime
screen-sharing premise remain separate from the compiled cache correction.

## Matched private Mesa images (2026-10-09 UTC)

Evidence `170-mesa-image-pair` derives two images from frozen image 143,
`f49bc81a`, using the reviewed pair from 167. CPU preparation ran once from
06:52:06Z to 06:54:19Z and exited zero, with devices and network hidden.
The original image is
`e9597490d1484928e7064fbf7d94566bddba4c105bc433a42d41b648d3f1d57f`;
the patched image is
`8edf6031cc2838c5448f99e08ac4b6b7233e97de3c55ca86e14b41353503587e`.

Each derivation replaces five Mesa library bodies, removes 54 foreign DRI/VA
driver links and adds nothing. The remaining 5917 entries retain the base
metadata; every non-Mesa body and header is byte-identical to 143. The
original and patched archives differ only in `libgallium-26.2.3.so`, with
equal metadata. The frozen test `0d9c10c8`, init `f49c78d0`, guest tools,
Sophia binaries, loader cache, vendor JSON and build-parameter record remain
unchanged. The external `classify_handle5` and its strict foreign-line rule
also remain unchanged. `MAP.json` records the derivation; the preserved
build-parameter does not claim a new build.

All 17 archive/audit controls passed, including byte-exact round trip,
unauthorized path changes, changed trailer, reordered entries and non-block
padding. Compression validation, decompression comparison and independent
content/metadata listings passed. The five libraries keep their original
SONAMEs, have no runtime path override, and resolve all 44 direct dependency
edges inside the image with every required symbol version present. The
unchanged loader cache and GLVND lookup resolve to the same entries.
An independent review recomputed the image hashes, stream hashes and full
listings and found no discrepancy.

The read-only evidence manifest is
`52431174075a8f9d0e1c2455e799aabd9b5adadee06cefd994000a91067d1367`
(153 entries, including both compressed image paths); `RESULT.txt` is
`1b6dd935`. Neither image has booted. The unstripped private build grows the
unpacked archive from about 771.8 MB to 816.9 MB and the compressed image
from 373.8 MB to 387.5 MB. A future runner must record its memory setting
and classify an early boot OOM as infrastructure. The prior recipe used
2048 MiB. Stripping would break the reviewed binary identity.

These checks establish the image pair, not runtime loading, screen sharing
or a rendering repair. A bounded runner, its runtime premise and exact-argv
GO remain separate. Installed libraries and all previous series verdicts
are unchanged; t306/t307 remain unaccepted.

## CPU qualification of the screen-sharing witness (2026-10-09 UTC)

Evidence `171-premise-witness-design/DESIGN-R2.txt` replaces the proposed
separate premise boots with a common traced child pair. Screen sharing and
pixel records must come from the same boot: an original integer-descriptor
hash can collide, so a premise observed in one boot cannot establish another
boot's shape. The trace overhead is a shared intervention. The frozen test,
its arguments and environment, and `classify_handle5` remain unchanged.

The private init in `173-premise-witness-init`, `ca0706c3`, adds a top-level
block and three call sites. Removing those four marked regions reproduces
the frozen init `f49c78d0` byte for byte. It records 27 syscall and scheduler
events, scoped to init and its descendants, and dumps the stopped trace
after the ordinary test-exited record. Eighteen shell stand-in scenarios
passed 139 assertions. Their first run exposed a real collector defect:
a failed trace-file redirection skipped the loop and reported an empty,
complete dump. Revision 3 opens the file explicitly and reports failure.
Both runs and their source identities are retained. These are stand-in
checks; real tracefs behavior, filtering and collection cost remain untested.

The frozen `174-premise-classifier` implements the corresponding reader.
Its manifest is
`c7c943f0eff49753cf996fd119be137cbb3374662911fb279fcf75db768a3a48`
(148 entries); `RESULT.txt` is `457f4552`. The descriptor model passed
23 CPU tests, and the complete synthetic reader passed 31. Source review
found two format mismatches missed by the first 27-test run: the x86 clock
name and a scheduler flag. Both defects are preserved and reproduced by
new controls. Seven validation-bypass mutants were also killed, and three
command-line controls passed. Independent freeze inspection found no
discrepancy.

The reader binds raw event counts to the header and each CPU's counters,
pairs syscalls, identifies the frozen test through exec and fork ancestry,
and follows descriptor generations through creation, duplication and close.
It requires a unique ordered embedding of the test's description-query
graph, allowing Mesa's additional queries. GET_CAPS and equality against
the producer's winsys distinguish a new sibling screen from a shared one.
The serial records must independently establish both arms' pre-drop device,
description, pixel-window and held-output premises. The screen verdict is
reported alongside the unchanged pixel verdict; it cannot rehabilitate an
`INVALID` test result.

The classifier assumes complete syscall capture and the frozen source's
shared descriptor table. Zero loss counters and balanced records cannot
prove that tracing omitted no entire syscall pair. Strict microsecond
ordering and an unmatched syscall at thread exit can cause conservative
refusals. No runtime screen sharing, rendering repair, handle mechanism or
subcontext mechanism has been established by this CPU work. Child images,
their bounded runner and runtime qualification remain separate.

The common traced child pair was subsequently prepared once in
`176-premise-child-images`, from 07:46:43Z to 07:48:53Z, with exit zero.
Its 11 archive controls passed. The images are original
`6b183464615c68a435024d430675534aaaa3a469c4dc2eed04c59fa3e1ae3c0b`
and patched
`ebd4f0fa53f208437bb70897d4508c7e5a4dc2b1270d9678e2e6001c18bcd1ba`.
Each replaces only `usr/bin/sophia-qemu-init` in its corresponding 170
parent with `ca0706c3`, preserving its mode 0700 and every header field
except file size. Entry count stays 5917; nothing is added or removed.
Every other record, entry order and trailer is byte-identical to its parent.
The children still differ only in libgallium, with equal metadata.

Independent output review re-hashed both images, raw streams and verified
decompressions, recomputed the complete content and metadata listings, and
checked both parent-to-child changes and the one-entry pair difference.
All checks passed. The read-only manifest is
`74ddfdea5075b4c7ef2104dca1b69c2767659b51757b25c7b220e143153a1217`
(52 entries, including the two image paths); `RESULT.txt` is `696ba22c`.
Neither child had booted at this image checkpoint. Collection bounds and
unchanged, separate test and infrastructure verdicts belong to the runner.

Package `177-mesa-pair-runner` subsequently froze the observer, original-child,
patched-child order under a 3000-second outer cap. Each context guest has a
900-second host collection deadline, a 960-second wrapper bound, a
1020-second harness kill and a watcher deadline extended to 1035 seconds.
Private runtime directories hold sockets and FIFOs outside the frozen image
caches. The observer keeps its earlier harness and watcher. Only the context
harness deadline and watcher constant change; their reviewed diffs are kept.

The advancement validator passed 130 cases plus repeat and usage checks.
Isolated controls of the actual runner with stand-in launches passed 46/46,
including all three guest positions, end-pin and source changes, classifier
failures, missing traces and three independent leftover-process forms. Five
checks exercised the real preflight's mode/image/test refusal before host
reads. Two synthetic captures at the 32768-line limit took about 0.34 seconds
each under the classifier's 20-second bound. Their roughly 4.13 MB size and
simple query graph do not prove a worst-case processing bound. The controls
ran from 08:22:00Z to 08:22:25Z, exit zero, without a guest or device access.

The original child permits the patched child only with clean infrastructure,
no remaining process, unchanged pins/source and both expected screen premises.
A pixel `INVALID` remains `INVALID` and blocks a usable pixel comparison;
screen observations cannot override it. Faithful serial bytes remain an
explicit assumption: framing, counts, grammar and endpoint checks cannot
detect every corruption that still forms valid text. No UART baud pacing or
drop-to-logger-error guarantee is claimed, and no checksum rule was added.
The frozen 177 manifest is
`953b34f4f6f3f4f17cf6fe9a65d4fa9291e9bd97407aad7e81e80a8e1eef7dee`
(4339 entries, including retained control cases). `READY-177.txt` is
`c9d1b85a`. This CPU qualification supplies no guest GO, runtime result,
production repair or t306/t307 acceptance.

The actual host preflight subsequently stopped before any guest or result
directory was created. Its environment scan could not read three older host
processes: two zombies and the live factotum. Device-hidden CPU controls had
not exposed that host condition. Evidence `179-runner-host-preflight` keeps
the refusal; 177 is unchanged and unrun, and series 178 remains absent.

Successor `180-mesa-pair-runner-scoped` records the runner's PID and start tick
once and requires that identity to remain a live ancestor of each checker.
Global busy-process and owned-path checks still cover all processes. Inherited
environment tags are checked for same-UID processes born at or after the
runner, including equal ticks; unreadable environments or zombies in that
interval refuse. A missing, forged, stale or unsafe marker also refuses.
An unrelated new unreadable process or zombie can therefore stop the series;
both agent lanes must stay idle during execution. No host process is signalled.

The successor's isolated orchestration controls passed 53/53 plus five real
preflight binding refusals, from 08:50:27Z to 08:50:56Z. Focused real-process
controls passed 31/31 against both checker versions, without namespace residue
or same-tick retries. On this kernel the earlier checker already refuses the
zombie case through its unreadable environment, so these runs do not show a
red-to-green separation. The successor's explicit zombie-state refusal is
exercised; protection against a readable, empty zombie environment rests on
source inspection. The actual host read-only preflight then passed with
`remaining=no`, the pinned device identity and both frozen worktrees clean.

The 180 manifest is
`8afbe059dac3b8518f403846695ceeb53ff73cb7960af09b58ebb2f6efb7b047`
(5040 entries), and `READY-180.txt` is `f6eeded1`. Images, tests, classifiers,
advance rules and runtime bounds remain those of 177; the unchanged classifier
capacity measurements are reused. The package reserves series 181 and a new
private runtime directory. It supplies no guest result or acceptance claim.

Series 181 then ran once under `REVIEW-CODEX-180-GO.txt` (`e47b2346`), from
08:59:48Z to 09:00:24Z. Two guests ran: the observer passed, and the original
private Mesa child reached its tests. Both infrastructure verdicts were
`P_CLEAN`, with clean endpoints, no remaining processes and unchanged end
pins and source. The original child's pixel verdict remains `INVALID`: its
separate arm passed, while the shared arm printed the same foreign Mesa line
as 141/144 before an after-drop pixel mismatch. Test exit 101 was correctly
bound to harness exit 1. Those raw observations do not override the verdict.

Trace setup refused before arming. A request of `4096` to `buffer_size_kb`
read back as `4099`; frozen init 173 and parser 174 incorrectly require an
exact `4096` readback. The kernel rounds the byte request up to whole
subbuffer capacities and reports the resulting capacity in KiB. With
4080 data bytes per page, this is 1029 pages, or 4099 KiB when reported.
The trace block records skipped stop/end and zero lines. Premise1 therefore
returned `INCOMPLETE`, and the runner correctly refused the patched guest.
There was no trace or Mesa pair comparison, and no replacement guest.

The 70 raw files remain read-only under the external manifest
`181-freeze-claude/181.SHA256SUMS`
(`bf580a1edb630d41918b712a77f2541b034e01f604f2c99b3774e49bebc603d9`).
Independent device-hidden replay reproduced all seven classifier and decision
records byte for byte, including exit statuses, with raw hashes unchanged.
That review is `182-mesa-pair-result-review`, manifest
`a27f6a3d8ae8f1df72f9c3ad442bf6a7f71ca7ffbfdcde68e7d4d173705a3736`.
The setup-contract defect needs a separately qualified successor; none of the
frozen verdicts, production code or installed components were changed.

Source audit `183-arming-readback-audit` also found a second defect behind the
buffer refusal. Enabling `event-fork` before reading `set_event_pid` adds the
readback's own child to the PID filter. That produces at least two lines and
would fail the original one-PID check. The old stand-in echoed writes as
readbacks, so it modeled neither the rounded capacity nor that fork behavior.

Successor init 184 (`2fcf5035`) keeps the request at 4096 and requires readback
4099. It writes and verifies `event-fork=0`, writes and reads the PID filter,
then enables and verifies `event-fork=1` before tracing. Its model controls
passed on 2026-10-09 from 09:15:24Z to 09:15:29Z: 267 assertions, exit zero.
They reproduce 181's refusal byte for byte, expose the second defect in a
buffer-only correction, retain all 18 earlier scenarios, and cover a dirty
initial fork-tracking state. Model mutations falsify the relevant controls.
The result manifest is
`ad0f57c4cde0d8bfb0593ec331c8403510f08e68cca1ad3b1567a3b9e57eab86`.

Parser successor 185 accepts only that new 65-setting sequence; core, serial
interpretation and CLI logic remain byte-identical to 174. Its controls bind
the parsed settings to the frozen init's actual arming calls and an independent
expected sequence. On 2026-10-09, 09:18:02Z–09:18:04Z, 23 core, 31 capture and
22 contract cases passed under device-hidden isolation. Each parser refuses
the other's contract, and both still reproduce 181's `INCOMPLETE` unchanged.
The frozen manifest is
`57877ef364a308b90d40ae44b7df76432319808565bbf9167bf97bfcb8820e21`
(90 files). These qualify the source and model correction only. Real tracefs
arming, filtering and collection remain untested; no corrected image or
subsequent guest had been prepared at this checkpoint.

Corrected child images `186-premise-readback-child-images` were then prepared
once, from 09:29:19Z to 09:31:31Z, after all eleven archive controls passed.
Independent device-hidden comparison reproduced their hashes, contents,
metadata and raw archive relationships. Each changes only the init entry from
its 170 parent; the pair still differs only in `libgallium`. The images are
`8cb1a4f55929c9debe51ce3c099eac5b892e89c14530ab0d8aba6f78428decee`
and `20eab3f5fb394771638178a7c56d6a0eb9f4282679c023ff7df9bc461740d7f4`.
The frozen 42-entry manifest is
`5df76cf1ab6d60952a139ab0ea7a08b73751b12fff74da1901f977ada2cd92ea`.
The original source manifest remains separately preserved. No image was booted
during preparation or review.

Successor runner `187-mesa-pair-readback-runner` binds those images and parser
185 to a new series 188. Its runtime changes from 180 are the package, result
and runtime paths, the image identities and the premise-classifier path.
Cleanup, advance rules, pixel classification, test, environment and bounds
remain unchanged. A separate guard verifies the filled identities against
the prepared images, manifests and all runner/control consumers before the
CPU cases and again at freeze.

The single CPU qualification ran from 09:37:53Z to 09:38:26Z: 53 orchestration
cases, five real preflight binding refusals, and 130 decision cases passed,
including repeat and usage checks. The current parser handled a synthetic
32768-line capture in 0.325 s for original and 0.331 s for patched. That remains
one full-line-cap shape, not a worst-case graph or byte-budget guarantee.
Source pins were unchanged. The actual host read-only preflight also passed,
with both frozen worktrees clean, correct device metadata and no remaining
process in its preflight scope. These checks establish neither a real trace
premise nor a pixel repair; 181's refused result remains unchanged.
The 187 manifest is
`2c6c31dd2380b44347ec2a17389f7fd6372f156bfd06669c69447489105e5e5d`
(6566 regular files, with one negative-case symlink separately recorded), and
`READY-187.txt` is `ed06074f`. No guest GO is contained in that package.

Series 188 ran once under the separate `REVIEW-CODEX-187-GO.txt` (`3843454b`),
from 09:43:18Z to 09:43:58Z. Two guests ran, both with `P_CLEAN` infrastructure,
clean endpoints and no remaining process. The observer passed. The original
child's pixel verdict remained `INVALID` on the same foreign Mesa line; its
test exit 101 was bound to harness exit 1. Handle5 and BP7 exited zero, while
premise2 exited one. The runner refused the patched guest, with no replacement.

The corrected init successfully armed real tracefs and collected a complete
2682-line dump. Parser 185 accepted its settings, counts, 2670 events, test
identity and both arm graphs. Each arm nevertheless classified `CREATE_FAILED`:
a descriptor generation had one `GET_CAPS` return of `-EINVAL`, and a nested
duplicate had a successful call. The frozen rule accepts success, or an
`-EINVAL` followed by success, within one generation; it therefore refused the
premise. The trace does not capture ioctl payloads, so calling those two
operations a single retry would exceed the observation.

All 70 raw files remain unchanged under the external manifest
`188-freeze-claude/188.SHA256SUMS`
(`1aa9fae0c2ed1ded4eeea9eb6c878b10541373656cabc400ca694d30be477520`).
Independent device-hidden replay reproduced all seven classifier and decision
outputs byte for byte with their exit statuses. Review `190-mesa-readback-result-review`
is frozen under manifest
`c53dfa9d8427178d1f0db24eea9ae0170c2e90ae59265f2dc7bfc215bf1bd784`.
This establishes the corrected arming observation under the declared capture
assumptions, but no screen premise, Mesa pair comparison or pixel repair.
The next source audit separates loader probing from winsys creation; 185 and
188 are preserved without rule changes or retrospective reclassification.

Source audit `191-source-audit`, with its correction addendum, distinguishes
the calls. The pipe loader duplicates the supplied descriptor into L and
performs an optional native-context capability probe. A cache miss later
duplicates L into the winsys descriptor W; that path issues seven `GETPARAM`
calls, a version query, context initialization and its own capability query.
The winsys's actual version fallback stays on W. The build includes three
`GET_CAPS` call sites, and the observed L/W sequences match these distinct
source paths. Their function attribution remains conditional on the frozen
source: the trace has no stack or ioctl payload and does not prove the cause
of L's `EINVAL`. Two `DRM_VERSION` ioctls are an observed restricted shape,
not a source-verified universal libdrm count.

The addendum also separates O, the test's observer duplicate created before
P, from EGL's later duplicate. Neither is interchangeable with L or W. The
191 manifest is
`58cffb009940fe215f2c043797b253746b0529ffaa1d427bc80aa745873cafe2`.
This supports preparing a narrower creation-signature classifier with its
own refusal controls. It supplies no revised verdict or further guest GO.

Successor classifier `192-premise-screen-signature-classifier` distinguishes
the loader probe from its direct child winsys construction. Its declared
contract binds the setup sequence, return values, descriptor generations and
arm thread. A cache comparison must name sibling L and producer W, follow
the loader probe, and precede any sibling winsys duplication. Partial or
unexplained construction attempts refuse. Capture, serial interpretation,
CLI, descriptor ledger and graph inference remain unchanged from 185.

Its isolated CPU qualification ran once on 2026-10-09, 10:09:50Z–10:09:51Z:
24 core, 31 capture and 19 signature tests passed, including subcases and
the synthetic old/new classifier contrast. Source pins were unchanged.
The source manifest is
`3ce71dbad585eb83516ef7ae7b1e0b585e2576db3caf6257b4b5b8f820a8f8d7`;
the result manifest is
`207fb30091e3ad8fb8c03f85449b317207d2d3d8b6b22493e0b95d1b4cc0dd3e`.
A labelled retrospective analysis of 188 recognizes both original-build
arms as `SCREEN_NEW` under the successor contract. Frozen 185 still
reproduces 188's canonical record byte for byte: both arms `CREATE_FAILED`,
`premise_established=false`, exit 1. That is a full classification, not a
top-level `INCOMPLETE` record; the separate clarification in
`189-claude-reviews/CLARIFICATION-192-RESULT-01.txt` (`58a4cc38`) corrects
that wording in the retained 192 documents. No 188 verdict changes, and
the runtime Mesa pair comparison remains unperformed.

Runner successor `193-mesa-pair-signature-runner` reuses both 186 images
unchanged and selects premise3 from 192. Its other runtime differences from
187 are the new package, evidence and runtime paths; decision predicates,
launches, bounds, environment, test and pixel classification are unchanged.
The single isolated CPU run, 10:16:13Z–10:16:46Z, passed 53 orchestration
cases, five precheck refusals and 130 decision cases with repeat and usage
checks. Its generated 192 synthetic captures remain labelled. Classification
at the 32768-line cap took about 0.347 s per mode; this is one capture shape,
not a worst-case search or byte-budget guarantee. Pins remained unchanged.
The mode/image binding guard and actual host read-only preflight passed.
The frozen manifest is
`6fd063f97774726b159a8a1f054cd92d18617b0c514d8f065adc5793b2e55145`
(6561 regular files, one refusal-case symlink listed separately), with
`READY-193.txt` `97c0c71d`. Series 194 and its runtime root were absent at
freeze. This package contains no guest GO or new runtime result.

Series 194 subsequently ran once under receipt `bc892c7d`, from 10:22:53Z
to 10:23:57Z. All three guests were `P_CLEAN`, with clean endpoints, no
remaining processes and unchanged pins and sources. The observer passed.
The original's screen premise was established: both arms `SCREEN_NEW`.
Its pixel verdict remains `INVALID` because the shared arm emitted the
known Mesa diagnostic; test exit was 101. The patched guest preserved both
arms' pixels and exited 0, but its screen premise was `INCOMPLETE`.

The patched capture contains the expected sibling-loader lookup miss in
the separate arm and hit in the shared arm. It also contains comparisons
whose first descriptor belongs to the sibling winsys. Cache insertion and
removal can invoke that same equality callback; 192's contract modelled
only lookup and therefore refuses those comparisons. The trace has no
stack, so this source correspondence is not function attribution. The
patched premise remains refused, and the original `INVALID` also prevents
an accepted pixel-pair conclusion.

Independent review `196-mesa-signature-result-review` replayed all eleven
frozen classifications and decisions under device-hidden isolation at
10:42:49Z–10:42:55Z, exit 0. Outputs and statuses match byte for byte; the
103 raw files verify unchanged before and after. Its manifest is
`39e6ee33dbfaa8512e9f52d8aaaa6f1f0dd2d43dd27e2ed96b2a938304733fe6`.
The raw manifest, outside the series in `194-freeze-claude`, is
`0b752fe4b32257c3f05718d83cd4c8caaf81dabcd25a5191f4e381e6ca4fd4c4`.
Niltempus approved a successor that resolves both the cache-lifecycle
contract and separately reported driver diagnostics before another paired
run. Neither correction changes 194 or establishes the original Sophia
workload's cause. Production-workload and hotplug qualification remain
separate requirements.

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

The subsequent source audit reads the guest's exact Linux 6.18.54 driver
path, using the Void package's recorded source revision and matching module
bytes. With the fields Mesa sets, an `ENOENT` from the submission path can
mean either that `virtio_gpu_array_alloc` failed or that
`drm_gem_object_lookup` found no object for a listed handle on that DRM file.
The diagnostic alone does not distinguish them. Module disassembly confirms
that the allocation and lookup calls are out of line and can be observed
separately. The same audit identifies all handle creation, PRIME reuse and
deletion paths. Evidence is `148-execbuffer-source-audit/AUDIT.txt`
(`c23976fc`). This is source and symbol inspection; no trace was run and
no mechanism or production trigger is attributed by it.

## Black, correct and black before input readiness (2026-10-09 UTC)

The one input-return guest in series 153 never reached its input fixture.
It used signed candidate `b12f0720c`, image 151, and the unchanged production
binaries from image 145. The new image changes only the guest's keyboard
write markers and build output-directory record. Before any key, unbind,
bind or display loss, output 1/head 1 records three consecutive compositions
of the 400x300 client region:

| Frame | Scene generation | Nonzero RGB pixels | Region checksum |
| --- | --- | --- | --- |
| 15 | 26 | 0 | `8572701038929191205` |
| 16 | 1 | 120,000 | `15913682524319544229` |
| 17 | 1 | 0 | `8572701038929191205` |

Each region read follows its matching composition queue record and precedes
the corresponding frame's retirement on the same output/head. Frame 16's
checksum is the independently computed probe pattern. The source is recorded
as `renderer_image`; the log does not trace the producer storage, imported
texture or synchronization dependency responsible for the change. No Mesa
`got error from kernel` line appears. This is another black/correct/black
observation before hotplug, not an attribution to the descriptor hazard in
144.

Startup remains unready with `surface=1 focus_applied=1 visual_detail=0`.
The guest reports `unplug_session_exit=1`; the host reports
`baseline_ready_timeout`, and the frozen verifier refuses missing unplug
uevents. Infrastructure is separately `INFRA_REFUSED` because the harness
never emits `guest_exited`, despite clean QEMU, debugger, wrapper and kernel
exits and no remaining process. The accepted endpoint rule is unchanged.
Neither the raw pixels nor the completed runner accept the input test.

Source reading explains why the good retained frame does not establish
startup readiness. For this DMA-BUF surface, the CPU visual-detail term is
false. Stable GPU evidence is recorded when the client's Present itself
retires, requiring nonzero content for that transaction on its participating
outputs. That was black frame 15. Frame 16 is retained composition rather
than another Present retirement, and the client presents only once.
The host separately exits its failure path before collecting and printing
the QEMU/logger endpoint; its wait also misses the guest's own failure
marker and reports a timeout after QEMU exits. Evidence and source analysis
are in `155-input-153-startup-diagnosis/DIAGNOSIS.txt`. These findings do not
explain the black pixels or justify relaxing startup's proof requirement.

Evidence is the read-only `153-qemu-input-return-series-2`, manifest
`ed8a1a57302f756c9bdaae0ea80906a4530351056bdd5954eb5cc2544afac403`,
and `153-qemu-input-return-result-2/RESULT.txt` (`9374400a`). No replacement
run, production repair, mechanism trace or t306/t307 acceptance followed.

## Callback and pixel contracts after 194 (2026-10-09 UTC)

Series 194 exposed two independent limits of the comparison contract. The
original pixel result remains `INVALID` because handle5 rejects the Mesa
diagnostic. The patched premise remains refused because premise3 mistakes
cache maintenance comparisons for sibling lookups. Neither result is accepted
retrospectively. The authorized
[qualification plan](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md)
requires both successor contracts to be controlled before another comparison,
then a separate production-workload qualification.

Source audit `197-cache-callback-audit` binds the pinned Mesa implementation
to the retained trace. Its manifest is
`3756f72d336f2f5ba0a7f1fe3223c1c8275e51f17a5888ccb5b6d02db603de55`.
The table compares the searched or inserted key against an existing winsys
key. After a miss, the loader's LOOKUP sequence is repeated by the new winsys
during INSERT; destruction can issue another run during REMOVAL. With the
patched inode hash these comparisons become visible. They are not additional
screen-sharing observations.

The accepted bounded successor design assigns every such comparison to one
unique operation using live generations, returns, thread and source order.
Contiguity means the ordered cache-comparison subsequence, not adjacency in
the whole trace. Cross-thread relations require strictly ordered printed
times. INSERT must follow its loader lookup; an ambiguous INSERT/REMOVAL run
refuses. A queryless insertion is bounded after the screen's optional
DRM_CAP_PRIME query when present, and the process may contain no more than
16 proven winsys generations. Negative query returns refuse. Queryless
removal and the untraced same-fd shortcut remain declared limits. The audit's
original-hash injectivity proof is not a classification prerequisite.

`198-premise-callback-classifier` implements this design as a source draft,
with source-input manifest prefix `ad200d7d` and core prefix `b0f40d21`.
The independent source review found the callback assignment consistent with
the bounded contract: maintenance pairs bypass counting but must still receive
a unique global assignment; live-key bounds include the optional prime query;
thread, time and 16-generation limits remain explicit. All source inputs
verified under manifest
`ad200d7decaa4e7b264e056f5bec32656e8c3ed1cecaf9c57e6110be8ac42e54`.
The reviewed launcher allows one isolated CPU control run with a 1,800-second
outer KILL, enough for its six separately bounded steps. Its retrospective
comparison must reproduce 192's original outputs before recording successor
analysis separately. It cannot revise the verdict of 194.

The single control run at 11:22:57–11:23:00Z failed, outer exit 1, with source
checks unchanged. Core had seven errors; callback controls had nine failures;
signature controls had one failure and one error; capture controls had one
failure and seven errors. No mutant met its kill criterion (0/12). Both old
192 records reproduced exactly; successor analysis matched the original
guest but refused the patched guest with “ambiguous or unexplained winsys
maintenance run.” The same refusal affects the positive synthetic fixture,
so this is a classifier/design defect requiring diagnosis, not permission to
weaken the controls. `controls-01` remains intact. No replacement control run
or guest is qualified by the earlier source review.

A bounded read-only diagnostic (`198-diagnostic-01`, manifest prefix
`dd7afb55`) identified the run-forming defect in both the fixture and real
patched trace. The innermost sibling's INSERT and later REMOVAL have the same
first operand, with no other cache comparison between them. The classifier
therefore merged them despite intervening resource/use ioctls on that winsys.
The merged sequence satisfies neither operation shape. Other observed runs
assign uniquely; this does not prove there are no further defects.

The reviewed successor design splits at a winsys ioctl strictly between
consecutive comparisons, preserving thread/time ordering and unique operation
assignment. A single hash-table walk cannot issue that ioctl between its
comparisons. It must not split merely at the expected lookup length; a screen
with no separating use remains ambiguous and refuses. The failed run also
revealed that colored unittest output can defeat the mutant driver's literal
failure matching. Raw logs remain intact; the successor must disable color
for its own subprocesses while retaining the named-assertion-only kill rule.
Both corrections need new controlled evidence before a guest package.

Successor `198-02-premise-callback-classifier` ran once at
11:33:32–11:33:35Z, under source-input manifest `09369e2c`, and exited 1.
Source checks passed before and after, and no log contained an escape byte.
Core 24/24, callback 31/31 and signature 19/19 passed. Capture had one failure
out of 31, and mutants killed 13/14 after a passing unmutated baseline.
Both frozen 192 outputs reproduced exactly. Labelled successor analysis of
194 matched the declared expectations: original both `SCREEN_NEW`; patched
separate `SCREEN_NEW` with one miss, shared `SCREEN_SHARED` with one hit.
This does not revise 194 or establish a completed pixel comparison.

The failing capture control selected the last ioctl positionally. The newer
fixture ends with GEM_CLOSE, so the control changed that return instead of
the sibling winsys's GET_CAPS. The surviving removal-tail mutant was paired
with a refusal that also refused through another shape ambiguity; that
pairing did not isolate the omitted condition. These are proposed control
corrections, not a claim that no classifier defect remains.

`198-03-premise-callback-classifier` preserves the runtime core (`d825bfc2`),
capture parser, serial parser and CLI byte for byte. Its capture control
selects the shared sibling's GET_CAPS by tid, fd and command, asserting one
successful match before changing the return. The removal-tail mutant is
paired with the existing positive patched-count control. Its source-input
manifest is `a8edd8c6ed571a3e20d4d7742882c1848ce03f455a4223859cfa8c6caa94231b`.
It is unrun and awaiting review at shutdown; neither predecessor is repaired
in place. `198-run-records-claude`, manifest `c390bbf4`, preserves both failed
runs' launch, exit and result-hash records outside temporary storage. The
[resume checkpoint](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md#shutdown-and-resume-checkpoint-2026-10-09-utc)
identifies the pending qualification and saved runner drafts.

The pixel successor in `199-pixel-diagnostic-classifier` has completed CPU
qualification. It recognizes only the exact standalone line
`got error from kernel - expect bad rendering 2`, once per arm at most, as
the sole line between validated sibling-alive and after-drop records.
Full handle5 identity, freshness, readiness, assertion, summary, exit and
endpoint checks still apply. Raw logs are not rewritten. A separate report
marks a recognized diagnostic as driver `ERROR`, even if pixels match;
invalid records produce `UNQUALIFIED`, never healthy preservation.

All 60 retained policy controls, 20 new controls and five named mutant checks
passed. The old 194 handle5 outputs reproduced byte for byte. The frozen
54-file manifest is
`a977bfeed5a3d4f31749e60bc0b5f55b9ba1bc983929a9a95b686b0e3491b717`.
This qualifies a parser policy, not a Mesa repair or a runtime result. The
runner must still enforce the plan's original negative control and require
patched preservation with zero diagnostics. No new guest or production
workload result follows from these controls.

### 198-03 CPU qualification after resume (2026-10-09)

Independent review confirmed the GET_CAPS selector and the removal-tail kill
mechanism. One isolated run passed 24 core, 31 capture, 19 signature and 31
callback controls, then the baseline and all fourteen named mutants. The
removal-tail mutant fails `test_patched_counts_only_the_sibling_lookup` as
predicted. No mutant produced an ERROR or escape byte. Source pins verified
before and after; the 194 retrospective files equal 198-02 byte for byte.
This qualifies the classifier controls, not the Mesa repair.

The frozen package manifest is
`9619ca6a276318c8e9c9d77f0fbfb5da23a992ebb8fb95dcc83e8f5f3187ac28`.
`198-03-run-records-codex` records the declared wrapper and one launch under
manifest `fe33d10715d224bac4ec9ca34bf2a8d6bd6870e77b45a05f8f7aa9b05d4b2431`.
It also records the historical manifest construction error: all nineteen old
Claude payload entries verify, but its twentieth self-entry cannot verify.
That file and both failed predecessor packages remain unchanged. Selecting
the new removal-tail kill test after seeing the survivor limits independence
of its selection; the runtime core stayed byte-identical.

Preparation of 201 is now admitted around frozen 198-03 and 199. It still needs
independent review and controls before any 202 guest. The
[resume plan](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md#resume-progress-2026-10-09)
records the parallel lock gate and recovery candidate.

## The observable comparison reproduces loss and preservation (2026-10-09 UTC)

Runner `201-mesa-pair-observable-runner` is frozen 193 with premise4 from
198-03 and handle6 from 199. Its decision helper qualifies a context guest
only with clean infrastructure, the expected same-boot screen premise and the
declared pixel control. The original must keep the separate arm, lose the
shared arm on the named composition assertion with test exit 101, and report
at most one recognized diagnostic, in the shared arm. The patched guest must
preserve both arms with test exit 0 and a clean driver report. 201's own first
control run failed two orchestration cases and is preserved as failed, records
`34f867a0`. Both failures came from an expectation, not from the runner: 144's
serial is a valid PAIR under handle6, because its one Mesa line lies in the
declared interval. The control-only successor `201-02-controls` re-declared
those two cases, added two genuinely invalid ones and passed 64 orchestration
and 168 decision cases (package `b9d84497`, records `55c5339f`). 201 is frozen
under `e76032e45fda2997eef98f94b2a7b9ebb0f9f60950beb80889732141b911cb1b`
with READY `405512b9`. The host kernel is declared as 6.18.55_1; the guest
kernel stays 6.18.54_1, so 202 is not host-identical to 194.

The first launch was refused by its window preflight. A ninja and cc1 build
in another lane was running, and the runner's busy-process scan refused it;
no guest started. Those records stay in `202-launch-records-claude`, manifest
`d92a2a3e`. The successor launcher in `202-preflight-03` differs only in its
records and package paths (`65474c3d39f9126f0338919c7d2f877beec04a1196c0c55dbfdfd585435ee5b3`).
Its preflight pins the installed release and refuses a changed live session.
It ran once under an exact-argv GO from 15:13:52Z to 15:14:48Z, with every
other lane's builds held. The runner, pins and launcher all exited 0.

| Guest | Screen premise | Pixels | Test exit | Driver report |
| --- | --- | --- | --- | --- |
| observer | none | `OBSERVER_PASS` | 0 | none |
| original | separate and shared `SCREEN_NEW` | separate `PRESERVED`, shared `LOST_AFTER_SIBLING_DROP` | 101 | `ERROR`, one shared diagnostic |
| patched | separate `SCREEN_NEW` with one miss, shared `SCREEN_SHARED` with one hit | both `PRESERVED` | 0 | `CLEAN` |

Every guest had clean infrastructure, a clean endpoint, unchanged pins and
sources and no remaining process; all three decisions qualified. At context
level, then, the unpatched shared arm loses the producer's pixels after its
sibling is dropped, and the patched screen cache keeps them. This is a
screen-premise and pixel observation, not a mechanism trace. It establishes
no Sophia production effect, repair or t306/t307 acceptance.

The series manifest, outside the series in `202-freeze-claude`, is
`af1d65ee53589266c8932527483e03d1da89faef4219d3f3f92262e406ee33df`
for 103 files; the private runtime manifest is
`abcfa6a9d4df344d1781bc2560b5e4cdcae08b39ed303063535e7e1e926a304a`;
the launch records are
`146975d90640668bcfb702cb8f75cf71b50417317b101682969d59dc0cbf0af5`.
The production-workload comparison is the next authorized step.

## Sophia shares one card file description across renderer instances (hypothesis, 2026-10-09 UTC)

Source reading of the recovery candidate finds a Sophia shape that matches
the failing arm. `RealAtomicScanoutRenderDeviceDiscovery::from_card` keeps
`card.try_clone_file()`, and `open_render_device` returns `try_clone` of that
file (`hardware_validation/atomic_scanout_card/render_device.rs`). Both are
duplicates, so every renderer instance shares the KMS card's open file
description. Per-head workers are the default; the shared worker needs
`SOPHIA_ENABLE_SHARED_RENDERER_WORKER=1`. Each head's worker, and each
exporter's inline context, therefore opens its own GBM and EGL instance on
one description. GEM handles belong to the file description, so importing one
DMA-BUF through two instances yields one handle. Under unpatched virgl each
instance creates a new screen with its own buffer table, as 202's original
premise shows. When one instance tears down its import, as a lost head's worker
does, the handle closes for every other instance. That is 144's shared-arm
hazard, and it would leave retained images black on the remaining heads.

The KMS side is already careful about the same hazard. Compositor-owned
buffers come from the renderer's own duplicate and are used by handle without
a PRIME re-import. Only direct client scanout, which is opt-in, imports a
client buffer and closes its handle during cleanup. Image-import devices are
duplicated into each worker too, but they are opened only on the transfer path
after a direct import fails.

This is a hypothesis, not evidence. No trace ties the black frames of 153 or
earlier series to an instance teardown. The single-head 153 guest has one
worker, so another instance would have to be shown there, such as an inline
or capture context; that has not been established. The proposed control gives
each renderer instance a fresh open of the admitted device, revalidated
against the admission identity on the card, rather than a duplicate; 144
showed that a separately opened file preserves the handle. It belongs in the
production-workload comparison as a third arm on unpatched Mesa, with its own
CPU controls and review before any guest.

## The Mesa 26.2.4 context comparison repeats loss and preservation (2026-10-09 UTC)

The host now carries Mesa 26.2.4, so 202's 26.2.3 result could not stand for
the production workload's base. Package 209 built a matched private pair from
the 26.2.4 release tarball, whose checksum Void's template records, with 167's
configuration and patch; the two stage trees differ only in
`libgallium-26.2.4.so` (original `8dcf2b93`, patched `fdcaa7e6`). The five
files 165 reviewed are byte-identical to 26.2.3 and the patch applies without
fuzz. That similarity was recorded and qualified nothing. 210a-02 rebuilt the
143 device-test base on the 26.2.4 host (image `287918d8`; its predecessor
stopped on a pipefail record and is preserved). 210b and 210c derived the
original and patched images and their premise-init children (`1466b6cb` and
`7190493e`).

The first runner, 210d, stopped correctly at its original guest. Its
classifiers reach handle5's context identity check, which required handle3's
`Mesa 26.2.3`. The guest carried 26.2.4, so both classifiers refused the
identity before reading any pixel or premise. That series is preserved
unqualified (`7d1dda10`); no guest3 or replacement ran. An audit found this to
be the chain's only exact version dependency. 210e adds a context-only
`Mesa 26.2.4`, matched in handle3's three exact forms so that 26.2.40 is
refused; the observer, whose image is unchanged, keeps 26.2.3. 210e's CPU
controls ran over 210d's retained records. The old classifiers refuse the real
context byte for byte as retained. The successor prints, on the real log,
exactly what the old ones print when only its three version lines are renamed
to 26.2.3. Every wrong-version variant is refused at its stage. Runner 210f is
210d re-keyed to 210e. Its controls rename only the context fixtures' version
lines, so the positive series is mixed-version like the real one; they added
seven stage-specific wrong-version stops and passed 71 orchestration and 168
decision cases (records closure `bec3ec84`). The same controls with the old
classifiers fail 36 cases, every positive stopping at guest2 as 210d did.

210f ran once under an exact-argv GO from 17:55:12Z to 17:56:07Z, with the
installed release e3e6a9 live and root builds held. Its launcher reran the
window preflight; the launcher, runner and pins all exited 0.

| Guest | Screen premise | Pixels | Test exit | Driver report |
| --- | --- | --- | --- | --- |
| observer (26.2.3) | none | `OBSERVER_PASS` | 0 | none |
| original (26.2.4) | separate and shared `SCREEN_NEW` | separate `PRESERVED`, shared `LOST_AFTER_SIBLING_DROP` | 101 | `ERROR`, one shared diagnostic |
| patched (26.2.4) | separate `SCREEN_NEW` with one miss, shared `SCREEN_SHARED` with one hit | both `PRESERVED` | 0 | `CLEAN` |

All three decisions qualified, with clean infrastructure and endpoints and no
remaining process. At context level the 26.2.4 pair therefore repeats 202:
the unpatched shared arm loses the producer's pixels after its sibling is
dropped, and the patched screen cache keeps them. As with 202, this is a
screen-premise and pixel observation; it establishes no Sophia production
effect, repair or t306/t307 acceptance. A retrospective readout of 210d's
retained original guest under 210e has the same shape and qualifies nothing.

The series manifest is
`b50b28ea953bbc2b70c2b5e35c35e4198f4a3ed7a7d4693b48a2faf89bd7916e`
for 103 files; the launch records close under
`1089222ded4a19353df49e6973497f7f744f40a444fc9b05b96654c9488eb7d9`.
Package 211, the production-workload comparison on 26.2.4, is next. A
non-reproducing original can leave t307 as a declared limit only when that
original workload has complete sampling and importer coverage and a qualified
`RETAINED` verdict; `INSUFFICIENT` or an ordinary `UNREADY` is no such result.

## The production comparison stops before its first verdict (2026-10-09 UTC)

Package 211 ran part 2 on Mesa 26.2.4: the candidate `0f84dcb0c` built into
base image `1330b90a` (211a), 209's pair installed into it (211b, original
`556cf9f5` and patched `b3c2f677`, differing only in `libgallium-26.2.4.so`),
and a four-boot runner, original, patched, patched, original (211c). The
runner's host verifier is the 212-02 gate's build of `5efab3d2c`, xtask
`2c5c7d0b`, which refuses a recorded unreaped process or stopped guest before
any verdict. A boot qualifies only when the harness itself reached that
verifier through a fixed adapter, the recorded endpoint is one clean exit,
the verdict exits exactly 0 for `RETAINED` or 1 otherwise, and the harness
agrees. At this harness a session that never becomes ready ends before the
verifier, so `UNREADY` cannot qualify; the declared negative control is a
qualified `LOST`. The runner's controls passed 66 orchestration and 90
decision cases (records closure `b3d868a4`), after root's review withdrew a
first freeze whose decision accepted matching timeout exits and whose fourth
boot could pass unchecked.

The series ran once at 19:09Z under the checked launcher 211d, whose window
preflight passed again; the launcher, runner, final pins and outcome all
exited 0 and its records close under
`ede5fc33b827a08f95e1835a2f08cf94cb51203999e28dae629a31e3f328cd5e`. The
first boot, original, had clean infrastructure and one clean guest exit, but
its session never reached startup readiness: the guest ended with
`persistent live session never reached startup readiness: ... visual_detail=0`
and its failure record, the host stopped waiting for monitoring, and the
harness exited before its verifier. The boot was refused as declared, no
later boot ran, and the outcome is `INCOMPLETE`. 211 stays as it is.

Outside acceptance, the boot's own records show two renderer identities, one
per head, each importing the client buffer at the same geometry. Head 1's
first presented window region of the client's frame is entirely black and
head 2's is the client's frame. The native owner closed drained, with no
pending retirement; the `RetirementFailure` carries the readiness proof's
failure, not a stuck retirement. This matches the shared-description
hypothesis above but does not establish the Mesa handle-lifetime mechanism,
and it is not 211's negative control.

Root's decision is a separate patched-only diagnostic, package 213: two boots
of the same patched image with the same source, workload, renderer mode,
verifier, readiness and infrastructure checks, and no image rebuild,
readiness relaxation, startup substitution or fresh-open code. Only two
qualified `RETAINED` boots report `PATCHED_WORKLOAD_RETAINED`; a `LOST` or
`INSUFFICIENT` boot is diagnostic non-success, and a refusal stops the series
without replacement. None of these is `PATCH_EFFECT`, completes 211, or is
t306/t307 acceptance.

## t307

The [four-part qualification plan](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md)
records the authorized bounded comparison and production-workload exits.
The underlying investigation obligations remain:

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
