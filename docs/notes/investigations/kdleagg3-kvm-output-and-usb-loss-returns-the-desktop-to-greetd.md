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

## Keyboard-return guest exposes a fixture ordering gap (2026-10-09 UTC)

CPU preparation 145 built the signed `6b66c45c7` candidate and image
`8d7127f0`, checking the three binaries, init, DRI3 probe and generic WM
against their in-image bytes. Its 37-entry manifest is
`ca46cb81ac1248dd68c4815bf582f3f4a6764a9db9f4f26357168d9fb156c997`.
The complete inventory comparison against 140 declares four changed paths
and five removed diagnostic paths; retained library contents, ownership and
non-directory hard-link groups match. The fixture source identities are in
`guest-tools/FIXTURE.txt`.

Package 146 froze the gate's own `xtask` binary. A strict adapter dispatches
only the one input-return verification command, so the harness cannot build
or select another checkout's verifier. CPU controls pass 9 adapter cases,
8 cleanup cases and 45 infrastructure cases. The latter joins the patched
QEMU's loaded identity, wrapper records and independent kernel exit witness;
it treats debugger loss as refusal and keeps the test verdict separate.

The one guest attempt, series 147, ran from 04:39:41Z to 04:40:31Z under
`REVIEW-CODEX-146-GO.txt`. Infrastructure was `INFRA_CLEAN`: QEMU, debugger
and wrapper exited 0, the guest powered down, and no process remained. The
frozen verifier refused the run with `keyboard off does not precede K0
removed`. Harness and direct verifier both exited 1 with the same diagnostic.
The refused verdict remains unchanged.

Raw records show keycode 38 reaching the client from K0=257, then keycode 56
from returned K1=262, both with `synthetic=0`. Session's removal of K0 is
line 230; the fixture's `off sent` marker is line 231. The fixture emits
that marker after the sysfs unbind returns, while the verifier requires it
before Session's resulting notification. Rebind has the analogous ordering
risk. This calls for explicit before-write and successful-completion markers,
with controls for both notification orders, rather than changing the old
verdict. The raw routing records alone do not accept this run.

Evidence: `146-hotplug-input-package`, manifest
`e9c12996815518f6f9a9bdd08ec3fb830022bbb31e49f3ff91523f8a82e8a30c`,
and read-only `147-qemu-input-return-series`, whose 36-entry manifest is
`fce040b9b34f0b1c54f31de5fea6b987a2e056c24e0aa13f74626a18538ec41c`.
The result note is `147-qemu-input-return-result/RESULT.txt`. No replacement
guest, display-loss, lock, physical KVM or t306 acceptance followed.

### Keyboard write markers and the corrected CPU gate

Signed candidate `b12f0720c` corrects the fixture's ordering contract. It
prints `sending` before each keyboard unbind or bind write and retains
`sent` after a successful write. The input verifier requires exactly one
attempt and completion per action, all naming the same virtio device.
Session's removal or admission may precede or follow the completion, but
must follow the attempt and precede the corresponding phase marker.
The existing key, focus, device identity and endpoint checks remain.
The input-return verifier now has its own module; reconstructing the
parent from `6b66c45c7` confirms that display and static-image logic did
not change. This four-file commit changes no production renderer or
session code.

Evidence `149-hotplug-write-marker-patch` contains the reviewed patch,
24 passing focused tests, all four notification/completion orderings,
and 21 new refusals. The same tests against `6b66c45c7` have four failures;
the reproduction of 147's order fails with its original diagnostic.
The corrected verifier still refuses the unchanged 147 log because its
attempt markers are absent. A separately labelled synthetic copy with
those markers inserted passes the later chain; it is informative only.

Gate `150-hotplug-write-marker-cpu` ran on clean `b12f0720c` from
04:53:53Z to 04:58:20Z on October 9, using the dedicated hotplug target.
The full check exited 0, including layout and both ordering regressions.
Its 526 libtest summary lines total 7,288 passed, zero failed and 101
ignored, including nested summaries. The independent source/target audit
passed eight controls. The frozen 22-entry manifest is
`1c7fff159c2cf5b0d5d7506b67d660da3701ba529d3d01782484a0df1a308ff4`.
This qualifies the corrected fixture for a new bounded guest; it does not
replace 147's refusal or accept t306.

### The next input guest stops before the fixture

