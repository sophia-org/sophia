---
id: t972gtpa
date: 2026-09-17
kind: concept
status: established
tags: [concept, pinned, shell, validation]
---
# Prove the shell lifecycle before spending operator time

> **Pinned retrospective.** The operator observed that getting the shell working
> took a long time. Keep this lesson visible when planning the next shell tranche.

## What cost us time

The bar exposed incomplete contracts across confined GPU discovery, presentation,
input targeting, resource retirement and shutdown. A visible bar was only one
step; a useful shell also needed correct per-output workspaces, reliable clicks,
continuous updates, bounded ownership and an orderly exit.

Too much integration discovery happened through repeated attended desktop runs.
The operator paid for that with interruptions, repeated commands and uncertain
outcomes. Reusable infrastructure and new regressions are valuable results, but
they do not erase that cost.

Several tests proved individual components without joining the production
ownership boundaries between them. Some diagnostics dropped the fields needed
to distinguish causes. We also misinterpreted counters before checking their
meaning: raw pointer motion and delivered client-surface motion are different
measurements. Missing evidence is not evidence of a missing event until the
recorder's coverage is established.

## How to work differently

- Establish an end-to-end acceptance path early: admission, publication, presented
  input, effect admission, rendering, reclamation and shutdown. Identify explicitly
  where a fixture substitutes completion, policy execution or device behavior.
- Keep changes small enough to isolate the observed failure. Do not let a nearby
  repair absorb an unexplained transport, input or shutdown incident.
- Exercise the shared production transitions headlessly before asking the operator
  to repeat a run. Include interleavings, backpressure, stale identity and cleanup;
  use behavioral mutations to prove the regression detects the original defect.
- Check diagnostics as part of the feature. Record bounded identities and reasons
  that survive capture, and define what each counter actually measures.
- Reserve live tests for hardware-specific discovery and confirmation of a checked
  integration. Offline tests cannot prove driver compatibility, KMS or real seat
  behavior; each attended run should answer a stated remaining question.
- Keep exact source and binary identities with evidence. Report source review,
  simulated completion, native observation and workload acceptance separately.

These are development lessons, not new protocol authority or a claim that every
future defect can be reproduced without hardware.

## Evidence and limits

The [shell critical-path plan](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
retains the investigations and their individual evidence scopes.

- `20260917T000849Z`, Sophia 855138f9 / Lom a317370: six captured clicks joined
  to six committed actions, including a refresh-crossing click on each output.
  Shutdown still failed after allocation publication continued beyond frontend
  drain. This was not the full forty-action workload.
- `20260917T002514Z`, Sophia d414b7e5 / Lom a317370: allocation publication stopped
  before frontend drain, logout quiescence completed in 87 ms, and the run exited
  0 with renderer workers joined and shell resources/credits at zero. It recorded
  no bar clicks, so it adds shutdown evidence, not click coverage.

Raw captures remain under `.artifacts/lom-panel-native/`; scoped summaries are
under `.artifacts/click-continuity/` and `.artifacts/allocation-shutdown/`.
Clean shutdown and working sampled clicks do not close daily-driver acceptance.
[Todo](../../../todo.md) remains the source of task status.
