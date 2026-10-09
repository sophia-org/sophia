---
id: u9rtb0ml
date: 2026-10-09
kind: plan
tags: [plan, rendering, topology, validation]
---
# Qualify Mesa lifetime repair and KVM hotplug recovery

## Scope and authority

This records the four-part plan niltempus authorized with “Implement the plan”
on October 9. It joins the bounded Mesa comparison in t307 to the lock and
hotplug qualification in t306. It does not promote the t310 monitor-policy
candidate, authorize an iGPU passthrough action, or accept either task.
Task state and execution order remain in [todo.md](../../../todo.md).

The [sampling investigation](../investigations/r2m9cx6v-static-retained-dma-buf-images-can-sample-black-before-hotplug.md)
owns rendering diagnoses and results. The
[KVM investigation](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md)
owns topology, input and lock results. Numbered directories under
`~/.local/state/sophia/development-evidence/t306-01/` retain raw logs, source
snapshots, exact run contracts and manifests. Those artifacts support the
notebook; they are not another work queue or the sole record of a decision.
Frozen artifacts and their original verdicts remain unchanged.

## 1. Qualify the Mesa comparison contracts

Resolve both known contract gaps before another guest series. The screen
premise must distinguish LOOKUP from INSERT and REMOVAL callbacks using live
descriptor generations, source-bound ordering and the same boot's trace.
Only the sibling loader's lookup against the producer winsys counts toward
sharing. Maintenance callbacks cannot establish a hit or miss. Unknown or
ambiguous assignments refuse; the bounded contract allows at most 16 proven
winsys generations across the process.

The pixel classifier may recognize the one exact known Mesa diagnostic, at
most once per arm, solely between validated `sibling_alive` and
`after_sibling_drop` records. It reports driver health separately. A diagnostic
never counts as healthy rendering, and foreign or misplaced text still refuses.

Require independent source review, retained refusal controls, meaningful
mutants, runner controls and a frozen package before the one-shot series:
observer, private original Mesa, then private patched Mesa. Keep the existing
test, matched images, bounds, infrastructure checks and no-replacement rule.
Advance requires all of these original-arm results:

- observer passes and infrastructure is clean;
- same-boot screen premises establish the expected original topology;
- separate arm preserves pixels and AddFB handle without a diagnostic;
- shared arm has `LOST_AFTER_SIBLING_DROP`, the named composition assertion,
  test exit 101, and zero or one recognized shared diagnostic.

The patched guest qualifies only with its expected same-boot screen premises,
both arms `PRESERVED`, test exit 0, clean infrastructure and no diagnostics.
Invalid evidence, observation errors, AddFB loss and unreached premises do not
substitute for the original negative control. A stopped or inconclusive series
gets a disposition, not an automatic replacement. Retrospective analyses of
194 cannot change its frozen verdicts or satisfy this future comparison.

## 2. Test the narrow Mesa repair in Sophia's production workload

Use the matched private original and patched Mesa builds with one frozen,
reviewed Sophia integration candidate. The workload is one virtual card,
two heads, per-head workers, Sophia's generic WM fixture and a static DRI3
client that Presents once. It contains no hotplug intervention.

Declare and freeze the comparison before running it: original, patched,
patched, original, four boots, with no replacements and a stop on
infrastructure failure. Require both patched boots to reach readiness and
retain the independently expected pixels, and at least one original boot to
reproduce the relevant failure. If the original never reproduces, the outcome
is limited to successful patched runs; it does not establish a repair effect.

The context-level result from part 1 is a prerequisite for interpreting this
workload as the cache patch's effect. It does not itself identify the cause of
the original Sophia black frames. Keep the patch narrow and retain the matched
build provenance; do not install private Mesa or patched QEMU into the host.

## 3. Characterize lock custody and prove cover retirement

Exercise real LockPublication, LockFileCustody and SessionLockFrames through
candidate publication, output removal and return. Distinguish provider
revocation from Session's pending retirement. Cover recovery by presentation
of the old image, absorption of the stale outcome, no pending candidate,
new lock, reconnect, unrelated output, and wrong receipt identity.

Topology tests must use the production rebind/resume boundary with controlled
device facts, rather than assign the runtime output list directly. Require
locked loss/return, loss during Locking, mirrored heads and all-output absence.
Mutants that clear the cover during rebind or resume must fail the relevant
tests. CPU characterization does not prove native device retirement.

Add a passive topology- and lock-epoch coverage record only after every
current head has retired a cover frame. Qualify its binding, duplicate
suppression and refusal during suspension or incomplete coverage. This is
diagnostic evidence for the locked guest; it does not change lock policy or
replace the existing Locked transition.

## 4. Qualify hotplug and prepare an attended rollout

Integrate an explicit reviewed repair scope on current master, preserving the
installed baseline. The raw t306 diagnostic branch is not an install candidate.
Use Rust/xtask for maintained tooling. Carry the corrected bounded endpoint
collector and refuse recorded unreaped processes even if a later scan finds
none. Correct display attempt/completion ordering before guest qualification.

Freeze fixed qualification runs for managed-head return, all-head return,
keyboard return, combined loss/return and locked all-return. Use a 60-second
session for the combined case with bounded host collection. Require independent
pixel and input-routing proof, complete endpoints, lock coverage for the
returned topology and no automatic reruns. Startup instability remains a
refusal; a later good retained frame does not silently discharge its obligation.

Gate the exact signed candidate, prepare its release and concrete install and
rollback commands, and keep the attended physical KVM check separate. Physical
acceptance must verify restored displays, keyboard shortcuts, pointer routing
and lock/unlock on niltempus's devices. Neither CPU tests nor virtual guests
close that final obligation.
