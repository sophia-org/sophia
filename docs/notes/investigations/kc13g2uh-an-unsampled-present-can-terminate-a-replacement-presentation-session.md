---
id: kc13g2uh
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, session, rendering]
---
# An unsampled Present can terminate a replacement presentation session

## Question

Can a legitimate application Present terminate the session while a WM publication
replaces application drawings but does not preview the presenting application?
This investigation follows niltempus's report of a desktop exit after reloading
Hagia and opening and navigating overview on 2026-09-25.

## Evidence

The immutable incident bundle is
`~/.local/state/sophia/development-evidence/overview-crash-20260925-122750`.
Its installed Sophia revision is `8bd8c41c2ced6a06200ed20faa870c4180889155`.
The restarted WM connection reached epoch 2 and continued committing policy
transactions. In `events.0.log`, event 898374 accepts client 1's Present,
transaction 584776, at UTC millisecond 1790353669953. Event 898375 records the
owner-loop fatal at the same millisecond, with bounded cleanup and failure code
`unclassified`. Event 898391 records phase `authority`; the outcome is exit 1.

The original error and exact publication are unavailable in the reduced records.
The epoch-2 executable digest identifies the bwrap launcher, not the Hagia binary.
Neither fact establishes which publication or binary produced the failure.
Cleanup removals happen after the fatal and are not evidence of its trigger.

## Reproduced failure and responsible boundary

Claude's signed test-only revision `de5173d56e422871909ce5f378dbdaa18a765e8f`
constructs two committed application sources, installs `ReplaceApplications`,
and previews only one source. It then collects Present sources for the omitted
application using the actual output display list. Collection returns
`visible Present surface is missing from the presentation order`.

The collector requires the current Present source to occur in at least one
applicable display list. Replacement legitimately removes application Surface
commands, however, and need not replace each with a SurfaceInstance. The error
occurs before the existing lowered-frame capture check, which already handles
an unsampled Present through bounded first-visibility deferral or paced skipping.

The session propagation path is:

```
owner_loop/authority_production.rs
  -> run_gpu_production_cycle (gpu_cycle.rs)
  -> run_batch (authority_batch.rs)
  -> drive_gpu_presentation (present.rs)
  -> live_present_head_composition_sources (compositor_graphics.rs)
  -> owner_loop/physical_input_loop.rs terminal error handler
```

These calls propagate the error with `?`. The session classifier has no approved
code for that invariant string, so it reports `unclassified`, then performs
bounded cleanup. This is a reproducible failure compatible with the incident's
ordering and classification, **not proof of its exact physical cause**. That
would require the missing error or evidence that the actual publication omitted
the presenting source.

The correction belongs at the backend visibility/Present boundary. Session-wide
error suppression would conceal unrelated renderer failures. A correction must
retain refusal for genuinely missing sampled sources, the clearing repaint,
bounded first-visibility/pacing behavior, and buffer retirement ownership.

## Independent validation and remaining work

The test was cherry-picked with a signature as `0c07cff0` on
`session/overview-crash` in `/home/niltempus/dev/sophia-borders`, based on the
installed revision. Its production source is unchanged. A device-hidden bwrap
run with `libdrm-events,gbm-probe` executed the single reproduction successfully;
success here means the test observed the expected error, not that the bug is fixed.
The log is `.artifacts/overview-crash/reproduction-native-features.log` in that
worktree. An initial invocation with insufficient features and an exact short
filter executed zero tests; its separate `reproduction.log` is retained and does
not count as validation.

Claude owns the bounded backend fix and production settlement controls. The
session review has not established a separate session source defect. Integration
must review the signed fix, preserve red and green evidence, and verify hidden,
previewed, restored, and missing-required-source cases. Tracking allocation is
owned by the director; this investigation does not edit the queue.

No main-tree operation, live install, reload, GPU execution, or physical-device
test was performed for this reproduction. Physical acceptance remains outstanding.

## Connections

The [WM presentation contract](../../wm-presentation.md) permits replacement
scenes and separates presentation authority from source retention. The
[rendering foundation](../../rendering-foundation.md) describes the generic
source-instance composition path involved here.
