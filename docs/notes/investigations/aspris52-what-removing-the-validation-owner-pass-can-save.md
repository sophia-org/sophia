---
id: aspris52
date: 2026-11-10
kind: investigation
status: investigated
tags: [investigation, session, shell, performance, sft]
---
# What removing the validation-owner pass can save

## Question

The three-service frame-job design (n7fj3c2p) hands the validated frame from
the validation worker straight to the renderer, so the owner no longer wakes
for the validation completion and no longer installs content and dispatches the
render itself. Stages 1 to 4a proved its credit, payload, Release and wake
pieces as test-only seams. Stage 4b and everything after it is production code.
Before that code is written, this note asks how much owner CPU the removed
pass could carry at most, read from receipts that are already sealed. It
changes no source, builds nothing, launches nothing and claims no saving.

## Inputs

All numbers come from the dcp-01 per-class row means as tabulated in
[the four lean owner wakes](edc0te2v-which-action-each-of-the-four-lean-owner-wakes-owes-and-which-transition-a-consolidation-would-remove.md):
one instrumented, ptrace-traced arm, 7,999 whole owner rows over 2,000
completions, root result seal `7a0677ef…`, independent recount `4d159153…`.
The four row means sum to 1017.3 µs of owner thread CPU per completion. The
uninstrumented owner share is taken from decomp-03 (owner 416 of 674 combined
µs/frame), which ran on a different host before A3 and B3, and the current
uninstrumented combined figure from the sdk-02 control arms (fixed host
`6817e7bca`, combined [700.596013, 701.109918] µs/frame, result
`sophia-sdk-lean-native-result-01`).

## What the design removes from the cycle

The lean frame runs four owner waits in a fixed order: wire, fd, ring+service,
native. Read from source in edc0te2v, the fd wake belongs to the image-worker
completion, and the body that follows it takes the validated result, reconciles
the grant, installs the content and dispatches the render through the pre-wait
`service_native`. The ring wake that follows belongs to the render completion,
whose Timeout tail submits the flip.

Under the design the validator forwards the frame to the renderer itself. The
fd wait and one loop body leave the cycle; the wire, ring and native wakes
stay, and the owner's acceptance checks and the existing DRM submission move to
the ring wake. Three waits per frame is a hypothesis about the implemented
design, read from the obligation map, and the mechanism capture in plan step 4
must prove it.

## Three readings of the removed work

Each reading is an association between source and row means, not a
measurement of any call.

| reading | terms (dcp-01 row means, µs per completion) | owner µs | share of owner |
|---|---|---:|---:|
| only the pass goes | components 98.1 + other 56.7 of the native-ended body, which owes no frame action, + authority 21.9 of the fd wait | 176.7 | 17.4 % |
| the fd-wake body goes, native kept | components 141.5 + other 55.6 of the ring+service-ended body + authority 21.9 of the fd wait | 219.0 | 21.5 % |
| the fd-wake body goes entirely | the previous row + all 97.0 of that row's native phase | 316.0 | 31.1 % |

The first reading assumes every owed action of the fd-wake body reappears in
the acceptance turn and only the per-pass obligations of one iteration go. The
third is a ceiling. It counts that row's whole native phase, which also covers
the Timeout-tail flip submission that the design keeps; the row does not
record the split.

Applying decomp-03's owner share of the combined figure (61.7 %) to the sdk-02
control midpoint gives about 75, 93 and 134 µs/frame combined for the three
readings. That scaling joins two hosts and two instrumentation levels. It shows
order of magnitude, not an expected result.

## What the readings leave out

None of the readings subtracts the work the design adds. Service 1 must build
a provisional composition plan without a lease and reserve renderer-job and
result capacity. The validator adds one bounded send. The renderer receives a
combined job on its own channel. The owner's acceptance runs FALLBACK.md's
identity, grant, allocation, scene and native checks and a new
install-already-rendered operation. The scene-only fallback adds a retained
Release and a conventional submission on the frames it takes. Per-pass
obligations that run in every body (B6 custody polls, B7 reaps, B8 settlement)
are kept by design. Validation and rendering CPU move between threads and stay
inside the Session process; the readings count only the owner thread.

## Reading for the decision

Even the first reading is roughly ten times the effects that ring-01 and
sdk-02 left inconclusive. The removed pass is the largest single owner-side
target on record that keeps validation off the owner and DRM on the owner.
That makes the production stages worth pricing by a real build. It does not
predict their result. A smaller net effect, after the added work, may still be
inconclusive in one matched block, and this note sets no detectability floor.

The decision is niltempus's. Option (a) of the stage 4b scoping
(`tools/models/combined-frame-job/PLACEMENT-SCOPING.md` on
`design/combined-frame-job`, `02ca8874f`) starts the production diff. Option
(c) records the lane at its test-only boundary.

## Limits

- One instrumented, traced arm and one topology; every row mean carries clock
  reads and the tracer.
- Row-to-action associations are edc0te2v's, read from source at
  `0b6a1515a`; no receipt names the visit that ran.
- The combined scaling uses an owner share from a different host and an older
  lane.
- No build, native run, source change or merge follows from this note.

## Connections

- Three-service frame jobs with owner authority (n7fj3c2p) is the selected
  design and its gates. It is branch-only: `~/dev/sophia-framejob`,
  `design/combined-frame-job`,
  `docs/notes/plans/n7fj3c2p-three-service-frame-jobs-with-owner-authority.md`.
- [The four lean owner wakes](edc0te2v-which-action-each-of-the-four-lean-owner-wakes-owes-and-which-transition-a-consolidation-would-remove.md)
  supplies the cycle, the row means and the associations.
- [The post-lean decomposition](q2m7v8ka-what-the-post-lean-decomposition-says-about-the-next-native-frame-change.md)
  holds the accepted dcp-01 numbers and limits.
