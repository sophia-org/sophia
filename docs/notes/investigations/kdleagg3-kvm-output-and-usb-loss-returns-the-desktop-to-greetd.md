---
id: kdleagg3
date: 2026-10-04
kind: investigation
status: investigating
tags: [investigation, topology, drm, input, recovery]
---
# KVM output and USB loss returns the desktop to greetd

## Question

Why did switching the operator's monitor and USB peripherals through a KVM
away and back end the published desktop session? Recover supported output loss
and return while retaining input, buffer custody and lock-cover guarantees.

## Evidence

Read-only report: `kvm-hotplug-exit-01/SUMMARY.txt` under
`~/.local/state/sophia/development-evidence/`. The installed release was
niltempus-99bb041fe3535b5d265d, Sophia 6ae5df00a; session
00000001791159617112-54e9ffa4-f317-482f-abcf-f73d50eaa600.
The operator observed a return to greetd after switching the KVM.

The retained timeline uses UTC on 2026-10-05 (local evening 2026-10-04):

- 00:39:32.919: USB hub 5-2 and child disconnect; input epoch 5 at .929.
- 00:39:33.034: amdgpu 0000:03:00.0 reports REG_WAIT timeout in
  dcn32_program_compbuf_size during the display change.
- 00:39:33.149: native owner records settled=true, in_flight=false.
- 00:39:33.163: owner-loop runtime fatal, failure_code=unclassified;
  native suspend records forced_detach_drain_error, drained=false.
- Session failure phase=topology, exit 1; handoff returns to the display manager.

The source sets failure_phase=Topology before including topology_phase.rs at
physical_input_loop.rs:560-563. Several fallible operations live in that phase;
the phase label does not identify which one failed. No preceding topology
records or original error text were retained in the session evidence. Session
health reports 279564 suppressed and 14047 discarded records. Sophia stderr on
the greetd VT was not forwarded to socklog.

## Finding and resolution

The disconnect is the observed trigger. The exact Sophia error and any causal
role of the amdgpu timeout remain unknown. Do not classify this as the rare
QEMU flip stall or infer that suppressing every topology error is safe.

There are two concrete work items: preserve a bounded, useful topology error in
the session records, and diagnose why the observed loss ended this session.
Expected recoverable output removal must have a defined parked or remaining-head
state and a return path. Irrecoverable device or ownership faults still need an
explicit, safe disposition. The operator currently avoids KVM switching as a
workaround; that is not acceptance of recovery.

## A maintenance release prepared from current master (2026-10-08)

After the descriptor-isolation comparison in series 132, niltempus asked
to prepare a new live candidate. Signed Sophia `19403a511` starts from
master `d336f698b` and contains the installed `825d9146` baseline. It
adds three reviewed repairs from the diagnostic branch: bounded private
failure-cause reporting (`85bbc869b`), ownership of the descriptor
libseat returns (`fc7ca1e07`), and release of that device when duplicating
its descriptor fails (`5f954ef3d`). The port retains master's t309 failure
recording and t312 seat scoping, including its udev feature dependency.

The larger topology and renderer-image restore changes are excluded.
The early topology repair discards static content, and its later
replacement still lacks accepted managed-head and all-return evidence.
The existing t307 black-frame investigation remains open. This is a
maintenance candidate, not a claim that monitor loss now recovers.

A new CPU regression exercises the actual ownership adapter through
libseat's noop backend on `/dev/null`, in a child bounded by ten seconds.
Across 64 open/close cycles every released descriptor disappears and
the descriptor count returns to baseline. A disposable leak mutant is
refused. The parent also requires the named child test to run; a filter
matching zero tests is refused. Logind's release-before-close ordering
and the broker's duplicate-failure path retain source-review coverage.

The final full `cargo xtask check` passed on clean `19403a511`, including
strict lint, layout and retained archive checks, with devices and network
hidden. An earlier test lint failure is preserved in the evidence; its
correction and the final gate were independently reviewed with Claude.
No guest or physical acceptance test ran for this maintenance candidate.

Signed niltempus integration `2f3ed993` changes only the Sophia lock node
from the installed integration `83c34a4`. The Nix build produced
`niltempus-087445319affcb9bfc53`; profile and policy validation passed,
and all 87 release checksums verified. Hagia, narthex, Lom, Bemenu and
kleis binaries match the installed release byte for byte. The desktop
profile differs only in its embedded release paths.

