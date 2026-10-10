---
id: dueqd6r0
date: 2026-10-10
kind: milestone
status: recorded
tags: [milestone, 9p, validation]
---
# Separate shared-core readiness from full WM qualification

## Sequencing change

On 2026-10-10 niltempus approved keeping full t249 open, finishing only the
checks that can change the shared-core design first, then returning to WM
qualification after the role-facing foundation is stable. After reviewing the
Plan 9/rio lessons, niltempus requested implementation of that plan. This is a
sequencing change, not acceptance or a reduced t249 exit.

The [t323 plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t323)
defines the new bounded readiness gate. Administration, broker and portal
migrations depend on that gate in place of full t249. The admission review,
recipe design and confined-group proof precede authenticated attach and
namespace composition. t317/t318 are explicitly promoted with their existing
exits: t317 gains t323 as a prerequisite and t318 inherits it through t317.
The root integration agent drives the sequence. Full t249 returns after t318,
whose prerequisites include t317
and t142. This avoids waiting for final legacy retirement t255, which itself
depends on WM physical acceptance t250 and full t249.

The existing observer CLI and authorized capture remain the first usable
consumer described by the convergence plan. Their admission, portal and
capture prerequisites remain enforced; this change does not promote input
driving, a native application frontend or physical testing. X admission,
Engine ownership and public wire contracts do not change in this record.

## Evidence preserved

The baseline is Sophia code pin `8ce7c40effce25251856d52d3fb234532ca9e921`,
Hagia `1a2902e1739f4abf31933429a5900dceb8bba60b`, and niltempus runner
`cb9ddebc9a2abb0d5a73271961bfd99da9ee1614` (documentation follow-up `38e2ae3`).
The [source-repaint investigation](../investigations/5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md)
records eighteen passing cases in `t249-repaint-02`, manifest
`c2d8a0ab683dc73dadce802c170608e7c053c2b53a06e92da13df57c0df736cf`,
the fail-closed resolver control, the distinct compound readback control,
strict checks and cleanup. None is reclassified by this split.

The frozen failed measurement campaign, numeric budgets, rollback proof,
capability matrix and remaining Session/backend joins are retained. New
candidate reuse requires impact review; existing source-only observations
are not promoted to executed evidence.

## Why this should reduce repeated work

[Plan 9's namespace paper](https://9p.io/sys/doc/names.html) describes common
file operations and substitutable service views. [Pike's rio design](https://3e8.org/pub/scheme/doc/rio_slides.pdf)
uses the same environment for clients and the window system and separates
blocking I/O from shared-state ownership. These motivate testing shared
transport invariants at their owner, then testing each role-specific join.
They are design references, not evidence that Sophia's contracts already pass.

In particular, [9P flush](https://9fans.github.io/plan9port/man/man9/flush.html)
has reply-ordering semantics; it is not a general rollback mechanism. Wire
cancellation, policy cancellation, handle lifetime and grant revocation keep
separate tests. Production decisions must remain in the exercised path when
device completions are simulated. No wholesale concurrency rewrite, new
production test API or drawing protocol follows from the comparison.

Task status lives only in [todo.md](../../../todo.md). The readiness gate,
foundation and full qualification must each meet their own linked exits before
completion; this documentation change claims none of those exits has passed.

## Validation

The revised queue has 304 unique task IDs across open and completed records.
The changed tasks' transitive dependency graph is acyclic; all 25 added
relative links resolve. `zk index` and whitespace checks pass, and the existing
25-note broken-link set is unchanged. Independent read-only review reports no
remaining blocker after the sequencing and snapshot-custody clarifications.
The eighteen-case baseline manifest independently verifies. No build, test,
product run, installed change or new qualification result was made for this
documentation-only implementation of the sequence.
