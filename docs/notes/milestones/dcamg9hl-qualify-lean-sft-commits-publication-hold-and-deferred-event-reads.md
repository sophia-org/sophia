---
id: dcamg9hl
date: 2026-11-07
kind: milestone
status: recorded
tags: [milestone, shell, performance, custody]
---
# Qualify lean SFT commits, the publication hold and deferred event reads

## Result or change

Mason approved integrating three separately qualified Sophia Frame Transaction
(SFT) steps on the changing 1920×24 owned-ring bar at iGPU HDMI-A-2 60 Hz,
each a negotiated or opt-in option that leaves the default r13 path unchanged:

1. **Lean commits, capability bit 20** (`2c3284749`, contract `da202ec69`,
   wait-cause receipt `92ad80b8a`): the FrameCommit header word carries the
   cumulative ACK, no Submitted record is published, the exact Rwrite is
   custody, and the reusable writer recycles at admission. Requires bits 15,
   18 and 19. LeanPrefix.tla and the SDK scripted traces hold the custody
   invariants; the host refuses a nonzero header word without bit 20.
2. **Publication hold** (`7ec29c592`, counter fix `9406f7219`): while a lean
   frame is in flight the host keeps its source's ResourceStatus and release
   record back from `events` reads until the terminal is journaled, so one
   read carries all three. Journal order, refusals and channel closure are
   unchanged. `9406f7219` settles the in-flight counter on FrameOutcome, not
   CandidateOutcome; the earlier keying closed the stream on credit timeout
   when a client paused, caught by the in-process owner test before any run.
3. **Deferred `events` re-arm**, Rust SDK opt-in `set_deferred_event_reads`
   (`0621e10d`, pinned here as `da1ad8f`): the Tread re-arm travels in the
   same socket write as the next request or in the pass a wait begins.
   Off by default; the C SDK has no port (API-PARITY records the gap).

The landing adds no behaviour: rustfmt on two runtime files (`034e58722`
drift), a scoped clippy allow on the owned mixed export, re-vendored SDKs
carrying the hold paragraph of the wire contract, and three layout
exception rows for external test fixtures included with `#[path]`.

## Evidence and decisions

Every comparison is a four-arm ABBA, native Void builds, identical toolchain
and features, process-clock CPUwin v2, 120 warmup and 2000 counted frames per
arm, saving rule candidate upper below baseline lower, nonregression 1.05×.
Bounds are Session+peer process-clock accounting bounds, not confidence
intervals, and the three runs are not pooled: each varies one binary against
its own control.

| Package | Varies | Baseline, µs/frame | Candidate, µs/frame | Candidate minus baseline |
| --- | --- | --- | --- | --- |
| `t331/sft-lean-01` | peer config, common host `2c3284749` | 811.0–811.5 (bit 19) | 750.0–750.6 (bit 20) | -61.5 to -60.5 (-7.5 %) |
| `t331/sft-hold-01` | host `2c3284749` → `7ec29c592`, lean peer | 759.0–759.5 | 699.8–700.3 | -59.6 to -58.7 (-7.8 %) |
| `t331/sft-lazy-01` | peer `0d35731` → `273d98c2`, host `9406f7219` | 693.7–694.2 | 673.7–674.1 | -20.5 to -19.5 (-2.9 %) |

Both pairs improve in every run. Owner wait-cause receipts explain each step:
lean removed the Submitted read turn and the /ack write (owner passes 6.63 →
5.91 per frame), the hold removed one of two wire passes (2.90 → 1.97), and the
deferred re-arm removed the other (1.97 → 1.00 exactly, every other class equal
to three decimals). The lean frame now wakes the owner four times: one wire
pass, the image-worker descriptor, the native completion and the Service
deadline. Per-pass cost is not uniform: the re-arm pass cost about 20 µs, the
FrameCommit pass about 60 µs, so flat µs-per-pass predictions are retired.

Independent read-only recounts from raw receipts agree with every analyzer
(`~/.local/state/sft-{lean,hold,lazy}-01-postrun-audit-claude-01`, 130, 142
and 146 checks). Packages and manifests: `ca59a375…`, `1332b6d3…`,
`f8cc7f7d…`; readiness receipts in `sophia-sft-{lean,hold,lazy}-readiness-01`;
build receipts in `sophia-sft-{lean,hold-fix,lazy}-native-build-01`. Lean's
prediction (-60) was met; the hold's (-150) was not, because the release fold
had already been paid by the one-Rread pass; the deferred read's (-60) was
not, because the removed pass was cheap. All three decisions came from the
predeclared rules, not the predictions.

## Remaining work

This is the bounded protocol lane, not the 3× CPU gap: Session+peer sits near
674 µs/frame against the Niri 415–455 reference. No redundant wire wake
remains; the next question is what each of the four passes does, starting
with the 1 ms Service deadline pass, and then the per-pass component walk.
The peer's SDK pins must move from `file://` to GitHub once these SDK
commits are pushed; the C SDK deferred-read port and the idle 250 ms ACK
hold test are open. Consumed packages are historical evidence, not launch
recipes; retiring the cited worktrees after integration changes nothing
recorded. The [continuous-frame CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
owns what follows.