The candidate is retained at
`/nix/store/7pfppl5qxdnf68s6xg33ksx5pgv9mk2q-niltempus-desktop-niltempus-087445319affcb9bfc53`,
with a GC-root link at `target/live-candidate-release` in the main
Sophia checkout. It has not been installed or published. At this review
the current release remains `niltempus-f18fc2ed5aa55e0f6132`; installation
and an attended new login remain separate from these checks.

Evidence: `t306-01/133-promotion-review` and
`134-live-maintenance-candidate`, whose 47-entry manifest is
`79a045f212086df0ddc187d1523b37537a49ff25f838934133cf158ebde403a8`.
The latter retains the source diffs, signed identities, controls, gates,
release hashes and exact proposed install/rollback commands. Neither
t306 nor t307 is accepted by this candidate.

## Installed baseline and public source pin (2026-10-09 UTC)

After that review, niltempus installed `niltempus-087445319affcb9bfc53`
and confirmed a new login. At 02:49:07Z the running session's PID 25091
resolved to that release's Sophia executable, whose SHA256 was
`2aca36ddd8e93b160516039259e3239eeb9ad08b86df833297515f8453bd4d79`.
Session `00000001791513578420-31f7e0c8-050d-49e7-a3a4-20dd934860ce`
records source `19403a511`. Startup completed preflight, input guard and
graphics takeover; the seat was active and fourteen input devices were
admitted. The sampled native-renderer and capture-failure counters were zero.

The read-only snapshot contains no session-failure or fatal record. Its
health reports no discarded records, rotated bytes or storage errors, and
28,395 suppressed records. The files were copied sequentially while the
session ran; suppression limits absence claims. Listing sessions printed
the running record and then failed the preserved-directory permission check;
inspection by the explicit session ID succeeded. No permissions changed.

Signed merge `9b1c86b18` publishes all five maintenance commits on master.
Its production tree equals qualified `19403a511`; only this investigation's
earlier candidate section differs. Signed niltempus `a08719f` publishes a
GitHub pin to exact Sophia `19403a511`. Its fetched content hash equals
the installed candidate's local pin, and every other lock field is unchanged.

That portable integration built `niltempus-f8cf882c158e59026c46` and passed
all 87 release checksums. Every binary is byte-identical to the installed
maintenance release; only release metadata and embedded profile paths differ.
It is retained through `target/live-maintenance-portable-release` and was
not installed. The current release and its `f18fc2ed` rollback remain intact.
The two superseded maintenance worktrees were removed after confirming they
contained no modified, untracked or ignored files; their branches remain.

Evidence: `t306-01/135-live-maintenance-publication`, whose 23-entry manifest
is `fa3badedb19a112eb6a8c6b1db502902e4e85172c637ca0d340c80966e4d9006`.
This records the live maintenance baseline and portable publication. It does
not qualify output loss/return, retained pixels, or physical KVM recovery.

## Complete restore unit ported and CPU-qualified (2026-10-09 UTC)

The isolated `candidate/t306-hotplug-20261008` branch at signed `45ac1d81c`
ports the seven remaining topology/restore commits onto maintenance master
`9b1c86b18`. It includes the per-device restore planner, source availability,
pending-snapshot custody, storage-progress wakeup and busy-alternate retry
corrections. All ports applied without textual conflicts. The early policy
that discarded retained images on a changed head set is not the endpoint.

The clean candidate passed the full device-hidden `cargo xtask check` from
02:51:15Z to 02:58:15Z: exit 0, strict lint, layout, verifier checks and six
retained direct-scanout archives. Raw Rust summaries total 7,264 passing,
zero failing and 101 ignored entries; nested summaries are not deduplicated.
An independent source review found no blocking overlap with current seat
scoping, CPU scene reconfiguration or lock handling.

The candidate is neither merged nor installed. No new guest ran. Series 45's
failed verdicts stand; managed-head and all-head loss/return, retained pixels
and native-owner retirement while locked still need qualification. The
existing CPU lock test covers addition of an output. It does not cover that
retirement path. The t307 black-frame problem remains unchanged.

Evidence: `t306-01/136-hotplug-current-master-cpu`, whose 12-entry manifest
is `ce787707cdde41e5b5ef45d4be0008ebfa6f771a119326535d7e6551df1b3687`.
It retains the source diff, signed identities, gate and semantic review.

## Generic fixture gate and remaining verifier gaps (2026-10-09 UTC)

Signed tooling candidate `7b9f80bc9` keeps the production restore unit above
and ports the generic WM, static DRI3 probe and output-unplug verifier. Its
full device-hidden gate passed from 03:05:15Z to 03:09:39Z, including layout;
raw Rust summaries total 7,281 passing, zero failing and 101 ignored entries.
Focused checks passed all seventeen output-unplug controls and four probe
controls. The verifier computes the probe's expected pixel checksum from its
pattern, binds presentation to the same native owner, refuses unstable
baselines and bounds image dimensions before computing the reference.

