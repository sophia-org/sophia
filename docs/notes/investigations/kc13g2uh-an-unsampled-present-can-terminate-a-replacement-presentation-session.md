---
id: kc13g2uh
date: 2026-09-25
kind: investigation
status: implemented
tags: [investigation, session, rendering]
---
# An unsampled Present can terminate a replacement presentation session

## Question

Can a legitimate application Present terminate the session while a WM publication
replaces application drawings but does not preview the presenting application?
This investigation follows niltempus's report of a desktop exit after reloading
Hagia and opening and navigating overview on 2026-09-25.

## t246 scope and acceptance

The director reserved t246 for generic backend Present visibility and settlement.
An application excluded by a replacement publication must not terminate the
session merely because no display list samples its new buffer. This applies to
any WM policy; Sophia must not interpret overview action names or adopt Hagia's
layout rules. Hagia h004's fixed 0.5 zoom and clipped preview panning deliberately
leave sources outside the visible scene, making t246 a prerequisite for another
live acceptance attempt.

Development acceptance requires production-owner controls for bounded skip and
first-visibility deferral, subsequent restoration and clearing repaint, and source
retirement. Previewed sources must still consume their current buffer, and a
missing source that is actually sampled must remain a refusal. Retain the red
reproduction and green controls, run affected device-hidden tests on the signed
joined candidate, and document their exact identities. Add a bounded diagnostic
classification only if it establishes useful additional evidence. No main-tree
operation or live action is authorized by this development task.

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

The incident bundle's ten retained files passed its `SHA256SUMS` verification;
the independent log is `.artifacts/overview-crash/incident-integrity.log`.

The test was cherry-picked with a signature as `0c07cff0` on
`session/overview-crash` in `/home/niltempus/dev/sophia-borders`, based on the
installed revision. Its production source is unchanged. A device-hidden bwrap
run with `libdrm-events,gbm-probe` executed the single reproduction successfully;
success here means the test observed the expected error, not that the bug is fixed.
The log is `.artifacts/overview-crash/reproduction-native-features.log` in that
worktree. An initial invocation with insufficient features and an exact short
filter executed zero tests; its separate `reproduction.log` is retained and does
not count as validation.

Claude's initial correction `df01e8a0`, joined as signed `4e4b3ff8`, admits an
unsampled current source only when policy replacement covers every applicable
output. The independent device-hidden `presentation_present` run passed five
controls: hidden replacement, preview-only current source, missing sampled source,
omitted owner without replacement, and overlay. Its log is
`.artifacts/overview-crash/4e4b3ff8-presentation-present.log`. These controls reach
composition and capture selection, not the subsequent scheduler settlement; they
are not sufficient on their own for t246 acceptance.

Review therefore required a shared production settlement control, including the
feedback drain and resource release rather than only an empty scheduler queue.
That control exposed a second visibility defect: ordinary geometry could release
a first-visibility wait even though replacement still omitted the source. Repeated
release and re-parking reset its budget. This is a separate deterministic finding,
not evidence that the physical incident involved a first Present. Signed
`8c7fc06fad574de050ba3e590e5bc7c170a35cf3`, joined as `fc012aaf`, corrects that
eligibility check and extracts the existing no-captured-image settlement into
`settle_uncaptured_present<T: NativeCompositionTarget>`. The native retained queue
wrapper and trait call both use an empty required-retirement set; the extraction
does not substitute another frame owner.

The new controls queue a real backend Present and exercise that helper and the
runtime's first-visibility service. They assert repeated hidden-frame pacing and
clearing repaint, a hold at one second and expiry at 2.1 seconds, and recovery of
the same parked transaction after withdrawal without a new Present. Drained
Complete/Skipped and Idle records are backend-produced feedback ready for routing;
these fixtures do not show an independent X client receiving wire events.
Presentation state is removed and the live-presentation count returns to zero.

The controls do not execute the concrete `drive_gpu_presentation` device path,
DMA-BUF import, or its computation of `first_presentation`; the fixture supplies
that branch fact. While a tier is withheld for a missing source, ordinary drawing
can return before the visibility predicate stops seeing replacement. A parked
first Present can then wait for its bounded budget; no broader change is claimed.
Admission refuses, and source removal revokes, an invalid publication.