Image 151 reuses 145's frozen production binaries. Its complete comparison
retains all 5,966 paths, ownership, modes and hard-link groups; only the
corrected init bytes and build output-directory record differ. Its manifest
is `8928a87c39de050258f8db60d1804aa40ad7706df123dfb5d340fb2ad53fe9e1`.
Package 152 freezes gate 150's verifier `de99b628`, with nine passing
adapter controls and two exported-log controls. The unchanged infrastructure
and cleanup helpers explicitly reuse their 146 controls. Its manifest is
`dcfb5b72973c2eccabec1dbd7bae7ca32dbe1a49688f2877914fe136923fcfe1`.

Series 153 ran once under `REVIEW-CODEX-152-GO.txt` (`4fef92ab`), from
05:02:06Z to 05:02:52Z on October 9. It never reached the input fixture.
Sophia reported a startup `RetirementFailure`, with a surface and applied
focus but `visual_detail=0`; the guest then reported `unplug_session_exit`
and powered down. The host timed out waiting for baseline readiness before
issuing a key or keyboard write. The direct frozen verifier exited 1 for
missing unplug uevents. The harness exited before invoking its verifier
adapter, so there is no adapter argv record.

Infrastructure is `INFRA_REFUSED` solely for the absent `guest_exited`
endpoint. The watcher, QEMU identity, wrapper, debugger, inferior and kernel
exit checks pass; all processes are gone. Outer exit 0 records completion
only. The renderer records show black, correct-pattern and black frames
before any fixture action, as detailed in the
[sampling investigation](r2m9cx6v-static-retained-dma-buf-images-can-sample-black-before-hotplug.md).
The input-marker correction was not exercised; no replacement guest ran.

Evidence `153-qemu-input-return-series-2` is read-only, with 38-entry manifest
`ed8a1a57302f756c9bdaae0ea80906a4530351056bdd5954eb5cc2544afac403`.
The separate result is `153-qemu-input-return-result-2/RESULT.txt`
(`9374400a`). Source and pins remain unchanged. Display loss/return, lock
coverage across topology changes and attended physical KVM acceptance remain
unproved; the installed session is unchanged.

### Preserve the endpoint after a fixture failure

Signed candidate `7d4bad641` changes the host harness's failure path without
changing the guest image, startup proof or production code. Its wait helper
distinguishes a guest failure, a process exit and an actual timeout. An
existing failure wins over a readiness marker. After recording the original
failure, the harness collects QEMU and logger statuses once, exits 1 and
does not invoke the verifier. A clean endpoint after a failed fixture thus
remains separate from the test result.

Cleanup uses bounded TERM and KILL phases for QEMU, the serial logger and
the private display bus. A child still present after the final deadline is
recorded as `unreaped`, with its PID and no invented exit status. An
unreaped bus also prevents verification. The output-unplug cleanup trap is
installed before that bus starts; other scenarios retain their existing
cleanup. Local deadlines do not replace the runner's outer timeout or its
independent leftovers check.

The reviewed three-file patch is in `157-unplug-failure-endpoint-patch`,
revision `59edd9d0`, with source/CPU disposition in
`158-unplug-endpoint-root-review`. Eleven endpoint tests use stand-in
processes, including the series-153 failure shape, logger failure, forced
stops and simulated unreaped children; all pass. The existing 24 unplug
verifier tests, shell syntax checks and Rust formatting also pass. The
initial bare-rustc check lacked Cargo's manifest-directory environment;
that check-method failure is retained beside the corrected invocation.
The full integration gate 159 also passed: 527 libtest summary lines total
7,299 passed, zero failed and 101 ignored, including nested summaries;
source/target provenance passed 11 controls. Its 25-entry manifest is
`9d94c487778e7391cfc7c550e0953eb76e8b37facfb2d15422776a3217799f76`.

That passing gate does **not** qualify this candidate. An independent
stand-in reproduction found that Bash can wait for a whole pipeline job
after its last PID has disappeared. The real serial logger has that shape.
Calling the actual endpoint helper with an earlier pipeline member still
alive blocked until the four-second outer KILL, exit 137. The original
11 controls missed this case. `158/REVIEW-03.txt` supersedes the earlier
qualification; 159 retains the known blocker.

Signed successor `3fc8b4495` gives only output-unplug's serial logger a
dedicated background owner, preserving the original pipeline's shell
options and status. The added regression fails on the raw pipeline after
30.01 seconds and passes on the corrected owner within the declared test
bound. Equivalence controls retain reader-failure and `tee`-failure statuses;
the real FIFO launch also preserves CR stripping and the final unterminated
line. All 13 endpoint tests and the existing 24 verifier tests pass. Patch,
earlier draft, and red/green results are in `160-unplug-endpoint-r3`, manifest
`91f254c8b28b6b65dcf52a72967e1a0319f13804751412be7039445783176bbb`.

