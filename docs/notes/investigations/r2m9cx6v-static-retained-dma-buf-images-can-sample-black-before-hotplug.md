---
id: r2m9cx6v
date: 2026-10-05
kind: investigation
status: investigating
tags: [investigation, rendering, dmabuf, synchronization, validation]
---
# Static retained DMA-BUF images can sample black before hotplug

## Question

Why can a static client image render correctly, then sample as black in a
later retained composition without another client Present? The symptom occurs
on master before any output removal. It needs a separate investigation from
the t306 hotplug repair; its cause is not established.

## Evidence

Evidence paths are under `~/.local/state/sophia/development-evidence/t306-01/`.
The bounded comparison `45-startup-comparison` used four master f87184d20
boots and four candidate e142593fe boots, in order M C C M M C C M. Both
images used f6865c946 tools, the same kernel and libraries, one virtio GPU
with two heads, virgl on host renderD128, and the no-WM static DRI3 client.
The client submitted one LINEAR XR24 400x300 frame, then held it unchanged.
The images differed only in Sophia and dracut source-path metadata.

The frozen first-region outcome was master content 4/4; candidate content
3/4, black 1/4. Keep that outcome unchanged. Four runs per arm establish
neither a failure rate nor a causal effect of t306.

Later reads reveal more than the first-region summary:

- `05-master.log`: line 61 has 120000 nonzero pixels and checksum
  `15913682524319544229`. Line 85 reads the same window region as all black,
  checksum `8572701038929191205`. Frame 6 retires at line 87. The removal
  command is later, at line 90.
- `02-candidate.log`: first region correct at 63, black retained composition
  at 86, black frame retired at 88, removal at 93. Regions 103 and 122 have
  the correct checksum after restoration into successive owners.
- `06-candidate.log`: first region correct at 62, black at 86, black frame
  retired at 88, removal at 91. Regions 101 and 120 are correct after restore.
- `07-candidate.log`: the first region is black at 71; readiness never
  completes, and no removal occurs.

The post-removal correct regions in 02 and 06 precede matching bootstrap
compositions and topology publications. The original verifier chooses the
last pre-removal region as its reference, which is black in those runs, so
it rejects the restored correct content as a mismatch. Their original
FAILED results remain. This reference problem does not dismiss the black
composition before removal.

`SERIES.txt`, `IDENTITIES.txt`, `ANALYSIS.txt` and
`REVIEW-CODEX-14-comparison-disposition.txt` retain the scope and source
anchors. Earlier black-startup observations are in 37, 41 and 44; the
identical-pins repeat in 38 started with correct pixels. The mixed fixture
history is not a matched frequency estimate.

## Current limits

Master demonstrates the later correct-to-black symptom without t306. This
does not prove that every black first frame has the same cause, or that
t306 cannot change its frequency. The region records identify the same
logical retained source; they do not establish the exact texture, storage
or EGL context used by each render. Guest renderer readback is evidence
of composed pixels, not physical scanout. No equivalent live-hardware
failure has been established here.

## t307

1. Trace the successful mixed Present and subsequent retained composition:
   source buffer, snapshot identity and storage, texture/import identity,
   EGL context and completion dependencies. Distinguish GBM producer writes,
   snapshot capture and later sampling before choosing a repair.
2. Propose the smallest bounded control that separates a named hypothesis.
   A source readback, explicit synchronization or second Present changes
   execution and must be recorded as an intervention, not substituted into
   the original evidence. Do not continue general screening unboundedly.
3. Make verifier diagnostics distinguish an unstable pre-removal baseline
   from failed preservation. Use the expected submitted pixels as the
   reference. A run with contradictory baseline pixels still fails overall;
   post-loss content can be reported separately with owner/frame proof.
   Preserve original logs and verdicts when adding corrected analysis.
4. Repair the demonstrated lifetime or synchronization defect and require
   a regression that fails without the repair. Retain snapshot immutability,
   source release, bounded storage and per-output ownership guarantees.

The task is high priority because pixels can disappear without a new client
frame, and this currently obscures hotplug qualification. t306 need not
absorb its root-cause repair, but failed or unstable runs are not acceptance
of the still-unproved managed-head loss and all-return cases.

Task state and execution order live in [todo.md](../../../todo.md).

## Connections

- [KVM hotplug recovery](kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md)
  exposed this separate symptom while testing static DMA-BUF preservation.
- [CPU reduction plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md)
  includes the published snapshot reuse work; no causal link is assumed.
