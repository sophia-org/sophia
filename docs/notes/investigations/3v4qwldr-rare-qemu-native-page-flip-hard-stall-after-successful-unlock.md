---
id: 3v4qwldr
date: 2026-10-04
kind: investigation
status: investigating
tags: [investigation, rendering, drm, validation]
---
# Rare QEMU native page-flip hard stall after successful unlock

## Question

Why did one native output exceed the 500 ms page-flip watchdog in a QEMU lock
fixture after unlock and input checks succeeded? The cause is unknown and may
not be lock-specific. Closing t302 does not resolve this failure.

## Evidence

All paths below are under `~/.local/state/sophia/development-evidence/`.
The original failure is `t302-qemu-unlock-01/series-10/baseline-3`; its summary
and binary/source identities are bound by
`t302-lock-performance-01/QEMU-SERIES-10-DISPOSITION.json`. It ran candidate 03,
not a byte-exact build of the old base revision.

Output 1's frame 89 crossed the unchanged 500 ms boundary with its out-fence
pending. Output 2 kept retiring; no DRM reader error or refused callback was
reported. The drain poll accepted frame 89 about 25 ms later, before suspend,
detach or disable. No intervening commit or framebuffer removal explains it.
The counts reconcile as one initial submission, 88 retired and one outstanding.

Later evidence must keep its cohorts distinct:

- `p1`: ten alternating control/candidate pairs passed. Neither arm stalled.
  Control includes the two startup repairs; it is not the old base alone.
- `s11`: twenty candidate baseline runs passed with SysRq enabled. No stall
  triggered a dump. Including the earlier baseline runs, the historical
  candidate total is one hard stall in 36 runs; control is zero in ten.
  Those 36 already include s11 and use differing fixture revisions.
- `t302-integration-01/sq1`: twenty stall-mode and five flood-mode runs passed;
  flood-6 failed after bounded completion on an unclassified client teardown.
  No page-flip stall occurred. Baseline was not reached. The failed result and
  its extra host sampler and rust-analyzer activity stay recorded.
- `t302-integration-01/sq2`: one flood run on published Sophia 6ae5df00a passed
  with the diagnostic fixture. One post-completion ESTALE was accepted; there
  was no page-flip stall or SysRq dump. This is one smoke result, not a bound
  on the rare failure rate.

Old images contained verified strip-g derivatives of pinned executables.
The later fixture uses dracut --nostrip and checks exact in-image identities.
Do not merge these cohorts into a matched failure-rate estimate.

## Finding and resolution

No causal link to t302 has been established or ruled out. The completion audit
and independent-output custody tests in `t302-sophia-forward-01` found no new
production defect. A completion can signal during the final drain without a
teardown operation. Upstream/source analysis in series-10 FINDING-10 narrowed
possible causes to guest commit-worker scheduling or a control-ring wait;
these remain hypotheses. No blocked-task stack was captured.

## t305

Keep ordinary screening stopped. Resume on a recurrence with useful diagnostics,
a deterministic reproduction, or an explicit bounded experiment that separates
a named hypothesis. Preserve the first failure, exact source/binary/image and
host identities, output buffer kind, callback/fence history and timestamps.
Use the opt-in bounded SysRq capture to obtain a blocked-task stack if it recurs.

Determine whether the delay is in Sophia, the guest kernel/DRM worker or QEMU
before proposing a repair. Do not loosen the watchdog to make a run pass. A
repair needs a discriminating regression and appropriate device or guest proof;
more passing runs alone do not explain the failure.

Task state and execution order live in [todo.md](../../../todo.md).

## Connections

- [Lock repair and prior qualification](../plans/qrstyyjn-restore-lock-animation-and-input-responsiveness.md)
  records the original fixture and retained failures.
- [Sophia CPU plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md)
  keeps optimization claims separate from completion correctness.