The successor's full isolated gate 161 ran from 05:35:28Z to 05:39:32Z on
October 9, exiting 0. Its 527 summary lines total 7,301 passed, zero failed
and 101 ignored, including nested summaries. All 13 endpoint regressions
ran in this gate, and source/target provenance passed 12 controls. The
freeze checks both committed blobs and working files against the reviewed
patch hashes. The 27-entry manifest is
`7334e4860a304cb23356b2f14a15d77f6bb7787645e1b6ae783255e598550f21`.

A future runner must pin the new sourced helper and refuse recorded
unreaped processes even if they disappear before its final scan. The older
infrastructure classifier is not automatically qualified for these new
records. No image or guest follows from this CPU repair, and 147 and 153
keep their original verdicts. The black-frame startup failure still blocks
qualification of the input-return fixture.

## Lock publication and pending-image scope (2026-10-09 UTC)

Read-only review `195-lock-publication-source-review` narrows the proposed
lock/hotplug characterization in 154. Custody revokes an outstanding provider
candidate when publication removes its allocation or changes its generation,
and journals that outcome to the provider. Session receives no corresponding
revocation event; its publication hook updates diagnostic pacing state while
retaining the pending candidate and shown image. A demand for the returning
allocation can therefore remain behind that candidate.

This source observation does not establish a permanent stall. Retirement of
the old image on the returned output can remove Session's pending candidate
and release the demand. Custody refuses the then-stale outcome, and the service
explicitly absorbs that refusal. A CPU characterization must exercise both
the publication/custody path and this recovery path before choosing a repair.
It would not itself prove actual native topology installation or returned-head
retirement. Those tests still need a legitimate concrete scanout seam.

The review is source-only on `8e1782309c`, with exact source hashes retained
under manifest
`1a02b0fd10fe278bf7f72d4490563ffba9acef5ecf17f2ba2523f19a162bb927`.
No test, production change, lock guest or physical acceptance followed from
this review.

## Lock characterizations and topology coverage (2026-10-09 UTC)

The authorized [qualification plan](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md)
separates CPU lock characterization, production topology coverage, virtual
hotplug qualification and attended physical acceptance. Evidence
`200-lock-publication-characterization` initially tested an uncommitted
implementation on signed base `a7050f1e1`; its per-run source snapshots
identify the tested bytes. This is not yet a gated integration candidate or
a frozen package.

The publication tests exercise the real LockPublication, LockFileCustody and
SessionLockFrames. They distinguish custody revocation from Session's retained
candidate, demonstrate old-image retirement releasing the returning output's
demand, and cover unrelated outputs, wrong receipts, resource retirement,
new lock and reconnect. A separate real service test sends a late outcome
for a revoked candidate and confirms that the worker continues to process a
subsequent request. These characterize the recovery described in review 195;
they do not justify a new revocation policy.

The topology tests now enter shared production rebind/resume functions with
controlled device facts. They do not directly replace the runtime's output
list. Coverage includes loss/return while locked, loss before initial cover
proof, mirrored heads, all-output absence followed by resume, and an unlocked
control. On an exported source copy, the four-test baseline passed; clearing
the cover inside either actual rebind or resume caused its named regression
to fail with exit 101. Both mutant results are retained in
`mutations-01/results.json`.

A passive diagnostic implementation reports lock epoch, topology epoch,
output count and head count after current cover retirement. Its tests require
the current lock epoch, unique current head identities, every head retired,
and no suspension; publication suppresses repeats and stale topology epochs.
This adds observability without changing the Locked transition. The binding
between native coverage and the published topology is part of the independent
source review, not an assumed guest qualification.

Independent review `189-claude-reviews/REVIEW-200-01.txt` (`54894568`), bound
to the working diff `258d2090` and untracked-file hashes on `a7050f1e1`,
confirmed the device-call equivalence and the current-epoch all-head proof.
It found a diagnostic attribution gap: installed heads can belong to N+1
while the WM still publishes topology N during its presentation wait. Their
retired covers must not be reported as proof for N. The record needs a settled
installation/publication binding and a control for that interval before it
serves as qualification evidence. The review also requests direct refusal
controls for unavailable frame service and a target naming another output.