Source review found two inherited acceptance gaps despite those passing
controls. All-head return accepts any topology record after the first
removal; it does not require an unavailable topology after the last removal
or a guest observation that every connector is disconnected. Input return
accepts udev counts without proving that Session admitted the returned
keyboard and delivered its input to a client. Both modes need stronger
fixtures and refusal controls before their verdicts can support acceptance.

Evidence: `t306-01/137-hotplug-tooling-cpu`, whose 15-entry manifest is
`9203a878bc22aaee60ab4921268c67005712cbb1cf3b878a53088c257210ab73`.
The review's appended correction supersedes its initial no-finding statement
for those modes. No guest, merge or installation followed this gate.

Signed candidate `c12e87063` corrects the all-heads premise. The guest samples
connector status throughout the removal window; the verifier requires zero
connected outputs and a Session unavailable topology after the last removal
and before the first return. Missing, malformed, inconsistent or repeated
observation fields are refused. All eighteen focused verifier tests pass,
including sixteen new refusal cases. The new regression fails against the
old verifier because it accepts a log with the zero-connected witness removed.

Evidence: `t306-01/138-all-heads-fixture-correction`, whose 20-entry manifest
is `44027f85c68c208425b850c746359f093831baf50243610751f2bfbda7dac362`.
The tested diff equals the signed commit's diff. This is a focused CPU check
of test tooling; the combined fixture gate, input-return proof and guest
qualification remain separate. The production restore candidate is unchanged.

Signed candidate `6b66c45c7` adds the input-return chain: Session admits K0,
the managed client receives the baseline key, K0 is removed, a distinct K1
is admitted, and the returned key reaches that client. Synthetic X events,
probe failures, ambiguous records and sends completed after shutdown are
refused. Input mode requires a normal client exit. All modes now bind the
guest's scenario completion and zero QEMU exit to the bounded endpoint.
The focused controls pass 21 Rust tests, including 45 chain refusals and
the endpoint cases, plus five probe/C tests.

The combined gate passed from 04:09:32Z to 04:17:28Z on a dedicated hotplug
target: strict lint, layout, repository checks and six retained archives.
Its 526 raw Rust summaries total 7,285 passing, zero failing and 101 ignored
entries, including nested summaries. An independent audit verifies the
source worktree, target and relevant test family in the actual log. The
earlier attempt's exit 0 was rejected: a shared target reused an `xtask`
executable bound to the freshness worktree. That attempt and the correction
remain in the evidence; they do not qualify this candidate.

Evidence `139-hotplug-fixtures-cpu` has 41 entries under manifest
`d7dbb53573c4ff3a1ac344c627fc424521950485599a9d18ca927d8724f50a04`.
The production restore unit remains unchanged, and no hotplug guest or
installation follows from this CPU result. Retained-pixel, loss/return,
lock and attended-device acceptance remain separate.

## t306

1. Preserve the incident records. Make the next failure name the responsible
   operation and retain its bounded error text independently of ordinary event
   suppression or stderr routing. Cover diagnostic retention in a regression.
2. Trace loss/rescan/preparation/retirement and error propagation. Reproduce the
   relevant transition with a bounded fixture: distinguish one-head loss,
   all-head loss, USB-only loss and combined loss/return. QEMU unplug or t303's
   virtual output work may help, but must actually exercise the same boundary.
3. Repair the identified owner transition. Keep submitted buffers until their
   completion or safe device teardown; preserve input epochs and held-key
   cleanup, lock coverage, and bounded retry without spinning. Do not blanket
   ignore topology errors or relax the page-flip watchdog.
4. Require a regression that fails without the repair, repository checks, and
   an attended KVM away/back test on the operator's devices. Check restored
   outputs, keyboard shortcuts, pointer routing and lock/unlock afterward.

The task has high priority because normal device switching ends the session.
State and execution order live in [todo.md](../../../todo.md).

## Connections

- [Rare QEMU page-flip stall](3v4qwldr-rare-qemu-native-page-flip-hard-stall-after-successful-unlock.md)
  is a separate unresolved incident; no shared cause is established.
- [VKMS capture candidate](jweorh0z-headless-sophia-validation-and-capture-with-vkms-writeback.md)
  may provide an additional reproducible output fixture.
- [Published lock repair](../plans/qrstyyjn-restore-lock-animation-and-input-responsiveness.md)
  names the accepted release in which this later incident occurred.
