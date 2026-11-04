---
id: r7q2m9xp
date: 2026-11-03
kind: milestone
status: recorded
tags: [milestone, rendering, performance]
---
# Skip protected retained-projection retries

## Result or change

Mason approved integrating `a26dd0a4b`, based on adopted `8d694554c`, after
retained-layer saving and changing-strip nonregression qualification.
The periodic native service pass now avoids rebuilding retained frames when
every logical output owns a protected retirement: admission would suppress
those frames anyway. Pending work remains set and retries against the latest
scene after retirement. Empty and mixed-output cases keep the existing path.
Direct candidate intake, validation, completion processing, recovery and
deadlines remain unchanged. No cache, protocol, public API or new state.

## Evidence and decisions

Both comparisons use native Void builds, identical build commands/toolchains/
features, iGPU HDMI-A-2 at 1920x1080/60 Hz, W2 off, and 120 warmup plus 2000
counted frames per arm. Only the host source/binary changes within each ABBA.
These are combined Session+peer CPU accounting bounds, not confidence intervals.

| Workload | Baseline mean, us/frame | Candidate mean, us/frame | Candidate minus baseline |
| --- | --- | --- | --- |
| Four retained 480x512 tiles | 1140–1180 | 1065–1105 | -115 to -35 |
| Changing owned-ring strip | 1082.5–1122.5 | 1042.5–1082.5 | -80 to 0 |

Retained layers demonstrate a midpoint saving of 75 us/frame (6.47%); both
pairs improve by 75 us. Session-only difference is -87.5 to -47.5 us/frame,
while peer difference includes zero. The changing strip passes nonregression
without a demonstrated saving. All counted cadence, identity, postflight and
cleanup gates pass. Independent read-only recounts agree for both comparisons.

Evidence on crunch:

- `~/.local/state/sophia-protected-retry-build-01/`: signed candidate, source
  diff, native release receipt, 363 passing backend library tests (one ignored),
  formatting and strict crate-local Clippy. Dependency-wide Clippy stopped on
  an existing renderer-live warning; no unrelated source was changed.
- `~/.local/state/sophia-protected-retry-qualified-01/`: combined result seal
  `54090f64a368a13c7cfe400abc766e9ed7632caba128c594f9c3741b06245cc0`.
- `~/.local/state/t331/cpu-protected-retry-igpu-01/` and
  `~/.local/state/t331/retry-strip-02/`: saved native comparisons.
- `~/.local/state/sophia-protected-retry-strip-refusal-01/`: the earlier strip
  package's 108-byte Unix-socket pathname refusal, before Session or sampling
  started. Shorter paths and an early pathname check fixed preparation; no
  binary, workload or threshold changed, and no failed-attempt data are pooled.

Only documentation changes after the measured candidate. Retiring its clean
worktree after integration does not alter the recorded commits or receipts;
consumed packages are historical evidence, not reusable launch recipes.

## Remaining work

This completes the bounded retry optimization, not the historical 3x CPU-gap
task. Native multi-output performance and general desktop speedup are not
established. Installation and a normal desktop smoke are separate steps.
The [continuous-frame CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
owns subsequent work; use the saved service/IPC profiles rather than restarting
a per-pixel fill or damage-cache redesign.
