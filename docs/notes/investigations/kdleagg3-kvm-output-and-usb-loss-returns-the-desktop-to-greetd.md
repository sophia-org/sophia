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