The source correction requires the topology owner to be Stable, no hardware
publication pending, no active policy candidate, and the public epoch equal
to the installed owner's epoch. Its new control exercises a real topology
owner through rebuild and presentation settlement, withholding the first
coverage record until those identities agree. The two backend refusal controls
are added, and the unrelated owner-loop formatting was dropped. Focused
`checks-05` passed twelve backend coverage tests, 26 real topology-owner tests
(including the new attribution regression), and two diagnostic publication
tests. Clippy with warnings denied passed for the backend, Session and runtime
crates, all features and tests. The 18 source files in
`source-05.SHA256SUMS` verified before and after. Earlier passing logs remain
bound to their earlier source snapshots.

Focused isolated checks passed: seven publication tests, ten service tests,
and, after the diagnostic addition, twelve backend coverage tests plus two
diagnostic publication tests. `source-03.SHA256SUMS` and
`source-04.SHA256SUMS` bind the corresponding snapshots and logs. The first
publication run failed because its fixture requested a one-second permit
against the protocol's 250-millisecond maximum; it is retained alongside the
corrected 100-millisecond fixture. No production defect is inferred from that
failed fixture. Independent review, full integration gate, locked guest and
physical KVM acceptance remain separate obligations.

Independent delta review `189-claude-reviews/REVIEW-200-02.txt`
(`a6c8826b`) compared all 18 files with `source-05` and found the settled
epoch binding and requested controls resolved. The implementation and its
normative diagnostic description were signed as
`006434bedcb723c2b5d3eafb8c4ecf596ab4c4a5` on master.

