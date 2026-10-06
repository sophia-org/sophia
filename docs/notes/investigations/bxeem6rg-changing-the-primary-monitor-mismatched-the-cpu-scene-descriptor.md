---
id: bxeem6rg
date: 2026-10-06
kind: investigation
status: investigating
tags: [investigation, rendering, session, validation]
---
# Changing the primary monitor mismatched the CPU scene descriptor

## Trigger and evidence

The first native wmbench qualification failed before the benchmark workload on
2026-10-06. Sophia `97a9e4ce6`, binary SHA256
`65c3fc0fc396803246339134db3e2d12bdb62c620e85c6bd14c7173a5d6fbaf1`,
ran from the operator's active tty3. The private profile kept both physical
outputs: DP-1 at 2560×1440/120 Hz and DP-2 at 1920×1080/60 Hz. It placed DP-2 at
0,0, selected it for startup focus and gave it policy key 1; DP-1 moved to
1920,0 with policy key 2. Output identity remained DP-1 = 1, DP-2 = 2.

Both heads bootstrapped, both candidate frames presented, and the new layout
committed. The next WM policy cycle failed with:

```text
production CPU cycle failed in phase FrameComposition:
CPU scene output descriptor has a mismatched size
```

Native drain and application cleanup completed. The wrapper restored display
mode 0, keyboard mode 3 and termios; no test processes survived. This run is
**FAILED**, yields no performance result, and has not been replaced by a rerun.

Evidence: `~/.local/state/sophia/development-evidence/t289-native-wmbench-01/`
`smoke-20261006T115454Z/`, especially `session/untrusted-session-output.log`,
`session/recovery.log`, `verdict.json` and `client-group-cleanup.json`.
The benchmark application launcher started, but wmbench itself did not run.

## Cause and correction

Topology reconciliation resized `LiveProductionCpuScene` using the policy's
selected primary output. CPU production cycles, GPU fallback cycles and ordinary
repaints compose the first entry in their output descriptor slice. When the
primary is the second monitor and sizes differ, those two choices disagree.
The renderer correctly refuses the mismatched descriptor.

`LiveProductionCpuScene::reconfigure_output_descriptors` now selects the same
first descriptor as composition. Session uses it at candidate reconciliation,
rollback and topology replacement. The selected policy primary still reaches
the WM and Session focus/placement state. Output IDs, ordering, viewport
publication and native head sizes are unchanged. The existing invalidation and
CPU-buffer retention behavior stays in `reconfigure_output_size`.

## Validation and limits

The renderer regression composes real display lists and checks pixels and each
output frame's size. It covers unchanged descriptors when only the policy
primary changes, a size-changing apply, rollback, removal of the first output,
and refusal of empty or invalid descriptors without losing the existing frame.
The focused renderer suites pass (27 tests). A mutant selecting the last
descriptor fails at the named composition assertion with the same mismatched
size error as the native run; restored source passes.

These are CPU regressions. They do not execute a native topology transaction,
and the recorded mutant establishes descriptor selection rather than proving
every Session callsite. Peer source review checks those three sites; the
workspace gate does not prove a native transaction followed by repaint.
A fresh native run remains the physical qualification. No performance gain or
successful hardware retry is claimed.

Development checks and mutant receipts are in
`~/.local/state/sophia/development-evidence/t289-native-primary-scene-01/`.

## Connections

This blocks the hardware baseline in the
[CPU optimization plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md).
The [remaining hot-path investigation](ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md)
records the preceding software measurements and the real render-node correctness
smoke. [t289](../../../todo.md) owns native requalification and the next measured
optimization; this startup repair establishes no new CPU saving.