## Joined development checks

The joined native backend/session all-feature suites passed on `fc012aaf`:
1,749 passed, zero failed, 39 ignored. Subsequent revisions `045e90c8` and
`c866e112` only move or clarify comments; the retained `comment-only-delta.patch`
records the exact change. On `c866e112`, paired `policy_transport` passed 13 tests
with zero failures and one existing ignored case, strict backend/session
all-target Clippy passed, and the workspace all-feature/all-target check passed.
Fmt, metadata, layout and whitespace checks also passed in the isolated worktree.

The paired binary is the frozen Hagia h004 executable at
`~/.local/state/hagia/development-evidence/h004-e3fe2b4/hagia`, SHA256
`8dae3277819b319962d8c85fb18437d2b0e63971a9615f8a4b0e70716b5d5d04`.
Checks used bwrap with devices and installed-session sockets hidden, a private
disk-backed `/tmp`, a disk Cargo cache, nice level 10, and two build jobs. No
live-session environment or smoke permission was retained.

Logs remain in `/home/niltempus/dev/sophia-borders/.artifacts/overview-crash`.
The `negative-controls` subdirectory retains Claude's four failing mutations and
source hashes: `present-unsampled-refused`, `present-always-released`,
`hidden-first-released`, and `uncaptured-no-repaint`. Each exits 101 with a test
failure; the last two establish that the hidden hold and clearing repaint are
observed by the controls. Their original provenance is
`/home/niltempus/dev/sophia-t196/.artifacts/t244-controls/controls.json`; that
worktree was removed on 2026-09-26, and the retained controls are
`~/.local/state/sophia/development-evidence/rendering-6251aa79/t244-controls/`.

The session review has not established a separate session source defect. The
director accepted the bounded headless scope. The separate Hagia clipped-border
concern was not reproduced: valid
geometry constraints and the backend's clipping-before-border construction
exclude the hypothesized case. No border repair is attributed to t246.

## Final gate and development closure

The final signed candidate is `ae7f1578402fb5b45c39333f28b5a2d5bfdd5abd`.
In the director's exclusive main-tree window, fmt, diff, metadata and layout
all exited zero. Device-hidden default workspace tests passed 3,781 cases with
zero failures and 33 ignored, from 17:01:53Z to 17:05:12Z on 2026-09-25.
The native protocol family passed all eight phases from 17:05:46Z to 17:11:13Z:
isolation, independent WM, independent shell, protocol/runtime, Engine owners,
live-output owner, output client and control service. Its process exited zero
and its start/end source identities match.

The frozen paired sources were Hagia
`9dc9d2be8b5f2f6888cc3fffada2ab93895c0bf8` (documentation above tested h004
source `e3fe2b4`), Narthex `50b9014d96f675f515b5e092c071427fb8e34423`,
and Lom `97b6f6345176841ebf0398df468015febe59fd6c`. Lom's diagnostic binary
hash remained `bf32e46e5955131c41aac60f942ce7df69e9ae29c612b18beb4bc6efd2948b04`.
All four roots were clean and unchanged at end capture, and the frozen Hagia
binary retained the hash recorded above. The Hagia source freeze was released
only after this capture.

Durable evidence is
`~/.local/state/sophia/development-evidence/t246-ae7f1578`: `manifest.json`,
`gate-summary.json`, start/end identities, native-family report and logs,
worktree logs and all four negative controls. Its `SHA256SUMS` covers 54 files;
verification passed. Temporary directories and compiler caches are excluded.
The SHA256 of `SHA256SUMS` is
`c58507225d372235b493cbf1e4e854696397f1e76702bc6b1cdd5570d371b310`.
The original main gate directory is
`/home/niltempus/dev/sophia/.artifacts/t246-ae7f1578-74jtg8wi`.

The director authorized t246 **development** closure after this result. No
separate session correction was needed. The concrete-driver, wire-delivery and
physical-cause limits above remain; this does not claim physical acceptance or
authorize installing or reloading either project. Main is held at the gated
candidate pending coordinated docs verification and merge authorization.

## Connections

The [WM presentation contract](../../wm-presentation.md) permits replacement
scenes and separates presentation authority from source retention. The
[rendering foundation](../../rendering-foundation.md) describes the generic
source-instance composition path involved here.