The first full gate on that commit, `200-lock-publication-characterization/gate-01`,
ran 11:35:02–11:35:09Z and exited 1 on the C SDK contract drift check for
`docs/sophia-lock-files.md`. The SDK's immutable snapshot retains the previous
contract bytes; the Rust SDK also compares this document. No full test-suite
pass follows. Source pins passed, the source tree stayed clean, and the
launcher recorded `STOPPED gate_exit=1 pins_exit=0`. The preserved gate
manifest is `b41a3a44f3022d5ac3141c4d53e2cfdcdca909be9019a1505169e16141fdd442`.
The gate finished before shutdown preparation and was not killed. The
[resume checkpoint](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md#shutdown-and-resume-checkpoint-2026-10-09-utc)
records the documentation/provenance correction and fresh gate still needed.
No locked guest, live installation or physical acceptance occurred.

### Resumed lock gate and recovery integration (2026-10-09)

The resumed full gate passed on signed `f1effad0daf38e7a9ad4eaef20fb2214dea2ac27`.
`3539415dc` restored the provider contract's previous bytes and moved the
internal coverage description to [Session lock diagnostics](../../session-lock-diagnostics.md);
no vendored snapshot changed. The following commit only orders one import.
Gate 02 preserves that formatting refusal. Gate 03 reached an unchanged
conformance fixture whose Unix socket exceeded `SUN_LEN` under the long private
target; gate 04 uses `~/.cache/sophia-lock-200-target` with the same isolation,
umask 0077 and source. It passed workspace tests, SDK checks, clippy, layout
and verifier archives. Its source checks pass and the tree remained clean.
The frozen `200-lock-publication-characterization/gate-04` manifest is
`c2517d2c837bce6a140311d50d7b01964b02be326732c68c4f343efa9aef63bb`.

The new recovery worktree combines current lock coverage, the frozen hotplug
candidate and accepted startup fallback. Signed merges `75af47276` and
`497464309` preserve both the production resume test boundary and image
restoration bookkeeping. This is a candidate, with display marker ordering,
integration checks and guest qualification still outstanding. The installed
session remains unchanged. The [resume plan](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md#resume-progress-2026-10-09)
records identities and the classifier result prerequisite.

### Installed Session renderer failure interrupts qualification (2026-10-09)

niltempus reported a fresh live-session crash during the resumed work. The
installed t310 release, Sophia `838d5b16a`, recorded
`renderer_retained_buffer_missing` at 13:29:50.007Z, retained the cause
`live renderer scanout export failed: RetainedBufferMissing`, drained native
scanout during cleanup, and returned to greetd with exit 1 at 13:29:56.008Z.
Its last retained ordinary records show WM layout transactions 30 and 31,
about 98 milliseconds before the fatal record. Niltempus subsequently confirmed
switching workspaces; no hotplug cause is established.

`204-live-session-crash-20261009/session` preserves all seventeen session files
with a separate checksum manifest. The diagnostic health reports 63,301
discarded and 1,676,130 suppressed records, zero storage errors: absence from
this event stream cannot exclude an unrecorded event. The host's `oom_kill`
counter is zero since boot; the later capture has 58,875,576 KiB available.
Unprivileged kernel-log access failed and noninteractive sudo required a
password, so no kernel GPU-reset verdict is available.

At the reported crash, the root lane was compiling the signed combined
candidate `1e63d9c71` inside device-hidden isolation with eight jobs. It had not
installed that candidate or launched a guest. Claude's controls ended at
13:23:07Z; subsequent work was source reading and package hashing/freezing.
Both lanes stopped. Root sent SIGTERM only to its identified gate timeout;
`203-recovery-integration/gate-01` retains exit 143 and unchanged source pins,
an intentional interruption rather than a test failure. Its earlier focused
display/endpoint checks passed 29 and 13 tests. No automatic build or guest
restart followed this incident. Association with compilation alone does not
establish causation.

Source review and a device-free real-exporter regression subsequently found a
deterministic late-render defect, recorded in the
[worker-stall investigation](h833kgfy-one-hard-stall-of-the-rendered-scanout-export-worker-ends-the-session.md#late-completion-defect-found-after-workspace-switch-crash-2026-10-09).
The installed worker discarded a reply after declaring a hard stall, then
returned `Idle`; the exporter had no staged replacement and refused with
`RetainedBufferMissing`. Presentation withholding makes that empty slot
reachable. The retained physical incident cannot prove this path, because
worker warnings were excluded from daily capture, and the same detail named
several producers.

Signed candidate `bacfdb207567afa8dc57ba5a99ba14f381afbe74` retains the
accepted identity and validates late replies, captures worker transitions
and sampled presentation deferrals,
and gives missing-frame/descriptor/owner failures distinct typed codes.
The backend/renderer suites passed 1,136 tests (11 ignored); six reducer
tests and the CLI durable-capture test passed with console logging disabled.
Independent review found no blocking issue. The repair passed its full CPU gate,
one job at nice 10 with devices hidden, in `205-workspace-render-recovery/gate-01`.
Niltempus requested merging the fixes for the daily session. Signed merge
`22b124c882be0044fb5fceb7c9bdf5d3c6d6f0f0` combines the accepted startup
fallback, renderer repair and existing lock coverage on master. Independent
merge review found no lost code or startup-policy facts. The merged source
passed the full isolated gate in `205-workspace-render-recovery/gate-02`,
14:17:33–14:26:39Z, with unchanged source pins and a clean tree. Its frozen
manifest is `ac29e00024f0c1f511a922e41f0b78dd9f4d04d8d1c62e15c27fe5b18bc1f088`.
The gate includes workspace tests, SDK checks, strict lint, layout and all six
verifier archives; native pixels and physical lock retirement remain unproved.

Master `22b124c88` is published. Signed desktop integration
`39fdba80befb00a188cb08ebe224b84a396a6dba` replaces the temporary local pin
with that exact public source; other flake inputs are unchanged. The standalone
release launcher was never run and is marked superseded. The merged release
build passed in `205-workspace-render-recovery/release-02`, producing
`niltempus-e3e6a9c375a1bfa4c7bc`. All release checksums, source identities and
the build-time profile preflight passed. Its Sophia binary hashes to
`5ca5a64f1a3dd3d26cc06c778598a7ed2894aed5daeebaea3f74c9dc82b527d5`.
The store output is retained by `release-02/result`. Both source repositories
are published. The initial install handoff required the operator's sudo
authentication. Niltempus subsequently installed the release and confirmed
normal login. At 14:40:20Z, the read-only `installed-01` observation bound
the running executable and session manifest to that exact binary and commit.
Session `00000001791556726570-719a02b7-1f2d-4b3f-b9ad-88c7bbd5257d` completed
startup; diagnostic recording was running with zero storage errors and six
durable presentation-deferral records. No worker stall record, failure-cause
file or terminal outcome was present in that observation. Its frozen manifest
is `73300a2470ae99224358d0789413c593ce0c8dc690ca19a8bbc385388a84a0bd`.
The previous release remains `niltempus-19efce64b00ae803d566` for rollback.
The clean merged repair worktrees were removed after their pins
were replaced; signed commits and frozen gate records remain. Physical
workspace-switch acceptance is still outstanding. The 202 guest and combined
hotplug gate stay stopped.

The release build used one job and one core as a precaution after the crash
during an eight-job build. Niltempus challenged that restriction; load was
never established as the cause. Compilation had finished before that exchange,
so no restart followed. The limit belonged to this recorded launcher, not
the user's Nix configuration; subsequent builds should use parallelism.

## t306

The [four-part qualification plan](../plans/u9rtb0ml-qualify-mesa-lifetime-repair-and-kvm-hotplug-recovery.md)
records the authorized lock, hotplug and rollout scope. The incident's exits
remain:

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
