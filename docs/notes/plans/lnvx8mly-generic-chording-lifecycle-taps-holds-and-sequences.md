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

## Connections

- [Idle wakeups, t276](../investigations/qvrk2298-remove-timer-polling-from-idle-desktop-workers.md)
- [WM file contract](../../sophia-wm-files.md)
