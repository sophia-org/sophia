---
id: lnvx8mly
date: 2026-10-01
kind: plan
tags: [plan, milestone]
---
# Generic chording lifecycle, taps, holds and sequences

## Scope and exit

Implement the generic chording facility approved by niltempus: action lifecycle,
modifier-only taps, long press, and key sequences. Sophia owns matching, timing,
cancellation and protocol facts; clients own the meaning and presentation.
Client-specific switcher behavior belongs in Hagia.

The detailed approved design is retained at
`~/.local/state/sophia/development-evidence/chording-recent-windows-plan.md`.
Review the additive contract before signing it. Publish in order: Sophia
contract, C SDK, coherent Sophia implementation and vendor import, Hagia,
Rust SDK contract import, then niltempus pins. Keep incompatible intermediate
contract trees off the main branch.

## Task details

t277 is delegated to pW in `~/dev/sophia-chording`, branch
`feature/generic-chording`, based on signed `dd62b50c8`. Canonical Sophia is
reserved for t276. Integrate chord deadlines through `next_deadline()` when
merging with the notification-driven owner loop; idle input must not restore
periodic polling.

Validate chord identity, release pairing, two keyboards, cancellation, tap/hold
exclusion, sequences, capability negotiation and exact codec layouts. Require
focused negative controls, the full Sophia gate and strict Clippy before a
coherent candidate. Keep device-free evidence separate from attended testing.
The approved scope fallback is lifecycle and modifier taps, then holds, with
sequences deferred explicitly if necessary.

niltempus requested an installable live-session release after completion. Root
will integrate t277 with the t276 idle-wakeup fixes, qualify their shared input
deadlines, and prepare the matching client builds through the external niltempus
installer. The handoff must include one install command and preserve the current
release for rollback. Record measured performance separately from the code
checks; live CPU improvement remains unmeasured until the attended comparison.

Heavy builds are coordinated with root, at caller priority and available CPUs,
in private targets. Small pure Nim tests may run independently.

## Combined candidate, 2026-10-02

The reviewed t277 implementation through `5ed1f20dd` is integrated with t276
`1b0b5d088`. All four shortcut shapes are implemented. The matching client is
Hagia `252ee7ef`, with C SDK `8f59a9cd` (0.6.0); the Rust SDK contract import
is `fe5e0960`.

The only merge conflict was the owner receive path. It keeps t276's
notification and fd receive, with a `next_deadline()` cap for hold decisions,
Held causes and sequence timeouts. An open chord without a timer does not
shorten the idle wait. Four Session tests cover these deadlines, the return
to idle and a notification interrupting a future deadline. pF independently
reviewed the conflict and all six automatic overlaps; no blocker was found.

Evidence: `~/.local/state/sophia/development-evidence/t276-t277-integration-01/`.
The frozen combined source passed 405 test binaries: 6,206 passed, none failed,
78 ignored, with all workspace features and targets enabled. Strict Clippy,
formatting, layout, protocol generation and both SDK checks passed. Child
test-process summaries are excluded from those totals. The source inventory
was unchanged across the run.

A shape-changing profile reload can queue cancelled terminals after shortcut
service. They wait for the next maintenance turn, whose requested wait is at
most 25 ms, or an earlier wake. This is not a scheduler-latency bound. The
deadline helper is tested; its placement in the full owner loop is reviewed
in source. No hardware or live performance claim follows from these checks.

Prepare one combined release through niltempus. Installation must also select
the matching personal Hagia before the next login: the installer preserves
an existing personal WM. The personal profile remains user-owned; its current
version has no recent-window bindings. Attended chording and idle-performance
acceptance remain pending.

## Connections

- [Idle wakeups, t276](../investigations/qvrk2298-remove-timer-polling-from-idle-desktop-workers.md)
- [WM file contract](../../sophia-wm-files.md)
