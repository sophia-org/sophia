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
