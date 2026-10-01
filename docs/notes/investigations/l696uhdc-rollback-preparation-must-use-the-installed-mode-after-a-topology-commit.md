---
id: l696uhdc
date: 2026-09-30
kind: investigation
status: closed
tags: [investigation]
---
# Rollback preparation must use the installed mode after a topology commit

## Question and evidence

Why does a runtime Apply fail after a successful profile startup mode change?

The attended t253 run used signed Sophia `170d606b6`, integration `4106be4`,
and the pinned generic C SDK peer. Validate and semantic rejection passed.
In commit-restore, startup changed one head from 60 Hz to 120 Hz and committed
topology epoch 2. The peer's first Apply was admitted, but rollback-resource
preparation returned `PublishedSnapshotMismatch` before any candidate KMS submit.
The peer received Rejected. Baseline and peer-exit KMS/owner rows matched, and
console recovery was clean. Peer-death was not run.

Retained evidence under `~/.local/state/sophia/development-evidence/ipc-retirement/`:

- `t253-native-run-4106be4-01/run/commit-restore/session.log`, especially
  lines 109–139, plus all three stages' recovery and foreground-console records.
- `t253-commit-restore-mismatch-audit-01/AUDIT.md`: independent source trace.
- `t253-installed-rollback-01/`: isolated regression and validation logs.

## Finding and resolution

`published_output_topology` used `output_capabilities().selected_mode()` to
reconstruct rollback timing. Those capabilities are read using the card session's
construction-time selections. Topology installation updates the live head's
selection and refresh, but does not change those discovery selections. The
rollback projection therefore compared the old 60 Hz timing with the installed
120 Hz state and correctly refused the inconsistent rollback image.

Rollback now derives its full timing from the installed head selection. Missing
or unusable modes remain errors. No capability query or device rediscovery is
needed for this projection.

Keep the discovery ordering unchanged: `bounded_timings` puts the discovery
selected mode first, and public mode IDs depend on those positions. Making that
field live would reorder IDs after a commit and break profile reload against the
published table. Separating stable discovery ordering from live current mode is
a broader follow-up; this fix does not redefine capability or snapshot semantics.

## Validation and remaining work

The device-free regression uses a 60 Hz discovery table with a committed 120 Hz
head, then a later 60 Hz commit. It checks the complete rollback modeline and
generation. Supplying the stale discovery timing reproduces
`PublishedSnapshotMismatch`. A second case checks a different clock, sync,
blanking and flags at the same nominal refresh, plus missing and zero-rate mode
refusals. These tests use the private projection helper called by production.

The first compile failed because the test imported the internal `drm` module
instead of the external crate; explicit `::drm` paths corrected it. Both regression
tests then passed. Logs preserve the initial failure.
The backend library suite passed 197 tests and `libdrm_events_feature` passed
305. Strict backend all-targets clippy passed with both native features enabled.

This changes the native acceptance candidate. Fresh signed preparation,
performance evidence, integration pins and sealed inputs must precede another
attended four-stage run. No native acceptance or t253/t272 completion follows
from the deterministic result.

## Connections

- [t253 plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t253--migrate-the-separate-output-role)
  owns the remaining native acceptance gate.
- [Native proof guide](../../testing/output-file-native-proof.md) separates
  supplied owner observations from physical restoration evidence.

## Physical acceptance, 2026-10-01

Signed Sophia `ddd27bd6d9`, assembled by integration `bec6db137d`, passed all
four attended stages, including commit-restore and peer death after apply.
Restored KMS and owner readbacks, local RolledBack settlement, the joined
peer-loss verdict and clean native/console shutdown are present. The
[acceptance record](../milestones/ofard23a-accept-the-revision-1-output-file-role-through-native-rollback.md)
binds the exact manifests, independent audits and performance evidence.
This closes the observed defect for the declared one-card, two-head,
refresh-only fixture. Earlier failures and the deterministic-test limits
above remain evidence; they are not relabelled as passing runs.
