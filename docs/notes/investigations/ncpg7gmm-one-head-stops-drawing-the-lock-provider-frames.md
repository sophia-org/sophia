---
id: ncpg7gmm
date: 2026-10-05
kind: investigation
status: investigating
tags: [investigation, lock, session, diagnostics]
---
# One head stops drawing the lock provider's frames

## Question

Why did one of two heads stop showing new lock frames for six hours while the
other kept animating, with no error record, until the session was unlocked?

## Evidence

Read-only evidence is under
`~/.local/state/sophia/development-evidence/dp2-frozen-lock-20261005/`
(`SUMMARY.txt`, thread samples and the operator's kernel-stack capture). The
operator's desktop locked at 22:42 local on 2026-10-04 with the kleis provider.
The ratio of composed target pixels to composed frames in
`sophia_live_render_work` was the mean of both heads until about 00:04, exactly
DP-1's 2560x1440 from about 00:06 to 06:15, and DP-2's 1920x1080 again after the
06:23 unlock. DP-2 therefore composed no frame for six hours; DP-1 composed at
about 58 per second throughout. No thread of Sophia or kleis was in a DRM, fence
or GPU wait. Per-output present records had been suppressed by the record budget.

Lock frames follow a handshake per allocation: the provider demands a frame,
Session permits it only while that allocation has no candidate in flight, the
provider sends a candidate, and Session reports Presented only after every head
retired that exact candidate. Neither side has a timeout, and Session drops a
command silently when the provider's command queue is full. Any lost step
leaves that allocation idle until the next lock or connection, which matches
the recovery at unlock. The candidates are a dropped permit or outcome, an
in-flight candidate that never retires on its head, a frame admitted but never
rendered, and the provider marking the output blocked. None is established.

## t308

1. Record every command the lock provider queue drops, by kind, so a lost permit
   or outcome is never silent.
2. Behind a default-off opt-in, sample each allocation's pacing: lock and
   connection identity, allocation generation, output, held demand, in-flight
   candidate generation, and saturating counts of demands, permits, candidates
   and outcomes. A repeated in-flight generation, an idle allocation with drops,
   and an idle allocation without them then name different causes.
3. Reproduce the stall under the opt-in, or capture it on the operator's
   desktop, before changing pacing. A repair needs a regression that fails
   without it and keeps presentation pacing the provider at its slowest head.

Task state and execution order live in [todo.md](../../../todo.md).

## Connections

- [Session lock rendering and latency plan](../plans/qrstyyjn-restore-lock-animation-and-input-responsiveness.md)
  introduced the provider's frame pacing.
